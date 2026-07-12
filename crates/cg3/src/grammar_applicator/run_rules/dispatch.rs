//! `GrammarApplicator` — the reading/cohort callbacks (the C++ RuleCallback bodies) and the per-rule-type actions.
//!
//! Split out of the wave-2 monolithic `run_rules.rs` (wave 4, w4-file-split-fmt).

use crate::arena::{CohortId, CtxId, ReadingId, RuleId, TagId};
use crate::cohort::CohortSet;
use crate::inlines::insert_if_exists;
use crate::reading::ReadingList;
use crate::rule::{
    RF_AFTER, RF_ALLOWCROSS, RF_ALLOWLOOP, RF_DELAYED, RF_IGNORED, RF_NEAREST, RF_OUTPUT,
    RF_REPEAT, RF_REVERSE, RF_SAFE, RF_UNMAPLAST, RF_UNSAFE,
};
use crate::strings::KEYWORDS::{self};
use crate::tag::{T_BASEFORM, T_MAPPING, T_SPECIAL, T_VARSTRING, T_WORDFORM, TagList};
use crate::types::TagHash;

// C++ anonymous `enum { RV_NOTHING = 1, RV_SOMETHING = 2, RV_DELIMITED = 4,
// RV_TRACERULE = 8 };` — the return-value bit flags of runRulesOnSingleWindow.

use super::*;

impl crate::grammar_applicator::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    /// C++ `cohort_cb` lambda of `runRulesOnSingleWindow` — the per-cohort action
    /// invoked once after all matched readings have been through `reading_cb`.
    /// Dispatches SELECT/REMOVE finalisation, IFF, JUMP, REM/SETVARIABLE, DELIMIT,
    /// EXTERNAL, and REMCOHORT. `&mut RRState` carries the mutable rule-loop state.
    pub(crate) fn cohort_cb_dispatch(&mut self, st: &mut RRState) {
        let rule = st.rule;
        let rtype = self.grammar.rule_by_number.get(rule.0).r#type;
        let rflags = self.grammar.rule_by_number.get(rule.0).flags;
        let rnumber = self.grammar.rule_by_number.get(rule.0).number;
        let rsub_reading = self.grammar.rule_by_number.get(rule.0).sub_reading;

        if rtype == K_SELECT || (rtype == K_IFF && !st.selected.is_empty()) {
            let target = self.get_apply_to().cohort.unwrap();
            let treadings = self.store.cohorts.get(target.0).readings.len();
            if st.selected.len() < treadings && !st.selected.is_empty() {
                let mut drop: ReadingList = Vec::new();
                let mut si = 0usize;
                let readings = self.store.cohorts.get(target.0).readings.clone();
                for rd_orig in readings.iter().copied() {
                    let mut rd = rd_orig;
                    if rsub_reading != GSR_ANY {
                        if let Some(sr) = self.get_sub_reading(rd, rsub_reading) {
                            rd = sr;
                        } else {
                            // rd stays; C++ leaves `rd` at the sub or nullptr — if
                            // null, the hit_by push is skipped.
                        }
                    }
                    // Manually trace non-matching readings.
                    let sr_opt = if rsub_reading != GSR_ANY {
                        self.get_sub_reading(rd_orig, rsub_reading)
                    } else {
                        Some(rd)
                    };
                    if let Some(sr) = sr_opt {
                        self.store.readings.get_mut(sr.0).hit_by.push(rnumber);
                    }
                    if si < st.selected.len() && rd_orig == st.selected[si] {
                        si += 1;
                    } else {
                        self.store.readings.get_mut(rd_orig.0).deleted = true;
                        drop.push(rd_orig);
                    }
                }
                // target->readings.swap(selected)
                {
                    let sel = st.selected.clone();
                    self.store.cohorts.get_mut(target.0).readings = sel;
                }
                if rflags.intersects(RF_DELAYED) {
                    self.store
                        .cohorts
                        .get_mut(target.0)
                        .delayed
                        .extend(drop.iter().copied());
                } else if rflags.intersects(RF_IGNORED) {
                    self.store
                        .cohorts
                        .get_mut(target.0)
                        .ignored
                        .extend(drop.iter().copied());
                } else {
                    self.store
                        .cohorts
                        .get_mut(target.0)
                        .deleted
                        .extend(drop.iter().copied());
                }
                st.readings_changed = true;
            }
            st.selected.clear();
        } else if rtype == K_REMOVE || rtype == K_IFF {
            let target = self.get_apply_to().cohort.unwrap();
            let treadings = self.store.cohorts.get(target.0).readings.len();
            let cond = !st.removed.is_empty()
                && (st.removed.len() < treadings
                    || (self.r#unsafe && !rflags.intersects(RF_SAFE))
                    || rflags.intersects(RF_UNSAFE));
            if cond {
                if rflags.intersects(RF_DELAYED) {
                    self.store
                        .cohorts
                        .get_mut(target.0)
                        .delayed
                        .extend(st.removed.iter().copied());
                } else if rflags.intersects(RF_IGNORED) {
                    self.store
                        .cohorts
                        .get_mut(target.0)
                        .ignored
                        .extend(st.removed.iter().copied());
                } else {
                    self.store
                        .cohorts
                        .get_mut(target.0)
                        .deleted
                        .extend(st.removed.iter().copied());
                }
                let mut oz = self.store.cohorts.get(target.0).readings.len();
                while let Some(back) = st.removed.last().copied() {
                    self.store.readings.get_mut(back.0).deleted = true;
                    let mut k = 0usize;
                    while k < oz {
                        if self.store.cohorts.get(target.0).readings[k] == back {
                            oz -= 1;
                            self.store.cohorts.get_mut(target.0).readings.swap(k, oz);
                        }
                        k += 1;
                    }
                    st.removed.pop();
                }
                self.store.cohorts.get_mut(target.0).readings.truncate(oz);
                st.readings_changed = true;
            }
            if self.store.cohorts.get(target.0).readings.is_empty() {
                self.init_empty_cohort(target);
            }
            st.selected.clear();
        } else if rtype == K_JUMP {
            let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
            if let Some(to) = maplist.and_then(|ml| self.rr_first_taglist_tag(ml)) {
                let to_hash = self.grammar.single_tags_list.get(to.0).hash;
                let anchor = self.grammar.anchors.find(to_hash.get());
                if anchor == self.grammar.anchors.end() {
                    // Warning: JUMP could not find anchor (I/O omitted).
                } else {
                    let dest = anchor.get().1;
                    let lb = st.intersects.lower_bound(dest);
                    st.iter_val = if lb == st.intersects.end() {
                        st.iter_val
                    } else {
                        lb.value()
                    };
                    self.finish_cohort_loop = false;
                    st.should_repeat = true;
                }
            }
        } else if rtype == K_REMVARIABLE {
            let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
            if let Some(ml) = maplist {
                let names = self.get_tag_list_of_set(ml, false);
                let current = st.current;
                for tag0 in names {
                    let mut tag = tag0;
                    let ttype = self.grammar.single_tags_list.get(tag.0).r#type;
                    if ttype.intersects(T_VARSTRING) {
                        tag = self.generate_varstring_tag_id(tag);
                    }
                    let (tt, th) = {
                        let t = self.grammar.single_tags_list.get(tag.0);
                        (t.r#type, t.hash)
                    };
                    let found: Option<u32> = if tt.intersects(crate::tag::T_REGEXP) {
                        let tagv = self.grammar.single_tags_list.get(tag.0).clone();
                        let vars: Vec<(u32, u32)> = self.variables_entries();
                        let mut f = None;
                        for (k, _) in vars {
                            if self.does_tag_match_regexp(k, &tagv, false) != 0 {
                                f = Some(k);
                                break;
                            }
                        }
                        f
                    } else if tt.intersects(crate::tag::T_CASE_INSENSITIVE) {
                        let tagv = self.grammar.single_tags_list.get(tag.0).clone();
                        let vars: Vec<(u32, u32)> = self.variables_entries();
                        let mut f = None;
                        for (k, _) in vars {
                            if self.does_tag_match_icase(k, &tagv, false) != 0 {
                                f = Some(k);
                                break;
                            }
                        }
                        f
                    } else if self.variables.find(th.get()) != self.variables.end() {
                        Some(th.get())
                    } else {
                        None
                    };
                    if let Some(key) = found {
                        if rflags.intersects(RF_OUTPUT) {
                            self.store
                                .single_windows
                                .get_mut(current.0)
                                .variables_output
                                .insert(key);
                        }
                        self.variables.erase(key);
                    }
                }
            }
        } else if rtype == K_SETVARIABLE {
            let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
            let sublist = self.grammar.rule_by_number.get(rule.0).sublist;
            let name = maplist.and_then(|ml| self.rr_first_taglist_tag(ml));
            let value = sublist.and_then(|sl| self.rr_first_taglist_tag(sl));
            if let (Some(name), Some(value)) = (name, value) {
                let nh = self.grammar.single_tags_list.get(name.0).hash;
                let vh = self.grammar.single_tags_list.get(value.0).hash;
                // C++ `variables[nh] = vh` overwrites; flat `insert()` does not.
                *self.variables.index_or_insert(nh.get()) = vh.get();
                if rflags.intersects(RF_OUTPUT) {
                    self.store
                        .single_windows
                        .get_mut(st.current.0)
                        .variables_output
                        .insert(nh.get());
                }
            }
        } else if rtype == K_DELIMIT {
            let cohort = self.get_apply_to().cohort.unwrap();
            let (parent, ln) = {
                let c = self.store.cohorts.get(cohort.0);
                (c.parent.unwrap(), c.local_number)
            };
            if (self.store.single_windows.get(parent.0).cohorts.len() as u32) > ln + 1 {
                self.delimit_at(st.current, cohort);
                st.delimited = true;
                st.readings_changed = true;
            }
        } else if rtype == K_EXTERNAL_ONCE || rtype == K_EXTERNAL_ALWAYS {
            let current = st.current;
            let rline = self.grammar.rule_by_number.get(rule.0).line;
            if rtype == K_EXTERNAL_ONCE {
                // .insert(...).second — true iff newly inserted.
                if !self
                    .store
                    .single_windows
                    .get_mut(current.0)
                    .hit_external
                    .insert(rline)
                    .1
                {
                    return;
                }
            }
            // C++ `src/version.hpp`: constexpr uint32_t CG3_EXTERNAL_PROTOCOL = 7226;
            const CG3_EXTERNAL_PROTOCOL: u32 = 7226;

            // auto ei = externals.find(rule->varname); if miss, spawn the child
            // and handshake the protocol revision.
            let varname = self.grammar.rule_by_number.get(rule.0).varname;
            if !self.externals.contains_key(&varname) {
                // Tag* ext = grammar->single_tags.find(rule->varname)->second;
                // u_strToUTF8(cbuffers[0], ...) — the UTF-8 port uses the tag
                // text directly (the C++ CG3_BUFFER_SIZE-1 truncation elided).
                let ext_tid = {
                    let it = self.grammar.single_tags.find(varname);
                    it.get().1
                };
                let cmd = self.grammar.single_tags_list.get(ext_tid.0).tag.clone();

                // Process& es = externals[rule->varname]; es.start(...);
                // writeRaw(es, CG3_EXTERNAL_PROTOCOL); — a throw is caught as
                // "Error: External on line %u resulted in error: %s" + CG3Quit(1).
                let mut es = crate::process::Process::new();
                if let Err(e) = es.start(&cmd) {
                    tracing::error!("Error: External on line {} resulted in error: {}", rline, e);
                    crate::inlines::cg3_quit(1, None, 0);
                }
                // writeRaw(es, CG3_EXTERNAL_PROTOCOL) — raw host-order u32.
                if let Err(e) = es.write(&CG3_EXTERNAL_PROTOCOL.to_ne_bytes(), 4) {
                    tracing::error!("Error: External on line {} resulted in error: {}", rline, e);
                    crate::inlines::cg3_quit(1, None, 0);
                }
                self.externals.insert(varname, es);
            }

            // pipeOutSingleWindow(current, ei->second);
            // pipeInSingleWindow(current, ei->second);
            // C++ holds `Process&` into the map; the port lifts the Process out
            // of self.externals (and the store out of self) for the duration of
            // the round-trip to satisfy the borrow checker, then restores both.
            let mut es = self.externals.remove(&varname).expect("external process");
            self.pipe_out_single_window(current, &mut es);
            self.pipe_in_single_window(current, &mut es);
            self.externals.insert(varname, es);

            self.index_single_window(current);
            st.readings_changed = true;
            self.index_ruleCohort_no.clear(0);
            st.intersects = self
                .store
                .single_windows
                .get(current.0)
                .valid_rules
                .intersect(&st.rules);
            let lb = st.intersects.find(rnumber);
            st.iter_val = if lb == st.intersects.end() {
                st.iter_val
            } else {
                lb.value()
            };
            self.reset_cohorts_for_loop = true;
        } else if rtype == K_REMCOHORT {
            let apply = self.get_apply_to().cohort.unwrap();
            if rflags.intersects(RF_IGNORED) {
                let childset1 = self.grammar.rule_by_number.get(rule.0).childset1.get();
                let mut cohorts = CohortSet::new();
                self.rr_collect_subtree(st.current, &mut cohorts, apply, childset1);
                for c in cohorts.iter_rev().copied().collect::<Vec<_>>() {
                    self.rr_ignore_cohort(rnumber, c);
                }
                self.rr_reindex(st.current);
                self.reflow_dependency_window(0);
            } else {
                self.rr_rem_cohort(rnumber, apply);
            }
            // If we removed the last cohort, add <<< to the new last cohort.
            let apply_front = self.store.cohorts.get(apply.0).readings[0];
            let has_endtag = {
                let r = self.store.readings.get(apply_front.0);
                r.tags.find(self.endtag.get()) != r.tags.end()
            };
            if has_endtag {
                let back = *self
                    .store
                    .single_windows
                    .get(st.current.0)
                    .cohorts
                    .last()
                    .unwrap();
                let rs = self.store.cohorts.get(back.0).readings.clone();
                let endtag = self.tag_by_hash(self.endtag);
                for r in rs {
                    self.add_tag_to_reading(r, endtag);
                    if self.update_valid_rules(
                        &st.rules.clone(),
                        &mut st.intersects,
                        self.endtag.get(),
                        r,
                    ) {
                        st.iter_val = rnumber;
                    }
                }
                self.index_ruleCohort_no.clear(0);
            }
            st.readings_changed = true;
            self.reset_cohorts_for_loop = true;
        }
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    /// C++ `reading_cb` lambda of `runRulesOnSingleWindow` — the per-matched-reading
    /// action. Dispatches every reading-level rule type. `&mut RRState` carries the
    /// mutable rule-loop state (`removed`/`selected`/`intersects`/`iter_val`/…).
    ///
    /// The heavier window-restructuring types (ADDCOHORT, SPLITCOHORT, MERGECOHORTS,
    /// COPYCOHORT, MOVE/SWITCH, the dependency/relation family) are delegated to
    /// `rr_*` helper methods so this dispatcher stays a readable jump table.
    pub(crate) fn reading_cb_dispatch(&mut self, st: &mut RRState) {
        let rule = st.rule;
        let (rtype, rflags, rnumber, rsub_reading) = {
            let r = self.grammar.rule_by_number.get(rule.0);
            (r.r#type, r.flags, r.number, r.sub_reading)
        };

        if rtype == K_SELECT || (rtype == K_IFF && self.apply_to_matched_tests()) {
            let r = self.get_apply_to().reading.unwrap();
            st.selected.push(r);
            self.index_ruleCohort_no.clear(0);
        } else if rtype == K_REMOVE || rtype == K_IFF {
            let cohort_readings = self
                .store
                .cohorts
                .get(self.get_apply_to().cohort.unwrap().0)
                .readings
                .len();
            if rtype == K_REMOVE
                && (rflags.intersects(RF_UNMAPLAST))
                && st.removed.len() == cohort_readings - 1
            {
                let sr = self.get_apply_to().subreading.unwrap();
                if self.unmap_reading(sr, rnumber) {
                    st.readings_changed = true;
                }
            } else {
                self.trace(rnumber, rsub_reading);
                st.removed.push(self.get_apply_to().reading.unwrap());
            }
            self.index_ruleCohort_no.clear(0);
        } else if rtype == K_PROTECT {
            self.trace(rnumber, rsub_reading);
            self.store
                .readings
                .get_mut(self.get_apply_to().subreading.unwrap().0)
                .immutable = true;
        } else if rtype == K_UNPROTECT {
            self.trace(rnumber, rsub_reading);
            self.store
                .readings
                .get_mut(self.get_apply_to().subreading.unwrap().0)
                .immutable = false;
        } else if rtype == K_UNMAP {
            let sr = self.get_apply_to().subreading.unwrap();
            if self.unmap_reading(sr, rnumber) {
                self.index_ruleCohort_no.clear(0);
                st.readings_changed = true;
            }
        } else {
            self.reading_cb_rest(st, rule, rtype, rflags, rnumber, rsub_reading);
        }
    }

    /// Whether the apply-to subreading matched its tests — the C++
    /// `(rule->type == K_IFF && get_apply_to().subreading->matched_tests)` guard.
    fn apply_to_matched_tests(&self) -> bool {
        self.get_apply_to()
            .subreading
            .map(|sr| self.store.readings.get(sr.0).matched_tests)
            .unwrap_or(false)
    }

    /// The tail of [`Self::reading_cb_dispatch`] — the non-SELECT/REMOVE reading
    /// actions (ADD/MAP/RESTORE/REPLACE/SUBSTITUTE/APPEND/COPY/… and the cohort/
    /// dependency/relation families). Split out only to keep the top-level jump
    /// table small.
    fn reading_cb_rest(
        &mut self,
        st: &mut RRState,
        rule: RuleId,
        rtype: KEYWORDS,
        rflags: crate::rule::RuleFlags,
        rnumber: u32,
        rsub_reading: i32,
    ) {
        match rtype {
            K_SELECT | K_IFF => {
                // IFF-as-select with matched tests handled in the top dispatcher;
                // reaching here means IFF with unmatched tests → no-op.
            }
            K_ADDCOHORT_AFTER | K_ADDCOHORT_BEFORE => {
                self.index_ruleCohort_no.clear(0);
                self.trace(rnumber, rsub_reading);
                self.rr_addcohort(st, rule);
                st.readings_changed = true;
                self.reset_cohorts_for_loop = true;
            }
            K_SPLITCOHORT => {
                self.rr_splitcohort(st, rule);
            }
            K_ADD | K_MAP => {
                self.rr_add_map(st, rule, rtype, rflags, rnumber, rsub_reading);
            }
            K_RESTORE => {
                self.rr_restore(rule, rflags, rnumber, rsub_reading);
                self.finish_reading_loop = false;
            }
            K_REPLACE => {
                self.rr_replace(st, rule, rnumber, rsub_reading);
            }
            K_SUBSTITUTE => {
                self.rr_substitute(st, rule, rnumber, rsub_reading);
            }
            K_APPEND => {
                self.index_ruleCohort_no.clear(0);
                self.trace(rnumber, rsub_reading);
                self.rr_append(st, rule, rnumber);
                st.readings_changed = true;
                self.finish_reading_loop = false;
            }
            K_COPY => {
                self.rr_copy(st, rule, rnumber, rsub_reading);
            }
            K_MERGECOHORTS => {
                self.rr_mergecohorts(st, rule);
            }
            K_COPYCOHORT => {
                self.rr_copycohort(st, rule, rnumber);
            }
            K_SETPARENT | K_SETCHILD | K_ADDRELATION | K_SETRELATION | K_REMRELATION
            | K_ADDRELATIONS | K_SETRELATIONS | K_REMRELATIONS => {
                self.rr_dep_relation(st, rule, rtype, rnumber, rsub_reading);
                self.finish_reading_loop = false;
            }
            K_REMPARENT => {
                self.finish_reading_loop = false;
                self.trace(rnumber, rsub_reading);
                self.store
                    .cohorts
                    .get_mut(self.get_apply_to().cohort.unwrap().0)
                    .dep_parent = None;
            }
            K_SWITCHPARENT => {
                self.finish_reading_loop = false;
                self.trace(rnumber, rsub_reading);
                self.rr_switchparent(rule);
            }
            K_MOVE_AFTER | K_MOVE_BEFORE | K_SWITCH => {
                self.finish_reading_loop = false;
                self.rr_move_switch(st, rule, rtype, rnumber);
            }
            K_WITH => {
                self.trace(rnumber, rsub_reading);
                self.rr_with(st, rule, rflags);
                self.finish_reading_loop = false;
            }
            _ if rtype != K_REMCOHORT => {
                self.trace(rnumber, rsub_reading);
            }
            _ => {}
        }
    }

    /// `getTagList(*rule->maplist, theTags)` with `T_VSTR` pre-varstringify, then
    /// full varstringify per tag — the C++ prologue shared by ADDCOHORT/APPEND/
    /// SPLITCOHORT/COPYCOHORT. Returns the expanded, varstring-resolved tag list.
    pub(crate) fn rr_maplist_tags(&mut self, rule: RuleId) -> TagList {
        let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
        let mut out = TagList::new();
        if let Some(ml) = maplist {
            let raw = self.get_tag_list_of_set(ml, false);
            for &t0 in &raw {
                let mut t = t0;
                while self
                    .grammar
                    .single_tags_list
                    .get(t.0)
                    .r#type
                    .intersects(T_VARSTRING)
                {
                    t = self.generate_varstring_tag_id(t);
                }
                out.push(t);
            }
        }
        out
    }

    /// K_WITH: mark TRACE, then run each sub-rule (repeating while `RF_REPEAT`),
    /// aggregating `readings_changed`. `in_nested` is toggled around the block.
    fn rr_with(&mut self, st: &mut RRState, rule: RuleId, _rflags: crate::rule::RuleFlags) {
        let mut any_readings_changed = false;
        st.readings_changed = false;
        self.in_nested = true;
        let sub_rules: Vec<RuleId> = self.grammar.rule_by_number.get(rule.0).sub_rules.clone();
        let current = st.current;
        let cur_was = self.current_rule;
        // C++ `rule = sr` — the reading/cohort callbacks dispatch on the current
        // rule, so the nested sub-rule must be seated in the shared state too.
        let rule_was = st.rule;
        for sr in sub_rules {
            self.current_rule = Some(sr);
            st.rule = sr;
            let sr_flags = self.grammar.rule_by_number.get(sr.0).flags;
            loop {
                st.readings_changed = false;
                let result = self.run_single_rule(current, sr, st);
                any_readings_changed = any_readings_changed || result || st.readings_changed;
                if !((result || st.readings_changed) && (sr_flags.intersects(RF_REPEAT))) {
                    break;
                }
            }
        }
        self.current_rule = cur_was;
        st.rule = rule_was;
        self.in_nested = false;
        st.readings_changed = any_readings_changed;
    }

    /// K_SWITCHPARENT: reparent the target cohort above its current parent (and
    /// siblings) — the per-cohort dependency rotation.
    fn rr_switchparent(&mut self, rule: RuleId) {
        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1.get();
        let child = self.get_apply_to().cohort.unwrap();
        let current = self.store.cohorts.get(child.0).parent.unwrap();
        let child_dp = self.store.cohorts.get(child.0).dep_parent;
        let parent = *self.gWindow.cohort_map.get(&child_dp.unwrap()).unwrap();
        let parent_gn = self.store.cohorts.get(parent.0).global_number;
        let grandparent_number = self.store.cohorts.get(parent.0).dep_parent;
        let mut siblings: Vec<CohortId> = Vec::new();
        let cohorts = self.store.single_windows.get(current.0).cohorts.clone();
        for c in cohorts {
            if self.store.cohorts.get(c.0).dep_parent == Some(parent_gn)
                && self.does_set_match_cohort_normal(c, childset1, None)
            {
                siblings.push(c);
            }
        }
        self.store.cohorts.get_mut(child.0).dep_parent = None;
        self.store.cohorts.get_mut(parent.0).dep_parent = None;
        for &s in &siblings {
            self.store.cohorts.get_mut(s.0).dep_parent = None;
        }
        if let Some(&gp) = grandparent_number.and_then(|g| self.gWindow.cohort_map.get(&g)) {
            self.attach_parent_child(gp, child, false, false);
        }
        self.attach_parent_child(child, parent, false, false);
        for s in siblings {
            self.attach_parent_child(child, s, false, false);
        }
    }

    /// K_RESTORE: move readings back from `deleted`/`delayed`/`ignored` whose
    /// analysis matches `rule.maplist`.
    fn rr_restore(
        &mut self,
        rule: RuleId,
        rflags: crate::rule::RuleFlags,
        rnumber: u32,
        rsub_reading: i32,
    ) {
        let cohort = self.get_apply_to().cohort.unwrap();
        let maplist_num = self
            .grammar
            .rule_by_number
            .get(rule.0)
            .maplist
            .map(|s| self.grammar.sets_list[s.0].number.get())
            .unwrap_or(0);
        let which = if rflags.intersects(RF_DELAYED) {
            0
        } else if rflags.intersects(RF_IGNORED) {
            1
        } else {
            2
        };
        let mut did_restore = false;
        let list = match which {
            0 => self.store.cohorts.get(cohort.0).delayed.clone(),
            1 => self.store.cohorts.get(cohort.0).ignored.clone(),
            _ => self.store.cohorts.get(cohort.0).deleted.clone(),
        };
        let mut keep: ReadingList = Vec::new();
        for rid in list {
            if self.does_set_match_reading(rid, maplist_num, false, false) {
                {
                    let r = self.store.readings.get_mut(rid.0);
                    r.deleted = false;
                    r.hit_by.push(rnumber);
                }
                self.store.cohorts.get_mut(cohort.0).readings.push(rid);
                did_restore = true;
            } else {
                keep.push(rid);
            }
        }
        match which {
            0 => self.store.cohorts.get_mut(cohort.0).delayed = keep,
            1 => self.store.cohorts.get_mut(cohort.0).ignored = keep,
            _ => self.store.cohorts.get_mut(cohort.0).deleted = keep,
        }
        if did_restore {
            self.trace(rnumber, rsub_reading);
        }
    }

    /// C++ `APPEND_TAGLIST_TO_READING(taglist, reading)` macro: varstringify each
    /// tag, route mapping tags to `mappings`, else `addTagToReading`, then
    /// `updateValidRules`. Re-seats `iter_val` when the rule set grew.
    fn rr_append_taglist_to_reading(
        &mut self,
        st: &mut RRState,
        rnumber: u32,
        taglist: &TagList,
        reading: ReadingId,
        mappings: &mut TagList,
    ) {
        let mapping_prefix = self.grammar.mapping_prefix;
        for &t0 in taglist {
            let mut tter = t0;
            while self
                .grammar
                .single_tags_list
                .get(tter.0)
                .r#type
                .intersects(T_VARSTRING)
            {
                tter = self.generate_varstring_tag_id(tter);
            }
            let (ttype, thash, first) = {
                let t = self.grammar.single_tags_list.get(tter.0);
                (t.r#type, t.hash, t.tag.chars().next())
            };
            let mut hash = thash;
            if ttype.intersects(T_MAPPING) || first == Some(mapping_prefix) {
                mappings.push(tter);
            } else {
                hash = self.add_tag_to_reading(reading, tter);
            }
            if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash.get(), reading) {
                st.iter_val = rnumber;
            }
        }
    }

    /// C++ `FILL_TAG_LIST(taglist)` macro: keep only the pattern tags that appear
    /// in the apply-to reading, replacing `T_SPECIAL` ones with the concrete
    /// matched tag. Operates on the apply-to subreading.
    fn rr_fill_tag_list(&mut self, taglist: &mut TagList) {
        let reading = self.get_apply_to().subreading.unwrap();
        let mut out: TagList = Vec::new();
        for &tt in taglist.iter() {
            let (thash, ttype) = {
                let t = self.grammar.single_tags_list.get(tt.0);
                (t.hash, t.r#type)
            };
            let present = {
                let r = self.store.readings.get(reading.0);
                r.tags.find(thash.get()) != r.tags.end()
            };
            if present {
                out.push(tt);
            } else if ttype.intersects(T_SPECIAL) {
                let tagv = self.grammar.single_tags_list.get(tt.0).clone();
                let stag = self.does_tag_match_reading(reading, &tagv, false, true);
                if stag != 0 {
                    out.push(self.tag_by_hash(TagHash(stag)));
                }
            }
        }
        *taglist = out;
    }

    /// K_ADD / K_MAP.
    fn rr_add_map(
        &mut self,
        st: &mut RRState,
        rule: RuleId,
        rtype: KEYWORDS,
        rflags: crate::rule::RuleFlags,
        rnumber: u32,
        rsub_reading: i32,
    ) {
        self.trace(rnumber, rsub_reading);
        let reading = self.get_apply_to().subreading.unwrap();
        let state_hash = self.store.readings.get(reading.0).hash;
        self.index_ruleCohort_no.clear(0);
        self.store.readings.get_mut(reading.0).noprint = false;
        let mut mappings = TagList::new();
        let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
        let mut the_tags = TagList::new();
        if let Some(ml) = maplist {
            the_tags = self.get_tag_list_of_set(ml, false);
        }

        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1.get();
        let mut did_insert = false;
        if childset1 != 0 {
            let mut spot_tags = self.get_tag_list_of_set_number(childset1, false);
            self.rr_fill_tag_list(&mut spot_tags);
            // Find the spot in reading.tags_list matching all of spot_tags.
            let tags_list = self.store.readings.get(reading.0).tags_list.clone();
            let spot_hashes: Vec<u32> = spot_tags
                .iter()
                .map(|t| self.grammar.single_tags_list.get(t.0).hash.get())
                .collect();
            let mut found_at: Option<usize> = None;
            'outer_spot: for start in 0..tags_list.len() {
                for (k, &sh) in spot_hashes.iter().enumerate() {
                    if start + k >= tags_list.len() || tags_list[start + k] != sh {
                        continue 'outer_spot;
                    }
                }
                found_at = Some(start);
                break;
            }
            if let Some(mut at) = found_at {
                if rflags.intersects(RF_AFTER) {
                    at += spot_tags.len();
                }
                if at < self.store.readings.get(reading.0).tags_list.len() {
                    self.rr_insert_taglist_to_reading(st, at, &the_tags, reading, &mut mappings);
                    did_insert = true;
                }
            }
        }
        if !did_insert {
            self.rr_append_taglist_to_reading(st, rnumber, &the_tags, reading, &mut mappings);
        }
        if !mappings.is_empty() {
            let cohort = self.get_apply_to().cohort.unwrap();
            self.split_mappings(&mut mappings, cohort, reading, rtype == K_MAP);
        }
        if rtype == K_MAP {
            self.store.readings.get_mut(reading.0).mapped = true;
        }
        if self.store.readings.get(reading.0).hash != state_hash {
            st.readings_changed = true;
        }
    }

    /// K_REPLACE: replace the reading's whole tag list (wordform + maplist tags,
    /// preserving `SUBSTITUTE`-excepted tags and re-adding the baseform).
    fn rr_replace(&mut self, st: &mut RRState, rule: RuleId, rnumber: u32, rsub_reading: i32) {
        let reading = self.get_apply_to().subreading.unwrap();
        let cohort = self.get_apply_to().cohort.unwrap();
        let state_hash = self.store.readings.get(reading.0).hash;
        self.index_ruleCohort_no.clear(0);
        self.trace(rnumber, rsub_reading);
        self.store.readings.get_mut(reading.0).noprint = false;

        let mut excepts = TagList::new();
        let sublist = self.grammar.rule_by_number.get(rule.0).sublist;
        if let Some(sl) = sublist {
            let tags = self.get_tag_list_of_set(sl, false);
            self.get_tags_matching(reading, &tags, &mut excepts);
        }

        let wf_hash = {
            let wf = self.store.cohorts.get(cohort.0).wordform.unwrap();
            self.grammar.single_tags_list.get(wf.0).hash
        };
        {
            let r = self.store.readings.get_mut(reading.0);
            r.tags_list.clear();
            r.tags_list.push(wf_hash.get());
        }
        let bform = self
            .store
            .readings
            .get(reading.0)
            .baseform
            .unwrap_or(TagHash(0));
        self.store.readings.get_mut(reading.0).baseform = None;
        self.reflow_reading(reading);

        let mut mappings = TagList::new();
        let the_tags = {
            let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
            match maplist {
                Some(ml) => self.get_tag_list_of_set(ml, false),
                None => TagList::new(),
            }
        };
        self.rr_append_taglist_to_reading(st, rnumber, &the_tags, reading, &mut mappings);
        for tter in excepts {
            self.add_tag_to_reading(reading, tter);
        }
        if self.store.readings.get(reading.0).baseform.is_none() {
            let bf_tag = self.tag_by_hash(bform);
            self.add_tag_to_reading(reading, bf_tag);
        }
        if !mappings.is_empty() {
            self.split_mappings(&mut mappings, cohort, reading, true);
        }
        if self.store.readings.get(reading.0).hash != state_hash {
            st.readings_changed = true;
        }
    }

    /// K_APPEND: append fresh readings (each starting at a baseform) to the cohort.
    fn rr_append(&mut self, st: &mut RRState, rule: RuleId, rnumber: u32) {
        let cohort = self.get_apply_to().cohort.unwrap();
        let the_tags = self.rr_maplist_tags(rule);
        // Group tags into readings, each starting at a T_BASEFORM.
        let mut readings: Vec<TagList> = Vec::new();
        let mut have_bf = false;
        for tter in the_tags {
            let ttype = self.grammar.single_tags_list.get(tter.0).r#type;
            if ttype.intersects(T_BASEFORM) {
                have_bf = true;
                readings.push(TagList::new());
            }
            if !have_bf {
                // Error: baseform must come first (I/O omitted); skip.
                continue;
            }
            readings.last_mut().unwrap().push(tter);
        }
        let wordform = self.store.cohorts.get(cohort.0).wordform.unwrap();
        for rit in readings {
            let creading = crate::reading::alloc_reading(&mut self.store, Some(cohort));
            self.numReadings = self.numReadings.wrapping_add(1);
            let sets_any = self.grammar.sets_any.clone();
            insert_if_exists(
                &mut self.store.cohorts.get_mut(cohort.0).possible_sets,
                sets_any.as_ref(),
            );
            self.add_tag_to_reading(creading, wordform);
            {
                let r = self.store.readings.get_mut(creading.0);
                r.hit_by.push(rnumber);
                r.noprint = false;
            }
            let mut mappings = TagList::new();
            let mapping_prefix = self.grammar.mapping_prefix;
            for t0 in rit {
                let mut tter = t0;
                let mut hash = self.grammar.single_tags_list.get(tter.0).hash;
                while self
                    .grammar
                    .single_tags_list
                    .get(tter.0)
                    .r#type
                    .intersects(T_VARSTRING)
                {
                    tter = self.generate_varstring_tag_id(tter);
                }
                let (ttype, first) = {
                    let t = self.grammar.single_tags_list.get(tter.0);
                    (t.r#type, t.tag.chars().next())
                };
                if ttype.intersects(T_MAPPING) || first == Some(mapping_prefix) {
                    mappings.push(tter);
                } else {
                    hash = self.add_tag_to_reading(creading, tter);
                }
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash.get(), creading)
                {
                    st.iter_val = rnumber;
                }
            }
            if !mappings.is_empty() {
                self.split_mappings(&mut mappings, cohort, creading, false);
            }
            crate::cohort::append_reading(&mut self.store, cohort, creading);
        }
        // Drop noprint readings when more than one remains.
        if self.store.cohorts.get(cohort.0).readings.len() > 1 {
            let rs = self.store.cohorts.get(cohort.0).readings.clone();
            let mut keep: ReadingList = Vec::new();
            for r in rs {
                if self.store.readings.get(r.0).noprint {
                    let opt = Some(r);
                    crate::reading::free_reading(&mut self.store, opt);
                } else {
                    keep.push(r);
                }
            }
            self.store.cohorts.get_mut(cohort.0).readings = keep;
        }
    }

    /// K_COPY: clone the apply-to reading, then optionally strip `sublist` tags and
    /// splice in maplist tags (at a `childset1` spot or appended).
    fn rr_copy(&mut self, st: &mut RRState, rule: RuleId, rnumber: u32, _rsub_reading: i32) {
        let cohort = self.get_apply_to().cohort.unwrap();
        let src = self.get_apply_to().reading.unwrap();
        // C++ `allocateAppendReading(*get_apply_to().reading)` — exactly ONE
        // copy-construction (number + 100, one deep clone of the next chain).
        let src_snapshot = self.clone_reading_value(src);
        let creading =
            crate::cohort::allocate_append_reading_copy(&mut self.store, cohort, &src_snapshot);
        self.numReadings = self.numReadings.wrapping_add(1);
        self.index_ruleCohort_no.clear(0);
        self.trace_reading(creading, rnumber);
        {
            let r = self.store.readings.get_mut(creading.0);
            r.hit_by.push(rnumber);
            r.noprint = false;
        }

        let sublist = self.grammar.rule_by_number.get(rule.0).sublist;
        if let Some(sl) = sublist {
            let tags = self.get_tag_list_of_set(sl, false);
            let mut excepts = TagList::new();
            self.get_tags_matching(creading, &tags, &mut excepts);
            excepts.extend(tags.iter().copied());
            let mut rc = Some(creading);
            while let Some(r) = rc {
                for &tter in &excepts {
                    self.del_tag_from_reading(r, tter);
                }
                rc = self.store.readings.get(r.0).next;
            }
        }

        let mut mappings = TagList::new();
        let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
        let the_tags = match maplist {
            Some(ml) => self.get_tag_list_of_set(ml, false),
            None => TagList::new(),
        };

        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1.get();
        let rflags = self.grammar.rule_by_number.get(rule.0).flags;
        let mut did_insert = false;
        if childset1 != 0 {
            let mut spot_tags = self.get_tag_list_of_set_number(childset1, false);
            self.rr_fill_tag_list(&mut spot_tags);
            let tags_list = self.store.readings.get(creading.0).tags_list.clone();
            let spot_hashes: Vec<u32> = spot_tags
                .iter()
                .map(|t| self.grammar.single_tags_list.get(t.0).hash.get())
                .collect();
            let mut at: usize = tags_list.len();
            'outer: for start in 0..tags_list.len() {
                for (k, &sh) in spot_hashes.iter().enumerate() {
                    if start + k >= tags_list.len() || tags_list[start + k] != sh {
                        continue 'outer;
                    }
                }
                at = start;
                break;
            }
            if rflags.intersects(RF_AFTER) {
                let mut cnt = 0;
                while at < self.store.readings.get(creading.0).tags_list.len()
                    && cnt != spot_tags.len()
                {
                    at += 1;
                    cnt += 1;
                }
            }
            if at < self.store.readings.get(creading.0).tags_list.len() {
                self.rr_insert_taglist_to_reading(st, at, &the_tags, creading, &mut mappings);
                did_insert = true;
            }
        }
        if !did_insert {
            self.rr_append_taglist_to_reading(st, rnumber, &the_tags, creading, &mut mappings);
        }
        if !mappings.is_empty() {
            self.split_mappings(&mut mappings, cohort, creading, true);
        }
        st.readings_changed = true;
        self.reflow_reading(creading);
    }

    /// `TRACE` variant that pushes `rnumber` onto a specific reading's `hit_by`
    /// (the C++ COPY branch traces `cReading`, not the apply-to). Handles the
    /// GSR_ANY whole-reading push too.
    fn trace_reading(&mut self, reading: ReadingId, rnumber: u32) {
        self.store.readings.get_mut(reading.0).hit_by.push(rnumber);
    }

    /// K_SUBSTITUTE: remove the `sublist` tags from the subreading (marking the
    /// spot with `substtag`) then splice in the `maplist` tags at that spot.
    /// Faithful port of the substitute action (append-any special-cased).
    // Faithful-port mirrors: assignments kept 1:1 with the C++ text even where
    // the ported reads were elided (see the deferred-I/O / driver notes).
    #[allow(unused_assignments, unused_variables)]
    fn rr_substitute(&mut self, st: &mut RRState, rule: RuleId, rnumber: u32, rsub_reading: i32) {
        let cohort = self.get_apply_to().cohort.unwrap();
        let sr = self.get_apply_to().subreading.unwrap();
        let state_hash = self.store.readings.get(sr.0).hash;
        let sublist = self.grammar.rule_by_number.get(rule.0).sublist;
        let mut the_tags = match sublist {
            Some(sl) => self.get_tag_list_of_set(sl, false),
            None => TagList::new(),
        };
        let appending = the_tags.len() == 1
            && self
                .grammar
                .single_tags_list
                .get(the_tags[0].0)
                .comparison_hash
                == self.grammar.tag_any;

        // FILL_TAG_LIST equivalent on the subreading.
        self.rr_fill_tag_list_of(sr, &mut the_tags);
        let the_hashes: Vec<u32> = the_tags
            .iter()
            .map(|t| self.grammar.single_tags_list.get(t.0).hash.get())
            .collect();
        let substtag = self.substtag.get();

        let mut tpos: usize = usize::MAX;
        let mut plain = true;
        let mut i = 0usize;
        while i < self.store.readings.get(sr.0).tags_list.len() {
            let remter = self.store.readings.get(sr.0).tags_list[i];
            if plain && !the_hashes.is_empty() && remter == the_hashes[0] {
                if self.store.readings.get(sr.0).baseform == Some(TagHash(remter)) {
                    self.store.readings.get_mut(sr.0).baseform = None;
                }
                self.store.readings.get_mut(sr.0).tags_list[i] = substtag;
                tpos = i;
                let mut j = 1usize;
                while j < the_hashes.len() && i < self.store.readings.get(sr.0).tags_list.len() {
                    let cur = self.store.readings.get(sr.0).tags_list[i];
                    let tter = the_hashes[j];
                    if cur != tter {
                        plain = false;
                        break;
                    }
                    self.store.readings.get_mut(sr.0).tags_list.remove(i);
                    self.store.readings.get_mut(sr.0).tags.erase(tter);
                    if self.store.readings.get(sr.0).baseform == Some(TagHash(tter)) {
                        self.store.readings.get_mut(sr.0).baseform = None;
                    }
                    j += 1;
                }
                continue;
            }
            for &th in &the_hashes {
                if remter != th {
                    continue;
                }
                tpos = i;
                self.store.readings.get_mut(sr.0).tags_list[i] = substtag;
                self.store.readings.get_mut(sr.0).tags.erase(th);
                if self.store.readings.get(sr.0).baseform == Some(TagHash(th)) {
                    self.store.readings.get_mut(sr.0).baseform = None;
                }
            }
            i += 1;
        }

        if appending {
            self.store.readings.get_mut(sr.0).tags_list.push(substtag);
            tpos = self.store.readings.get(sr.0).tags_list.len();
            self.reflow_reading(sr);
        }

        if tpos != usize::MAX {
            if !plain {
                let mut k = 0usize;
                while k < self.store.readings.get(sr.0).tags_list.len() && k < tpos {
                    if self.store.readings.get(sr.0).tags_list[k] == substtag {
                        self.store.readings.get_mut(sr.0).tags_list.remove(k);
                        tpos -= 1;
                    } else {
                        k += 1;
                    }
                }
            }
            self.index_ruleCohort_no.clear(0);
            self.trace(rnumber, rsub_reading);
            self.store.readings.get_mut(sr.0).noprint = false;
            if tpos >= self.store.readings.get(sr.0).tags_list.len() {
                tpos = self.store.readings.get(sr.0).tags_list.len() - 1;
            }
            tpos += 1;
            let mut mappings = TagList::new();
            let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
            let map_tags = match maplist {
                Some(ml) => self.get_tag_list_of_set(ml, false),
                None => TagList::new(),
            };
            let mut wf: Option<TagId> = None;
            let mapping_prefix = self.grammar.mapping_prefix;
            let mut idx = 0usize;
            while idx < self.store.readings.get(sr.0).tags_list.len() {
                if self.store.readings.get(sr.0).tags_list[idx] == substtag {
                    self.store.readings.get_mut(sr.0).tags_list.remove(idx);
                    tpos = idx;
                    for t0 in &map_tags {
                        let mut tag = *t0;
                        if self
                            .grammar
                            .single_tags_list
                            .get(tag.0)
                            .r#type
                            .intersects(T_VARSTRING)
                        {
                            tag = self.generate_varstring_tag_id(tag);
                        }
                        let (thash, ttype, first) = {
                            let t = self.grammar.single_tags_list.get(tag.0);
                            (t.hash, t.r#type, t.tag.chars().next())
                        };
                        if thash.get() == self.grammar.tag_any {
                            break;
                        }
                        if ttype.intersects(T_MAPPING) || first == Some(mapping_prefix) {
                            mappings.push(tag);
                        } else {
                            if ttype.intersects(T_WORDFORM) {
                                wf = Some(tag);
                            }
                            self.store
                                .readings
                                .get_mut(sr.0)
                                .tags_list
                                .insert(tpos, thash.get());
                            tpos += 1;
                        }
                        if self.update_valid_rules(
                            &st.rules.clone(),
                            &mut st.intersects,
                            thash.get(),
                            sr,
                        ) {
                            st.iter_val = rnumber;
                        }
                    }
                } else {
                    idx += 1;
                }
            }
            self.reflow_reading(sr);
            if !mappings.is_empty() {
                self.split_mappings(&mut mappings, cohort, sr, true);
            }
            // Wordform swap across the parent's readings (rare path).
            let parent = self.store.readings.get(sr.0).parent.unwrap();
            let parent_wf = self.store.cohorts.get(parent.0).wordform;
            if let Some(wf) = wf
                && Some(wf) != parent_wf {
                    let pwf = parent_wf.unwrap();
                    for list_kind in 0..3 {
                        let rs = match list_kind {
                            0 => self.store.cohorts.get(parent.0).readings.clone(),
                            1 => self.store.cohorts.get(parent.0).deleted.clone(),
                            _ => self.store.cohorts.get(parent.0).delayed.clone(),
                        };
                        for r in rs {
                            self.del_tag_from_reading(r, pwf);
                            self.add_tag_to_reading(r, wf);
                        }
                    }
                    self.store.cohorts.get_mut(parent.0).wordform = Some(wf);
                    let wf_rules = self.grammar.wf_rules.clone();
                    let current = st.current;
                    for r in wf_rules {
                        let rw = self.grammar.rule_by_number.get(r.0).wordform;
                        let rn = self.grammar.rule_by_number.get(r.0).number;
                        let cs = crate::grammar_applicator::CsRef::Window {
                            sw: current,
                            rule: rn,
                        };
                        if self.does_wordforms_match(Some(wf), rw) {
                            self.cohortset_insert_at(cs, cohort);
                            st.intersects.insert(rn);
                        } else {
                            self.cohortset_erase_at(cs, cohort);
                        }
                    }
                    let wf_hash = self.grammar.single_tags_list.get(wf.0).hash;
                    self.update_valid_rules(&st.rules.clone(), &mut st.intersects, wf_hash.get(), sr);
                    st.iter_val = rnumber;
                }
        }
        if self.store.readings.get(sr.0).hash != state_hash {
            st.readings_changed = true;
        }
    }

    /// FILL_TAG_LIST on an explicit reading (not the apply-to) — used by
    /// SUBSTITUTE which operates on `get_apply_to().subreading`.
    fn rr_fill_tag_list_of(&mut self, reading: ReadingId, taglist: &mut TagList) {
        let mut out: TagList = Vec::new();
        for &tt in taglist.iter() {
            let (thash, ttype) = {
                let t = self.grammar.single_tags_list.get(tt.0);
                (t.hash, t.r#type)
            };
            let present = {
                let r = self.store.readings.get(reading.0);
                r.tags.find(thash.get()) != r.tags.end()
            };
            if present {
                out.push(tt);
            } else if ttype.intersects(T_SPECIAL) {
                let tagv = self.grammar.single_tags_list.get(tt.0).clone();
                let stag = self.does_tag_match_reading(reading, &tagv, false, true);
                if stag != 0 {
                    out.push(self.tag_by_hash(TagHash(stag)));
                }
            }
        }
        *taglist = out;
    }

    /// K_SETPARENT/K_SETCHILD and the ADD/SET/REM RELATION(S) family: locate the
    /// attach target via `rule.dep_target` (+ `dep_tests`), then apply the
    /// dependency attach or relation edit, iterating onward on loop/cross failures.
    ///
    /// The `swapper<Cohort*>` (RF_REVERSE) target/attach swap and the RTAG textual
    /// bookkeeping are reproduced. The onward-scan loop mirrors the C++ `while
    /// (true)` with the `dep_target->offset` temporary +/-1 override.
    fn rr_dep_relation(
        &mut self,
        st: &mut RRState,
        rule: RuleId,
        rtype: KEYWORDS,
        rnumber: u32,
        rsub_reading: i32,
    ) {
        let dep_target = match self.grammar.rule_by_number.get(rule.0).dep_target {
            Some(dt) => dt,
            None => return,
        };
        let rflags = self.grammar.rule_by_number.get(rule.0).flags;
        let orgoffset = self.grammar.contexts_arena[dep_target.0].offset;
        let mut seen_targets: Vec<u32> = Vec::new();
        let orgtarget = self.context_stack.last().unwrap().target.clone();

        loop {
            let target = self.context_stack.last().unwrap().target.cohort.unwrap();
            let target_gn = self.store.cohorts.get(target.0).global_number.get();
            seen_targets.push(target_gn);
            self.dep_deep_seen.clear();
            self.tmpl_cntx = crate::grammar_applicator::tmpl_context_t::default();
            {
                let f = self.context_stack.last_mut().unwrap();
                f.attach_to = crate::grammar_applicator::ReadingSpec::default();
            }
            self.seen_barrier = false;
            let (tparent, tlocal) = {
                let c = self.store.cohorts.get(target.0);
                (c.parent, c.local_number)
            };
            let mut attach_out: Option<CohortId> = None;
            let res =
                self.run_contextual_test(tparent, tlocal, dep_target, Some(&mut attach_out), None);
            if res.is_some()
                && let Some(mut attach) = attach_out
            {
                self.profile_rule_context(true, rule, dep_target);
                let break_after = self.seen_barrier || (rflags.intersects(RF_NEAREST));
                if let Some(at) = self.get_attach_to().cohort {
                    attach = at;
                }
                self.context_target = Some(attach);
                let mut good = true;
                let dep_tests: Vec<CtxId> = self
                    .grammar
                    .rule_by_number
                    .get(rule.0)
                    .dep_tests
                    .iter()
                    .copied()
                    .collect();
                for it in dep_tests {
                    self.set_mark_frame(attach);
                    self.dep_deep_seen.clear();
                    self.tmpl_cntx = crate::grammar_applicator::tmpl_context_t::default();
                    let (aparent, alocal) = {
                        let c = self.store.cohorts.get(attach.0);
                        (c.parent, c.local_number)
                    };
                    let tg = self
                        .run_contextual_test(aparent, alocal, it, None, None)
                        .is_some();
                    self.profile_rule_context(tg, rule, it);
                    if !tg {
                        good = false;
                        break;
                    }
                }
                if self.get_attach_to().cohort.is_none() {
                    self.context_stack.last_mut().unwrap().attach_to.cohort = Some(attach);
                }
                if good {
                    let temp = self.context_stack.last().unwrap().target.clone();
                    self.context_stack.last_mut().unwrap().target = orgtarget.clone();
                    let attached = self.rr_dep_target_cb(st, rule, rtype, rnumber, rsub_reading);
                    if attached {
                        break;
                    } else {
                        self.context_stack.last_mut().unwrap().target = temp;
                    }
                }
                if break_after {
                    break;
                }
                let attach_gn = self.store.cohorts.get(attach.0).global_number.get();
                if seen_targets.contains(&attach_gn) {
                    break;
                }
                seen_targets.push(attach_gn);
                let at = self.context_stack.last().unwrap().attach_to.clone();
                self.context_stack.last_mut().unwrap().target = at;
                let off = self.grammar.contexts_arena[dep_target.0].offset;
                if off != 0 {
                    self.grammar.contexts_arena[dep_target.0].offset = if off < 0 { -1 } else { 1 };
                }
            } else {
                break;
            }
        }
        self.grammar.contexts_arena[dep_target.0].offset = orgoffset;
    }

    /// `set_mark` targeting the current frame with a concrete cohort (the dep loop
    /// uses `context_stack.back().mark = attach`).
    pub(crate) fn set_mark_frame(&mut self, cohort: CohortId) {
        if let Some(f) = self.context_stack.last_mut() {
            f.mark = Some(cohort);
        }
    }

    /// `dep_target_cb` lambda of the dependency/relation branch: perform the
    /// actual attach/relation edit on the resolved (target, attach) pair.
    fn rr_dep_target_cb(
        &mut self,
        st: &mut RRState,
        rule: RuleId,
        rtype: KEYWORDS,
        rnumber: u32,
        rsub_reading: i32,
    ) -> bool {
        let _ = st;
        let rflags = self.grammar.rule_by_number.get(rule.0).flags;
        let mut target = self.context_stack.last().unwrap().target.cohort.unwrap();
        let mut attach = self.context_stack.last().unwrap().attach_to.cohort.unwrap();
        if rflags.intersects(RF_REVERSE) {
            std::mem::swap(&mut target, &mut attach);
        }

        if rtype == K_SETPARENT || rtype == K_SETCHILD {
            self.has_dep = true;
            let attached = if rtype == K_SETPARENT {
                self.attach_parent_child(
                    attach,
                    target,
                    rflags.intersects(RF_ALLOWLOOP),
                    rflags.intersects(RF_ALLOWCROSS),
                )
            } else {
                self.attach_parent_child(
                    target,
                    attach,
                    rflags.intersects(RF_ALLOWLOOP),
                    rflags.intersects(RF_ALLOWCROSS),
                )
            };
            if attached {
                self.index_ruleCohort_no.clear(0);
                let at_was = self.context_stack.last().unwrap().attach_to.cohort;
                self.context_stack.last_mut().unwrap().attach_to.cohort = None;
                self.trace(rnumber, rsub_reading);
                self.context_stack.last_mut().unwrap().attach_to.cohort = at_was;
                let sr = self
                    .context_stack
                    .last()
                    .unwrap()
                    .target
                    .subreading
                    .unwrap();
                self.store.readings.get_mut(sr.0).noprint = false;
                self.has_dep = true;
                st.readings_changed = true;
            }
            return attached;
        }

        // Relation family.
        self.has_relations = true;
        let is_plural = matches!(rtype, K_ADDRELATIONS | K_SETRELATIONS | K_REMRELATIONS);
        let mut rel_did_anything = false;
        let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
        let map_tags = match maplist {
            Some(ml) => self.get_tag_list_of_set(ml, false),
            None => TagList::new(),
        };
        for t0 in map_tags {
            let mut tter = t0;
            while self
                .grammar
                .single_tags_list
                .get(tter.0)
                .r#type
                .intersects(T_VARSTRING)
            {
                tter = self.generate_varstring_tag_id(tter);
            }
            let thash = self.grammar.single_tags_list.get(tter.0).hash;
            let attach_gn = self.store.cohorts.get(attach.0).global_number.get();
            match rtype {
                K_ADDRELATION | K_ADDRELATIONS => {
                    if !is_plural {
                        crate::cohort::set_related(&mut self.store, attach);
                    }
                    crate::cohort::set_related(&mut self.store, target);
                    rel_did_anything |= self
                        .store
                        .cohorts
                        .get_mut(target.0)
                        .add_relation(thash.get(), attach_gn);
                    self.rr_add_relation_rtag(target, tter, attach_gn);
                }
                K_SETRELATION | K_SETRELATIONS => {
                    if !is_plural {
                        crate::cohort::set_related(&mut self.store, attach);
                    }
                    crate::cohort::set_related(&mut self.store, target);
                    rel_did_anything |= self
                        .store
                        .cohorts
                        .get_mut(target.0)
                        .set_relation(thash.get(), attach_gn);
                    self.rr_set_relation_rtag(target, tter, attach_gn);
                }
                _ => {
                    rel_did_anything |= self
                        .store
                        .cohorts
                        .get_mut(target.0)
                        .rem_relation(thash.get(), attach_gn);
                    self.rr_rem_relation_rtag(target, tter, attach_gn);
                }
            }
        }
        // Plural variants also relate `attach` back to `target` via `sublist`.
        if is_plural {
            let sublist = self.grammar.rule_by_number.get(rule.0).sublist;
            let sub_tags = match sublist {
                Some(sl) => self.get_tag_list_of_set(sl, false),
                None => TagList::new(),
            };
            let target_gn = self.store.cohorts.get(target.0).global_number.get();
            for t0 in sub_tags {
                let mut tter = t0;
                while self
                    .grammar
                    .single_tags_list
                    .get(tter.0)
                    .r#type
                    .intersects(T_VARSTRING)
                {
                    tter = self.generate_varstring_tag_id(tter);
                }
                let thash = self.grammar.single_tags_list.get(tter.0).hash;
                match rtype {
                    K_ADDRELATIONS => {
                        crate::cohort::set_related(&mut self.store, attach);
                        rel_did_anything |= self
                            .store
                            .cohorts
                            .get_mut(attach.0)
                            .add_relation(thash.get(), target_gn);
                        self.rr_add_relation_rtag(attach, tter, target_gn);
                    }
                    K_SETRELATIONS => {
                        crate::cohort::set_related(&mut self.store, attach);
                        rel_did_anything |= self
                            .store
                            .cohorts
                            .get_mut(attach.0)
                            .set_relation(thash.get(), target_gn);
                        self.rr_set_relation_rtag(attach, tter, target_gn);
                    }
                    _ => {
                        rel_did_anything |= self
                            .store
                            .cohorts
                            .get_mut(attach.0)
                            .rem_relation(thash.get(), target_gn);
                        self.rr_rem_relation_rtag(attach, tter, target_gn);
                    }
                }
            }
        }
        if rel_did_anything {
            self.index_ruleCohort_no.clear(0);
            let at_was = self.context_stack.last().unwrap().attach_to.cohort;
            self.context_stack.last_mut().unwrap().attach_to.cohort = None;
            self.trace(rnumber, rsub_reading);
            self.context_stack.last_mut().unwrap().attach_to.cohort = at_was;
            let sr = self
                .context_stack
                .last()
                .unwrap()
                .target
                .subreading
                .unwrap();
            self.store.readings.get_mut(sr.0).noprint = false;
            st.readings_changed = true;
        }
        // Relation rules never scan onward.
        true
    }
}
