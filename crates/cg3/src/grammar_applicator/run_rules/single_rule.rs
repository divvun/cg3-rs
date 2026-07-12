//! `GrammarApplicator` — runSingleRule — the per-rule cohort loop, its cohortset descriptors, and cohort removal.
//!
//! Split out of the wave-2 monolithic `run_rules.rs` (wave 4, w4-file-split-fmt).

use crate::arena::{CohortId, CtxId, RuleId, SetId, SwId, TagId};
use crate::cohort::{CT_ENCLOSED, CT_IGNORED, CT_NUM_CURRENT, CT_REMOVED, CohortSet};
use crate::contextual_test::{POS_NO_PASS_ORIGIN, POS_PASS_ORIGIN};
use crate::inlines::{hash_value, ui32};
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

impl crate::grammar_applicator::GrammarApplicator {
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
    ) -> bool {
        self.finish_cohort_loop = true;
        let rnumber = self.grammar.rule_by_number.get(rule.0).number;

        // cohortset = &current.rule_to_cohorts[rule.number]; override_cohortset()
        // may re-seat it to the nested set (in_nested). `nested` records which one.
        let nested = self.rr_override_cohortset(current, rnumber);
        let cohortset = self.rr_cohortset_ref(current, rnumber, nested);
        self.cohortsets.push(cohortset);
        // The frame's iteration cursor, OWNED in `rocits` (the C++ parks
        // `&rocit`, a stack local; wave 4 makes the parked slot the cursor).
        self.rocits.push(0);

        // Run the body; the scope_guard `popper` (pop cohortsets/rocits) runs on
        // EVERY exit path, so it is applied here after the body returns.
        let anything_changed =
            self.run_single_rule_body(current, rule, rnumber, nested, cohortset, st);

        // popper dtor: cohortsets.pop_back(); rocits.pop_back();
        self.cohortsets.pop();
        self.rocits.pop();
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
        if !self.in_nested {
            return false;
        }
        let rtarget = self.grammar.rule_by_number.get(rule_number).target;
        // Gather T_CONTEXT context cohorts from the target set's trie_special.
        let ctx_len = self
            .context_stack
            .last()
            .map(|f| f.context.len())
            .unwrap_or(0);
        let mut ctx_cohorts: Vec<CohortId> = Vec::new();
        let trie_special = self.grammar.set_by_number(rtarget).trie_special.clone();
        for (&tid, _) in trie_special.iter() {
            let t = self.grammar.single_tags_list.get(tid.0);
            let crp = t.context_ref_pos();
            if t.r#type.intersects(crate::tag::T_CONTEXT) && (crp as usize) <= ctx_len
                && let Some(Some(c)) = self
                    .context_stack
                    .last()
                    .map(|f| f.context.get((crp - 1) as usize).copied().flatten())
                {
                    ctx_cohorts.push(c);
                }
        }
        let apply = self.get_apply_to().cohort;
        let sw = self.store.single_windows.get_mut(current.0);
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
        *self.cohortsets.last_mut().unwrap() = cs;
        let idx = self.rocits.len() - 1;
        let gac = self.get_apply_to().cohort;
        if let Some(gac) = gac {
            let gac_local = self.store.cohorts.get(gac.0).local_number as usize;
            // C++ reads `current.cohorts[gac->local_number]` unchecked. After a
            // REMCOHORT of the last cohort, `local_number == cohorts.size()` and
            // the C++ reads the stale vector slot, which still holds the removed
            // cohort's own pointer (erase of the tail element moves nothing).
            // Emulate that by probing with `gac` itself when out of range.
            let front_at_local = self
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
                self.rocits[idx] = size;
            } else {
                let at = self.cs_ref(cs).as_slice()[lb];
                self.rocits[idx] = self.cohortset_find_n_at(cs, at);
            }
            let gac_type = self.store.cohorts.get(gac.0).r#type;
            let new_size = self.cs_ref(cs).size();
            if !gac_type.intersects(CT_REMOVED | CT_IGNORED) && self.rocits[idx] < new_size {
                self.rocits[idx] += 1;
            }
        }
        cs
    }

    /// The body of [`Self::run_single_rule`] (everything inside the `popper`
    /// scope guard). Split out so the guard's `cohortsets`/`rocits` pop runs on
    /// every early-return path. See [`Self::run_single_rule`] for the markers.
    #[allow(clippy::too_many_arguments)]
    // Faithful-port mirrors: assignments kept 1:1 with the C++ text even where
    // the ported reads were elided (see the deferred-I/O / driver notes).
    #[allow(unused_assignments, unused_variables)]
    fn run_single_rule_body(
        &mut self,
        current: SwId,
        rule: RuleId,
        rnumber: u32,
        nested: bool,
        mut cohortset: crate::grammar_applicator::CsRef,
        st: &mut RRState,
    ) -> bool {
        let mut anything_changed = false;
        let (rtype0, rflags, rsub_reading, rtarget, rline) = {
            let r = self.grammar.rule_by_number.get(rule.0);
            (r.r#type, r.flags, r.sub_reading, r.target, r.line)
        };
        let set_type = self.grammar.set_by_number(rtarget).r#type;
        let _ = (nested, rline);

        // The frame's cursor lives in `rocits[depth]` — ONE object, exactly the
        // C++ parked `rocit`; inner frames and update_rule_to_cohorts may adjust
        // it, so it is re-read from the slot at every use.
        let depth = self.rocits.len() - 1;
        loop {
            let rocit = self.rocits[depth];
            if rocit >= self.cs_ref(cohortset).size() {
                break;
            }
            let cohort = self.cs_ref(cohortset).as_slice()[rocit];
            self.rocits[depth] = rocit + 1;

            self.finish_reading_loop = true;

            // Skip the initial >>> cohort.
            if self.store.cohorts.get(cohort.0).local_number == 0 {
                continue;
            }
            // Skip removed/ignored cohorts.
            if self
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_REMOVED | CT_IGNORED)
            {
                continue;
            }
            let c = self.store.cohorts.get(cohort.0).local_number;
            // Skip parentheses-enclosed or foreign-parented cohorts.
            if self
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_ENCLOSED)
                || self.store.cohorts.get(cohort.0).parent != Some(current)
            {
                continue;
            }
            // Skip cohorts with no readings.
            if self.store.cohorts.get(cohort.0).readings.is_empty() {
                continue;
            }
            // RESTORE with nothing to restore.
            if rtype0 == K_RESTORE {
                let cc = self.store.cohorts.get(cohort.0);
                if (rflags.intersects(RF_DELAYED)) && cc.delayed.is_empty() {
                    continue;
                } else if (rflags.intersects(RF_IGNORED)) && cc.ignored.is_empty() {
                    continue;
                } else if !rflags.intersects(RF_DELAYED | RF_IGNORED) && cc.deleted.is_empty() {
                    continue;
                }
            }
            // Target-set possibility pre-check.
            if rsub_reading == 0 {
                let ps = &self.store.cohorts.get(cohort.0).possible_sets;
                if rtarget.get() as usize >= ps.len() || !ps[rtarget.get() as usize] {
                    continue;
                }
            }

            let mut r#type = rtype0;
            // Single-reading fast skips.
            let nreadings = self.store.cohorts.get(cohort.0).readings.len();
            if nreadings == 1 {
                if r#type == K_SELECT {
                    continue;
                }
                if r#type == K_REMOVE || r#type == K_IFF {
                    let front = self.store.cohorts.get(cohort.0).readings[0];
                    if self.store.readings.get(front.0).noprint {
                        continue;
                    }
                    if (!self.r#unsafe || (rflags.intersects(RF_SAFE)))
                        && !rflags.intersects(RF_UNSAFE)
                    {
                        continue;
                    }
                }
            } else if r#type == K_UNMAP && rflags.intersects(RF_SAFE) {
                continue;
            }
            // Delimit at final cohort.
            if r#type == K_DELIMIT
                && c == (self.store.single_windows.get(current.0).cohorts.len() as u32) - 1
            {
                continue;
            }

            // Enclosure inner/outer gating.
            if rflags.intersects(RF_ENCL_INNER) {
                if self.par_left_pos == 0 {
                    continue;
                }
                let ln = self.store.cohorts.get(cohort.0).local_number;
                if ln < self.par_left_pos || ln > self.par_right_pos {
                    continue;
                }
            } else if rflags.intersects(RF_ENCL_OUTER) {
                let ln = self.store.cohorts.get(cohort.0).local_number;
                if self.par_left_pos != 0 && ln >= self.par_left_pos && ln <= self.par_right_pos {
                    continue;
                }
            }

            // SETPARENT SAFE / NOPARENT with existing parent.
            let dep_parent = self.store.cohorts.get(cohort.0).dep_parent;
            if r#type == K_SETPARENT && (rflags.intersects(RF_SAFE)) && dep_parent.is_some() {
                continue;
            }
            if (rflags.intersects(RF_NOPARENT)) && dep_parent.is_some() {
                continue;
            }
            // REMPARENT / SWITCHPARENT with no parent.
            if (r#type == K_REMPARENT || r#type == K_SWITCHPARENT) && dep_parent.is_none() {
                continue;
            }

            // rule/cohort no-match cache.
            let gn = self.store.cohorts.get(cohort.0).global_number.get();
            let ih = hash_value(rnumber, gn);
            if self.index_ruleCohort_no.contains(ih) {
                continue;
            }
            self.index_ruleCohort_no.insert(ih);

            let mut num_active: usize = 0;
            let mut num_iff: usize = 0;
            let mut num_immutable: usize = 0;
            let mut reading_contexts: Vec<crate::grammar_applicator::Rule_Context> = Vec::new();

            // Assume Iff is Remove until a context matches.
            if rtype0 == K_IFF {
                r#type = K_REMOVE;
            }

            let mut did_test;
            let mut test_good = false;
            let mut matched_target = false;

            self.readings_plain.clear();
            self.subs_any_clear();

            // Per-cohort regex/unif capture state.
            self.regexgrps_z.clear();
            self.regexgrps_c.clear();
            self.unif_tags_rs.clear();
            self.unif_sets_rs.clear();

            self.used_regex = 0;
            let nread = self.store.cohorts.get(cohort.0).readings.len();
            if self.regexgrps_store.len() < nread {
                self.regexgrps_store.resize_with(nread, Vec::new);
            }
            let mut used_unif: usize = 0;
            if self.unif_tags_store.len() < nread + 1 {
                self.unif_tags_store
                    .resize_with(nread + 1, Default::default);
            }
            if self.unif_sets_store.len() < nread + 1 {
                self.unif_sets_store
                    .resize_with(nread + 1, Default::default);
            }

            // Push the per-cohort context frame.
            {
                let mut ctx = crate::grammar_applicator::Rule_Context::default();
                ctx.target.cohort = Some(cohort);
                ctx.is_with = rtype0 == K_WITH;
                self.context_stack.push(ctx);
            }

            // State snapshot for change detection.
            let state_num_readings = self.store.cohorts.get(cohort.0).readings.len();
            let state_num_removed = self.store.cohorts.get(cohort.0).deleted.len();
            let state_num_delayed = self.store.cohorts.get(cohort.0).delayed.len();
            let state_num_ignored = self.store.cohorts.get(cohort.0).ignored.len();

            let mut i = 0usize;
            while i < self.store.cohorts.get(cohort.0).readings.len() {
                let reading_i = self.store.cohorts.get(cohort.0).readings[i];
                let reading = match self.get_sub_reading(reading_i, rsub_reading) {
                    Some(r) => r,
                    None => {
                        let rr = self.store.readings.get_mut(reading_i.0);
                        rr.matched_target = false;
                        rr.matched_tests = false;
                        i += 1;
                        continue;
                    }
                };
                {
                    let f = self.context_stack.last_mut().unwrap();
                    f.target.reading = Some(reading_i);
                    f.target.subreading = Some(reading);
                }
                {
                    let r = self.store.readings.get_mut(reading.0);
                    r.matched_target = false;
                    r.matched_tests = false;
                }

                let (r_mapped, r_noprint, r_immutable, r_hash_plain, r_hash, r_number) = {
                    let r = self.store.readings.get(reading.0);
                    (
                        r.mapped,
                        r.noprint,
                        r.immutable,
                        r.hash_plain,
                        r.hash,
                        r.number,
                    )
                };
                if r_mapped && (rtype0 == K_MAP || rtype0 == K_ADD || rtype0 == K_REPLACE) {
                    i += 1;
                    continue;
                }
                if r_mapped && (rflags.intersects(RF_NOMAPPED)) {
                    i += 1;
                    continue;
                }
                if r_noprint && !self.allow_magic_readings {
                    i += 1;
                    continue;
                }
                if r_immutable && rtype0 != K_UNPROTECT {
                    if matches!(
                        rtype0,
                        K_PROTECT
                            | K_ADD
                            | K_MAP
                            | K_REPLACE
                            | K_SELECT
                            | K_REMOVE
                            | K_IFF
                            | K_SUBSTITUTE
                            | K_UNMAP
                    ) {
                        num_active += 1;
                    }
                    if r#type == K_SELECT {
                        let r = self.store.readings.get_mut(reading.0);
                        r.matched_target = true;
                        r.matched_tests = true;
                        reading_contexts.push(self.context_stack.last().unwrap().clone());
                    }
                    num_iff += 1;
                    num_immutable += 1;
                    i += 1;
                    continue;
                }

                // Plain-signature cache.
                did_test = false;
                if !set_type.intersects(ST_SPECIAL | ST_MAPPING | ST_CHILD_UNIFY)
                    && !self.readings_plain.is_empty()
                    && let Some(&cached) = self.readings_plain.get(&r_hash_plain) {
                        let (mt, mtst) = {
                            let cr = self.store.readings.get(cached.0);
                            (cr.matched_target, cr.matched_tests)
                        };
                        {
                            let r = self.store.readings.get_mut(reading.0);
                            r.matched_target = mt;
                            r.matched_tests = mtst;
                        }
                        if mtst {
                            num_active += 1;
                        }
                        let cnum = self.store.readings.get(cached.0).number;
                        if let Some(&rgc) = self.regexgrps_c.get(&cnum) {
                            self.regexgrps_c.insert(r_number, rgc);
                            let z = *self.regexgrps_z.get(&cnum).unwrap();
                            self.regexgrps_z.insert(r_number, z);
                            let f = self.context_stack.last_mut().unwrap();
                            f.regexgrp_ct = z;
                            f.regexgrps = Some(rgc);
                        }
                        let ut = self.unif_tags_rs.get(&r_hash_plain).copied();
                        let us = self.unif_sets_rs.get(&r_hash_plain).copied();
                        {
                            let f = self.context_stack.last_mut().unwrap();
                            f.unif_tags = ut;
                            f.unif_sets = us;
                        }
                        test_good = mtst;
                        reading_contexts.push(self.context_stack.last().unwrap().clone());
                        i += 1;
                        continue;
                    }

                // Fresh per-reading regex/unif state (store INDICES, wave 4).
                {
                    let rgs = self.used_regex;
                    let uts = used_unif;
                    let uss = used_unif;
                    {
                        let f = self.context_stack.last_mut().unwrap();
                        f.regexgrp_ct = 0;
                        f.regexgrps = Some(rgs);
                        f.unif_tags = Some(uts);
                        f.unif_sets = Some(uss);
                    }
                    self.unif_tags_rs.insert(r_hash_plain, uts);
                    self.unif_sets_rs.insert(r_hash_plain, uss);
                    self.unif_tags_rs.insert(r_hash, uts);
                    self.unif_sets_rs.insert(r_hash, uss);
                    used_unif += 1;
                    self.unif_tags_store[uts].clear();
                    self.unif_sets_store[uss].clear();
                }

                self.unif_last_wordform = TagHash(0);
                self.unif_last_baseform = TagHash(0);
                self.unif_last_textual = TagHash(0);
                self.same_basic = r_hash_plain;
                self.rule_target = None;
                self.context_target = None;
                if self.context_stack.len() > 1 {
                    let m = self.context_stack[self.context_stack.len() - 2].mark;
                    if m.is_some() {
                        self.set_mark(m);
                    } else {
                        self.set_mark(Some(cohort));
                    }
                } else {
                    self.set_mark(Some(cohort));
                }
                let orz = self.context_stack.last().unwrap().regexgrp_ct;
                {
                    let mut rc = Some(reading_i);
                    while let Some(r) = rc {
                        self.store.readings.get_mut(r.0).active = true;
                        rc = self.store.readings.get(r.0).next;
                    }
                }
                self.rule_target = Some(cohort);

                // First check: does the rule target match?
                let target_matches = rtarget.get() != 0 && {
                    let bypass = set_type.intersects(ST_CHILD_UNIFY | ST_SPECIAL);
                    self.does_set_match_reading(reading, rtarget.get(), bypass, false)
                };
                if target_matches {
                    let mut regex_prop = true;
                    if orz != self.context_stack.last().unwrap().regexgrp_ct {
                        did_test = false;
                        regex_prop = false;
                    }
                    self.rule_target = Some(cohort);
                    self.context_target = Some(cohort);
                    self.store.readings.get_mut(reading.0).matched_target = true;
                    matched_target = true;
                    let mut good = true;
                    if !did_test {
                        self.context_stack.last_mut().unwrap().context.clear();
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
                            self.seen_barrier = false;
                            self.dep_deep_seen.clear();
                            for d in self.ci_depths.iter_mut() {
                                *d = 0;
                            }
                            self.tmpl_cntx = crate::grammar_applicator::tmpl_context_t::default();
                            let tpos = self.grammar.contexts_arena[test.0].pos;
                            let mut result: Option<CohortId> = None;
                            let with_deep = rtype0 == K_WITH;
                            if with_deep {
                                self.merge_with = None;
                            }
                            let mut deep_ref: Option<&mut Option<CohortId>> =
                                if with_deep { Some(&mut result) } else { None };
                            let next_test = if !tpos.intersects(POS_PASS_ORIGIN)
                                && (self.no_pass_origin || (tpos.intersects(POS_NO_PASS_ORIGIN)))
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
                            let ctx_push = if self.merge_with.is_some() {
                                self.merge_with
                            } else {
                                result
                            };
                            self.context_stack
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
                            let (ut_empty, us_empty) = {
                                let f = self.context_stack.last().unwrap();
                                (
                                    f.unif_tags
                                        .map(|i| self.unif_tags_store[i].is_empty())
                                        .unwrap_or(true),
                                    f.unif_sets
                                        .map(|i| self.unif_sets_store[i].is_empty())
                                        .unwrap_or(true),
                                )
                            };
                            did_test = !set_type.intersects(ST_CHILD_UNIFY | ST_SPECIAL)
                                && ut_empty
                                && us_empty;
                            ti += 1;
                        }
                    } else {
                        good = test_good;
                    }
                    if good {
                        // Iff → Select once a context matches.
                        if rtype0 == K_IFF && r#type != K_SELECT {
                            r#type = K_SELECT;
                            if self.grammar.has_protect {
                                let mut j = 0usize;
                                while j < i {
                                    let rj = self.store.cohorts.get(cohort.0).readings[j];
                                    if let Some(sr) = self.get_sub_reading(rj, rsub_reading)
                                        && self.store.readings.get(sr.0).immutable {
                                            let r = self.store.readings.get_mut(sr.0);
                                            r.matched_target = true;
                                            r.matched_tests = true;
                                            num_active += 1;
                                            num_iff += 1;
                                        }
                                    j += 1;
                                }
                            }
                        }
                        self.store.readings.get_mut(reading.0).matched_tests = true;
                        num_active += 1;
                        if self.profiler.is_some() {
                            // Profiler::Key k{ET_RULE, rule.number + 1}; ++entries[k].num_match
                            let rnum = self.grammar.rule_by_number.get(rule.0).number;
                            let k = crate::profiler::Key {
                                r#type: crate::profiler::ET_RULE,
                                id: rnum + 1,
                            };
                            let p = self.profiler.as_mut().unwrap();
                            let e = p.entries.entry(k).or_default();
                            e.num_match += 1;
                            if e.example_window == 0 {
                                self.add_profiling_example(k);
                            }
                        }
                        if !self.debug_rules.empty() && self.debug_rules.contains(rline) {
                            self.rr_print_debug_rule(rule, true, true);
                        }
                        // Propagate regex captures from a prior reading.
                        if regex_prop && i != 0 && !self.regexgrps_c.is_empty() {
                            let mut z = i;
                            while z > 0 {
                                let prev = self.store.cohorts.get(cohort.0).readings[z - 1];
                                let prev_num = self.store.readings.get(prev.0).number;
                                if let Some(&rgc) = self.regexgrps_c.get(&prev_num) {
                                    self.regexgrps_c.insert(r_number, rgc);
                                    let zz = *self.regexgrps_z.get(&prev_num).unwrap();
                                    self.regexgrps_z.insert(r_number, zz);
                                    break;
                                }
                                z -= 1;
                            }
                        }
                    } else {
                        self.context_stack.last_mut().unwrap().regexgrp_ct = orz;
                        if !self.debug_rules.empty() && self.debug_rules.contains(rline) {
                            self.rr_print_debug_rule(rule, true, false);
                        }
                    }
                    num_iff += 1;
                } else {
                    self.context_stack.last_mut().unwrap().regexgrp_ct = orz;
                    if self.profiler.is_some() {
                        // Profiler::Key k{ET_RULE, rule.number + 1}; ++entries[k].num_fail
                        let rnum = self.grammar.rule_by_number.get(rule.0).number;
                        let k = crate::profiler::Key {
                            r#type: crate::profiler::ET_RULE,
                            id: rnum + 1,
                        };
                        let p = self.profiler.as_mut().unwrap();
                        p.entries.entry(k).or_default().num_fail += 1;
                    }
                    if !self.debug_rules.empty() && self.debug_rules.contains(rline) {
                        self.rr_print_debug_rule(rule, false, false);
                    }
                }

                self.readings_plain.insert(r_hash_plain, reading);
                {
                    let mut rc = Some(reading_i);
                    while let Some(r) = rc {
                        self.store.readings.get_mut(r.0).active = false;
                        rc = self.store.readings.get(r.0).next;
                    }
                }
                if reading != reading_i {
                    let (mt, mtst) = {
                        let r = self.store.readings.get(reading.0);
                        (r.matched_target, r.matched_tests)
                    };
                    let ri = self.store.readings.get_mut(reading_i.0);
                    ri.matched_target = mt;
                    ri.matched_tests = mtst;
                }
                let rgc_ct = self.context_stack.last().unwrap().regexgrp_ct;
                if rgc_ct != 0 {
                    let rgs = self.context_stack.last().unwrap().regexgrps.unwrap();
                    self.regexgrps_c.insert(r_number, rgs);
                    self.regexgrps_z.insert(r_number, rgc_ct);
                    self.used_regex += 1;
                }
                reading_contexts.push(self.context_stack.last().unwrap().clone());
                i += 1;
            }

            let (now_readings, now_removed, now_delayed, now_ignored) = {
                let cc = self.store.cohorts.get(cohort.0);
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
                self.store.cohorts.get_mut(cohort.0).r#type &= !CT_NUM_CURRENT;
            }

            // No valid targets → drop this cohort from the rule set.
            if num_active == 0 && (num_iff == 0 || rtype0 != K_IFF) {
                if num_immutable == 0 && !matched_target {
                    let ro = self.rocits[depth] - 1;
                    self.cs_mut(cohortset).erase_n(ro);
                    self.rocits[depth] = ro;
                }
                self.context_stack.pop();
                continue;
            }
            // All readings valid → nothing to do for Select / safe Remove.
            if num_active == self.store.cohorts.get(cohort.0).readings.len() {
                if r#type == K_SELECT {
                    self.context_stack.pop();
                    continue;
                }
                if r#type == K_REMOVE
                    && (!self.r#unsafe || (rflags.intersects(RF_SAFE)))
                    && !rflags.intersects(RF_UNSAFE)
                {
                    self.context_stack.pop();
                    continue;
                }
            }

            // Dispatch each matched reading.
            let mut broke = false;
            for ctx in reading_contexts.into_iter() {
                let (mt, mtst) = {
                    let sr = ctx.target.subreading.unwrap();
                    let r = self.store.readings.get(sr.0);
                    (r.matched_target, r.matched_tests)
                };
                if !mt {
                    continue;
                }
                if !mtst && rtype0 != K_IFF {
                    continue;
                }
                *self.context_stack.last_mut().unwrap() = ctx;
                self.reset_cohorts_for_loop = false;
                self.reading_cb_dispatch(st);
                if !self.finish_cohort_loop {
                    self.context_stack.pop();
                    return anything_changed;
                }
                if self.reset_cohorts_for_loop {
                    cohortset = self.rr_reset_cohorts(current, rnumber);
                    broke = true;
                    break;
                }
                if !self.finish_reading_loop {
                    break;
                }
            }
            let _ = broke;

            self.reset_cohorts_for_loop = false;
            self.cohort_cb_dispatch(st);
            if !self.finish_cohort_loop {
                self.context_stack.pop();
                return anything_changed;
            }
            if self.reset_cohorts_for_loop {
                cohortset = self.rr_reset_cohorts(current, rnumber);
            }
            self.context_stack.pop();
        }
        anything_changed
    }

    /// `ignore_cohort(cohort)` lambda of `runSingleRule`: mark a cohort
    /// `CT_IGNORED`, hit_by its readings, erase it from every rule's cohortset,
    /// detach it, and remove it from the window's `cohorts` (kept in `all_cohorts`).
    pub(crate) fn rr_ignore_cohort(&mut self, rule_number: u32, cohort: CohortId) {
        let current = self.store.cohorts.get(cohort.0).parent.unwrap();
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            self.store.readings.get_mut(r.0).hit_by.push(rule_number);
        }
        // Erase from every rule's cohortset.
        self.rr_erase_from_all_cohortsets(current, cohort);
        {
            let c = self.store.cohorts.get_mut(cohort.0);
            c.r#type |= CT_IGNORED;
        }
        crate::cohort::detach(&mut self.store, cohort);
        let gn = self.store.cohorts.get(cohort.0).global_number;
        self.gWindow.cohort_map.remove(&gn);
        let ln = self.store.cohorts.get(cohort.0).local_number as usize;
        self.store
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
        let current = self.store.cohorts.get(cohort.0).parent.unwrap();
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            let rr = self.store.readings.get_mut(r.0);
            rr.hit_by.push(rule_number);
            rr.deleted = true;
            if self.trace {
                rr.noprint = false;
            }
        }
        self.rr_erase_from_all_cohortsets(current, cohort);
        // Forward children to the parent.
        loop {
            let ch = {
                let dc = &self.store.cohorts.get(cohort.0).dep_children;
                if dc.empty() {
                    break;
                }
                dc.back()
            };
            let dp = self.store.cohorts.get(cohort.0).dep_parent;
            let parent_key = dp.unwrap_or(GlobalNumber(0));
            let (pc, cc) = (
                self.gWindow.cohort_map.get(&parent_key).copied(),
                self.gWindow.cohort_map.get(&GlobalNumber(ch)).copied(),
            );
            if let (Some(pc), Some(cc)) = (pc, cc) {
                self.attach_parent_child(pc, cc, true, true);
            }
            self.store.cohorts.get_mut(cohort.0).dep_children.erase(ch);
        }
        self.store.cohorts.get_mut(cohort.0).r#type |= CT_REMOVED;
        crate::cohort::detach(&mut self.store, cohort);
        let dep_self = self
            .store
            .cohorts
            .get(cohort.0)
            .dep_self
            .map_or(0, |g| g.get());
        let keys: Vec<GlobalNumber> = self.gWindow.cohort_map.keys().copied().collect();
        for k in keys {
            let cid = *self.gWindow.cohort_map.get(&k).unwrap();
            self.store
                .cohorts
                .get_mut(cid.0)
                .dep_children
                .erase(dep_self);
        }
        let gn = self.store.cohorts.get(cohort.0).global_number;
        self.gWindow.cohort_map.remove(&gn);
        let ln = self.store.cohorts.get(cohort.0).local_number as usize;
        self.store
            .single_windows
            .get_mut(current.0)
            .cohorts
            .remove(ln);
        self.rr_renumber(current);

        // Window emptied (only >>> left) and not the active window → drop it.
        if self.store.single_windows.get(current.0).cohorts.len() == 1
            && Some(current) != self.gWindow.current
        {
            let empty_cohort = self.store.single_windows.get(current.0).cohorts[0];
            self.rr_erase_from_all_cohortsets(current, empty_cohort);
            crate::cohort::detach(&mut self.store, empty_cohort);
            let ds = self
                .store
                .cohorts
                .get(empty_cohort.0)
                .dep_self
                .map_or(0, |g| g.get());
            let keys: Vec<GlobalNumber> = self.gWindow.cohort_map.keys().copied().collect();
            for k in keys {
                let cid = *self.gWindow.cohort_map.get(&k).unwrap();
                self.store.cohorts.get_mut(cid.0).dep_children.erase(ds);
            }
            let egn = self.store.cohorts.get(empty_cohort.0).global_number;
            self.gWindow.cohort_map.remove(&egn);
            let opt = Some(empty_cohort);
            crate::cohort::free_cohort(&mut self.store, Some(&mut self.gWindow), opt);
            // if (current.previous) { previous->text += current.text + text_post;
            //   previous->all_cohorts += current.all_cohorts[1..]; }
            // else if (current.next) { next->text = text_post + next->text;
            //   next->all_cohorts.insert(begin+1, current.all_cohorts[1..]); }
            let (prev, next) = {
                let sw = self.store.single_windows.get(current.0);
                (sw.previous, sw.next)
            };
            if let Some(prev) = prev {
                let (text, text_post, rest) = {
                    let sw = self.store.single_windows.get(current.0);
                    (
                        sw.text.clone(),
                        sw.text_post.clone(),
                        sw.all_cohorts.iter().skip(1).copied().collect::<Vec<_>>(),
                    )
                };
                {
                    let psw = self.store.single_windows.get_mut(prev.0);
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
                    self.store.cohorts.get_mut(c.0).parent = Some(prev);
                }
            } else if let Some(next) = next {
                let (text_post, rest) = {
                    let sw = self.store.single_windows.get(current.0);
                    (
                        sw.text_post.clone(),
                        sw.all_cohorts.iter().skip(1).copied().collect::<Vec<_>>(),
                    )
                };
                {
                    let nsw = self.store.single_windows.get_mut(next.0);
                    let mut t = text_post;
                    t.push_str(&nsw.text);
                    nsw.text = t;
                    let at = 1.min(nsw.all_cohorts.len());
                    nsw.all_cohorts.splice(at..at, rest.iter().copied());
                }
                for c in rest {
                    self.store.cohorts.get_mut(c.0).parent = Some(next);
                }
            }
            self.store
                .single_windows
                .get_mut(current.0)
                .all_cohorts
                .clear();
            // Remove `current` from gWindow.previous / next.
            if let Some(pos) = self.gWindow.previous.iter().position(|&s| s == current) {
                let opt = Some(current);
                crate::single_window::free_swindow(&mut self.gWindow, &mut self.store, opt);
                self.gWindow.previous.remove(pos);
            }
            if let Some(pos) = self.gWindow.next.iter().position(|&s| s == current) {
                let opt = Some(current);
                crate::single_window::free_swindow(&mut self.gWindow, &mut self.store, opt);
                self.gWindow.next.remove(pos);
            }
            let gw = &mut self.gWindow;
            gw.rebuild_single_window_links(&mut self.store);
        }
        let gw = &mut self.gWindow;
        gw.rebuild_cohort_links(&mut self.store);
    }

    /// Renumber `current.cohorts[i].local_number = i` (the C++ `foreach` after a
    /// `cohorts.erase(...)`).
    pub(crate) fn rr_renumber(&mut self, current: SwId) {
        let n = self.store.single_windows.get(current.0).cohorts.len();
        for k in 0..n {
            let cid = self.store.single_windows.get(current.0).cohorts[k];
            self.store.cohorts.get_mut(cid.0).local_number = ui32(k);
        }
    }

    /// Snapshot the global `variables` map's live `(key, value)` entries in slot
    /// order (the C++ `for (auto& kv : variables)` iteration). Lets the REMVARIABLE
    /// branch scan while mutating `self`.
    pub(crate) fn variables_entries(&self) -> Vec<(u32, u32)> {
        self.variables.iter().copied().collect()
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
