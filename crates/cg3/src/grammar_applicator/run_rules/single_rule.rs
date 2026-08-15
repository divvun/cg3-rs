//! `GrammarApplicator` — runSingleRule — the per-rule cohort loop, its cohortset descriptors, and cohort removal.
//!
//! Split out of the wave-2 monolithic `run_rules.rs` (wave 4, w4-file-split-fmt).

use crate::arena::{CohortId, CtxId, RuleId, SetId, SwId, TagId};
use crate::cohort::{CT_ENCLOSED, CT_IGNORED, CT_REMOVED, CohortSet};
use crate::contextual_test::{POS_NO_PASS_ORIGIN, POS_PASS_ORIGIN};
use crate::inlines::ui32;
use crate::rule::{
    RF_DELAYED, RF_ENCL_INNER, RF_ENCL_OUTER, RF_IGNORED, RF_KEEPORDER, RF_NOMAPPED, RF_NOPARENT,
    RF_REMEMBERX, RF_RESETX, RF_SAFE, RF_UNSAFE,
};
use crate::set::{ST_CHILD_UNIFY, ST_MAPPING, ST_SPECIAL};
use crate::tag::T_VARSTRING;
use crate::types::{GlobalNumber, TagHash};

// C++ anonymous `enum { RV_NOTHING = 1, RV_SOMETHING = 2, RV_DELIMITED = 4,
// RV_TRACERULE = 8 };` — the return-value bit flags of runRulesOnSingleWindow.

use super::*;

impl crate::grammar_applicator::Engine<'_> {
    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    /// C++ `bool runSingleRule(SingleWindow& current, const Rule& rule,
    /// RuleCallback reading_cb, RuleCallback cohort_cb)`.
    ///
    /// The core per-rule application: iterate the rule's candidate cohorts (its
    /// `rule_to_cohorts[rule.number]` `CohortSet`), find valid target readings
    /// (target set + contextual tests), then hand each matched reading to
    /// `reading_cb` and finally the cohort to `cohort_cb`. `rule` is a `RuleId`
    /// (the C++ `const Rule&`, which it nonetheless mutates via `mutable` —
    /// reproduced by writing back into `self.grammar.rule_by_number`).
    ///
    /// FLAGGED QUIRK (reproduced): on a FAILING context test that is not the first
    /// test, the failing test is moved to the front of `rule.tests` (a self-reorder
    /// of the "const" rule, unless `RF_KEEPORDER`).
    ///
    /// RECONCILIATION: `current.rule_to_cohorts` / `current.nested_rule_to_cohorts`
    /// must be `Vec<CohortSet>` / `Option<Box<CohortSet>>` (NOTED mod.rs/
    /// single_window.rs field-type changes; currently `Vec<Vec<CohortId>>` /
    /// `Option<Box<Vec<CohortId>>>`). Sibling engine methods
    /// (`does_set_match_reading`, `run_contextual_test`, `get_sub_reading`,
    /// reflow/context) are called by their C++-matching, arena-adapted signatures.
    pub(crate) fn run_single_rule(
        &mut self,
        current: SwId,
        rule: RuleId,
        st: &mut RRState,
    ) -> Result<bool, crate::error::RunError> {
        self.scratch.finish_cohort_loop = true;
        let rnumber = self.grammar.rule_by_number.get(rule.0).number;

        // cohortset = &current.rule_to_cohorts[rule.number]; override_cohortset()
        // may re-seat it to the nested set (in_nested). `nested` records which one.
        let nested = self.rr_override_cohortset(current, rnumber);
        let cohortset = self.rr_cohortset_ref(current, rnumber, nested);
        self.scratch.cohortsets.push(cohortset);
        // The frame's iteration cursor, OWNED in `rocits` (the C++ parks
        // `&rocit`, a stack local; wave 4 makes the parked slot the cursor).
        self.scratch.rocits.push(0);

        // Run the body; the scope_guard `popper` (pop cohortsets/rocits) runs on
        // EVERY exit path, so it is applied here after the body returns.
        let anything_changed = self.run_single_rule_body(current, rule, rnumber, cohortset, st);

        // popper dtor: cohortsets.pop_back(); rocits.pop_back();
        self.scratch.cohortsets.pop();
        self.scratch.rocits.pop();
        anything_changed
    }

    /// C++ `override_cohortset` lambda. When `in_nested`, (re)build
    /// `current.nested_rule_to_cohorts` to hold the apply-to cohort plus every
    /// `T_CONTEXT` context cohort referenced by the rule's target set, and route
    /// the active cohortset to it. Returns `true` iff the nested set is now in use.
    ///
    /// RECONCILIATION: `nested_rule_to_cohorts` must be `Option<Box<CohortSet>>`
    /// (NOTED single_window.rs change). The context-tag scan uses the target set's
    /// `trie_special` keys with `T_CONTEXT` + `context_ref_pos`.
    fn rr_override_cohortset(&mut self, current: SwId, rule_number: u32) -> bool {
        if !self.scratch.in_nested {
            return false;
        }
        let rtarget = self.grammar.rule_by_number.get(rule_number).target;
        // Gather T_CONTEXT context cohorts from the target set's trie_special.
        let ctx_len = self
            .scratch
            .context_stack
            .last()
            .map(|f| f.context.len())
            .unwrap_or(0);
        let mut ctx_cohorts: Vec<CohortId> = Vec::new();
        let trie_special = self.grammar.set_by_number(rtarget).trie_special.clone();
        for &tid in trie_special.keys() {
            let t = self.grammar.single_tags_list.get(tid.0);
            let crp = t.context_ref_pos();
            if t.r#type.intersects(crate::tag::T_CONTEXT)
                && (crp as usize) <= ctx_len
                && let Some(Some(c)) = self
                    .scratch
                    .context_stack
                    .last()
                    .map(|f| f.context.get((crp - 1) as usize).copied().flatten())
            {
                ctx_cohorts.push(c);
            }
        }
        let apply = self.get_apply_to().cohort;
        let sw = self.doc.store.single_windows.get_mut(current.0);
        if sw.nested_rule_to_cohorts.is_none() {
            sw.nested_rule_to_cohorts = Some(Box::new(CohortSet::new()));
        }
        sw.nested_rule_to_cohorts.as_mut().unwrap().clear();
        if let Some(a) = apply {
            // insert apply-to + context cohorts with the store-aware comparator.
            let np = crate::grammar_applicator::CsRef::Nested { sw: current };
            self.cohortset_insert_at(np, a);
            for c in ctx_cohorts {
                self.cohortset_insert_at(np, c);
            }
        }
        true
    }

    /// Resolve the active cohortset pointer for `run_single_rule`: the nested set
    /// when `nested`, else `current.rule_to_cohorts[rule_number]`.
    ///
    /// RECONCILIATION: both must be `CohortSet` (NOTED single_window.rs change).
    fn rr_cohortset_ref(
        &self,
        current: SwId,
        rule_number: u32,
        nested: bool,
    ) -> crate::grammar_applicator::CsRef {
        if nested {
            crate::grammar_applicator::CsRef::Nested { sw: current }
        } else {
            crate::grammar_applicator::CsRef::Window {
                sw: current,
                rule: rule_number,
            }
        }
    }

    /// Bridge to the sibling `print_debug_rule`, whose signature threads
    /// `store: &mut RuntimeStore` separately from `&mut self`. Swap the store out
    /// so both borrows can be satisfied, then restore it. Diagnostic-only (gated
    /// on `debug_rules`).
    fn rr_print_debug_rule(&mut self, rule: RuleId, target: bool, cntx: bool) {
        self.print_debug_rule(rule, target, cntx);
    }

    /// `reset_cohorts` lambda body of `runSingleRule`: re-seat the active
    /// cohortset (and the outer `rocit` cursor) after a window-restructuring
    /// action. Returns the (possibly re-seated) cohortset pointer.
    /// Writes the (possibly re-seated) cursor into the CURRENT frame's
    /// `rocits` slot — the C++ wrote `rocit` (the frame's parked object).
    fn rr_reset_cohorts(
        &mut self,
        current: SwId,
        rule_number: u32,
    ) -> crate::grammar_applicator::CsRef {
        let nested = self.rr_override_cohortset(current, rule_number);
        let cs = self.rr_cohortset_ref(current, rule_number, nested);
        *self.scratch.cohortsets.last_mut().unwrap() = cs;
        let idx = self.scratch.rocits.len() - 1;
        let gac = self.get_apply_to().cohort;
        if let Some(gac) = gac {
            let gac_local = self.doc.store.cohorts.get(gac.0).local_number as usize;
            // C++ reads `current.cohorts[gac->local_number]` unchecked. After a
            // REMCOHORT of the last cohort, `local_number == cohorts.size()` and
            // the C++ reads the stale vector slot, which still holds the removed
            // cohort's own pointer (erase of the tail element moves nothing).
            // Emulate that by probing with `gac` itself when out of range.
            let front_at_local = self
                .doc
                .store
                .single_windows
                .get(current.0)
                .cohorts
                .get(gac_local)
                .copied()
                .unwrap_or(gac);
            let lb = self.cohortset_lower_bound_at(cs, front_at_local);
            let size = self.cs_ref(cs).size();
            if lb == size {
                self.scratch.rocits[idx] = size;
            } else {
                let at = self.cs_ref(cs).as_slice()[lb];
                self.scratch.rocits[idx] = self.cohortset_find_n_at(cs, at);
            }
            let gac_type = self.doc.store.cohorts.get(gac.0).r#type;
            let new_size = self.cs_ref(cs).size();
            if !gac_type.intersects(CT_REMOVED | CT_IGNORED) && self.scratch.rocits[idx] < new_size
            {
                self.scratch.rocits[idx] += 1;
            }
        }
        cs
    }

    /// The body of [`Self::run_single_rule`] (everything inside the `popper`
    /// scope guard). Split out so the guard's `cohortsets`/`rocits` pop runs on
    /// every early-return path. See [`Self::run_single_rule`] for the markers.
    fn run_single_rule_body(
        &mut self,
        current: SwId,
        rule: RuleId,
        rnumber: u32,
        mut cohortset: crate::grammar_applicator::CsRef,
        st: &mut RRState,
    ) -> Result<bool, crate::error::RunError> {
        let mut anything_changed = false;
        let (rtype0, rflags, rsub_reading, rtarget, rline) = {
            let r = self.grammar.rule_by_number.get(rule.0);
            (r.r#type, r.flags, r.sub_reading, r.target, r.line)
        };
        let set_type = self.grammar.set_by_number(rtarget).r#type;

        // The frame's cursor lives in `rocits[depth]` — ONE object, exactly the
        // C++ parked `rocit`; inner frames and update_rule_to_cohorts may adjust
        // it, so it is re-read from the slot at every use.
        let depth = self.scratch.rocits.len() - 1;
        loop {
            let rocit = self.scratch.rocits[depth];
            if rocit >= self.cs_ref(cohortset).size() {
                break;
            }
            let cohort = self.cs_ref(cohortset).as_slice()[rocit];
            self.scratch.rocits[depth] = rocit + 1;

            self.scratch.finish_reading_loop = true;

            // Skip the initial >>> cohort.
            if self.doc.store.cohorts.get(cohort.0).local_number == 0 {
                continue;
            }
            // Skip removed/ignored cohorts.
            if self
                .doc
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_REMOVED | CT_IGNORED)
            {
                continue;
            }
            let c = self.doc.store.cohorts.get(cohort.0).local_number;
            // Skip parentheses-enclosed or foreign-parented cohorts.
            if self
                .doc
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_ENCLOSED)
                || self.doc.store.cohorts.get(cohort.0).parent != Some(current)
            {
                continue;
            }
            // Skip cohorts with no readings.
            if self.doc.store.cohorts.get(cohort.0).readings.is_empty() {
                continue;
            }
            // RESTORE with nothing to restore.
            if rtype0 == KRestore {
                let cc = self.doc.store.cohorts.get(cohort.0);
                if ((rflags.intersects(RF_DELAYED)) && cc.delayed.is_empty())
                    || ((rflags.intersects(RF_IGNORED)) && cc.ignored.is_empty())
                    || (!rflags.intersects(RF_DELAYED | RF_IGNORED) && cc.deleted.is_empty())
                {
                    continue;
                }
            }
            // Target-set possibility pre-check.
            if rsub_reading == 0 {
                let ps = &self.doc.store.cohorts.get(cohort.0).possible_sets;
                if rtarget.get() as usize >= ps.len() || !ps[rtarget.get() as usize] {
                    continue;
                }
            }

            let mut r#type = rtype0;
            // Single-reading fast skips.
            let nreadings = self.doc.store.cohorts.get(cohort.0).readings.len();
            if nreadings == 1 {
                if r#type == KSelect {
                    continue;
                }
                if r#type == KRemove || r#type == KIff {
                    let front = self.doc.store.cohorts.get(cohort.0).readings[0];
                    if self.doc.store.readings.get(front.0).noprint {
                        continue;
                    }
                    if (!self.cfg.r#unsafe || (rflags.intersects(RF_SAFE)))
                        && !rflags.intersects(RF_UNSAFE)
                    {
                        continue;
                    }
                }
            } else if r#type == KUnmap && rflags.intersects(RF_SAFE) {
                continue;
            }
            // Delimit at final cohort.
            if r#type == KDelimit
                && c == (self.doc.store.single_windows.get(current.0).cohorts.len() as u32) - 1
            {
                continue;
            }

            // Enclosure inner/outer gating.
            if rflags.intersects(RF_ENCL_INNER) {
                if self.scratch.par_left_pos == 0 {
                    continue;
                }
                let ln = self.doc.store.cohorts.get(cohort.0).local_number;
                if ln < self.scratch.par_left_pos || ln > self.scratch.par_right_pos {
                    continue;
                }
            } else if rflags.intersects(RF_ENCL_OUTER) {
                let ln = self.doc.store.cohorts.get(cohort.0).local_number;
                if self.scratch.par_left_pos != 0
                    && ln >= self.scratch.par_left_pos
                    && ln <= self.scratch.par_right_pos
                {
                    continue;
                }
            }

            // SETPARENT SAFE / NOPARENT with existing parent.
            let dep_parent = self.doc.store.cohorts.get(cohort.0).dep_parent;
            if r#type == KSetparent && (rflags.intersects(RF_SAFE)) && dep_parent.is_some() {
                continue;
            }
            if (rflags.intersects(RF_NOPARENT)) && dep_parent.is_some() {
                continue;
            }
            // REMPARENT / SWITCHPARENT with no parent.
            if (r#type == KRemparent || r#type == KSwitchparent) && dep_parent.is_none() {
                continue;
            }

            // DIVERGENCE (operator decision, plan node `drop-rule-cohort-index`):
            // the C++ `index_ruleCohort_no` visited-set is deleted, not ported.
            // Upstream marked (rule, cohort) attempted here — insert BEFORE
            // evaluation — and manually cleared the set at 19 mutation sites so
            // the fixpoint loop would revisit; its key was the raw 32-bit
            // hash_value(rule.number, global_number), so a collision silently
            // suppressed a rule evaluation. Interleaved A/B benchmarking put its
            // benefit at noise level, so every (rule, cohort) pair is simply
            // re-evaluated each pass.

            let mut num_active: usize = 0;
            let mut num_iff: usize = 0;
            let mut num_immutable: usize = 0;
            let mut reading_contexts: Vec<crate::grammar_applicator::RuleContext> = Vec::new();

            // Assume Iff is Remove until a context matches.
            if rtype0 == KIff {
                r#type = KRemove;
            }

            let mut did_test;
            let mut test_good = false;
            let mut matched_target = false;

            self.scratch.readings_plain.clear();
            self.subs_any_clear();

            // Per-cohort regex/unif capture state.
            self.scratch.regexgrps_z.clear();
            self.scratch.regexgrps_c.clear();
            self.scratch.unif_tags_rs.clear();
            self.scratch.unif_sets_rs.clear();

            self.scratch.used_regex = 0;
            let nread = self.doc.store.cohorts.get(cohort.0).readings.len();
            if self.scratch.regexgrps_store.len() < nread {
                self.scratch.regexgrps_store.resize_with(nread, Vec::new);
            }
            let mut used_unif: usize = 0;
            if self.scratch.unif_tags_store.len() < nread + 1 {
                self.scratch
                    .unif_tags_store
                    .resize_with(nread + 1, Default::default);
            }
            if self.scratch.unif_sets_store.len() < nread + 1 {
                self.scratch
                    .unif_sets_store
                    .resize_with(nread + 1, Default::default);
            }

            // Push the per-cohort context frame.
            {
                let mut ctx = crate::grammar_applicator::RuleContext::default();
                ctx.target.cohort = Some(cohort);
                ctx.is_with = rtype0 == KWith;
                self.scratch.context_stack.push(ctx);
            }

            // State snapshot for change detection.
            let state_num_readings = self.doc.store.cohorts.get(cohort.0).readings.len();
            let state_num_removed = self.doc.store.cohorts.get(cohort.0).deleted.len();
            let state_num_delayed = self.doc.store.cohorts.get(cohort.0).delayed.len();
            let state_num_ignored = self.doc.store.cohorts.get(cohort.0).ignored.len();

            let mut i = 0usize;
            while i < self.doc.store.cohorts.get(cohort.0).readings.len() {
                let reading_i = self.doc.store.cohorts.get(cohort.0).readings[i];
                let reading = match self.get_sub_reading(reading_i, rsub_reading) {
                    Some(r) => r,
                    None => {
                        self.scratch.clear_matched(reading_i);
                        i += 1;
                        continue;
                    }
                };
                {
                    let f = self.scratch.context_stack.last_mut().unwrap();
                    f.target.reading = Some(reading_i);
                    f.target.subreading = Some(reading);
                }
                self.scratch.clear_matched(reading);

                let (r_mapped, r_noprint, r_immutable, r_hash_plain, r_hash, r_number) = {
                    let r = self.doc.store.readings.get(reading.0);
                    (
                        r.mapped,
                        r.noprint,
                        r.immutable,
                        r.hash_plain,
                        r.hash,
                        r.number,
                    )
                };
                if r_mapped && (rtype0 == KMap || rtype0 == KAdd || rtype0 == KReplace) {
                    i += 1;
                    continue;
                }
                if r_mapped && (rflags.intersects(RF_NOMAPPED)) {
                    i += 1;
                    continue;
                }
                if r_noprint && !self.cfg.allow_magic_readings {
                    i += 1;
                    continue;
                }
                if r_immutable && rtype0 != KUnprotect {
                    if matches!(
                        rtype0,
                        KProtect
                            | KAdd
                            | KMap
                            | KReplace
                            | KSelect
                            | KRemove
                            | KIff
                            | KSubstitute
                            | KUnmap
                    ) {
                        num_active += 1;
                    }
                    if r#type == KSelect {
                        self.scratch.matched_target.insert(reading);
                        self.scratch.matched_tests.insert(reading);
                        reading_contexts.push(self.scratch.context_stack.last().unwrap().clone());
                    }
                    num_iff += 1;
                    num_immutable += 1;
                    i += 1;
                    continue;
                }

                // Plain-signature cache.
                did_test = false;
                if !set_type.intersects(ST_SPECIAL | ST_MAPPING | ST_CHILD_UNIFY)
                    && !self.scratch.readings_plain.is_empty()
                    && let Some(&cached) = self.scratch.readings_plain.get(&r_hash_plain)
                {
                    // Copy the cached reading's matched flags — a full bool
                    // copy, absence included (the cached reading may have
                    // matched neither target nor tests).
                    let mt = self.scratch.matched_target.contains(&cached);
                    let mtst = self.scratch.matched_tests.contains(&cached);
                    self.scratch.set_matched_target(reading, mt);
                    self.scratch.set_matched_tests(reading, mtst);
                    if mtst {
                        num_active += 1;
                    }
                    let cnum = self.doc.store.readings.get(cached.0).number;
                    if let Some(&rgc) = self.scratch.regexgrps_c.get(&cnum) {
                        self.scratch.regexgrps_c.insert(r_number, rgc);
                        let z = *self.scratch.regexgrps_z.get(&cnum).unwrap();
                        self.scratch.regexgrps_z.insert(r_number, z);
                        let f = self.scratch.context_stack.last_mut().unwrap();
                        f.regexgrp_ct = z;
                        f.regexgrps = Some(rgc);
                    }
                    let ut = self.scratch.unif_tags_rs.get(&r_hash_plain).copied();
                    let us = self.scratch.unif_sets_rs.get(&r_hash_plain).copied();
                    {
                        let f = self.scratch.context_stack.last_mut().unwrap();
                        f.unif_tags = ut;
                        f.unif_sets = us;
                    }
                    test_good = mtst;
                    reading_contexts.push(self.scratch.context_stack.last().unwrap().clone());
                    i += 1;
                    continue;
                }

                // Fresh per-reading regex/unif state (store INDICES, wave 4).
                {
                    let rgs = self.scratch.used_regex;
                    let uts = used_unif;
                    let uss = used_unif;
                    {
                        let f = self.scratch.context_stack.last_mut().unwrap();
                        f.regexgrp_ct = 0;
                        f.regexgrps = Some(rgs);
                        f.unif_tags = Some(uts);
                        f.unif_sets = Some(uss);
                    }
                    self.scratch.unif_tags_rs.insert(r_hash_plain, uts);
                    self.scratch.unif_sets_rs.insert(r_hash_plain, uss);
                    self.scratch.unif_tags_rs.insert(r_hash, uts);
                    self.scratch.unif_sets_rs.insert(r_hash, uss);
                    used_unif += 1;
                    self.scratch.unif_tags_store[uts].clear();
                    self.scratch.unif_sets_store[uss].clear();
                }

                self.scratch.unif_last_wordform = TagHash(0);
                self.scratch.unif_last_baseform = TagHash(0);
                self.scratch.unif_last_textual = TagHash(0);
                self.scratch.same_basic = r_hash_plain;
                self.scratch.rule_target = None;
                if self.scratch.context_stack.len() > 1 {
                    let m = self.scratch.context_stack[self.scratch.context_stack.len() - 2].mark;
                    if m.is_some() {
                        self.set_mark(m);
                    } else {
                        self.set_mark(Some(cohort));
                    }
                } else {
                    self.set_mark(Some(cohort));
                }
                let orz = self.scratch.context_stack.last().unwrap().regexgrp_ct;
                {
                    let mut rc = Some(reading_i);
                    while let Some(r) = rc {
                        self.doc.store.readings.get_mut(r.0).active = true;
                        rc = self.doc.store.readings.get(r.0).next;
                    }
                }
                self.scratch.rule_target = Some(cohort);

                // First check: does the rule target match?
                let target_matches = rtarget.get() != 0 && {
                    let bypass = set_type.intersects(ST_CHILD_UNIFY | ST_SPECIAL);
                    self.does_set_match_reading(reading, rtarget.get(), bypass, false)
                };
                if target_matches {
                    let mut regex_prop = true;
                    if orz != self.scratch.context_stack.last().unwrap().regexgrp_ct {
                        did_test = false;
                        regex_prop = false;
                    }
                    self.scratch.rule_target = Some(cohort);
                    self.scratch.matched_target.insert(reading);
                    matched_target = true;
                    let mut good = true;
                    if !did_test {
                        self.scratch
                            .context_stack
                            .last_mut()
                            .unwrap()
                            .context
                            .clear();
                        let tests: Vec<CtxId> = self
                            .grammar
                            .rule_by_number
                            .get(rule.0)
                            .tests
                            .iter()
                            .copied()
                            .collect();
                        let mut ti = 0usize;
                        while ti < tests.len() {
                            let test = tests[ti];
                            if rflags.intersects(RF_RESETX) || !rflags.intersects(RF_REMEMBERX) {
                                self.set_mark(Some(cohort));
                            }
                            self.scratch.seen_barrier = false;
                            self.scratch.dep_deep_seen.clear();
                            for d in self.scratch.ci_depths.iter_mut() {
                                *d = 0;
                            }
                            self.scratch.tmpl_cntx =
                                crate::grammar_applicator::TmplContext::default();
                            let tpos = self.grammar.contexts_arena[test.0].pos;
                            let mut result: Option<CohortId> = None;
                            let with_deep = rtype0 == KWith;
                            if with_deep {
                                self.scratch.merge_with = None;
                            }
                            let mut deep_ref: Option<&mut Option<CohortId>> =
                                if with_deep { Some(&mut result) } else { None };
                            let next_test = if !tpos.intersects(POS_PASS_ORIGIN)
                                && (self.cfg.no_pass_origin
                                    || (tpos.intersects(POS_NO_PASS_ORIGIN)))
                            {
                                self.run_contextual_test(
                                    Some(current),
                                    c,
                                    test,
                                    deep_ref.take(),
                                    Some(cohort),
                                )
                            } else {
                                self.run_contextual_test(
                                    Some(current),
                                    c,
                                    test,
                                    deep_ref.take(),
                                    None,
                                )
                            };
                            let ctx_push = if self.scratch.merge_with.is_some() {
                                self.scratch.merge_with
                            } else {
                                result
                            };
                            self.scratch
                                .context_stack
                                .last_mut()
                                .unwrap()
                                .context
                                .push(ctx_push);
                            test_good = next_test.is_some();
                            self.profile_rule_context(test_good, rule, test);
                            if !test_good {
                                good = false;
                                // Self-reorder quirk: move failing test to front.
                                if ti != 0 && !rflags.intersects(RF_KEEPORDER) {
                                    let r = self.grammar.rule_by_number.get_mut(rule.0);
                                    r.tests.remove(ti);
                                    r.tests.push_front(test);
                                }
                                break;
                            }
                            // C++ recomputes `did_test` here (`did_test =
                            // ((set.type & (ST_CHILD_UNIFY | ST_SPECIAL)) == 0 &&
                            // unif_tags->empty() && unif_sets->empty())`), but
                            // the value is dead upstream too: the `did_test =
                            // false` reset above runs before every read.
                            ti += 1;
                        }
                    } else {
                        good = test_good;
                    }
                    if good {
                        // Iff → Select once a context matches.
                        if rtype0 == KIff && r#type != KSelect {
                            r#type = KSelect;
                            if self.grammar.has_protect {
                                let mut j = 0usize;
                                while j < i {
                                    let rj = self.doc.store.cohorts.get(cohort.0).readings[j];
                                    if let Some(sr) = self.get_sub_reading(rj, rsub_reading)
                                        && self.doc.store.readings.get(sr.0).immutable
                                    {
                                        self.scratch.matched_target.insert(sr);
                                        self.scratch.matched_tests.insert(sr);
                                        num_active += 1;
                                        num_iff += 1;
                                    }
                                    j += 1;
                                }
                            }
                        }
                        self.scratch.matched_tests.insert(reading);
                        num_active += 1;
                        if self.diag.profiler.is_some() {
                            // Profiler::Key k{ET_RULE, rule.number + 1}; ++entries[k].num_match
                            let rnum = self.grammar.rule_by_number.get(rule.0).number;
                            let k = crate::profiler::Key {
                                r#type: crate::profiler::ET_RULE,
                                id: rnum + 1,
                            };
                            let p = self.diag.profiler.as_mut().unwrap();
                            let e = p.entries.entry(k).or_default();
                            e.num_match += 1;
                            if e.example_window == 0 {
                                self.add_profiling_example(k);
                            }
                        }
                        if !self.cfg.debug_rules.empty() && self.cfg.debug_rules.contains(rline) {
                            self.rr_print_debug_rule(rule, true, true);
                        }
                        // Propagate regex captures from a prior reading.
                        if regex_prop && i != 0 && !self.scratch.regexgrps_c.is_empty() {
                            let mut z = i;
                            while z > 0 {
                                let prev = self.doc.store.cohorts.get(cohort.0).readings[z - 1];
                                let prev_num = self.doc.store.readings.get(prev.0).number;
                                if let Some(&rgc) = self.scratch.regexgrps_c.get(&prev_num) {
                                    self.scratch.regexgrps_c.insert(r_number, rgc);
                                    let zz = *self.scratch.regexgrps_z.get(&prev_num).unwrap();
                                    self.scratch.regexgrps_z.insert(r_number, zz);
                                    break;
                                }
                                z -= 1;
                            }
                        }
                    } else {
                        self.scratch.context_stack.last_mut().unwrap().regexgrp_ct = orz;
                        if !self.cfg.debug_rules.empty() && self.cfg.debug_rules.contains(rline) {
                            self.rr_print_debug_rule(rule, true, false);
                        }
                    }
                    num_iff += 1;
                } else {
                    self.scratch.context_stack.last_mut().unwrap().regexgrp_ct = orz;
                    if self.diag.profiler.is_some() {
                        // Profiler::Key k{ET_RULE, rule.number + 1}; ++entries[k].num_fail
                        let rnum = self.grammar.rule_by_number.get(rule.0).number;
                        let k = crate::profiler::Key {
                            r#type: crate::profiler::ET_RULE,
                            id: rnum + 1,
                        };
                        if let Some(p) = self.diag.profiler.as_mut() {
                            p.entries.entry(k).or_default().num_fail += 1;
                        }
                    }
                    if !self.cfg.debug_rules.empty() && self.cfg.debug_rules.contains(rline) {
                        self.rr_print_debug_rule(rule, false, false);
                    }
                }

                self.scratch.readings_plain.insert(r_hash_plain, reading);
                {
                    let mut rc = Some(reading_i);
                    while let Some(r) = rc {
                        self.doc.store.readings.get_mut(r.0).active = false;
                        rc = self.doc.store.readings.get(r.0).next;
                    }
                }
                if reading != reading_i {
                    // Copy the sub-reading's matched flags back onto the top
                    // reading — a full bool copy, absence included.
                    let mt = self.scratch.matched_target.contains(&reading);
                    let mtst = self.scratch.matched_tests.contains(&reading);
                    self.scratch.set_matched_target(reading_i, mt);
                    self.scratch.set_matched_tests(reading_i, mtst);
                }
                let rgc_ct = self.scratch.context_stack.last().unwrap().regexgrp_ct;
                if rgc_ct != 0 {
                    let rgs = self
                        .scratch
                        .context_stack
                        .last()
                        .unwrap()
                        .regexgrps
                        .unwrap();
                    self.scratch.regexgrps_c.insert(r_number, rgs);
                    self.scratch.regexgrps_z.insert(r_number, rgc_ct);
                    self.scratch.used_regex += 1;
                }
                reading_contexts.push(self.scratch.context_stack.last().unwrap().clone());
                i += 1;
            }

            let (now_readings, now_removed, now_delayed, now_ignored) = {
                let cc = self.doc.store.cohorts.get(cohort.0);
                (
                    cc.readings.len(),
                    cc.deleted.len(),
                    cc.delayed.len(),
                    cc.ignored.len(),
                )
            };
            if state_num_readings != now_readings
                || state_num_removed != now_removed
                || state_num_delayed != now_delayed
                || state_num_ignored != now_ignored
            {
                anything_changed = true;
            }

            // No valid targets → drop this cohort from the rule set.
            if num_active == 0 && (num_iff == 0 || rtype0 != KIff) {
                if num_immutable == 0 && !matched_target {
                    let ro = self.scratch.rocits[depth] - 1;
                    self.cs_mut(cohortset).erase_n(ro);
                    self.scratch.rocits[depth] = ro;
                }
                self.scratch.context_stack.pop();
                continue;
            }
            // All readings valid → nothing to do for Select / safe Remove.
            if num_active == self.doc.store.cohorts.get(cohort.0).readings.len() {
                if r#type == KSelect {
                    self.scratch.context_stack.pop();
                    continue;
                }
                if r#type == KRemove
                    && (!self.cfg.r#unsafe || (rflags.intersects(RF_SAFE)))
                    && !rflags.intersects(RF_UNSAFE)
                {
                    self.scratch.context_stack.pop();
                    continue;
                }
            }

            // Dispatch each matched reading.
            for ctx in reading_contexts.into_iter() {
                let (mt, mtst) = {
                    let sr = ctx.target.subreading.unwrap();
                    (
                        self.scratch.matched_target.contains(&sr),
                        self.scratch.matched_tests.contains(&sr),
                    )
                };
                if !mt {
                    continue;
                }
                if !mtst && rtype0 != KIff {
                    continue;
                }
                *self.scratch.context_stack.last_mut().unwrap() = ctx;
                self.scratch.reset_cohorts_for_loop = false;
                self.reading_cb_dispatch(st)?;
                if !self.scratch.finish_cohort_loop {
                    self.scratch.context_stack.pop();
                    return Ok(anything_changed);
                }
                if self.scratch.reset_cohorts_for_loop {
                    cohortset = self.rr_reset_cohorts(current, rnumber);
                    break;
                }
                if !self.scratch.finish_reading_loop {
                    break;
                }
            }

            self.scratch.reset_cohorts_for_loop = false;
            self.cohort_cb_dispatch(st)?;
            if !self.scratch.finish_cohort_loop {
                self.scratch.context_stack.pop();
                return Ok(anything_changed);
            }
            if self.scratch.reset_cohorts_for_loop {
                cohortset = self.rr_reset_cohorts(current, rnumber);
            }
            self.scratch.context_stack.pop();
        }
        Ok(anything_changed)
    }

    /// `ignore_cohort(cohort)` lambda of `runSingleRule`: mark a cohort
    /// `CT_IGNORED`, hit_by its readings, erase it from every rule's cohortset,
    /// detach it, and remove it from the window's `cohorts` (kept in `all_cohorts`).
    pub(crate) fn rr_ignore_cohort(&mut self, rule_number: u32, cohort: CohortId) {
        let current = self.doc.store.cohorts.get(cohort.0).parent.unwrap();
        let rs = self.doc.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            self.doc
                .store
                .readings
                .get_mut(r.0)
                .hit_by
                .push(rule_number);
        }
        // Erase from every rule's cohortset.
        self.rr_erase_from_all_cohortsets(current, cohort);
        {
            let c = self.doc.store.cohorts.get_mut(cohort.0);
            c.r#type |= CT_IGNORED;
        }
        crate::cohort::detach(&mut self.doc.store, cohort);
        let gn = self.doc.store.cohorts.get(cohort.0).global_number;
        self.doc.cohorts.cohort_map.remove(&gn);
        let ln = self.doc.store.cohorts.get(cohort.0).local_number as usize;
        self.doc
            .store
            .single_windows
            .get_mut(current.0)
            .cohorts
            .remove(ln);
    }

    /// Erase `cohort` from every `current.rule_to_cohorts[i]` (the C++
    /// `for (auto& cs : current.rule_to_cohorts) cs.erase(cohort);`).
    /// RECONCILIATION: `rule_to_cohorts` must be `Vec<CohortSet>` (NOTED).
    fn rr_erase_from_all_cohortsets(&mut self, current: SwId, cohort: CohortId) {
        let n = self
            .doc
            .store
            .single_windows
            .get(current.0)
            .rule_to_cohorts
            .len();
        for i in 0..n {
            self.cohortset_erase_at(
                crate::grammar_applicator::CsRef::Window {
                    sw: current,
                    rule: i as u32,
                },
                cohort,
            );
        }
    }

    /// `rem_cohort(cohort)` lambda of `runSingleRule`: fully remove a cohort —
    /// hit_by + mark deleted its readings, erase it from all rule cohortsets,
    /// forward its dependency children to its parent, mark `CT_REMOVED`, detach,
    /// prune it from every `dep_children`, drop it from `cohort_map` and the
    /// window's `cohorts`, renumber, and (when that empties a non-current window)
    /// splice the window out. Finally `rebuildCohortLinks()`.
    pub(crate) fn rr_rem_cohort(&mut self, rule_number: u32, cohort: CohortId) {
        let current = self.doc.store.cohorts.get(cohort.0).parent.unwrap();
        let rs = self.doc.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            let rr = self.doc.store.readings.get_mut(r.0);
            rr.hit_by.push(rule_number);
            rr.deleted = true;
            if self.cfg.trace {
                rr.noprint = false;
            }
        }
        self.rr_erase_from_all_cohortsets(current, cohort);
        // Forward children to the parent.
        loop {
            let ch = {
                let dc = &self.doc.store.cohorts.get(cohort.0).dep_children;
                if dc.empty() {
                    break;
                }
                dc.back()
            };
            let dp = self.doc.store.cohorts.get(cohort.0).dep_parent;
            let parent_key = dp.unwrap_or(GlobalNumber(0));
            let (pc, cc) = (
                self.doc.cohorts.cohort_map.get(&parent_key).copied(),
                self.doc.cohorts.cohort_map.get(&GlobalNumber(ch)).copied(),
            );
            if let (Some(pc), Some(cc)) = (pc, cc) {
                self.attach_parent_child(pc, cc, true, true);
            }
            self.doc
                .store
                .cohorts
                .get_mut(cohort.0)
                .dep_children
                .erase(ch);
        }
        self.doc.store.cohorts.get_mut(cohort.0).r#type |= CT_REMOVED;
        crate::cohort::detach(&mut self.doc.store, cohort);
        let dep_self = self
            .doc
            .store
            .cohorts
            .get(cohort.0)
            .dep_self
            .map_or(0, |g| g.get());
        let keys: Vec<GlobalNumber> = self.doc.cohorts.cohort_map.keys().copied().collect();
        for k in keys {
            let cid = *self.doc.cohorts.cohort_map.get(&k).unwrap();
            self.doc
                .store
                .cohorts
                .get_mut(cid.0)
                .dep_children
                .erase(dep_self);
        }
        let gn = self.doc.store.cohorts.get(cohort.0).global_number;
        self.doc.cohorts.cohort_map.remove(&gn);
        let ln = self.doc.store.cohorts.get(cohort.0).local_number as usize;
        self.doc
            .store
            .single_windows
            .get_mut(current.0)
            .cohorts
            .remove(ln);
        self.rr_renumber(current);

        // Window emptied (only >>> left) and not the active window → drop it.
        if self.doc.store.single_windows.get(current.0).cohorts.len() == 1
            && Some(current) != self.doc.stream.current
        {
            let empty_cohort = self.doc.store.single_windows.get(current.0).cohorts[0];
            self.rr_erase_from_all_cohortsets(current, empty_cohort);
            crate::cohort::detach(&mut self.doc.store, empty_cohort);
            let ds = self
                .doc
                .store
                .cohorts
                .get(empty_cohort.0)
                .dep_self
                .map_or(0, |g| g.get());
            let keys: Vec<GlobalNumber> = self.doc.cohorts.cohort_map.keys().copied().collect();
            for k in keys {
                let cid = *self.doc.cohorts.cohort_map.get(&k).unwrap();
                self.doc.store.cohorts.get_mut(cid.0).dep_children.erase(ds);
            }
            let egn = self.doc.store.cohorts.get(empty_cohort.0).global_number;
            self.doc.cohorts.cohort_map.remove(&egn);
            let opt = Some(empty_cohort);
            crate::cohort::free_cohort(
                &mut self.doc.store,
                Some((&mut self.doc.cohorts, &mut self.doc.deps)),
                opt,
            );
            // if (current.previous) { previous->text += current.text + text_post;
            //   previous->all_cohorts += current.all_cohorts[1..]; }
            // else if (current.next) { next->text = text_post + next->text;
            //   next->all_cohorts.insert(begin+1, current.all_cohorts[1..]); }
            let (prev, next) = {
                let sw = self.doc.store.single_windows.get(current.0);
                (sw.previous, sw.next)
            };
            if let Some(prev) = prev {
                let (text, text_post, rest) = {
                    let sw = self.doc.store.single_windows.get(current.0);
                    (
                        sw.text.clone(),
                        sw.text_post.clone(),
                        sw.all_cohorts.iter().skip(1).copied().collect::<Vec<_>>(),
                    )
                };
                {
                    let psw = self.doc.store.single_windows.get_mut(prev.0);
                    psw.text.push_str(&text);
                    psw.text.push_str(&text_post);
                    psw.all_cohorts.extend(rest.iter().copied());
                }
                // C++ leaves these cohorts' `parent` dangling at the pooled
                // (cleared: parent=nullptr) window, making their eventual
                // teardown map-erase a no-op; re-seat the id so the arena deref
                // stays valid — same observable behavior (their cohort_map
                // entries were already erased by rem_cohort).
                for c in rest {
                    self.doc.store.cohorts.get_mut(c.0).parent = Some(prev);
                }
            } else if let Some(next) = next {
                let (text_post, rest) = {
                    let sw = self.doc.store.single_windows.get(current.0);
                    (
                        sw.text_post.clone(),
                        sw.all_cohorts.iter().skip(1).copied().collect::<Vec<_>>(),
                    )
                };
                {
                    let nsw = self.doc.store.single_windows.get_mut(next.0);
                    let mut t = text_post;
                    t.push_str(&nsw.text);
                    nsw.text = t;
                    let at = 1.min(nsw.all_cohorts.len());
                    nsw.all_cohorts.splice(at..at, rest.iter().copied());
                }
                for c in rest {
                    self.doc.store.cohorts.get_mut(c.0).parent = Some(next);
                }
            }
            self.doc
                .store
                .single_windows
                .get_mut(current.0)
                .all_cohorts
                .clear();
            // Remove `current` from gWindow.previous / next.
            if let Some(pos) = self.doc.stream.previous.iter().position(|&s| s == current) {
                let opt = Some(current);
                crate::single_window::free_swindow(
                    &mut self.doc.store,
                    &mut self.doc.cohorts,
                    &mut self.doc.deps,
                    opt,
                );
                self.doc.stream.previous.remove(pos);
            }
            if let Some(pos) = self.doc.stream.next.iter().position(|&s| s == current) {
                let opt = Some(current);
                crate::single_window::free_swindow(
                    &mut self.doc.store,
                    &mut self.doc.cohorts,
                    &mut self.doc.deps,
                    opt,
                );
                self.doc.stream.next.remove(pos);
            }
            self.doc
                .stream
                .rebuild_single_window_links(&mut self.doc.store);
        }
        self.doc.stream.rebuild_cohort_links(&mut self.doc.store);
    }

    /// Renumber `current.cohorts[i].local_number = i` (the C++ `foreach` after a
    /// `cohorts.erase(...)`).
    pub(crate) fn rr_renumber(&mut self, current: SwId) {
        let n = self.doc.store.single_windows.get(current.0).cohorts.len();
        for k in 0..n {
            let cid = self.doc.store.single_windows.get(current.0).cohorts[k];
            self.doc.store.cohorts.get_mut(cid.0).local_number = ui32(k);
        }
    }

    /// Snapshot the global `variables` map's live `(key, value)` entries in slot
    /// order (the C++ `for (auto& kv : variables)` iteration). Lets the REMVARIABLE
    /// branch scan while mutating `self`.
    pub(crate) fn variables_entries(&self) -> Vec<(u32, u32)> {
        self.doc.variables.iter().copied().collect()
    }

    /// C++ `getTagList(*set).front()`-style first-tag helper with varstring
    /// resolution — returns the first tag of a set's expanded tag list, varstring-
    /// generated. Used by JUMP / SETVARIABLE.
    pub(crate) fn rr_first_taglist_tag(&mut self, set: SetId) -> Option<TagId> {
        let list = self.get_tag_list_of_set(set, false);
        let first = list.first().copied()?;
        let ttype = self.grammar.single_tags_list.get(first.0).r#type;
        if ttype.intersects(T_VARSTRING) {
            Some(self.generate_varstring_tag_id(first))
        } else {
            Some(first)
        }
    }
}
