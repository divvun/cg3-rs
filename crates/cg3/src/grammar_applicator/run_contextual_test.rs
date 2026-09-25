//! `src/GrammarApplicator_runContextualTest.cpp` — the central contextual-test
//! dispatcher and its dependency/parenthesis/relation/single-test helpers,
//! implemented on the [`Matcher`] split-borrow sub-view (plan node
//! `matcher-doc-split.matcher-view`; capability contract on [`Matcher`]).
//! Literal, bug-for-bug port.
//!
//! SIBLING methods CALLED here but DEFINED in other partials (also on
//! `impl Matcher`):
//!
//! - match_set: `does_set_match_cohort_normal` / `does_set_match_cohort_careful`
//!   (`(&mut self, cohort: CohortId, set: u32, context: Option<&mut
//!   dSMC_Context>) -> bool`), `does_set_match_reading` (`(&mut self, reading:
//!   ReadingId, set: u32, bypass_index: bool, unif_mode: bool) -> bool`),
//!   `does_tag_match_regexp` (`(&mut self, test: u32, tag: &Tag, bypass_index:
//!   bool) -> u32`).
//! - context: `get_mark` (`(&self) -> Option<CohortId>`), `get_attach_to`
//!   (`(&self) -> ReadingSpec`, uses `.cohort`), `set_mark` (`(&mut self,
//!   Option<CohortId>)`).
//! - reflow: `generate_varstring_tag` (`(&mut self, &Tag) -> TagId`).
//! - run_rules: `get_sub_reading` (`(&mut self, ReadingId, i32) ->
//!   Option<ReadingId>`) — used indirectly via the cohort matchers, not called
//!   here.
//!
//! EXPOSED here (match_set calls it directly; run_rules through the `Engine`
//! forwarder in mod.rs):
//!
//! - `run_contextual_test(&mut self, sw: Option<SwId>, position: u32, test:
//!   CtxId, deep: Option<&mut Option<CohortId>>, origin: Option<CohortId>) ->
//!   Option<CohortId>` — the exact shape match_set.rs already calls
//!   (`self.run_contextual_test(cparent, clocal, l, context.deep, Some(cohort))?`),
//!   where `cparent: Option<SwId>`, `clocal: u32` (a cohort's `local_number`).
//!
//! ARENA-MODEL / SIGNATURE NOTES
//! * `SingleWindow*& sWindow` (a by-reference, reassignable pointer) → an
//!   `Option<SwId>` LOCAL (`sw`): reassignments hop windows exactly as the C++
//!   does, and never escape. `size_t position` → `u32` (a cohort `local_number`).
//! * `sWindow->parent->cohort_map` (the owning `Window`'s map) → the applicator's
//!   inline `self.registry.cohort_map` — the port holds one `Window` per engine.
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

use crate::arena::{CohortId, CtxId, GenArena, SwId};
use crate::cohort::{CT_RELATED, CT_REMOVED, Cohort};
use crate::cohort_iterator::{
    CohortIterator, DepAncestorIter, DepDescendentIter, DepParentIter, IterArenas,
    TopologyLeftIter, TopologyRightIter,
};
use crate::contextual_test::PosJumpPos::{JumpAttach, JumpMark, JumpTarget};
use crate::contextual_test::{
    MASK_POS_LORR, MASK_SELF_NB, POS_ABSOLUTE, POS_ALL, POS_ATTACH_TO, POS_BAG_OF_TAGS,
    POS_CAREFUL, POS_DEP_CHILD, POS_DEP_DEEP, POS_DEP_GLOB, POS_DEP_PARENT, POS_DEP_SIBLING,
    POS_JUMP, POS_LEFT, POS_LEFT_PAR, POS_LEFTMOST, POS_LOOK_DELAYED, POS_LOOK_DELETED,
    POS_LOOK_IGNORED, POS_MARK_SET, POS_NEGATE, POS_NONE, POS_NOT, POS_PASS_ORIGIN, POS_RELATION,
    POS_RIGHT, POS_RIGHT_PAR, POS_RIGHTMOST, POS_SCANALL, POS_SCANFIRST, POS_SELF, POS_SPAN_BOTH,
    POS_SPAN_LEFT, POS_SPAN_RIGHT, POS_TMPL_OVERRIDE, POS_UNKNOWN, POS_WITH, PosFlags,
    TestOverride, TestRef,
};
use crate::inlines::{make_64, si32};
use crate::single_window::{SingleWindow, less_cohort};
use crate::tag::T_VARSTRING;
use crate::types::GlobalNumber;

use crate::sorted_vector::Uint32SortedVector;

use super::{CohortMatchContext, Matcher, TRV_BARRIER, TRV_BREAK, TRV_BREAK_DEFAULT};

/// Which iterator pool `runContextualTest` selected for the generic-iterator
/// arm (the C++ `CohortIterator* it`). The pools have incompatible concrete
/// `advance`/`current` signatures, so instead of a base pointer we remember the
/// choice + its `ci_depths` key and re-dispatch. `Left`/`Right` advance on the
/// cohort arena + grammar; the dep-parent iterator additionally takes the
/// single-window arena + registry and MUTATES on advance (its `m_seen` cycle
/// guard); the two precomputed dep iterators (`Glob`/`Ancestor`) hold their
/// own vectors.
#[derive(Copy, Clone)]
enum ItSel {
    Plain(u32),
    Left(u32),
    Right(u32),
    DepParent(u32),
    DepGlob(u32),
    DepAncestor(u32),
}

/// The `(test, deep, origin)` argument triple of C++ `runContextualTest`
/// (`const ContextualTest*`, `Cohort** deep`, `Cohort* origin`), threaded
/// intact into the extracted iterator/scan arms ([`Matcher::run_iter`],
/// [`Matcher::run_scan`]).
struct TestArgs<'a> {
    test: TestRef,
    /// C++ `Cohort** deep`.
    deep: Option<&'a mut Option<CohortId>>,
    /// C++ `Cohort* origin`.
    origin: Option<CohortId>,
}

/// A cohort whose dependents a deep `runDependencyTest` is walking: the
/// frame the C++ recursion keeps on the stack, kept on the heap.
struct DepLevel {
    current: CohortId,
    /// The dependents to test, by global number, in order.
    deps: Vec<u32>,
    /// The index in `deps` of the next one to test.
    next: usize,
    /// The result so far.
    rv: Option<CohortId>,
}

/// What testing one dependent does to a `runDependencyTest` walk.
enum DepNext {
    /// Go on to the next dependent.
    Next,
    /// The walk of this level ends with this result.
    End(Option<CohortId>),
    /// `ALL`: this one matched, and is the result unless a later one fails.
    Matched(CohortId),
    /// A deep test: walk this dependent's own dependents before the next.
    Descend(CohortId),
}

// --- Arena-aware `CohortSet` helpers (runRelationTest builds a `CohortSet`) ---
// A C++ `CohortSet` (`sorted_vector<Cohort*, compare_Cohort>`) is a
// `Vec<CohortId>`; the `compare_Cohort` order (`less_Cohort` — by `local_number`,
// tie-broken by owning-window `number`) needs the cohort/single-window arenas,
// so the sorted, dup-suppressing operations run against those arenas here
// (mirrors the private `cs_*` helpers in `cohort_iterator.rs`).

fn cs_lower_bound(
    cohorts: &GenArena<Cohort>,
    windows: &GenArena<SingleWindow>,
    v: &[CohortId],
    t: CohortId,
) -> usize {
    v.partition_point(|&x| less_cohort(cohorts, windows, x, t))
}

// Wave 4 (w4-file-split-fmt): the verbatim Reading field-copy is
// consolidated in `crate::reading::clone_verbatim`.
use crate::reading::clone_verbatim as clone_reading;

fn cs_insert(
    cohorts: &GenArena<Cohort>,
    windows: &GenArena<SingleWindow>,
    v: &mut Vec<CohortId>,
    t: CohortId,
) -> bool {
    if v.is_empty() {
        v.push(t);
        return true;
    }
    let it = cs_lower_bound(cohorts, windows, v, t);
    if it == v.len() {
        v.push(t);
        return true;
    }
    if less_cohort(cohorts, windows, v[it], t) || less_cohort(cohorts, windows, t, v[it]) {
        v.insert(it, t);
        return true;
    }
    false
}

impl Matcher<'_> {
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
        test: TestRef,
        rvs: &mut u8,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<(Option<CohortId>, bool), crate::error::RunError> {
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
            let c = &self.grammar.contexts_arena;
            (
                test.pos(c),
                c[test.id.0].target.get(),
                test.offset(c),
                test.barrier(c).get(),
                test.cbarrier(c).get(),
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
                    self.scratch.clear_matched(reading);
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
        let mut context = CohortMatchContext {
            test: Some(test.id),
            deep,
            origin,
            options: test_pos,
            did_test: false,
            matched_target: false,
            matched_tests: false,
            in_barrier: false,
        };

        if test_pos.intersects(POS_CAREFUL) {
            *retval = self.does_set_match_cohort_careful(cid, test_target, Some(&mut context))?;
            if !context.matched_target && (test_pos.intersects(POS_SCANFIRST)) {
                context.did_test = true;
                // Intentionally ignoring the return value to populate matched_target.
                self.does_set_match_cohort_normal(cid, test_target, Some(&mut context))?;
            }
        } else {
            *retval = self.does_set_match_cohort_normal(cid, test_target, Some(&mut context))?;
        }

        // origin loop-back detection.
        if let Some(org) = origin {
            let scan = test_pos.intersects(POS_SCANALL | POS_SCANFIRST);
            if (test_offset != 0 || scan)
                && Some(org) == cohort
                && self.cohorts.get(org.0).local_number != 0
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
            let mut bctx = CohortMatchContext {
                test: None,
                deep: None,
                origin: None,
                options: test_pos & !POS_CAREFUL,
                did_test: false,
                matched_target: false,
                matched_tests: false,
                in_barrier: true,
            };
            let barrier = self.does_set_match_cohort_normal(cid, test_barrier, Some(&mut bctx))?;
            if barrier {
                self.scratch.seen_barrier = true;
                *rvs |= TRV_BREAK | TRV_BARRIER;
                *rvs &= !TRV_BREAK_DEFAULT;
            }
        }
        if test_cbarrier != 0
            && let Some(cid) = cohort
        {
            let mut cbctx = CohortMatchContext {
                test: None,
                deep: None,
                origin: None,
                options: test_pos | POS_CAREFUL,
                did_test: false,
                matched_target: false,
                matched_tests: false,
                in_barrier: true,
            };
            let cbarrier =
                self.does_set_match_cohort_careful(cid, test_cbarrier, Some(&mut cbctx))?;
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
        Ok((cohort, retval_v))
    }

    /// C++ overload `Cohort* runSingleTest(SingleWindow* sWindow, size_t i, ...)`:
    /// out-of-range `i` sets `rvs |= TRV_BREAK`, returns `(None, false)`;
    /// otherwise forwards `sWindow->cohorts[i]`.
    fn run_single_test_at(
        &mut self,
        sw: SwId,
        i: i32,
        test: TestRef,
        rvs: &mut u8,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<(Option<CohortId>, bool), crate::error::RunError> {
        let len = self.single_windows.get(sw.0).cohorts.len() as i32;
        if i < 0 || i >= len {
            *rvs |= TRV_BREAK;
            return Ok((None, false));
        }
        let cohort = self.single_windows.get(sw.0).cohorts[i as usize];
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
        let c = self.cohorts.get(cohort.0);
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
        test: TestRef,
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
        let (cohorts, windows) = (&self.cohorts, &self.single_windows);
        cs.sort_by(|&a, &b| {
            if less_cohort(cohorts, windows, a, b) {
                std::cmp::Ordering::Less
            } else if less_cohort(cohorts, windows, b, a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        let (test_pos, test_offset) = {
            let c = &self.grammar.contexts_arena;
            (test.pos(c), test.offset(c))
        };

        // If the override included * or @, offsets are irrelevant.
        if test_pos.intersects(POS_SCANFIRST | POS_SCANALL | POS_ABSOLUTE) {
            good = true;
        } else {
            let cs0_ln = self.cohorts.get(cs[0].0).local_number;
            let cs3_ln = self.cohorts.get(cs[3].0).local_number;
            if (test_offset > 0 && si32(cs0_ln) - si32(position) == test_offset)
                || (test_offset < 0 && si32(cs3_ln) - si32(position) == test_offset)
            {
                good = true;
            }
        }
        // Deep result left the window (no span flag).
        if !test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT | POS_SPAN_RIGHT) {
            let cdeep_parent = self.cohorts.get(cdeep.0).parent;
            if cdeep_parent != Some(sw) {
                good = false;
            }
        }
        // Origin-straddle vetoes (raw unsigned local_number).
        if !test_pos.intersects(POS_PASS_ORIGIN) {
            let cs0_ln = self.cohorts.get(cs[0].0).local_number;
            let cs3_ln = self.cohorts.get(cs[3].0).local_number;
            if (test_offset < 0 && cs3_ln > position) || (test_offset > 0 && cs0_ln < position) {
                good = false;
            }
        }
        good
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    /// Runs one template (`tmpl`) on behalf of the outer `test`, optionally
    /// imposing the outer test's position onto the template, then validating the
    /// result. C++ `Cohort* runContextualTest_tmpl(SingleWindow*, size_t, const
    /// ContextualTest* test, ContextualTest* tmpl, Cohort*& cdeep, Cohort*
    /// origin)`. `cdeep` (the deepest reached cohort) is an out-param (`&mut`).
    ///
    /// DIVERGENCE (same behaviour, different mechanism): C++ imposes the
    /// override by WRITING `pos`/`offset`/`cbarrier`/`barrier` into the shared
    /// `tmpl` object and restoring them after the call. The template is handed to
    /// every test that names it, so that write is to state common to every such
    /// rule. The port builds a [`TestRef`] carrying those four values instead and
    /// passes it down; the arena is never touched.
    pub fn run_contextual_test_tmpl(
        &mut self,
        sw: Option<SwId>,
        position: u32,
        test: TestRef,
        tmpl: CtxId,
        cdeep: &mut Option<CohortId>,
        origin: Option<CohortId>,
    ) -> Result<Option<CohortId>, crate::error::RunError> {
        let min = self.scratch.tmpl_cntx.min;
        let max = self.scratch.tmpl_cntx.max;
        let in_template = self.scratch.tmpl_cntx.in_template;
        self.scratch.tmpl_cntx.in_template = true;

        let test_linked = self.grammar.contexts_arena[test.id.0].linked;
        if let Some(l) = test_linked {
            self.scratch.tmpl_cntx.linked.push(l);
        }

        // The OUTER test's EFFECTIVE fields — already overridden themselves when
        // this is a nested template call, which is exactly what the C++ reads
        // back out of the arena at this point.
        let (test_pos, test_offset, test_cbarrier, test_barrier) = {
            let c = &self.grammar.contexts_arena;
            (
                test.pos(c),
                test.offset(c),
                test.cbarrier(c),
                test.barrier(c),
            )
        };

        let override_applied = test_pos.intersects(POS_TMPL_OVERRIDE);
        let tmpl_ref = if override_applied {
            let c = &self.grammar.contexts_arena;
            let mut pos = test_pos;
            pos &= !(POS_NEGATE | POS_NOT | POS_JUMP);
            if test_offset != 0 && !test_pos.intersects(POS_SCANFIRST | POS_SCANALL | POS_ABSOLUTE)
            {
                pos |= POS_SCANALL;
            }
            // The two barriers are only imposed when the outer test HAS one;
            // otherwise the template keeps its own (C++ `if (test->cbarrier)`).
            let cbarrier = if test_cbarrier.get() != 0 {
                test_cbarrier
            } else {
                c[tmpl.0].cbarrier
            };
            let barrier = if test_barrier.get() != 0 {
                test_barrier
            } else {
                c[tmpl.0].barrier
            };
            TestRef::overridden(
                tmpl,
                TestOverride {
                    pos,
                    offset: test_offset,
                    barrier,
                    cbarrier,
                },
            )
        } else {
            TestRef::new(tmpl)
        };

        // cohort = runContextualTest(sWindow, position, tmpl, &cdeep, origin)
        let mut cohort =
            self.run_template_test(test.id, sw, position, tmpl_ref, Some(&mut *cdeep), origin)?;

        if override_applied
            && let (Some(c), Some(cd)) = (cohort, *cdeep)
            && test_offset != 0
        {
            let sw_id = sw.expect(
                "runContextualTest_tmpl: posOutputHelper needs a window but sWindow is null",
            );
            if !self.pos_output_helper(sw_id, position, test, c, cd) {
                cohort = None;
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

        Ok(cohort)
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// The central contextual-test dispatcher. C++ `Cohort*
    /// runContextualTest(SingleWindow* sWindow, size_t position, const
    /// ContextualTest*, Cohort** deep, Cohort* origin)`. Returns the matched
    /// cohort, `None` on failure, or `sWindow->cohorts[0]` as a truthy
    /// success-with-no-cohort sentinel.
    pub fn run_contextual_test(
        &mut self,
        sw: Option<SwId>,
        position: u32,
        test: TestRef,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<Option<CohortId>, crate::error::RunError> {
        let mut sw = sw;
        let mut position = position;
        let mut origin = origin;

        let test_pos = test.pos(&self.grammar.contexts_arena);
        if test_pos.intersects(POS_UNKNOWN) {
            // C++: print the error, then CG3Quit(1). A textual grammar is refused
            // for this when it is parsed; a compiled one can still get here.
            let line = self.grammar.contexts_arena[test.id.0].line;
            return Err(crate::error::RunError::PositionWithoutOverride { line });
        }

        let mut cohort: Option<CohortId> = None;
        let mut retval = true;

        if test_pos.intersects(POS_JUMP) {
            let jump_pos = self.grammar.contexts_arena[test.id.0].jump_pos;
            let mut j: Option<CohortId> = None;
            if jump_pos == JumpMark as i8 {
                j = self.get_mark();
            } else if jump_pos == JumpAttach as i8 {
                j = self.get_attach_to().cohort;
            } else if jump_pos == JumpTarget as i8 {
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
                let c = self.cohorts.get(jc.0);
                sw = c.parent;
                position = c.local_number;
            } else {
                retval = false;
            }
        }
        // The window `position` counts in, for the SELF probe: the jump
        // target's once a jump moved it. DIVERGENCE: the C++ `orgSWin` is taken
        // before the jump, and indexed the window the test left with a position
        // from the one it jumped to.
        let self_swin = sw;

        let test_offset = test.offset(&self.grammar.contexts_arena);
        // [spec:cg3:req:robustness.checked-arithmetic]
        // Saturates: an offset near i32::MAX lands outside every window, as
        // it means to; the C++ overflows (undefined behaviour).
        let mut pos = si32(position).saturating_add(test_offset);

        if !retval {
            // Jump failed because the position does not exist.
            return Ok(self.finalize_got_a_cohort(sw, test, cohort, retval));
        }

        let test_tmpl = self.grammar.contexts_arena[test.id.0].tmpl;
        let has_ors = !self.grammar.contexts_arena[test.id.0].ors.is_empty();

        if let Some(tmpl) = test_tmpl {
            let mut cdeep: Option<CohortId> = None;
            cohort = self.run_contextual_test_tmpl(sw, position, test, tmpl, &mut cdeep, origin)?;
            if let Some(d) = deep.as_deref_mut() {
                *d = cdeep;
            }
        } else if has_ors {
            let mut cdeep: Option<CohortId> = None;
            let ors = self.grammar.contexts_arena[test.id.0].ors.clone();
            for iter in ors {
                self.scratch.dep_deep_seen.clear();
                cohort =
                    self.run_contextual_test_tmpl(sw, position, test, iter, &mut cdeep, origin)?;
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
                origin = Some(self.single_windows.get(sw_id.0).cohorts[0]);
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
                let iter = DepAncestorIter::new(
                    Some(cid),
                    Some(test),
                    self.cfg.always_span,
                    self.split_for_iters(),
                );
                self.scratch.dep_ancestor_iters.insert(key, iter);
                it = Some(ItSel::DepAncestor(key));
            } else if test_pos.intersects(POS_DEP_PARENT) {
                let key = self.scratch.ci_depths[3];
                self.scratch.ci_depths[3] += 1;
                let iter = DepParentIter::new(
                    Some(cid),
                    Some(test),
                    self.cfg.always_span,
                    self.split_for_iters(),
                );
                self.scratch.dep_parent_iters.insert(key, iter);
                it = Some(ItSel::DepParent(key));
            } else if test_pos.intersects(POS_DEP_GLOB) {
                let key = self.scratch.ci_depths[4];
                self.scratch.ci_depths[4] += 1;
                let iter = DepDescendentIter::new(
                    Some(cid),
                    Some(test),
                    self.cfg.always_span,
                    self.split_for_iters(),
                );
                self.scratch.dep_descendent_iters.insert(key, iter);
                it = Some(ItSel::DepGlob(key));
            } else if test_pos.intersects(POS_DEP_CHILD | POS_DEP_SIBLING) {
                let nc =
                    self.run_dependency_test(sw_id, cid, test, deep.as_deref_mut(), origin, None)?;
                if let Some(nc) = nc {
                    cohort = Some(nc);
                    retval = true;
                    sw = self.cohorts.get(nc.0).parent;
                } else {
                    retval = false;
                }
                if test_pos.intersects(POS_NONE) {
                    retval = !retval;
                }
            } else if test_pos.intersects(POS_LEFT_PAR | POS_RIGHT_PAR) {
                let nc =
                    self.run_parenthesis_test(sw_id, cid, test, deep.as_deref_mut(), origin)?;
                if let Some(nc) = nc {
                    cohort = Some(nc);
                    retval = true;
                } else {
                    retval = false;
                }
            } else if test_pos.intersects(POS_RELATION) {
                let nc = self.run_relation_test(sw_id, cid, test, deep.as_deref_mut(), origin)?;
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
                let test_target = self.grammar.contexts_arena[test.id.0].target.get();
                let mut m = self.match_bag_of_tags(sw_id, test_target)?;
                if !m && (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT | POS_SPAN_RIGHT)) {
                    let mut left = self.single_windows.get(sw_id.0).previous;
                    let mut right = self.single_windows.get(sw_id.0).next;
                    while left.is_some() || right.is_some() {
                        if left.is_some() && (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT)) {
                            let lw = left.unwrap();
                            m = self.match_bag_of_tags(lw, test_target)?;
                            left = self.single_windows.get(lw.0).previous;
                        } else {
                            left = None;
                        }
                        if right.is_some() && (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT))
                        {
                            let rw = right.unwrap();
                            m = self.match_bag_of_tags(rw, test_target)?;
                            right = self.single_windows.get(rw.0).next;
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
                    let test_linked = self.grammar.contexts_arena[test.id.0].linked;
                    if let Some(l) = test_linked {
                        // A LINK target is its own test object: the C++ arena
                        // write never reached it, so no override travels here.
                        cohort = self.run_linked_test(
                            sw,
                            position,
                            TestRef::new(l),
                            deep.as_deref_mut(),
                            origin,
                        )?;
                    }
                } else {
                    retval = false;
                }
            } else if test_offset == 0 && (test_pos.intersects(POS_SCANFIRST | POS_SCANALL)) {
                // Symmetric bidirectional scan.
                let args = TestArgs {
                    test,
                    deep: deep.as_deref_mut(),
                    origin,
                };
                let (c, rv) = self.run_scan(sw_id, cid, pos, args, retval)?;
                cohort = c;
                retval = rv;
            } else if test_offset < 0 {
                let key = self.scratch.ci_depths[1];
                self.scratch.ci_depths[1] += 1;
                let iter = TopologyLeftIter::new(Some(cid), Some(test), self.cfg.always_span);
                self.scratch.topology_left_iters.insert(key, iter);
                it = Some(ItSel::Left(key));
            } else if test_offset > 0 {
                let key = self.scratch.ci_depths[2];
                self.scratch.ci_depths[2] += 1;
                let iter = TopologyRightIter::new(Some(cid), Some(test), self.cfg.always_span);
                self.scratch.topology_right_iters.insert(key, iter);
                it = Some(ItSel::Right(key));
            } else {
                let key = self.scratch.ci_depths[0];
                self.scratch.ci_depths[0] += 1;
                let iter = CohortIterator::new(Some(cid), Some(test), self.cfg.always_span);
                self.scratch.cohort_iterators.insert(key, iter);
                it = Some(ItSel::Plain(key));
            }

            if let Some(sel) = it {
                let args = TestArgs { test, deep, origin };
                let (c, rv) = self.run_iter(sel, self_swin, position, cid, args, retval)?;
                cohort = c;
                retval = rv;
            }
        }

        Ok(self.finalize_got_a_cohort(sw, test, cohort, retval))
    }

    /// C++ `label_gotACohort:` finalize block of `runContextualTest`.
    fn finalize_got_a_cohort(
        &self,
        sw: Option<SwId>,
        test: TestRef,
        mut cohort: Option<CohortId>,
        mut retval: bool,
    ) -> Option<CohortId> {
        let (test_pos, test_linked) = {
            let c = &self.grammar.contexts_arena;
            (test.pos(c), c[test.id.0].linked)
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
            cohort = Some(self.single_windows.get(sw_id.0).cohorts[0]);
        }
        cohort
    }

    /// C++ `tmpl_cntx.min`/`.max` extension for a matched cohort (the inline
    /// `make_64(parent->number, local_number)` bound update).
    fn extend_tmpl_bounds(&mut self, c: CohortId) {
        let (cwin, cln) = {
            let co = self.cohorts.get(c.0);
            let win = self.single_windows.get(co.parent.unwrap().0).number;
            (win, co.local_number)
        };
        let gpos = make_64(cwin, cln);
        let min_gpos = self.scratch.tmpl_cntx.min.map(|m| {
            let mo = self.cohorts.get(m.0);
            make_64(
                self.single_windows.get(mo.parent.unwrap().0).number,
                mo.local_number,
            )
        });
        if min_gpos.is_none() || gpos < min_gpos.unwrap() {
            self.scratch.tmpl_cntx.min = Some(c);
        }
        let max_gpos = self.scratch.tmpl_cntx.max.map(|m| {
            let mo = self.cohorts.get(m.0);
            make_64(
                self.single_windows.get(mo.parent.unwrap().0).number,
                mo.local_number,
            )
        });
        if max_gpos.is_none() || gpos > max_gpos.unwrap() {
            self.scratch.tmpl_cntx.max = Some(c);
        }
    }

    /// Split `self` into the [`IterArenas`] view the dep iterators dereference
    /// (cohort arena, single-window arena, grammar, cohort registry) without
    /// aliasing — the iterator pools live on `self` separately from these fields.
    fn split_for_iters(&self) -> IterArenas<'_> {
        IterArenas {
            cohorts: self.cohorts,
            windows: self.single_windows,
            grammar: self.grammar,
            registry: self.registry,
        }
    }

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// The POS_SELF probe of [`Self::run_iter`]: run the test on the cohort at
    /// `position` in `sw`, the window that position counts in. A position the
    /// window does not reach fails the probe (the C++ asserted it could not
    /// happen, and read out of bounds in a release build).
    fn run_self_probe(
        &mut self,
        sw: Option<SwId>,
        position: u32,
        test: TestRef,
        rvs: &mut u8,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<(Option<CohortId>, bool), crate::error::RunError> {
        let window = sw.map(|w| &self.single_windows.get(w.0).cohorts);
        match window.and_then(|cohorts| cohorts.get(position as usize).copied()) {
            Some(self_c) => self.run_single_test(self_c, test, rvs, deep, origin),
            None => Ok((None, false)),
        }
    }

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// The C++ generic-iterator arm (`if (it) { ... }`): resets nothing here (the
    /// port ctors already seat the iterator), runs the optional POS_SELF probe,
    /// then walks the iterator to the null sentinel. Returns `(cohort, retval)`.
    fn run_iter(
        &mut self,
        sel: ItSel,
        self_swin: Option<SwId>,
        position: u32,
        cohort: CohortId,
        args: TestArgs<'_>,
        mut retval: bool,
    ) -> Result<(Option<CohortId>, bool), crate::error::RunError> {
        let TestArgs {
            test,
            mut deep,
            origin,
        } = args;
        let test_pos = test.pos(&self.grammar.contexts_arena);

        let mut nc: Option<CohortId> = None;
        let mut rvs: u8 = 0;
        let mut seen: usize = 0;

        // POS_SELF probe on the origin cohort.
        let self_probe = (test_pos.intersects(POS_SELF))
            && (!test_pos.intersects(MASK_POS_LORR)
                || ((test_pos.intersects(POS_DEP_PARENT)) && (!test_pos.intersects(POS_DEP_GLOB))));
        if self_probe {
            seen += 1;
            (nc, retval) = self.run_self_probe(
                self_swin,
                position,
                test,
                &mut rvs,
                deep.as_deref_mut(),
                origin,
            )?;
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
                if (test_pos.intersects(POS_LEFT))
                    && less_cohort(self.cohorts, self.single_windows, current, itc)
                {
                    nc = None;
                    retval = false;
                    break;
                }
                if (test_pos.intersects(POS_RIGHT))
                    && !less_cohort(self.cohorts, self.single_windows, current, itc)
                {
                    nc = None;
                    retval = false;
                    break;
                }
                (nc, retval) =
                    self.run_single_test(itc, test, &mut rvs, deep.as_deref_mut(), origin)?;
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
        Ok((nc, retval))
    }

    /// C++ `**it` — the iterator's current cohort (dispatch by selected pool).
    fn iter_current(&self, sel: ItSel) -> Option<CohortId> {
        match sel {
            ItSel::Plain(k) => self
                .scratch
                .cohort_iterators
                .get(&k)
                .and_then(|i| i.current()),
            ItSel::Left(k) => self
                .scratch
                .topology_left_iters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::Right(k) => self
                .scratch
                .topology_right_iters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::DepParent(k) => self
                .scratch
                .dep_parent_iters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::DepGlob(k) => self
                .scratch
                .dep_descendent_iters
                .get(&k)
                .and_then(|i| i.base.current()),
            ItSel::DepAncestor(k) => self
                .scratch
                .dep_ancestor_iters
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
                if let Some(i) = self.scratch.cohort_iterators.get_mut(&k) {
                    i.advance();
                }
            }
            ItSel::Left(k) => {
                if let Some(i) = self.scratch.topology_left_iters.get_mut(&k) {
                    i.advance(self.cohorts, self.grammar);
                }
            }
            ItSel::Right(k) => {
                if let Some(i) = self.scratch.topology_right_iters.get_mut(&k) {
                    i.advance(self.cohorts, self.grammar);
                }
            }
            ItSel::DepParent(k) => {
                if let Some(i) = self.scratch.dep_parent_iters.get_mut(&k) {
                    i.advance(IterArenas {
                        cohorts: self.cohorts,
                        windows: self.single_windows,
                        grammar: self.grammar,
                        registry: self.registry,
                    });
                }
            }
            ItSel::DepGlob(k) => {
                if let Some(i) = self.scratch.dep_descendent_iters.get_mut(&k) {
                    i.advance();
                }
            }
            ItSel::DepAncestor(k) => {
                if let Some(i) = self.scratch.dep_ancestor_iters.get_mut(&k) {
                    i.advance();
                }
            }
        }
    }

    /// The `test->offset == 0 && (SCANFIRST|SCANALL)` bidirectional scan arm.
    /// Returns `(cohort, retval)`; the C++ `goto label_gotACohort` short-circuits
    /// become early returns of the current `(cohort, retval)`.
    fn run_scan(
        &mut self,
        sw: SwId,
        start_cohort: CohortId,
        pos: i32,
        args: TestArgs<'_>,
        mut retval: bool,
    ) -> Result<(Option<CohortId>, bool), crate::error::RunError> {
        let TestArgs {
            test,
            mut deep,
            origin,
        } = args;
        let test_pos = test.pos(&self.grammar.contexts_arena);

        let mut right: Option<SwId> = Some(sw);
        let mut left: Option<SwId> = Some(sw);
        let mut rpos: i32 = pos;
        let mut lpos: i32 = pos;

        let mut cohort: Option<CohortId> = Some(start_cohort);
        let mut rvs: u8 = 0;

        if test_pos.intersects(POS_SELF) {
            (cohort, retval) =
                self.run_single_test(start_cohort, test, &mut rvs, deep.as_deref_mut(), origin)?;
            if !retval && (rvs & TRV_BREAK_DEFAULT != 0) {
                rvs &= !(TRV_BREAK | TRV_BREAK_DEFAULT);
            }
        }
        if (rvs & TRV_BREAK != 0) && retval {
            return Ok((cohort, retval));
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
                )?;
                if (rvs & TRV_BREAK != 0) && retval {
                    return Ok((cohort, retval));
                } else if rvs & TRV_BREAK != 0 {
                    left = None;
                    if test_pos.intersects(POS_NOT) {
                        right = None;
                    }
                } else if lpos - i == 0 {
                    if (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT)) || self.cfg.always_span
                    {
                        left = self.single_windows.get(lw.0).previous;
                        if let Some(nl) = left {
                            lpos = i + self.single_windows.get(nl.0).cohorts.len() as i32;
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
                )?;
                if (rvs & TRV_BREAK != 0) && retval {
                    return Ok((cohort, retval));
                } else if rvs & TRV_BREAK != 0 {
                    right = None;
                    if test_pos.intersects(POS_NOT) {
                        left = None;
                    }
                } else {
                    let rlen = self.single_windows.get(rw.0).cohorts.len() as i32;
                    if rpos + i == rlen - 1 {
                        if (test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT))
                            || self.cfg.always_span
                        {
                            right = self.single_windows.get(rw.0).next;
                            rpos = (0 - i) - 1;
                        } else {
                            right = None;
                        }
                    }
                }
            }
            i += 1;
        }
        Ok((cohort, retval))
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.get-cohort-in-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.get-cohort-in-window-fn]
    /// C++ free fn `Cohort* getCohortInWindow(SingleWindow*& sWindow, size_t
    /// position, const ContextualTest*, int32_t& pos)`. Resolves a plain
    /// positional test to a concrete cohort, hopping at most one window boundary
    /// (an overshoot yields `None`). `sWindow`/`pos` are in/out (`&mut`). Ported
    /// as a method purely to reach `self.single_windows`/`self.grammar` (no
    /// `self` state is otherwise touched).
    pub fn get_cohort_in_window(
        &self,
        sw: &mut Option<SwId>,
        position: u32,
        test: TestRef,
        pos: &mut i32,
    ) -> Option<CohortId> {
        let mut cohort: Option<CohortId> = None;
        let (test_pos, test_offset) = {
            let c = &self.grammar.contexts_arena;
            (test.pos(c), test.offset(c))
        };
        *pos = si32(position).saturating_add(test_offset);

        let cur = sw.expect("getCohortInWindow: sWindow is null");

        if (test_pos.intersects(POS_ABSOLUTE))
            && (test_pos.intersects(POS_SPAN_LEFT | POS_SPAN_RIGHT))
        {
            let prev = self.single_windows.get(cur.0).previous;
            let next = self.single_windows.get(cur.0).next;
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
                *pos = self.single_windows.get(cur.0).cohorts.len() as i32 + test_offset;
            } else {
                *pos = test_offset;
            }
        }

        let cur_len = self.single_windows.get(cur.0).cohorts.len() as i32;
        if *pos >= 0 {
            if *pos >= cur_len
                && (test_pos.intersects(POS_SPAN_RIGHT | POS_SPAN_BOTH))
                && self.single_windows.get(cur.0).next.is_some()
            {
                cur = self.single_windows.get(cur.0).next.unwrap();
                *sw = Some(cur);
                *pos = 0;
            }
        } else {
            if (test_pos.intersects(POS_SPAN_LEFT | POS_SPAN_BOTH))
                && self.single_windows.get(cur.0).previous.is_some()
            {
                cur = self.single_windows.get(cur.0).previous.unwrap();
                *sw = Some(cur);
                *pos = self.single_windows.get(cur.0).cohorts.len() as i32 - 1;
            }
        }

        let cur_len = self.single_windows.get(cur.0).cohorts.len() as i32;
        if *pos >= 0 && *pos < cur_len {
            cohort = Some(self.single_windows.get(cur.0).cohorts[*pos as usize]);
        }
        cohort
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:req:robustness.depth-bounded]
    /// Traverses dependency children/parents/siblings from `current`, testing
    /// each, optionally recursing (deep). C++ `Cohort* runDependencyTest(
    /// SingleWindow*, Cohort* current, const ContextualTest*, Cohort** deep,
    /// Cohort* origin, const Cohort* self)`.
    ///
    /// A deep test (`c*`, `s*`) walks the dependency tree depth first, which
    /// the C++ does by recursing once per level of the tree: as deep as the
    /// input's dependency chain. Here each cohort whose dependents are being
    /// walked is a level on a heap stack instead, entered where the C++
    /// recurses and left where it returns, so the cohorts are tested in the
    /// same order and a chain of any length costs no stack.
    pub fn run_dependency_test(
        &mut self,
        // C++ reads `sWindow->parent->cohort_map` throughout, which is the
        // applicator's single inline `self.registry` in the port; the `sWindow`
        // argument is therefore unused here (kept to mirror the C++ signature).
        _sw: SwId,
        current: CohortId,
        test: TestRef,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
        self_cohort: Option<CohortId>,
    ) -> Result<Option<CohortId>, crate::error::RunError> {
        let selfc = self_cohort.unwrap_or(current);
        let mut levels: Vec<DepLevel> = Vec::new();
        let mut finished = self.dep_test_enter(
            &mut levels,
            current,
            self_cohort,
            test,
            deep.as_deref_mut(),
            origin,
        )?;
        loop {
            // A level that has finished hands its result to the one above,
            // which ends with it if it found a cohort.
            if let Some(result) = finished.take() {
                let Some(parent) = levels.last_mut() else {
                    return Ok(result);
                };
                if result.is_some() {
                    parent.rv = result;
                    parent.next = parent.deps.len();
                }
            }
            let Some(level) = levels.last_mut() else {
                return Ok(None);
            };
            let Some(&dter) = level.deps.get(level.next) else {
                finished = levels.pop().map(|done| done.rv);
                continue;
            };
            level.next += 1;
            let at = level.current;
            match self.dep_test_next(at, dter, test, deep.as_deref_mut(), origin)? {
                DepNext::Next => {}
                DepNext::End(rv) => {
                    if let Some(level) = levels.last_mut() {
                        level.rv = rv;
                        level.next = level.deps.len();
                    }
                }
                DepNext::Matched(cohort) => {
                    if let Some(level) = levels.last_mut() {
                        level.rv = Some(cohort);
                    }
                }
                DepNext::Descend(cohort) => {
                    finished = self.dep_test_enter(
                        &mut levels,
                        cohort,
                        Some(selfc),
                        test,
                        deep.as_deref_mut(),
                        origin,
                    )?;
                }
            }
        }
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    /// What `runDependencyTest` does on entry for `current`, before it walks
    /// the cohort's dependents: the cycle and self checks, the `SELF` test,
    /// and the choice of dependents. Pushes a level for the dependents to walk,
    /// or returns the result when the entry already decides it.
    fn dep_test_enter(
        &mut self,
        levels: &mut Vec<DepLevel>,
        current: CohortId,
        self_cohort: Option<CohortId>,
        test: TestRef,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<Option<Option<CohortId>>, crate::error::RunError> {
        if self_cohort == Some(current) {
            return Ok(Some(None));
        }

        let (test_pos, test_hash) = {
            let c = &self.grammar.contexts_arena;
            (test.pos(c), c[test.id.0].hash)
        };

        if test_pos.intersects(POS_DEP_DEEP) {
            let key = (test_hash, self.cohorts.get(current.0).global_number.get());
            if self.scratch.dep_deep_seen.contains(key) {
                return Ok(Some(None));
            }
            self.scratch.dep_deep_seen.insert(key);
        }

        if (test_pos.intersects(POS_SELF)) && (!test_pos.intersects(MASK_POS_LORR)) {
            let mut rvs: u8 = 0;
            let (tmc, retval) = self.run_single_test(current, test, &mut rvs, deep, origin)?;
            if retval {
                return Ok(Some(tmc));
            }
            if rvs & TRV_BARRIER != 0 {
                return Ok(Some(None));
            }
        }

        let Some(deps) = self.dep_test_dependents(current, test_pos) else {
            return Ok(Some(None));
        };
        levels.push(DepLevel {
            current,
            deps: self.dep_test_ordered(current, test_pos, deps),
            next: 0,
            rv: None,
        });
        Ok(None)
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    /// The global numbers of the dependents `runDependencyTest` walks from
    /// `current`: its children, or its siblings — `None` when a sibling test
    /// finds the cohort has none.
    fn dep_test_dependents(&self, current: CohortId, test_pos: PosFlags) -> Option<Vec<u32>> {
        if test_pos.intersects(POS_DEP_CHILD) {
            return Some(self.cohorts.get(current.0).dep_children.as_slice().to_vec());
        }
        if self.cohorts.get(current.0).dep_parent == Some(GlobalNumber(0)) {
            let parent_sw = self.cohorts.get(current.0).parent.unwrap();
            let root = self.single_windows.get(parent_sw.0).cohorts[0];
            return Some(self.cohorts.get(root.0).dep_children.as_slice().to_vec());
        }
        let dep_parent = self.cohorts.get(current.0).dep_parent;
        let mapped = dep_parent
            .and_then(|dp| self.registry.cohort_map.get(&dp))
            .copied();
        match mapped {
            Some(pc) if !self.cohorts.get(pc.0).dep_children.empty() => {
                Some(self.cohorts.get(pc.0).dep_children.as_slice().to_vec())
            }
            _ => {
                if self.cfg.verbosity_level > 0 {
                    let (ds, dp) = {
                        let c = self.cohorts.get(current.0);
                        (c.dep_self, c.dep_parent)
                    };
                    tracing::warn!(
                        "Warning: Cohort {} (parent {}) did not have any siblings.",
                        ds.map_or(0, |g| g.get()),
                        dp.map_or(crate::cohort::DEP_NO_PARENT, |g| g.get())
                    );
                }
                None
            }
        }
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    /// `deps` in the order `runDependencyTest` walks them: as they are, or,
    /// for a test with a left/right position, rebuilt from the whole cohort map
    /// (the slower container) in cohort order.
    fn dep_test_ordered(&self, current: CohortId, test_pos: PosFlags, deps: Vec<u32>) -> Vec<u32> {
        if !test_pos.intersects(MASK_POS_LORR) {
            return deps;
        }
        let mut tmp_deps = Uint32SortedVector::new();
        let map: Vec<CohortId> = self.registry.cohort_map.values().copied().collect();
        for citer in map {
            let gnum = self.cohorts.get(citer.0).global_number.get();
            if deps.contains(&gnum) {
                if test_pos.intersects(POS_LEFT) {
                    if less_cohort(self.cohorts, self.single_windows, citer, current) {
                        tmp_deps.insert(gnum);
                    }
                } else if test_pos.intersects(POS_RIGHT) {
                    if less_cohort(self.cohorts, self.single_windows, current, citer) {
                        tmp_deps.insert(gnum);
                    }
                } else {
                    tmp_deps.insert(gnum);
                }
            }
        }
        if test_pos.intersects(POS_SELF) {
            let gnum = self.cohorts.get(current.0).global_number.get();
            tmp_deps.insert(gnum);
        }
        let mut tmp_vec = tmp_deps.as_slice().to_vec();
        if (test_pos.intersects(POS_RIGHTMOST)) && !tmp_vec.is_empty() {
            tmp_vec.reverse();
        }
        tmp_vec
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
    /// One dependent, `dter`, of `current` in a `runDependencyTest` walk: test
    /// it, and say what that does to the walk.
    fn dep_test_next(
        &mut self,
        current: CohortId,
        dter: u32,
        test: TestRef,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<DepNext, crate::error::RunError> {
        let test_pos = test.pos(&self.grammar.contexts_arena);
        let cur_gnum = self.cohorts.get(current.0).global_number.get();
        if dter == cur_gnum && (!test_pos.intersects(POS_SELF)) {
            return Ok(DepNext::Next);
        }
        let mapped = self.registry.cohort_map.get(&GlobalNumber(dter)).copied();
        let Some(cohort) = mapped else {
            if self.cfg.verbosity_level > 0 {
                let ds = self.cohorts.get(current.0).dep_self.map_or(0, |g| g.get());
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
            return Ok(DepNext::Next);
        };
        if self.cohorts.get(cohort.0).r#type.intersects(CT_REMOVED) {
            return Ok(DepNext::Next);
        }
        let mut retval = false;
        let mut rvs: u8 = 0;
        if self.dep_in_span(current, cohort, test_pos) {
            (_, retval) = self.run_single_test(cohort, test, &mut rvs, deep, origin)?;
        }
        Ok(if test_pos.intersects(POS_ALL) {
            if !retval {
                DepNext::End(None)
            } else {
                DepNext::Matched(cohort)
            }
        } else if retval {
            DepNext::End(Some(cohort))
        } else if rvs & TRV_BARRIER != 0 {
            DepNext::Next
        } else if test_pos.intersects(POS_DEP_DEEP) {
            DepNext::Descend(cohort)
        } else {
            DepNext::Next
        })
    }

    /// Whether `runDependencyTest` tests `cohort`, a dependent of `current`:
    /// always in the same window, and in another only where the test spans
    /// that way.
    fn dep_in_span(&self, current: CohortId, cohort: CohortId, test_pos: PosFlags) -> bool {
        let (cur_parent, coh_parent) = {
            (
                self.cohorts.get(current.0).parent,
                self.cohorts.get(cohort.0).parent,
            )
        };
        if cur_parent == coh_parent {
            return true;
        }
        let cur_win = self.single_windows.get(cur_parent.unwrap().0).number;
        let coh_win = self.single_windows.get(coh_parent.unwrap().0).number;
        !(((!test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT)) && coh_win < cur_win)
            || ((!test_pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT)) && coh_win > cur_win))
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
        test: TestRef,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<Option<CohortId>, crate::error::RunError> {
        let ln = self.cohorts.get(current.0).local_number;
        if ln < self.scratch.par_left_pos || ln > self.scratch.par_right_pos {
            return Ok(None);
        }
        let mut rv: Option<CohortId> = None;

        let mut rvs: u8 = 0;
        let test_pos = test.pos(&self.grammar.contexts_arena);
        let cohort = if test_pos.intersects(POS_LEFT_PAR) {
            self.single_windows.get(sw.0).cohorts[self.scratch.par_left_pos as usize]
        } else {
            self.single_windows.get(sw.0).cohorts[self.scratch.par_right_pos as usize]
        };
        let (_, retval) = self.run_single_test(cohort, test, &mut rvs, deep, origin)?;
        if retval {
            rv = Some(cohort);
        }
        Ok(rv)
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
        // is the applicator's single inline `self.registry` in the port — so the
        // window parameter is unused here (kept to mirror the C++ signature).
        _sw: SwId,
        current: CohortId,
        test: TestRef,
        mut deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<Option<CohortId>, crate::error::RunError> {
        {
            let c = self.cohorts.get(current.0);
            if (!c.r#type.intersects(CT_RELATED)) || c.relations.is_empty() {
                return Ok(None);
            }
        }

        let mut rels: Vec<CohortId> = Vec::new();
        let regexgrpz = self.scratch.context_stack.last().unwrap().regexgrp_ct;

        let test_relation = self.grammar.contexts_arena[test.id.0].relation;
        // rtag = grammar->single_tags[test->relation]; while T_VARSTRING, expand.
        let mut rtag_id = {
            let it = self.grammar.single_tags().find(test_relation);
            it.get().1
        };
        loop {
            let ttype = self.grammar.tag_type(rtag_id);
            if !ttype.intersects(T_VARSTRING) {
                break;
            }
            let tclone = self.grammar.single_tags_list[rtag_id.0].clone();
            rtag_id = self.generate_varstring_tag(rtag_id, &tclone)?;
        }
        let rtag_hash = self.grammar.single_tags_list[rtag_id.0].hash;
        let rtag_type = self.grammar.tag_type(rtag_id);

        let test_pos = test.pos(&self.grammar.contexts_arena);

        // Snapshot the relation map (u32 name-hash -> sorted target global numbers).
        let relations: Vec<(u32, Vec<u32>)> = self
            .cohorts
            .get(current.0)
            .relations
            .iter()
            .map(|(k, v)| (*k, v.as_slice().to_vec()))
            .collect();

        if rtag_hash.get() == self.grammar.tag_any {
            for (_name, targets) in &relations {
                for &citer in targets {
                    if let Some(&c) = self.registry.cohort_map.get(&GlobalNumber(citer)) {
                        cs_insert(self.cohorts, self.single_windows, &mut rels, c);
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
                    if self.registry.cohort_map.contains_key(&GlobalNumber(citer))
                        && self.does_tag_match_regexp(*name, &rtag, caps != 0) != 0
                    {
                        let c = *self.registry.cohort_map.get(&GlobalNumber(citer)).unwrap();
                        cs_insert(self.cohorts, self.single_windows, &mut rels, c);
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
                    if let Some(&c) = self.registry.cohort_map.get(&GlobalNumber(citer)) {
                        cs_insert(self.cohorts, self.single_windows, &mut rels, c);
                    }
                }
            }
        }

        // Order/filter `rels`.
        if test_pos.intersects(POS_LEFT) {
            let lb = cs_lower_bound(self.cohorts, self.single_windows, &rels, current);
            rels = rels[..lb].to_vec();
        }
        if test_pos.intersects(POS_RIGHT) {
            let lb = cs_lower_bound(self.cohorts, self.single_windows, &rels, current);
            rels = rels[lb..].to_vec();
        }
        if test_pos.intersects(POS_SELF) {
            cs_insert(self.cohorts, self.single_windows, &mut rels, current);
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
                self.run_single_test(iter, test, &mut rvs, deep.as_deref_mut(), origin)?;
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
        Ok(rv)
    }

    /// POS_BAG_OF_TAGS match against a window's embedded `bag_of_tags` reading.
    /// The reading is not an arena object, so it is cloned into the readings arena
    /// (as `does_set_match_reading` needs a `ReadingId`), matched with
    /// `bypass_index = true`, then the slot is freed. Port adaptation — the
    /// embedded-value `Reading&` of the C++ `doesSetMatchReading(sWindow->
    /// bag_of_tags, test->target, true)` has no arena identity.
    fn match_bag_of_tags(&mut self, sw: SwId, target: u32) -> Result<bool, crate::error::RunError> {
        let bag = clone_reading(&self.single_windows.get(sw.0).bag_of_tags);
        let rid = self.readings.alloc(bag);
        let m = self.does_set_match_reading(crate::arena::ReadingId(rid), target, true, false)?;
        self.readings.free_slot(rid);
        Ok(m)
    }
}
