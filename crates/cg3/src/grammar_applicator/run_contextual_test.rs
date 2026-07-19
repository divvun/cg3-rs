//! `src/GrammarApplicator_runContextualTest.cpp` impl of `GrammarApplicator` —
//! the central contextual-test dispatcher and its dependency/parenthesis/
//! relation/single-test helpers. Literal, bug-for-bug Wave-2 port.
//!
//! ============================================================================
//! CROSS-PARTIAL / SCAFFOLD DEPENDENCIES (NOT edited here — see task report)
//! ============================================================================
//! SIBLING GrammarApplicator methods CALLED here but DEFINED in other partials:
//!   - match_set: does_set_match_cohort_normal / does_set_match_cohort_careful
//!    (`(&mut self, cohort: CohortId, set: u32,
//!    context: Option<&mut dSMC_Context>) -> bool`),
//!    does_set_match_reading
//!    (`(&mut self, reading: ReadingId, set: u32, bypass_index: bool,
//!    unif_mode: bool) -> bool`),
//!    does_tag_match_regexp
//!    (`(&mut self, test: u32, tag: &Tag, bypass_index: bool) -> u32`).
//!   - context:   get_mark (`(&self) -> Option<CohortId>`),
//!    get_attach_to (`(&self) -> ReadingSpec`, uses `.cohort`),
//!    set_mark (`(&mut self, Option<CohortId>)`).
//!   - reflow:    generate_varstring_tag (`(&mut self, &Tag) -> TagId`).
//!   - run_rules: get_sub_reading (`(&mut self, ReadingId, i32)
//!    -> Option<ReadingId>`) — used indirectly via the cohort
//!    matchers, not called here.
//!
//! EXPOSED here (run_rules / match_set call these):
//!   - run_contextual_test(&mut self, sw: Option<SwId>, position: u32,
//!         test: CtxId, deep: Option<*mut Option<CohortId>>,
//!         origin: Option<CohortId>) -> Option<CohortId>
//!     This is the exact shape match_set.rs already calls
//!     (`self.run_contextual_test(cparent, clocal, l, context.deep, Some(cohort))`),
//!     where `cparent: Option<SwId>`, `clocal: u32` (a cohort's `local_number`).
//!
//! ARENA-MODEL / SIGNATURE NOTES
//! * `SingleWindow*& sWindow` (a by-reference, reassignable pointer) → an
//!   `Option<SwId>` LOCAL (`sw`): reassignments hop windows exactly as the C++
//!   does, and never escape. `size_t position` → `u32` (a cohort `local_number`).
//! * `sWindow->parent->cohort_map` (the owning `Window`'s map) → the applicator's
//!   inline `self.doc.cohorts.cohort_map` — the port holds one `Window` per engine.
//! * The C++ `CohortIterator*` base-pointer virtual dispatch (`++(*it)`, `**it`)
//!   is modelled by [`ItSel`]: the six iterator pools have distinct concrete
//!   `advance`/`current` signatures (some take store/grammar/window), so the
//!   selected pool + key is remembered and re-dispatched each loop turn.
//! * `bag_of_tags` (POS_BAG_OF_TAGS) is an EMBEDDED `Reading` on `SingleWindow`,
//!   not an arena object, but `does_set_match_reading` needs a `ReadingId`. The
//!   reading is cloned into the readings arena for the duration of the match and
//!   the slot freed afterwards (`with_bag_of_tags`).
//!
//! REPRODUCED QUIRKS
//! * getCohortInWindow crosses at most ONE window boundary; an offset
//!   overshooting by more than one window yields `None`.
//! * posOutputHelper mixes signed (`SI32`) offset math with the raw UNSIGNED
//!   `local_number` in the two origin vetoes (kept as `u32` comparisons).
//! * runContextualTest returns `sWindow->cohorts[0]` as a truthy
//!   success-with-no-cohort sentinel (e.g. a matched NONE test).

// The module doc above is a hand-aligned cross-partial reference block (wrapped
// method signatures under list items); it trips the markdown-list doc lints
// without being a real rendering problem.
#![allow(clippy::doc_lazy_continuation, clippy::doc_overindented_list_items)]

use crate::arena::{CohortId, CtxId, SwId};
use crate::cohort::{CT_RELATED, CT_REMOVED};
use crate::cohort_iterator::{
    CohortIterator, DepAncestorIter, DepDescendentIter, DepParentIter, TopologyLeftIter,
    TopologyRightIter,
};
use crate::contextual_test::POS_JUMP_POS::{JUMP_ATTACH, JUMP_MARK, JUMP_TARGET};
use crate::contextual_test::{
    MASK_POS_LORR, MASK_SELF_NB, POS_ABSOLUTE, POS_ALL, POS_ATTACH_TO, POS_BAG_OF_TAGS,
    POS_CAREFUL, POS_DEP_CHILD, POS_DEP_DEEP, POS_DEP_GLOB, POS_DEP_PARENT, POS_DEP_SIBLING,
    POS_JUMP, POS_LEFT, POS_LEFT_PAR, POS_LEFTMOST, POS_LOOK_DELAYED, POS_LOOK_DELETED,
    POS_LOOK_IGNORED, POS_MARK_SET, POS_NEGATE, POS_NONE, POS_NOT, POS_PASS_ORIGIN, POS_RELATION,
    POS_RIGHT, POS_RIGHT_PAR, POS_RIGHTMOST, POS_SCANALL, POS_SCANFIRST, POS_SELF, POS_SPAN_BOTH,
    POS_SPAN_LEFT, POS_SPAN_RIGHT, POS_TMPL_OVERRIDE, POS_UNKNOWN, POS_WITH,
};
use crate::inlines::{make_64, si32};
use crate::single_window::less_cohort;
use crate::store::RuntimeStore;
use crate::tag::T_VARSTRING;
use crate::types::GlobalNumber;

use crate::sorted_vector::uint32SortedVector;

use super::{Engine, TRV_BARRIER, TRV_BREAK, TRV_BREAK_DEFAULT, dSMC_Context};

/// Which iterator pool `runContextualTest` selected for the generic-iterator
/// arm (the C++ `CohortIterator* it`). The pools have incompatible concrete
/// `advance`/`current` signatures, so instead of a base pointer we remember the
/// choice + its `ci_depths` key and re-dispatch. `Plain`/`Left`/`Right` take
/// only `&store,&grammar`; the dep iterators additionally take `&window`; the
/// dep-parent iterator MUTATES on advance (its `m_seen` cycle guard) and the two
/// precomputed dep iterators (`Glob`/`Ancestor`) hold their own vectors.
#[derive(Copy, Clone)]
enum ItSel {
    Plain(u32),
    Left(u32),
    Right(u32),
    DepParent(u32),
    DepGlob(u32),
    DepAncestor(u32),
}

// --- Store-aware `CohortSet` helpers (runRelationTest builds a `CohortSet`) ---
// A C++ `CohortSet` (`sorted_vector<Cohort*, compare_Cohort>`) is a
// `Vec<CohortId>`; the `compare_Cohort` order (`less_Cohort` — by `local_number`,
// tie-broken by owning-window `number`) needs the store, so the sorted,
// dup-suppressing operations run against the store here (mirrors the private
// `cs_*` helpers in `cohort_iterator.rs`).

fn cs_lower_bound(store: &RuntimeStore, v: &[CohortId], t: CohortId) -> usize {
    v.partition_point(|&x| less_cohort(store, x, t))
}

// Wave 4 (w4-file-split-fmt): the verbatim Reading field-copy is
// consolidated in `crate::reading::clone_verbatim`.
use crate::reading::clone_verbatim as clone_reading;

fn cs_insert(store: &RuntimeStore, v: &mut Vec<CohortId>, t: CohortId) -> bool {
    if v.is_empty() {
        v.push(t);
        return true;
    }
    let it = cs_lower_bound(store, v, t);
    if it == v.len() {
        v.push(t);
        return true;
    }
    if less_cohort(store, v[it], t) || less_cohort(store, t, v[it]) {
        v.insert(it, t);
        return true;
    }
    false
}

impl Engine<'_> {
    /// C++ constructor sets `ci_depths(6, 0)`; the scaffold `new()` leaves it
    /// empty. Grow-to-6 lazily so the six pooled-iterator counters are always
    /// indexable (a no-op once `ci_depths` is 6-wide). NOTE: the real
    /// `grammar-applicator-fn` constructor (core.rs) should size `ci_depths` to 6
    /// zeros; until then this guard keeps the arm panic-free.
    fn ensure_ci_depths(&mut self) {
        if self.scratch.ci_depths.len() < 6 {
            self.scratch.ci_depths.resize(6, 0);
        }
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-single-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-single-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-single-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-test-fn]
    /// The atomic "does this one cohort match the test" step; also computes the
    /// barrier/scan-break signals into the `rvs` accumulator.
    /// C++ `Cohort* runSingleTest(Cohort* cohort, const ContextualTest*, uint8_t&
    /// rvs, bool* retval, Cohort** deep, Cohort* origin)`. Returns
    /// `(cohort, matched)` — the C++ `bool* retval` out-param is the second
    /// return (wave 4); `rvs` stays a by-ref accumulator because callers
    /// genuinely thread barrier state ACROSS calls (set and cleared between
    /// iterations).
    pub fn run_single_test(
        &mut self,
        cohort: CohortId,
        test: CtxId,
        rvs: &mut u8,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> (Option<CohortId>, bool) {
        let mut retval_v = false;
        let retval = &mut retval_v;
        let mut cohort: Option<CohortId> = Some(cohort);
        let cid = cohort.unwrap();

        let regexgrpz = if self.scratch.context_stack.is_empty() {
            0
        } else {
            self.scratch.context_stack.last().unwrap().regexgrp_ct
        };

        let (test_pos, test_target, test_offset, test_barrier, test_cbarrier) = {
            let t = &self.grammar.contexts_arena[test.0];
            (
                t.pos,
                t.target.get(),
                t.offset,
                t.barrier.get(),
                t.cbarrier.get(),
            )
        };

        if test_pos.intersects(POS_MARK_SET) {
            self.set_mark(Some(cid));
        }
        if test_pos.intersects(POS_ATTACH_TO) && self.get_attach_to().cohort != Some(cid) {
            // Clear readings for rules that care about readings.
            let lists = self.rst_gather_lists(cid, test_pos);
            for list in lists.into_iter().flatten() {
                for reading in list {
                    let r = self.doc.store.readings.get_mut(reading.0);
                    r.matched_target = false;
                    r.matched_tests = false;
                }
            }
        }
        if test_pos.intersects(POS_WITH) {
            self.scratch.merge_with = Some(cid);
        }
        if let Some(d) = deep.as_deref_mut() {
            *d = Some(cid);
        }

        // dSMC_Context context = { test, deep, origin, test->pos };
        let mut context = dSMC_Context {
            test: Some(test),
            deep,
            origin,
            options: test_pos,
            did_test: false,
            matched_target: false,
            matched_tests: false,
            in_barrier: false,
        };

        if test_pos.intersects(POS_CAREFUL) {
            *retval = self.does_set_match_cohort_careful(cid, test_target, Some(&mut context));
            if !context.matched_target && (test_pos.intersects(POS_SCANFIRST)) {
                context.did_test = true;
                // Intentionally ignoring the return value to populate matched_target.
                self.does_set_match_cohort_normal(cid, test_target, Some(&mut context));
            }
        } else {
            *retval = self.does_set_match_cohort_normal(cid, test_target, Some(&mut context));
        }

        // origin loop-back detection.
        if let Some(org) = origin {
            let scan = test_pos.intersects(POS_SCANALL | POS_SCANFIRST);
            if (test_offset != 0 || scan)
                && Some(org) == cohort
                && self.doc.store.cohorts.get(org.0).local_number != 0
            {
                cohort = None;
                *rvs |= TRV_BREAK;
            }
        }
        if context.matched_target && (test_pos.intersects(POS_SCANFIRST)) {
            *rvs |= TRV_BREAK;
        } else if !test_pos.intersects(POS_SCANALL | POS_SCANFIRST | POS_DEP_DEEP | POS_DEP_GLOB) {
            *rvs |= TRV_BREAK | TRV_BREAK_DEFAULT;
        }

        let broken = (*rvs & TRV_BREAK) != 0;

        context.test = None;
        context.deep = None;
        context.origin = None;
        context.did_test = true;

        if test_barrier != 0
            && let Some(cid) = cohort
        {
            let mut bctx = dSMC_Context {
                test: None,
                deep: None,
                origin: None,
                options: test_pos & !POS_CAREFUL,
                did_test: false,
                matched_target: false,
                matched_tests: false,
                in_barrier: true,
            };
            let barrier = self.does_set_match_cohort_normal(cid, test_barrier, Some(&mut bctx));
            if barrier {
                self.scratch.seen_barrier = true;
                *rvs |= TRV_BREAK | TRV_BARRIER;
                *rvs &= !TRV_BREAK_DEFAULT;
            }
        }
        if test_cbarrier != 0
            && let Some(cid) = cohort
        {
            let mut cbctx = dSMC_Context {
                test: None,
                deep: None,
                origin: None,
                options: test_pos | POS_CAREFUL,
                did_test: false,
                matched_target: false,
                matched_tests: false,
                in_barrier: true,
            };
            let cbarrier = self.does_set_match_cohort_careful(cid, test_cbarrier, Some(&mut cbctx));
            if cbarrier {
                self.scratch.seen_barrier = true;
                *rvs |= TRV_BREAK | TRV_BARRIER;
                *rvs &= !TRV_BREAK_DEFAULT;
            }
        }
        if context.matched_target && *retval {
            *rvs |= TRV_BREAK;
        }
        if !broken && (*rvs & TRV_BARRIER != 0) && test_pos.contains(MASK_SELF_NB) {
            *rvs &= !(TRV_BREAK | TRV_BARRIER);
        }
        if !*retval && !self.scratch.context_stack.is_empty() {
            self.scratch.context_stack.last_mut().unwrap().regexgrp_ct = regexgrpz;
        }
        (cohort, retval_v)
    }

    /// C++ overload `Cohort* runSingleTest(SingleWindow* sWindow, size_t i, ...)`:
    /// out-of-range `i` sets `rvs |= TRV_BREAK`, returns `(None, false)`;
    /// otherwise forwards `sWindow->cohorts[i]`.
    fn run_single_test_at(
        &mut self,
        sw: SwId,
        i: i32,
        test: CtxId,
        rvs: &mut u8,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> (Option<CohortId>, bool) {
        let len = self.doc.store.single_windows.get(sw.0).cohorts.len() as i32;
        if i < 0 || i >= len {
            *rvs |= TRV_BREAK;
            return (None, false);
        }
        let cohort = self.doc.store.single_windows.get(sw.0).cohorts[i as usize];
        self.run_single_test(cohort, test, rvs, deep, origin)
    }

    /// C++ `runSingleTest`'s `ReadingList* lists[4]` collection: slot 0 =
    /// `readings`; 1/2/3 = `deleted`/`delayed`/`ignored` when the matching
    /// `POS_LOOK_*` flag is set. Cloned so the cohorts arena is not borrowed while
    /// mutating the readings arena. Not a manifest symbol.
    fn rst_gather_lists(
        &self,
        cohort: CohortId,
        pos: crate::contextual_test::PosFlags,
    ) -> [Option<Vec<crate::arena::ReadingId>>; 4] {
        let c = self.doc.store.cohorts.get(cohort.0);
        let mut lists: [Option<Vec<crate::arena::ReadingId>>; 4] =
            [Some(c.readings.clone()), None, None, None];
        if pos.intersects(POS_LOOK_DELETED) {
            lists[1] = Some(c.deleted.clone());
        }
        if pos.intersects(POS_LOOK_DELAYED) {
            lists[2] = Some(c.delayed.clone());
        }
        if pos.intersects(POS_LOOK_IGNORED) {
            lists[3] = Some(c.ignored.clone());
        }
        lists
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.pos-output-helper-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.pos-output-helper-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pos-output-helper-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pos-output-helper-fn]
    /// Validates that a template match landed where an overriding test demands.
    /// C++ `bool posOutputHelper(const SingleWindow* sWindow, size_t position,
    /// const ContextualTest*, const Cohort* cohort, const Cohort* cdeep)`.
    /// QUIRK: the two origin vetoes compare the raw UNSIGNED `local_number`
    /// against `position` while the offset math above them is signed.
    pub fn pos_output_helper(
        &self,
        sw: SwId,
        position: u32,
        test: CtxId,
        cohort: CohortId,
        cdeep: CohortId,
    ) -> bool {
        let mut good = false;

        // const Cohort* cs[4] = { cohort, cdeep, cohort, cdeep };
        let mut cs: [CohortId; 4] = [cohort, cdeep, cohort, cdeep];
        if let Some(m) = self.scratch.tmpl_cntx.min {
            cs[2] = m;
        }
        if let Some(m) = self.scratch.tmpl_cntx.max {
            cs[3] = m;
        }

        // std::sort(cs, cs + 4, compare_Cohort());
        let store = &self.doc.store;
        cs.sort_by(|&a, &b| {
            if less_cohort(store, a, b) {
                std::cmp::Ordering::Less
            } else if less_cohort(store, b, a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        let (test_pos, test_offset) = {
            let t = &self.grammar.contexts_arena[test.0];
            (t.pos, t.offset)
        };

        // If the override included * or @, offsets are irrelevant.
        if test_pos.intersects(POS_SCANFIRST | POS_SCANALL | POS_ABSOLUTE) {
            good = true;
        } else {
            let cs0_ln = self.doc.store.cohorts.get(cs[0].0).local_number;
            let cs3_ln = self.doc.store.cohorts.get(cs[3].0).local_number;
            if (test_offset > 0 && si32(cs0_ln) - si32(position) == test_offset)
                || (test_offset < 0 && si32(cs3_ln) - si32(position) == test_offset)
            {
                good = true;
            }
        }
        // Deep result left the window (no span flag).
        if !test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT | POS_SPAN_RIGHT) {
            let cdeep_parent = self.doc.store.cohorts.get(cdeep.0).parent;
            if cdeep_parent != Some(sw) {
                good = false;
            }
        }
        // Origin-straddle vetoes (raw unsigned local_number).
        if !test_pos.intersects(POS_PASS_ORIGIN) {
            let cs0_ln = self.doc.store.cohorts.get(cs[0].0).local_number;
            let cs3_ln = self.doc.store.cohorts.get(cs[3].0).local_number;
            if (test_offset < 0 && cs3_ln > position) || (test_offset > 0 && cs0_ln < position) {
                good = false;
            }
        }
        good
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
    /// Runs one template (`tmpl`) on behalf of the outer `test`, optionally
    /// imposing the outer test's position onto the template, then validating the
    /// result. C++ `Cohort* runContextualTest_tmpl(SingleWindow*, size_t, const
    /// ContextualTest* test, ContextualTest* tmpl, Cohort*& cdeep, Cohort*
    /// origin)`. `cdeep` (the deepest reached cohort) is an out-param (`&mut`).
    pub fn run_contextual_test_tmpl(
        &mut self,
        sw: Option<SwId>,
        position: u32,
        test: CtxId,
        tmpl: CtxId,
        cdeep: &mut Option<CohortId>,
        origin: Option<CohortId>,
    ) -> Option<CohortId> {
        let min = self.scratch.tmpl_cntx.min;
        let max = self.scratch.tmpl_cntx.max;
        let in_template = self.scratch.tmpl_cntx.in_template;
        self.scratch.tmpl_cntx.in_template = true;

        let test_linked = self.grammar.contexts_arena[test.0].linked;
        if let Some(l) = test_linked {
            self.scratch.tmpl_cntx.linked.push(l);
        }

        // Snapshot the template's own pos/offset/cbarrier/barrier.
        let orgpos = self.grammar.contexts_arena[tmpl.0].pos;
        let orgoffset = self.grammar.contexts_arena[tmpl.0].offset;
        let orgcbar = self.grammar.contexts_arena[tmpl.0].cbarrier;
        let orgbar = self.grammar.contexts_arena[tmpl.0].barrier;

        let (test_pos, test_offset, test_cbarrier, test_barrier) = {
            let t = &self.grammar.contexts_arena[test.0];
            (t.pos, t.offset, t.cbarrier, t.barrier)
        };

        let override_applied = test_pos.intersects(POS_TMPL_OVERRIDE);
        if override_applied {
            let t = &mut self.grammar.contexts_arena[tmpl.0];
            t.pos = test_pos;
            t.pos &= !(POS_NEGATE | POS_NOT | POS_JUMP);
            t.offset = test_offset;
            if test_offset != 0 && !test_pos.intersects(POS_SCANFIRST | POS_SCANALL | POS_ABSOLUTE)
            {
                t.pos |= POS_SCANALL;
            }
            if test_cbarrier.get() != 0 {
                t.cbarrier = test_cbarrier;
            }
            if test_barrier.get() != 0 {
                t.barrier = test_barrier;
            }
        }

        // cohort = runContextualTest(sWindow, position, tmpl, &cdeep, origin)
        let mut cohort = self.run_contextual_test(sw, position, tmpl, Some(&mut *cdeep), origin);

        if override_applied {
            let t = &mut self.grammar.contexts_arena[tmpl.0];
            t.pos = orgpos;
            t.offset = orgoffset;
            t.cbarrier = orgcbar;
            t.barrier = orgbar;
            if let (Some(c), Some(cd)) = (cohort, *cdeep)
                && test_offset != 0
            {
                let sw_id = sw.expect(
                    "runContextualTest_tmpl: posOutputHelper needs a window but sWindow is null",
                );
                if !self.pos_output_helper(sw_id, position, test, c, cd) {
                    cohort = None;
                }
            }
        }

        if test_linked.is_some() {
            self.scratch.tmpl_cntx.linked.pop();
        }
        if cohort.is_none() {
            self.scratch.tmpl_cntx.min = min;
            self.scratch.tmpl_cntx.max = max;
            self.scratch.tmpl_cntx.in_template = in_template;
        }

        cohort
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn]
    /// The central contextual-test dispatcher. C++ `Cohort*
    /// runContextualTest(SingleWindow* sWindow, size_t position, const
    /// ContextualTest*, Cohort** deep, Cohort* origin)`. Returns the matched
    /// cohort, `None` on failure, or `sWindow->cohorts[0]` as a truthy
    /// success-with-no-cohort sentinel.
    pub fn run_contextual_test(
        &mut self,
        sw: Option<SwId>,
        position: u32,
        test: CtxId,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Option<CohortId> {
        let mut sw = sw;
        let mut position = position;
        let mut origin = origin;

        let test_pos = self.grammar.contexts_arena[test.0].pos;
        if test_pos.intersects(POS_UNKNOWN) {
            // u_fprintf(...); CG3Quit(1);
            panic!(
                "Error: Contextual tests with position '?' cannot be used directly. Provide an override position."
            );
        }

        let mut cohort: Option<CohortId> = None;
        let mut retval = true;
        let org_swin = sw;

        if test_pos.intersects(POS_JUMP) {
            let jump_pos = self.grammar.contexts_arena[test.0].jump_pos;
            let mut j: Option<CohortId> = None;
            if jump_pos == JUMP_MARK as i8 {
                j = self.get_mark();
            } else if jump_pos == JUMP_ATTACH as i8 {
                j = self.get_attach_to().cohort;
            } else if jump_pos == JUMP_TARGET as i8 {
                for it in self.scratch.context_stack.iter().rev() {
                    if it.is_with {
                        j = it.target.cohort;
                    }
                }
            } else {
                if self.scratch.context_stack.len() > 1 {
                    let ctx = &self.scratch.context_stack[self.scratch.context_stack.len() - 2];
                    if ctx.context.len() >= jump_pos as usize {
                        j = ctx.context[(jump_pos - 1) as usize];
                    }
                }
            }
            if let Some(jc) = j {
                let c = self.doc.store.cohorts.get(jc.0);
                sw = c.parent;
                position = c.local_number;
            } else {
                retval = false;
            }
        }

        let test_offset = self.grammar.contexts_arena[test.0].offset;
        let mut pos = si32(position) + test_offset;

        if !retval {
            // Jump failed because the position does not exist.
            return self.finalize_got_a_cohort(sw, test, cohort, retval);
        }

        let test_tmpl = self.grammar.contexts_arena[test.0].tmpl;
        let has_ors = !self.grammar.contexts_arena[test.0].ors.is_empty();

        if let Some(tmpl) = test_tmpl {
            let mut cdeep: Option<CohortId> = None;
            cohort = self.run_contextual_test_tmpl(sw, position, test, tmpl, &mut cdeep, origin);
            if let Some(d) = deep.as_deref_mut() {
                *d = cdeep;
            }
        } else if has_ors {
            let mut cdeep: Option<CohortId> = None;
            let ors = self.grammar.contexts_arena[test.0].ors.clone();
            for iter in ors {
                self.scratch.dep_deep_seen.clear();
                cohort =
                    self.run_contextual_test_tmpl(sw, position, test, iter, &mut cdeep, origin);
                if cohort.is_some() {
                    break;
                }
            }
            if let Some(d) = deep.as_deref_mut() {
                *d = cdeep;
            }
        } else {
            cohort = self.get_cohort_in_window(&mut sw, position, test, &mut pos);
        }

        if cohort.is_none() {
            retval = false;
        } else if test_tmpl.is_some() || has_ors {
            // nothing...
        } else {
            let cid = cohort.unwrap();
            let sw_id = sw.unwrap();

            if test_pos.intersects(POS_PASS_ORIGIN) {
                origin = Some(self.doc.store.single_windows.get(sw_id.0).cohorts[0]);
            }
            if let Some(d) = deep.as_deref_mut() {
                *d = Some(cid);
            }
            if self.scratch.tmpl_cntx.in_template {
                self.extend_tmpl_bounds(cid);
                if let Some(d) = deep.as_deref()
                    && let Some(dc) = *d
                {
                    self.extend_tmpl_bounds(dc);
                }
            }

            self.ensure_ci_depths();
            let mut it: Option<ItSel> = None;

            if (test_pos.intersects(POS_DEP_PARENT)) && (test_pos.intersects(POS_DEP_GLOB)) {
                let key = self.scratch.ci_depths[5];
                self.scratch.ci_depths[5] += 1;
                let (store, grammar, window) = self.split_for_iters();
                let iter = DepAncestorIter::new(
                    Some(cid),
                    Some(test),
                    self.cfg.always_span,
                    store,
                    grammar,
                    window,
                );
                self.scratch.depAncestorIters.insert(key, iter);
                it = Some(ItSel::DepAncestor(key));
            } else if test_pos.intersects(POS_DEP_PARENT) {
                let key = self.scratch.ci_depths[3];
                self.scratch.ci_depths[3] += 1;
                let (store, grammar, window) = self.split_for_iters();
                let iter = DepParentIter::new(
                    Some(cid),
                    Some(test),
                    self.cfg.always_span,
                    store,
                    grammar,
                    window,
                );
                self.scratch.depParentIters.insert(key, iter);
                it = Some(ItSel::DepParent(key));
            } else if test_pos.intersects(POS_DEP_GLOB) {
                let key = self.scratch.ci_depths[4];
                self.scratch.ci_depths[4] += 1;
                let (store, grammar, window) = self.split_for_iters();
                let iter = DepDescendentIter::new(
                    Some(cid),
                    Some(test),
                    self.cfg.always_span,
                    store,
                    grammar,
                    window,
                );
                self.scratch.depDescendentIters.insert(key, iter);
                it = Some(ItSel::DepGlob(key));
            } else if test_pos.intersects(POS_DEP_CHILD | POS_DEP_SIBLING) {
                let nc =
                    self.run_dependency_test(sw_id, cid, test, deep.as_deref_mut(), origin, None);
                if let Some(nc) = nc {
                    cohort = Some(nc);
                    retval = true;
                    sw = self.doc.store.cohorts.get(nc.0).parent;
                } else {
                    retval = false;
                }
                if test_pos.intersects(POS_NONE) {
                    retval = !retval;
                }
            } else if test_pos.intersects(POS_LEFT_PAR | POS_RIGHT_PAR) {
                let nc = self.run_parenthesis_test(sw_id, cid, test, deep.as_deref_mut(), origin);
                if let Some(nc) = nc {
                    cohort = Some(nc);
                    retval = true;
                } else {
                    retval = false;
                }
            } else if test_pos.intersects(POS_RELATION) {
                let nc = self.run_relation_test(sw_id, cid, test, deep.as_deref_mut(), origin);
                if let Some(nc) = nc {
                    cohort = Some(nc);
                    retval = true;
                } else {
                    retval = false;
                }
                if test_pos.intersects(POS_NONE) {
                    retval = !retval;
                }
            } else if test_pos.intersects(POS_BAG_OF_TAGS) {
                let test_target = self.grammar.contexts_arena[test.0].target.get();
                let mut m = self.match_bag_of_tags(sw_id, test_target);
                if !m && (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT | POS_SPAN_RIGHT)) {
                    let mut left = self.doc.store.single_windows.get(sw_id.0).previous;
                    let mut right = self.doc.store.single_windows.get(sw_id.0).next;
                    while left.is_some() || right.is_some() {
                        if left.is_some() && (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT)) {
                            let lw = left.unwrap();
                            m = self.match_bag_of_tags(lw, test_target);
                            left = self.doc.store.single_windows.get(lw.0).previous;
                        } else {
                            left = None;
                        }
                        if right.is_some() && (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT))
                        {
                            let rw = right.unwrap();
                            m = self.match_bag_of_tags(rw, test_target);
                            right = self.doc.store.single_windows.get(rw.0).next;
                        } else {
                            right = None;
                        }
                        if m {
                            break;
                        }
                    }
                }
                if test_pos.intersects(POS_NOT) {
                    m = !m;
                }
                if m {
                    let test_linked = self.grammar.contexts_arena[test.0].linked;
                    if let Some(l) = test_linked {
                        cohort =
                            self.run_contextual_test(sw, position, l, deep.as_deref_mut(), origin);
                    }
                } else {
                    retval = false;
                }
            } else if test_offset == 0 && (test_pos.intersects(POS_SCANFIRST | POS_SCANALL)) {
                // Symmetric bidirectional scan.
                let (c, rv) =
                    self.run_scan(sw_id, cid, test, pos, deep.as_deref_mut(), origin, retval);
                cohort = c;
                retval = rv;
            } else if test_offset < 0 {
                let key = self.scratch.ci_depths[1];
                self.scratch.ci_depths[1] += 1;
                let iter = TopologyLeftIter::new(Some(cid), Some(test), self.cfg.always_span);
                self.scratch.topologyLeftIters.insert(key, iter);
                it = Some(ItSel::Left(key));
            } else if test_offset > 0 {
                let key = self.scratch.ci_depths[2];
                self.scratch.ci_depths[2] += 1;
                let iter = TopologyRightIter::new(Some(cid), Some(test), self.cfg.always_span);
                self.scratch.topologyRightIters.insert(key, iter);
                it = Some(ItSel::Right(key));
            } else {
                let key = self.scratch.ci_depths[0];
                self.scratch.ci_depths[0] += 1;
                let iter = CohortIterator::new(Some(cid), Some(test), self.cfg.always_span);
                self.scratch.cohortIterators.insert(key, iter);
                it = Some(ItSel::Plain(key));
            }

            if let Some(sel) = it {
                let (c, rv) =
                    self.run_iter(sel, org_swin, position, cid, test, deep, origin, retval);
                cohort = c;
                retval = rv;
            }
        }

        self.finalize_got_a_cohort(sw, test, cohort, retval)
    }

    /// C++ `label_gotACohort:` finalize block of `runContextualTest`.
    fn finalize_got_a_cohort(
        &self,
        sw: Option<SwId>,
        test: CtxId,
        mut cohort: Option<CohortId>,
        mut retval: bool,
    ) -> Option<CohortId> {
        let (test_pos, test_linked) = {
            let t = &self.grammar.contexts_arena[test.0];
            (t.pos, t.linked)
        };
        if cohort.is_none() {
            retval = false;
        }
        if cohort.is_none() && (test_pos.intersects(POS_NOT)) && test_linked.is_none() {
            retval = !retval;
        }
        if test_pos.intersects(POS_NEGATE) {
            retval = !retval;
        }

        // (The commented-out profiler block is inert.)

        if !retval {
            cohort = None;
        } else if cohort.is_none() {
            // Truthy success with no natural cohort: window's cohort[0].
            let sw_id = sw.expect("runContextualTest: sentinel needs a window");
            cohort = Some(self.doc.store.single_windows.get(sw_id.0).cohorts[0]);
        }
        cohort
    }

    /// C++ `tmpl_cntx.min`/`.max` extension for a matched cohort (the inline
    /// `make_64(parent->number, local_number)` bound update).
    fn extend_tmpl_bounds(&mut self, c: CohortId) {
        let (cwin, cln) = {
            let co = self.doc.store.cohorts.get(c.0);
            let win = self
                .doc
                .store
                .single_windows
                .get(co.parent.unwrap().0)
                .number;
            (win, co.local_number)
        };
        let gpos = make_64(cwin, cln);
        let min_gpos = self.scratch.tmpl_cntx.min.map(|m| {
            let mo = self.doc.store.cohorts.get(m.0);
            make_64(
                self.doc
                    .store
                    .single_windows
                    .get(mo.parent.unwrap().0)
                    .number,
                mo.local_number,
            )
        });
        if min_gpos.is_none() || gpos < min_gpos.unwrap() {
            self.scratch.tmpl_cntx.min = Some(c);
        }
        let max_gpos = self.scratch.tmpl_cntx.max.map(|m| {
            let mo = self.doc.store.cohorts.get(m.0);
            make_64(
                self.doc
                    .store
                    .single_windows
                    .get(mo.parent.unwrap().0)
                    .number,
                mo.local_number,
            )
        });
        if max_gpos.is_none() || gpos > max_gpos.unwrap() {
            self.scratch.tmpl_cntx.max = Some(c);
        }
    }

    /// Split `self` into the three read-only stores the dep iterators' ctors
    /// need (`&store`, `&grammar`, `&cohorts`) without aliasing — the iterator
    /// pools live on `self` separately from these three fields.
    fn split_for_iters(
        &self,
    ) -> (
        &RuntimeStore,
        &crate::grammar::Grammar,
        &crate::window::CohortRegistry,
    ) {
        (&self.doc.store, self.grammar, &self.doc.cohorts)
    }

    /// The C++ generic-iterator arm (`if (it) { ... }`): resets nothing here (the
    /// port ctors already seat the iterator), runs the optional POS_SELF probe,
    /// then walks the iterator to the null sentinel. Returns `(cohort, retval)`.
    #[allow(clippy::too_many_arguments)]
    fn run_iter(
        &mut self,
        sel: ItSel,
        org_swin: Option<SwId>,
        position: u32,
        cohort: CohortId,
        test: CtxId,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
        mut retval: bool,
    ) -> (Option<CohortId>, bool) {
        let test_pos = self.grammar.contexts_arena[test.0].pos;

        let mut nc: Option<CohortId> = None;
        let mut rvs: u8 = 0;
        let mut seen: usize = 0;

        // POS_SELF probe on the origin cohort.
        let self_probe = (test_pos.intersects(POS_SELF))
            && (!test_pos.intersects(MASK_POS_LORR)
                || ((test_pos.intersects(POS_DEP_PARENT)) && (!test_pos.intersects(POS_DEP_GLOB))));
        if self_probe {
            seen += 1;
            let org = org_swin.expect("run_iter: POS_SELF probe needs the origin window");
            let sw_len = self.doc.store.single_windows.get(org.0).cohorts.len();
            assert!(
                (position as usize) < sw_len,
                "Somehow, the input position wasn't inside the current window."
            );
            let self_c = self.doc.store.single_windows.get(org.0).cohorts[position as usize];
            (nc, retval) =
                self.run_single_test(self_c, test, &mut rvs, deep.as_deref_mut(), origin);
            if !retval && (rvs & TRV_BREAK_DEFAULT != 0) {
                rvs &= !(TRV_BREAK | TRV_BREAK_DEFAULT);
            }
        }

        if rvs & TRV_BREAK == 0 {
            let mut current = cohort;
            loop {
                let it_cur = self.iter_current(sel);
                let itc = match it_cur {
                    Some(c) => c,
                    None => break, // *it == CohortIterator(0)
                };
                seen += 1;
                if (test_pos.intersects(POS_LEFT)) && less_cohort(&self.doc.store, current, itc) {
                    nc = None;
                    retval = false;
                    break;
                }
                if (test_pos.intersects(POS_RIGHT)) && !less_cohort(&self.doc.store, current, itc) {
                    nc = None;
                    retval = false;
                    break;
                }
                (nc, retval) =
                    self.run_single_test(itc, test, &mut rvs, deep.as_deref_mut(), origin);
                if (test_pos.intersects(POS_ALL)) && !retval {
                    nc = None;
                    break;
                }
                if (test_pos.intersects(POS_NONE)) && retval {
                    nc = None;
                    break;
                }
                if rvs & TRV_BREAK != 0 {
                    break;
                }
                current = itc;
                self.iter_advance(sel);
            }
        }
        if seen == 0 {
            retval = false;
        }
        if !retval && (test_pos.intersects(POS_NONE)) {
            retval = true;
            nc = Some(cohort);
        }
        (nc, retval)
    }

    /// C++ `**it` — the iterator's current cohort (dispatch by selected pool).
    fn iter_current(&self, sel: ItSel) -> Option<CohortId> {
        match sel {
            ItSel::Plain(k) => self
                .scratch
                .cohortIterators
                .get(&k)
                .and_then(|i| i.current()),
            ItSel::Left(k) => self
                .scratch
                .topologyLeftIters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::Right(k) => self
                .scratch
                .topologyRightIters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::DepParent(k) => self
                .scratch
                .depParentIters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::DepGlob(k) => self
                .scratch
                .depDescendentIters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::DepAncestor(k) => self
                .scratch
                .depAncestorIters
                .get(&k)
                .and_then(|i| i.base.current()),
        }
    }

    /// C++ `++(*it)` — advance the iterator (dispatch by selected pool). The
    /// store/grammar/window borrows the dep/topology iterators need don't alias
    /// the iterator pool being advanced (distinct `self` fields).
    fn iter_advance(&mut self, sel: ItSel) {
        match sel {
            ItSel::Plain(k) => {
                if let Some(i) = self.scratch.cohortIterators.get_mut(&k) {
                    i.advance();
                }
            }
            ItSel::Left(k) => {
                if let Some(mut i) = self.scratch.topologyLeftIters.remove(&k) {
                    i.advance(&self.doc.store, self.grammar);
                    self.scratch.topologyLeftIters.insert(k, i);
                }
            }
            ItSel::Right(k) => {
                if let Some(mut i) = self.scratch.topologyRightIters.remove(&k) {
                    i.advance(&self.doc.store, self.grammar);
                    self.scratch.topologyRightIters.insert(k, i);
                }
            }
            ItSel::DepParent(k) => {
                if let Some(mut i) = self.scratch.depParentIters.remove(&k) {
                    i.advance(&self.doc.store, self.grammar, &self.doc.cohorts);
                    self.scratch.depParentIters.insert(k, i);
                }
            }
            ItSel::DepGlob(k) => {
                if let Some(i) = self.scratch.depDescendentIters.get_mut(&k) {
                    i.advance();
                }
            }
            ItSel::DepAncestor(k) => {
                if let Some(i) = self.scratch.depAncestorIters.get_mut(&k) {
                    i.advance();
                }
            }
        }
    }

    /// The `test->offset == 0 && (SCANFIRST|SCANALL)` bidirectional scan arm.
    /// Returns `(cohort, retval)`; the C++ `goto label_gotACohort` short-circuits
    /// become early returns of the current `(cohort, retval)`.
    #[allow(clippy::too_many_arguments)]
    fn run_scan(
        &mut self,
        sw: SwId,
        start_cohort: CohortId,
        test: CtxId,
        pos: i32,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
        mut retval: bool,
    ) -> (Option<CohortId>, bool) {
        let test_pos = self.grammar.contexts_arena[test.0].pos;

        let mut right: Option<SwId> = Some(sw);
        let mut left: Option<SwId> = Some(sw);
        let mut rpos: i32 = pos;
        let mut lpos: i32 = pos;

        let mut cohort: Option<CohortId> = Some(start_cohort);
        let mut rvs: u8 = 0;

        if test_pos.intersects(POS_SELF) {
            (cohort, retval) =
                self.run_single_test(start_cohort, test, &mut rvs, deep.as_deref_mut(), origin);
            if !retval && (rvs & TRV_BREAK_DEFAULT != 0) {
                rvs &= !(TRV_BREAK | TRV_BREAK_DEFAULT);
            }
        }
        if (rvs & TRV_BREAK != 0) && retval {
            return (cohort, retval);
        }

        let mut i: i32 = 1;
        while left.is_some() || right.is_some() {
            if let Some(lw) = left {
                rvs = 0;
                (cohort, retval) = self.run_single_test_at(
                    lw,
                    lpos - i,
                    test,
                    &mut rvs,
                    deep.as_deref_mut(),
                    origin,
                );
                if (rvs & TRV_BREAK != 0) && retval {
                    return (cohort, retval);
                } else if rvs & TRV_BREAK != 0 {
                    left = None;
                    if test_pos.intersects(POS_NOT) {
                        right = None;
                    }
                } else if lpos - i == 0 {
                    if (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT)) || self.cfg.always_span
                    {
                        left = self.doc.store.single_windows.get(lw.0).previous;
                        if let Some(nl) = left {
                            lpos = i + self.doc.store.single_windows.get(nl.0).cohorts.len() as i32;
                        }
                    } else {
                        left = None;
                    }
                }
            }
            if let Some(rw) = right {
                rvs = 0;
                (cohort, retval) = self.run_single_test_at(
                    rw,
                    rpos + i,
                    test,
                    &mut rvs,
                    deep.as_deref_mut(),
                    origin,
                );
                if (rvs & TRV_BREAK != 0) && retval {
                    return (cohort, retval);
                } else if rvs & TRV_BREAK != 0 {
                    right = None;
                    if test_pos.intersects(POS_NOT) {
                        left = None;
                    }
                } else {
                    let rlen = self.doc.store.single_windows.get(rw.0).cohorts.len() as i32;
                    if rpos + i == rlen - 1 {
                        if (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT))
                            || self.cfg.always_span
                        {
                            right = self.doc.store.single_windows.get(rw.0).next;
                            rpos = (0 - i) - 1;
                        } else {
                            right = None;
                        }
                    }
                }
            }
            i += 1;
        }
        (cohort, retval)
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.get-cohort-in-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.get-cohort-in-window-fn]
    /// C++ free fn `Cohort* getCohortInWindow(SingleWindow*& sWindow, size_t
    /// position, const ContextualTest*, int32_t& pos)`. Resolves a plain
    /// positional test to a concrete cohort, hopping at most one window boundary
    /// (an overshoot yields `None`). `sWindow`/`pos` are in/out (`&mut`). Ported
    /// as a method purely to reach `self.doc.store`/`self.grammar` (no `self` state is
    /// otherwise touched).
    pub fn get_cohort_in_window(
        &self,
        sw: &mut Option<SwId>,
        position: u32,
        test: CtxId,
        pos: &mut i32,
    ) -> Option<CohortId> {
        let mut cohort: Option<CohortId> = None;
        let (test_pos, test_offset) = {
            let t = &self.grammar.contexts_arena[test.0];
            (t.pos, t.offset)
        };
        *pos = si32(position) + test_offset;

        let cur = sw.expect("getCohortInWindow: sWindow is null");

        if (test_pos.intersects(POS_ABSOLUTE))
            && (test_pos.intersects(POS_SPAN_LEFT | POS_SPAN_RIGHT))
        {
            let prev = self.doc.store.single_windows.get(cur.0).previous;
            let next = self.doc.store.single_windows.get(cur.0).next;
            if prev.is_some() && (test_pos.intersects(POS_SPAN_LEFT)) {
                *sw = prev;
            } else if next.is_some() && (test_pos.intersects(POS_SPAN_RIGHT)) {
                *sw = next;
            } else {
                return cohort;
            }
        }

        let mut cur = sw.unwrap();

        if test_pos.intersects(POS_ABSOLUTE) {
            if test_offset < 0 {
                *pos = self.doc.store.single_windows.get(cur.0).cohorts.len() as i32 + test_offset;
            } else {
                *pos = test_offset;
            }
        }

        let cur_len = self.doc.store.single_windows.get(cur.0).cohorts.len() as i32;
        if *pos >= 0 {
            if *pos >= cur_len
                && (test_pos.intersects(POS_SPAN_RIGHT | POS_SPAN_BOTH))
                && self.doc.store.single_windows.get(cur.0).next.is_some()
            {
                cur = self.doc.store.single_windows.get(cur.0).next.unwrap();
                *sw = Some(cur);
                *pos = 0;
            }
        } else {
            if (test_pos.intersects(POS_SPAN_LEFT | POS_SPAN_BOTH))
                && self.doc.store.single_windows.get(cur.0).previous.is_some()
            {
                cur = self.doc.store.single_windows.get(cur.0).previous.unwrap();
                *sw = Some(cur);
                *pos = self.doc.store.single_windows.get(cur.0).cohorts.len() as i32 - 1;
            }
        }

        let cur_len = self.doc.store.single_windows.get(cur.0).cohorts.len() as i32;
        if *pos >= 0 && *pos < cur_len {
            cohort = Some(self.doc.store.single_windows.get(cur.0).cohorts[*pos as usize]);
        }
        cohort
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    /// Traverses dependency children/parents/siblings from `current`, testing
    /// each, optionally recursing (deep). C++ `Cohort* runDependencyTest(
    /// SingleWindow*, Cohort* current, const ContextualTest*, Cohort** deep,
    /// Cohort* origin, const Cohort* self)`.
    #[allow(clippy::too_many_arguments)]
    pub fn run_dependency_test(
        &mut self,
        // C++ reads `sWindow->parent->cohort_map` throughout, which is the
        // applicator's single inline `self.doc.cohorts` in the port; the `sWindow`
        // argument is therefore unused here (kept to mirror the C++ signature).
        _sw: SwId,
        current: CohortId,
        test: CtxId,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
        self_cohort: Option<CohortId>,
    ) -> Option<CohortId> {
        let mut rv: Option<CohortId> = None;

        let selfc = match self_cohort {
            Some(s) => {
                if s == current {
                    return None;
                }
                s
            }
            None => current,
        };

        let (test_pos, test_hash) = {
            let t = &self.grammar.contexts_arena[test.0];
            (t.pos, t.hash)
        };

        if test_pos.intersects(POS_DEP_DEEP) {
            let key = (
                test_hash,
                self.doc.store.cohorts.get(current.0).global_number.get(),
            );
            if self.scratch.dep_deep_seen.contains(key) {
                return None;
            }
            self.scratch.dep_deep_seen.insert(key);
        }

        if (test_pos.intersects(POS_SELF)) && (!test_pos.intersects(MASK_POS_LORR)) {
            let mut rvs: u8 = 0;
            let (tmc, retval) =
                self.run_single_test(current, test, &mut rvs, deep.as_deref_mut(), origin);
            if retval {
                return tmc;
            }
            if rvs & TRV_BARRIER != 0 {
                return None;
            }
        }

        // Select the walked dependency global-number set.
        let mut deps: Vec<u32>;
        if test_pos.intersects(POS_DEP_CHILD) {
            deps = self
                .doc
                .store
                .cohorts
                .get(current.0)
                .dep_children
                .as_slice()
                .to_vec();
        } else {
            if self.doc.store.cohorts.get(current.0).dep_parent == Some(GlobalNumber(0)) {
                let parent_sw = self.doc.store.cohorts.get(current.0).parent.unwrap();
                let root = self.doc.store.single_windows.get(parent_sw.0).cohorts[0];
                deps = self
                    .doc
                    .store
                    .cohorts
                    .get(root.0)
                    .dep_children
                    .as_slice()
                    .to_vec();
            } else {
                let dep_parent = self.doc.store.cohorts.get(current.0).dep_parent;
                let mapped = dep_parent
                    .and_then(|dp| self.doc.cohorts.cohort_map.get(&dp))
                    .copied();
                match mapped {
                    Some(pc) if !self.doc.store.cohorts.get(pc.0).dep_children.empty() => {
                        deps = self
                            .doc
                            .store
                            .cohorts
                            .get(pc.0)
                            .dep_children
                            .as_slice()
                            .to_vec();
                    }
                    _ => {
                        if self.cfg.verbosity_level > 0 {
                            let (ds, dp) = {
                                let c = self.doc.store.cohorts.get(current.0);
                                (c.dep_self, c.dep_parent)
                            };
                            tracing::warn!(
                                "Warning: Cohort {} (parent {}) did not have any siblings.",
                                ds.map_or(0, |g| g.get()),
                                dp.map_or(crate::cohort::DEP_NO_PARENT, |g| g.get())
                            );
                        }
                        return None;
                    }
                }
            }
        }

        if test_pos.intersects(MASK_POS_LORR) {
            // Rebuild `deps` by scanning the whole cohort_map (slower container).
            let mut tmp_deps = uint32SortedVector::new();
            let map: Vec<CohortId> = self.doc.cohorts.cohort_map.values().copied().collect();
            for citer in map {
                let gnum = self.doc.store.cohorts.get(citer.0).global_number.get();
                if deps.contains(&gnum) {
                    if test_pos.intersects(POS_LEFT) {
                        if less_cohort(&self.doc.store, citer, current) {
                            tmp_deps.insert(gnum);
                        }
                    } else if test_pos.intersects(POS_RIGHT) {
                        if less_cohort(&self.doc.store, current, citer) {
                            tmp_deps.insert(gnum);
                        }
                    } else {
                        tmp_deps.insert(gnum);
                    }
                }
            }
            if test_pos.intersects(POS_SELF) {
                let gnum = self.doc.store.cohorts.get(current.0).global_number.get();
                tmp_deps.insert(gnum);
            }
            let mut tmp_vec = tmp_deps.as_slice().to_vec();
            if (test_pos.intersects(POS_RIGHTMOST)) && !tmp_vec.is_empty() {
                tmp_vec.reverse();
            }
            deps = tmp_vec;
        }

        let cur_gnum = self.doc.store.cohorts.get(current.0).global_number.get();
        for dter in deps {
            if dter == cur_gnum && (!test_pos.intersects(POS_SELF)) {
                continue;
            }
            let mapped = self
                .doc
                .cohorts
                .cohort_map
                .get(&GlobalNumber(dter))
                .copied();
            let cohort = match mapped {
                None => {
                    if self.cfg.verbosity_level > 0 {
                        let ds = self
                            .doc
                            .store
                            .cohorts
                            .get(current.0)
                            .dep_self
                            .map_or(0, |g| g.get());
                        if test_pos.intersects(POS_DEP_CHILD) {
                            tracing::warn!(
                                "Warning: Child dependency {} -> {} does not exist - ignoring.",
                                ds,
                                dter
                            );
                        } else {
                            tracing::warn!(
                                "Warning: Sibling dependency {} -> {} does not exist - ignoring.",
                                ds,
                                dter
                            );
                        }
                    }
                    continue;
                }
                Some(c) => c,
            };
            if self
                .doc
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_REMOVED)
            {
                continue;
            }
            let mut good = true;
            let (cur_parent, coh_parent) = {
                (
                    self.doc.store.cohorts.get(current.0).parent,
                    self.doc.store.cohorts.get(cohort.0).parent,
                )
            };
            if cur_parent != coh_parent {
                let cur_win = self
                    .doc
                    .store
                    .single_windows
                    .get(cur_parent.unwrap().0)
                    .number;
                let coh_win = self
                    .doc
                    .store
                    .single_windows
                    .get(coh_parent.unwrap().0)
                    .number;
                if ((!test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT)) && coh_win < cur_win)
                    || ((!test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT)) && coh_win > cur_win)
                {
                    good = false;
                }
            }
            let mut retval = false;
            let mut rvs: u8 = 0;
            if good {
                (_, retval) =
                    self.run_single_test(cohort, test, &mut rvs, deep.as_deref_mut(), origin);
            }
            if test_pos.intersects(POS_ALL) {
                if !retval {
                    rv = None;
                    break;
                } else {
                    rv = Some(cohort);
                }
            } else if retval {
                rv = Some(cohort);
                break;
            } else if rvs & TRV_BARRIER != 0 {
                continue;
            } else if test_pos.intersects(POS_DEP_DEEP) {
                let coh_parent = self.doc.store.cohorts.get(cohort.0).parent.unwrap();
                let tmc = self.run_dependency_test(
                    coh_parent,
                    cohort,
                    test,
                    deep.as_deref_mut(),
                    origin,
                    Some(selfc),
                );
                if let Some(tmc) = tmc {
                    rv = Some(tmc);
                    break;
                }
            }
        }

        rv
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-parenthesis-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-parenthesis-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-parenthesis-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-parenthesis-test-fn]
    /// Tests one edge of the currently-unwrapped enclosure (parentheses). C++
    /// `Cohort* runParenthesisTest(SingleWindow*, const Cohort* current, const
    /// ContextualTest*, Cohort** deep, Cohort* origin)`.
    pub fn run_parenthesis_test(
        &mut self,
        sw: SwId,
        current: CohortId,
        test: CtxId,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Option<CohortId> {
        let ln = self.doc.store.cohorts.get(current.0).local_number;
        if ln < self.scratch.par_left_pos || ln > self.scratch.par_right_pos {
            return None;
        }
        let mut rv: Option<CohortId> = None;

        let mut rvs: u8 = 0;
        let test_pos = self.grammar.contexts_arena[test.0].pos;
        let cohort = if test_pos.intersects(POS_LEFT_PAR) {
            self.doc.store.single_windows.get(sw.0).cohorts[self.scratch.par_left_pos as usize]
        } else {
            self.doc.store.single_windows.get(sw.0).cohorts[self.scratch.par_right_pos as usize]
        };
        let (_, retval) = self.run_single_test(cohort, test, &mut rvs, deep, origin);
        if retval {
            rv = Some(cohort);
        }
        rv
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-relation-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-relation-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-relation-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-relation-test-fn]
    /// Follows named relations (`r:name`) from `current` to related cohorts,
    /// testing each. C++ `Cohort* runRelationTest(SingleWindow*, Cohort* current,
    /// const ContextualTest*, Cohort** deep, Cohort* origin)`.
    pub fn run_relation_test(
        &mut self,
        // C++ takes `sWindow` but only reads `sWindow->parent->cohort_map`, which
        // is the applicator's single inline `self.doc.cohorts` in the port — so the
        // window parameter is unused here (kept to mirror the C++ signature).
        _sw: SwId,
        current: CohortId,
        test: CtxId,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Option<CohortId> {
        {
            let c = self.doc.store.cohorts.get(current.0);
            if (!c.r#type.intersects(CT_RELATED)) || c.relations.is_empty() {
                return None;
            }
        }

        let mut rels: Vec<CohortId> = Vec::new();
        let regexgrpz = self.scratch.context_stack.last().unwrap().regexgrp_ct;

        let test_relation = self.grammar.contexts_arena[test.0].relation;
        // rtag = grammar->single_tags[test->relation]; while T_VARSTRING, expand.
        let mut rtag_id = {
            let it = self.grammar.single_tags.find(test_relation);
            it.get().1
        };
        loop {
            let ttype = self.grammar.single_tags_list[rtag_id.0].r#type;
            if !ttype.intersects(T_VARSTRING) {
                break;
            }
            let tclone = self.grammar.single_tags_list[rtag_id.0].clone();
            rtag_id = self.generate_varstring_tag(&tclone);
        }
        let (rtag_hash, rtag_type) = {
            let t = &self.grammar.single_tags_list[rtag_id.0];
            (t.hash, t.r#type)
        };

        let test_pos = self.grammar.contexts_arena[test.0].pos;

        // Snapshot the relation map (u32 name-hash -> sorted target global numbers).
        let relations: Vec<(u32, Vec<u32>)> = self
            .doc
            .store
            .cohorts
            .get(current.0)
            .relations
            .iter()
            .map(|(k, v)| (*k, v.as_slice().to_vec()))
            .collect();

        if rtag_hash.get() == self.grammar.tag_any {
            for (_name, targets) in &relations {
                for &citer in targets {
                    if let Some(&c) = self.doc.cohorts.cohort_map.get(&GlobalNumber(citer)) {
                        cs_insert(&self.doc.store, &mut rels, c);
                    }
                }
            }
        } else if rtag_type.intersects(crate::tag::T_REGEXP) {
            let caps = {
                let t = &self.grammar.single_tags_list[rtag_id.0];
                t.regexp
                    .as_ref()
                    .map(|re| re.captures_len() as i32 - 1)
                    .unwrap_or(0)
            };
            let rtag = self.grammar.single_tags_list[rtag_id.0].clone();
            for (name, targets) in &relations {
                for &citer in targets {
                    if self
                        .doc
                        .cohorts
                        .cohort_map
                        .contains_key(&GlobalNumber(citer))
                        && self.does_tag_match_regexp(*name, &rtag, caps != 0) != 0
                    {
                        let c = *self
                            .doc
                            .cohorts
                            .cohort_map
                            .get(&GlobalNumber(citer))
                            .unwrap();
                        cs_insert(&self.doc.store, &mut rels, c);
                        let cur = self.scratch.context_stack.last().unwrap().regexgrp_ct;
                        let capped = (regexgrpz as i32 + caps).clamp(0, u8::MAX as i32) as u8;
                        self.scratch.context_stack.last_mut().unwrap().regexgrp_ct =
                            cur.min(capped);
                    }
                }
            }
        } else {
            if let Some((_name, targets)) = relations.iter().find(|(k, _)| *k == rtag_hash.get()) {
                for &citer in targets {
                    if let Some(&c) = self.doc.cohorts.cohort_map.get(&GlobalNumber(citer)) {
                        cs_insert(&self.doc.store, &mut rels, c);
                    }
                }
            }
        }

        // Order/filter `rels`.
        if test_pos.intersects(POS_LEFT) {
            let lb = cs_lower_bound(&self.doc.store, &rels, current);
            rels = rels[..lb].to_vec();
        }
        if test_pos.intersects(POS_RIGHT) {
            let lb = cs_lower_bound(&self.doc.store, &rels, current);
            rels = rels[lb..].to_vec();
        }
        if test_pos.intersects(POS_SELF) {
            cs_insert(&self.doc.store, &mut rels, current);
        }
        if (test_pos.intersects(POS_LEFTMOST)) && !rels.is_empty() {
            let c = rels[0];
            rels.clear();
            rels.push(c);
        }
        if (test_pos.intersects(POS_RIGHTMOST)) && !rels.is_empty() {
            let c = *rels.last().unwrap();
            rels.clear();
            rels.push(c);
        }

        let mut rv: Option<CohortId> = None;
        for iter in rels {
            let mut rvs: u8 = 0;
            let (_, retval) =
                self.run_single_test(iter, test, &mut rvs, deep.as_deref_mut(), origin);
            if test_pos.intersects(POS_ALL) {
                if !retval {
                    rv = None;
                    break;
                } else {
                    rv = Some(iter);
                }
            } else if retval {
                rv = Some(iter);
                break;
            }
        }

        if rv.is_none() {
            self.scratch.context_stack.last_mut().unwrap().regexgrp_ct = regexgrpz;
        }
        rv
    }

    /// POS_BAG_OF_TAGS match against a window's embedded `bag_of_tags` reading.
    /// The reading is not an arena object, so it is cloned into the readings arena
    /// (as `does_set_match_reading` needs a `ReadingId`), matched with
    /// `bypass_index = true`, then the slot is freed. Port adaptation — the
    /// embedded-value `Reading&` of the C++ `doesSetMatchReading(sWindow->
    /// bag_of_tags, test->target, true)` has no arena identity.
    fn match_bag_of_tags(&mut self, sw: SwId, target: u32) -> bool {
        let bag = clone_reading(&self.doc.store.single_windows.get(sw.0).bag_of_tags);
        let rid = self.doc.store.readings.alloc(bag);
        let m = self.does_set_match_reading(crate::arena::ReadingId(rid), target, true, false);
        self.doc.store.readings.free_slot(rid);
        m
    }
}
