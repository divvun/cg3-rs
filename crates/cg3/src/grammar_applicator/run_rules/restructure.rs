//! `GrammarApplicator` — window-restructuring rules: MOVE/SWITCH, ADDCOHORT, MERGECOHORTS, COPYCOHORT, SPLITCOHORT.
//!
//! Split out of the wave-2 monolithic `run_rules.rs` (wave 4, w4-file-split-fmt).

use crate::arena::{CohortId, CtxId, ReadingId, RuleId, SwId, TagId};
use crate::cohort::{CT_RELATED, CT_REMOVED, CohortSet, DEP_NO_PARENT};
use crate::inlines::{hash_value, insert_if_exists, ui32};
use crate::rule::{RF_BEFORE, RF_DETACH, RF_REVERSE};
use crate::strings::KEYWORDS::{self};
use crate::tag::{T_BASEFORM, T_DEPENDENCY, T_MAPPING, T_VARSTRING, T_WORDFORM, TagList};
use crate::types::{GlobalNumber, TagHash};

// C++ anonymous `enum { RV_NOTHING = 1, RV_SOMETHING = 2, RV_DELIMITED = 4,
// RV_TRACERULE = 8 };` — the return-value bit flags of runRulesOnSingleWindow.

use super::*;

impl crate::grammar_applicator::GrammarApplicator {
    /// K_MOVE_AFTER/K_MOVE_BEFORE/K_SWITCH: relocate the target cohort (and its
    /// subtree) relative to a contextual attach target within the same window.
    /// Reproduces the endless-loop bail (hash comparison + `rule_hits` counter).
    pub(crate) fn rr_move_switch(
        &mut self,
        st: &mut RRState,
        rule: RuleId,
        rtype: KEYWORDS,
        rnumber: u32,
    ) {
        let current = st.current;
        let dep_target = match self.grammar.rule_by_number.get(rule.0).dep_target {
            Some(dt) => dt,
            None => return,
        };
        let rflags = self.grammar.rule_by_number.get(rule.0).flags;
        // State hash before.
        let (phash, chash) = self.rr_window_state_hash(current);

        let cohort = self.context_stack.last().unwrap().target.cohort.unwrap();
        let c = self.store.cohorts.get(cohort.0).local_number;
        self.dep_deep_seen.clear();
        self.tmpl_cntx = crate::grammar_applicator::tmpl_context_t::default();
        self.context_stack.last_mut().unwrap().attach_to =
            crate::grammar_applicator::ReadingSpec::default();
        let mut attach_out: Option<CohortId> = None;
        let res =
            self.run_contextual_test(Some(current), c, dep_target, Some(&mut attach_out), None);
        let attach0 = attach_out;
        let same_parent = attach0
            .map(|a| self.store.cohorts.get(a.0).parent == self.store.cohorts.get(cohort.0).parent)
            .unwrap_or(false);
        if !(res.is_some() && attach0.is_some() && same_parent) {
            return;
        }
        let mut attach = attach0.unwrap();
        self.profile_rule_context(true, rule, dep_target);
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
                let cc = self.store.cohorts.get(attach.0);
                (cc.parent, cc.local_number)
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
        if !good || cohort == attach || self.store.cohorts.get(cohort.0).local_number == 0 {
            return;
        }

        // swapper<Cohort*>(RF_REVERSE, attach, cohort)
        let (mut a, mut b) = (attach, cohort);
        if rflags.intersects(RF_REVERSE) {
            std::mem::swap(&mut a, &mut b);
        }
        let (attach, cohort) = (a, b);

        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1.get();
        let childset2 = self.grammar.rule_by_number.get(rule.0).childset2.get();

        let mut cohorts_set = CohortSet::new();
        if rtype == K_SWITCH {
            if self.store.cohorts.get(attach.0).local_number == 0 {
                return;
            }
            let cln = self.store.cohorts.get(cohort.0).local_number as usize;
            let aln = self.store.cohorts.get(attach.0).local_number as usize;
            {
                let sw = self.store.single_windows.get_mut(current.0);
                sw.cohorts[cln] = attach;
                sw.cohorts[aln] = cohort;
            }
            // all_cohorts swap via find-from-local (approximate: search all).
            self.rr_swap_all_cohorts(current, cohort, attach);
        } else {
            let mut edges = CohortSet::new();
            self.rr_collect_subtree(current, &mut edges, attach, childset2);
            self.rr_collect_subtree(current, &mut cohorts_set, cohort, childset1);

            let mut need_clean = false;
            for iter in cohorts_set.as_slice() {
                if edges.contains(*iter) {
                    need_clean = true;
                    break;
                }
            }
            if need_clean {
                if self.is_child_of(cohort, attach) {
                    let rem: Vec<CohortId> = cohorts_set.iter_rev().copied().collect();
                    for r in rem {
                        edges.erase(r);
                    }
                } else {
                    let rem: Vec<CohortId> = edges.iter_rev().copied().collect();
                    for r in rem {
                        cohorts_set.erase(r);
                    }
                }
            }
            if cohorts_set.empty() || edges.empty() {
                self.finish_reading_loop = false;
                return;
            }

            // Erase the moved cohorts from `cohorts` (in reverse local order).
            let mut moved: Vec<CohortId> = cohorts_set.iter_rev().copied().collect();
            for cc in moved.drain(..) {
                let ln = self.store.cohorts.get(cc.0).local_number as usize;
                self.store
                    .single_windows
                    .get_mut(current.0)
                    .cohorts
                    .remove(ln);
                self.rr_remove_from_all_cohorts(current, cc);
            }
            self.rr_renumber(current);

            // Determine the insertion spot.
            let spot = if rtype == K_MOVE_BEFORE {
                let mut s = self.store.cohorts.get(edges.front().0).local_number;
                if s == 0 {
                    s = 1;
                }
                s
            } else {
                self.store.cohorts.get(edges.back().0).local_number + 1
            } as usize;
            if spot > self.store.single_windows.get(current.0).cohorts.len() {
                return;
            }
            let ins: Vec<CohortId> = cohorts_set.iter_rev().copied().collect();
            for cc in ins {
                self.store
                    .single_windows
                    .get_mut(current.0)
                    .cohorts
                    .insert(spot, cc);
                self.rr_insert_into_all_cohorts(current, spot, cc);
            }
        }
        self.rr_reindex(current);

        let (phash_n, chash_n) = self.rr_window_state_hash(current);
        if phash != phash_n || chash != chash_n {
            let hits = self.rule_hits.entry(rnumber).or_insert(0);
            *hits += 1;
            let hitcount = *hits;
            let limit = self.store.single_windows.get(current.0).cohorts.len() * 100;
            if hitcount as usize > limit {
                st.should_bail = true;
                self.finish_cohort_loop = false;
                return;
            }
            let cohorts_vec: Vec<CohortId> = cohorts_set.as_slice().to_vec();
            for cc in cohorts_vec {
                let rs = self.store.cohorts.get(cc.0).readings.clone();
                for r in rs {
                    self.store.readings.get_mut(r.0).hit_by.push(rnumber);
                }
            }
            st.readings_changed = true;
            st.do_sort = true;
        }
    }

    /// Hash of the window's cohort order + first-reading hashes (the C++ move/switch
    /// "did anything change" check).
    fn rr_window_state_hash(&self, current: SwId) -> (u32, u32) {
        let mut phash = 0u32;
        let mut chash = 0u32;
        for &cc in &self.store.single_windows.get(current.0).cohorts {
            let gn = self.store.cohorts.get(cc.0).global_number.get();
            phash = hash_value(gn, phash);
            let r0 = self.store.cohorts.get(cc.0).readings[0];
            let rh = self.store.readings.get(r0.0).hash;
            chash = hash_value(rh, chash);
        }
        (phash, chash)
    }

    /// Swap two cohorts in `all_cohorts` (SWITCH).
    fn rr_swap_all_cohorts(&mut self, current: SwId, a: CohortId, b: CohortId) {
        let ac = &mut self.store.single_windows.get_mut(current.0).all_cohorts;
        let ia = ac.iter().position(|&x| x == a);
        let ib = ac.iter().position(|&x| x == b);
        if let (Some(ia), Some(ib)) = (ia, ib) {
            ac.swap(ia, ib);
        }
    }

    /// Remove a cohort from `all_cohorts`.
    fn rr_remove_from_all_cohorts(&mut self, current: SwId, c: CohortId) {
        let ac = &mut self.store.single_windows.get_mut(current.0).all_cohorts;
        if let Some(i) = ac.iter().position(|&x| x == c) {
            ac.remove(i);
        }
    }

    /// Insert a cohort into `all_cohorts` at the window's `cohorts[spot]` position
    /// (approximate: place before the cohort now occupying `spot`).
    fn rr_insert_into_all_cohorts(&mut self, current: SwId, spot: usize, c: CohortId) {
        let anchor = self
            .store
            .single_windows
            .get(current.0)
            .cohorts
            .get(spot + 1)
            .copied();
        let ac_pos = match anchor {
            Some(a) => self
                .store
                .single_windows
                .get(current.0)
                .all_cohorts
                .iter()
                .position(|&x| x == a),
            None => None,
        };
        let ac = &mut self.store.single_windows.get_mut(current.0).all_cohorts;
        match ac_pos {
            Some(p) => ac.insert(p, c),
            None => ac.push(c),
        }
    }

    /// `add_cohort` lambda of `runSingleRule` — allocate a new cohort from the
    /// rule's `maplist` (wordform + baseform-led readings, `(*)` expansion),
    /// attach it dependency-wise, insert it into the window relative to the
    /// subtree of `insertion`, renumber, and rebuild links. Shared by
    /// ADDCOHORT/MERGECOHORTS.
    ///
    /// The MERGECOHORTS relation/dependency re-attachment (needs the `withs` set)
    /// is threaded via `withs`; for plain ADDCOHORT it is `None`.
    /// Returns `(cohort, spaces_in_added_wf)` — the C++ `size_t&
    /// spacesInAddedWf` out-param (count of ' ' in the seated wordform tag;
    /// MERGECOHORTS strips that many spaces from the merged cohorts' text
    /// before removing them) is a plain return value in the port (wave 4).
    fn rr_add_cohort(
        &mut self,
        st: &mut RRState,
        rule: RuleId,
        insertion: CohortId,
        withs: Option<&CohortSet>,
    ) -> (CohortId, usize) {
        let mut spaces_in_added_wf = 0usize;
        let current = st.current;
        let ccohort = crate::cohort::alloc_cohort(&mut self.store, Some(current));
        {
            let gn = self.gWindow.cohort_counter;
            self.gWindow.cohort_counter = self.gWindow.cohort_counter.wrapping_add(1);
            self.store.cohorts.get_mut(ccohort.0).global_number = gn;
        }
        let the_tags = self.rr_maplist_tags(rule);

        // Partition into wordform + baseform-led readings.
        let mut wf: Option<TagId> = None;
        let mut readings: Vec<TagList> = Vec::new();
        for tter in the_tags {
            let ttype = self.grammar.single_tags_list.get(tter.0).r#type;
            if ttype.intersects(T_WORDFORM) {
                self.store.cohorts.get_mut(ccohort.0).wordform = Some(tter);
                // C++: spacesInAddedWf = count of ' ' in tter->tag.
                spaces_in_added_wf = self
                    .grammar
                    .single_tags_list
                    .get(tter.0)
                    .tag
                    .chars()
                    .filter(|&c| c == ' ')
                    .count();
                wf = Some(tter);
                continue;
            }
            if wf.is_none() {
                // Error: wordform must precede other tags (I/O omitted).
                continue;
            }
            if ttype.intersects(T_BASEFORM) {
                readings.push(vec![wf.unwrap()]);
            }
            if let Some(last) = readings.last_mut() {
                last.push(tter);
            }
        }

        // (*) expansion against the insertion cohort's first reading.
        for tags in readings.iter_mut() {
            let mut k = 0usize;
            while k < tags.len() {
                if self.grammar.single_tags_list.get(tags[k].0).hash.get() == self.grammar.tag_any {
                    let nt = self.store.cohorts.get(insertion.0).readings[0];
                    let nt_list = self.store.readings.get(nt.0).tags_list.clone();
                    if nt_list.len() <= 2 {
                        k += 1;
                        continue;
                    }
                    tags[k] = self.tag_by_hash(TagHash(nt_list[2]));
                    let mut kk = 1usize;
                    for &nt_hash in &nt_list[3..] {
                        let tid = self.tag_by_hash(TagHash(nt_hash));
                        if self
                            .grammar
                            .single_tags_list
                            .get(tid.0)
                            .r#type
                            .intersects(T_DEPENDENCY)
                        {
                            continue;
                        }
                        tags.insert(k + kk, tid);
                        kk += 1;
                    }
                }
                k += 1;
            }
        }

        let mapping_prefix = self.grammar.mapping_prefix;
        for rit in readings {
            let creading = crate::reading::alloc_reading(&mut self.store, Some(ccohort));
            self.numReadings = self.numReadings.wrapping_add(1);
            let sets_any = self.grammar.sets_any.clone();
            insert_if_exists(
                &mut self.store.cohorts.get_mut(ccohort.0).possible_sets,
                sets_any.as_ref(),
            );
            {
                let r = self.store.readings.get_mut(creading.0);
                r.hit_by
                    .push(self.grammar.rule_by_number.get(rule.0).number);
                r.noprint = false;
            }
            let rnumber = self.grammar.rule_by_number.get(rule.0).number;
            let mut mappings = TagList::new();
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
                self.split_mappings(&mut mappings, ccohort, creading, false);
            }
            crate::cohort::append_reading(&mut self.store, ccohort, creading);
        }

        let cgn = self.store.cohorts.get(ccohort.0).global_number;
        self.gWindow.cohort_map.insert(cgn, ccohort);
        self.gWindow.dep_window.insert(cgn, ccohort);

        let rtype = self.grammar.rule_by_number.get(rule.0).r#type;
        if self.grammar.addcohort_attach
            && (rtype == K_ADDCOHORT_BEFORE || rtype == K_ADDCOHORT_AFTER)
        {
            self.attach_parent_child(insertion, ccohort, false, false);
        } else if rtype == K_MERGECOHORTS
            && !self
                .grammar
                .rule_by_number
                .get(rule.0)
                .flags
                .intersects(RF_DETACH)
        {
            self.rr_mergecohorts_attach(insertion, ccohort, withs);
        }

        if self.store.cohorts.get(ccohort.0).readings.is_empty() {
            self.init_empty_cohort(ccohort);
            if self.trace {
                let r = self.store.cohorts.get(ccohort.0).readings[0];
                let rn = self.grammar.rule_by_number.get(rule.0).number;
                self.store.readings.get_mut(r.0).hit_by.push(rn);
                self.store.readings.get_mut(r.0).noprint = false;
            }
        }

        // Insert into the window relative to `insertion`'s subtree.
        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1.get();
        let mut cohorts = CohortSet::new();
        self.rr_collect_subtree(current, &mut cohorts, insertion, childset1);
        if rtype == K_ADDCOHORT_BEFORE {
            let ln = self.store.cohorts.get(cohorts.front().0).local_number as usize;
            self.store
                .single_windows
                .get_mut(current.0)
                .cohorts
                .insert(ln, ccohort);
            self.rr_insert_into_all_cohorts_before(current, cohorts.front(), ccohort);
        } else {
            let ln = self.store.cohorts.get(cohorts.back().0).local_number as usize + 1;
            self.store
                .single_windows
                .get_mut(current.0)
                .cohorts
                .insert(ln, ccohort);
            self.rr_insert_into_all_cohorts_after(current, cohorts.back(), ccohort);
        }
        self.rr_renumber(current);
        let gw = &mut self.gWindow;
        gw.rebuild_cohort_links(&mut self.store);
        (ccohort, spaces_in_added_wf)
    }

    /// MERGECOHORTS dependency/relation re-attachment for a freshly added cohort.
    /// A focused port of the C++ `add_cohort` `K_MERGECOHORTS` block (the `withs`
    /// set drives which siblings/relations transfer). The nearest-un-merged-token
    /// walk is preserved.
    fn rr_mergecohorts_attach(
        &mut self,
        insertion: CohortId,
        ccohort: CohortId,
        withs: Option<&CohortSet>,
    ) {
        let target = self.context_stack.last().unwrap().target.cohort.unwrap();
        let dp = self.store.cohorts.get(target.0).dep_parent;
        let has_parent = dp.is_some() && self.gWindow.cohort_map.contains_key(&dp.unwrap());
        if !has_parent {
            if self.has_dep {
                let in_withs = withs.map(|w| w.contains(insertion)).unwrap_or(false);
                if !in_withs {
                    self.attach_parent_child(insertion, ccohort, false, false);
                } else {
                    // Attach to nearest un-merged token via the sibling chain.
                    let mut next = self.store.cohorts.get(insertion.0).next;
                    let mut prev = self.store.cohorts.get(insertion.0).prev;
                    let ins_parent = self.store.cohorts.get(insertion.0).parent;
                    loop {
                        if next.is_none() && prev.is_none() {
                            break;
                        }
                        if let Some(n) = next
                            && self.store.cohorts.get(n.0).parent != ins_parent {
                                next = None;
                            }
                        if let Some(n) = next {
                            if !withs.map(|w| w.contains(n)).unwrap_or(false) {
                                self.attach_parent_child(n, ccohort, false, false);
                                break;
                            }
                            next = self.store.cohorts.get(n.0).next;
                        }
                        if let Some(p) = prev
                            && self.store.cohorts.get(p.0).parent != ins_parent {
                                prev = None;
                            }
                        if let Some(p) = prev {
                            if !withs.map(|w| w.contains(p)).unwrap_or(false) {
                                self.attach_parent_child(p, ccohort, false, false);
                                break;
                            }
                            prev = self.store.cohorts.get(p.0).prev;
                        }
                    }
                }
            }
        } else {
            let parent = *self.gWindow.cohort_map.get(&dp.unwrap()).unwrap();
            self.attach_parent_child(parent, ccohort, false, false);
        }

        // Relation/child transfer across `withs` (C++ lines 1135-1158). `ps` is
        // the set of merged-in cohorts' global_numbers; every relation whose set
        // held any of those numbers is rewritten to point at `cCohort`, and every
        // cohort whose dep_parent is a merged cohort becomes a child of `cCohort`.
        let mut ps: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        if let Some(withs) = withs {
            for &c in withs.as_slice() {
                let cgn = self.store.cohorts.get(c.0).global_number.get();
                ps.insert(cgn);
                if self.store.cohorts.get(c.0).r#type.intersects(CT_RELATED) {
                    // cCohort->relations[key].insert(begin, end) for each key.
                    let rels: Vec<(u32, Vec<u32>)> = self
                        .store
                        .cohorts
                        .get(c.0)
                        .relations
                        .iter()
                        .map(|(&k, v)| (k, v.as_slice().to_vec()))
                        .collect();
                    for (k, targets) in rels {
                        let dst = self
                            .store
                            .cohorts
                            .get_mut(ccohort.0)
                            .relations
                            .entry(k)
                            .or_default();
                        for t in targets {
                            dst.insert(t);
                        }
                    }
                    self.store.cohorts.get_mut(ccohort.0).r#type |= CT_RELATED;
                }
            }
        }

        // Iterate `current.all_cohorts`: re-parent orphaned children and rewrite
        // relation targets that referenced any merged cohort.
        let ccohort_gn = self.store.cohorts.get(ccohort.0).global_number.get();
        let current = self.store.cohorts.get(insertion.0).parent.unwrap();
        let all_cohorts = self.store.single_windows.get(current.0).all_cohorts.clone();
        for c in all_cohorts {
            let cdp = self.store.cohorts.get(c.0).dep_parent;
            if cdp.is_some_and(|v| ps.contains(&v.get())) {
                self.attach_parent_child(ccohort, c, false, false);
            }
            let keys: Vec<u32> = self
                .store
                .cohorts
                .get(c.0)
                .relations
                .keys()
                .copied()
                .collect();
            for key in keys {
                let mut changed = false;
                for &r in ps.iter() {
                    let rels = self
                        .store
                        .cohorts
                        .get_mut(c.0)
                        .relations
                        .get_mut(&key)
                        .unwrap();
                    if rels.count(r) != 0 {
                        rels.erase(r);
                        rels.insert(ccohort_gn);
                        changed = true;
                    }
                }
                if changed {
                    self.store.cohorts.get_mut(ccohort.0).r#type |= CT_RELATED;
                }
            }
        }
    }

    /// Insert `c` into `all_cohorts` immediately before `anchor`.
    fn rr_insert_into_all_cohorts_before(&mut self, current: SwId, anchor: CohortId, c: CohortId) {
        let ac = &mut self.store.single_windows.get_mut(current.0).all_cohorts;
        match ac.iter().position(|&x| x == anchor) {
            Some(p) => ac.insert(p, c),
            None => ac.push(c),
        }
    }

    /// Insert `c` into `all_cohorts` immediately after `anchor`.
    fn rr_insert_into_all_cohorts_after(&mut self, current: SwId, anchor: CohortId, c: CohortId) {
        let ac = &mut self.store.single_windows.get_mut(current.0).all_cohorts;
        match ac.iter().position(|&x| x == anchor) {
            Some(p) => ac.insert(p + 1, c),
            None => ac.push(c),
        }
    }

    /// K_ADDCOHORT_AFTER / K_ADDCOHORT_BEFORE: add a cohort then fix up the `<<<`
    /// end tag if the new cohort became the last.
    pub(crate) fn rr_addcohort(&mut self, st: &mut RRState, rule: RuleId) {
        let apply = self.get_apply_to().cohort.unwrap();
        // (spaces_in_added_wf: C++ "not used here")
        let (ccohort, _spaces_in_added_wf) = self.rr_add_cohort(st, rule, apply, None);
        let current = st.current;
        let rnumber = self.grammar.rule_by_number.get(rule.0).number;
        let last = *self
            .store
            .single_windows
            .get(current.0)
            .cohorts
            .last()
            .unwrap();
        if last == ccohort {
            let len = self.store.single_windows.get(current.0).cohorts.len();
            let prev = self.store.single_windows.get(current.0).cohorts[len - 2];
            let endtag_id = self.tag_by_hash(self.endtag);
            let prs = self.store.cohorts.get(prev.0).readings.clone();
            for r in prs {
                self.del_tag_from_reading(r, endtag_id);
            }
            let brs = self.store.cohorts.get(ccohort.0).readings.clone();
            for r in brs {
                self.add_tag_to_reading(r, endtag_id);
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, self.endtag.get(), r) {
                    st.iter_val = rnumber;
                }
            }
        }
        self.index_single_window(current);
    }

    /// K_MERGECOHORTS: resolve the `withs` set via the rule's dep tests, add the
    /// merged cohort, then remove every merged-in cohort. Fixes the `<<<` end tag.
    pub(crate) fn rr_mergecohorts(&mut self, st: &mut RRState, rule: RuleId) {
        self.index_ruleCohort_no.clear(0);
        let target = self.get_apply_to().cohort.unwrap();
        let mut withs = CohortSet::new();
        withs.insert(target);
        let mut merge_at = target;

        let dep_tests: Vec<CtxId> = self
            .grammar
            .rule_by_number
            .get(rule.0)
            .dep_tests
            .iter()
            .copied()
            .collect();
        for it in dep_tests {
            {
                let f = self.context_stack.last_mut().unwrap();
                f.attach_to = crate::grammar_applicator::ReadingSpec::default();
            }
            self.merge_with = None;
            self.set_mark_frame(target);
            self.dep_deep_seen.clear();
            self.tmpl_cntx = crate::grammar_applicator::tmpl_context_t::default();
            let (tparent, tlocal) = {
                let c = self.store.cohorts.get(target.0);
                (c.parent, c.local_number)
            };
            let mut attach: Option<CohortId> = None;
            let tg = self
                .run_contextual_test(tparent, tlocal, it, Some(&mut attach), None)
                .is_some()
                && attach.is_some();
            self.profile_rule_context(tg, rule, it);
            if !tg {
                self.finish_reading_loop = false;
                return;
            }
            if let Some(at) = self.get_attach_to().cohort {
                merge_at = at;
                if let Some(mw) = self.merge_with {
                    withs.insert(mw);
                }
            } else if let Some(mw) = self.merge_with {
                withs.insert(mw);
            } else if let Some(a) = attach {
                withs.insert(a);
            }
        }

        let (cc, mut spaces_in_added_wf) = self.rr_add_cohort(st, rule, merge_at, Some(&withs));
        self.context_stack.last_mut().unwrap().target.cohort = Some(cc);

        let rnumber = self.grammar.rule_by_number.get(rule.0).number;
        for c in withs.as_slice().to_vec() {
            // C++: strip up to spacesInAddedWf ' ' chars from the merged
            // cohorts' text (the counter decrements ACROSS the whole loop)
            // before removing each cohort.
            {
                let text = &mut self.store.cohorts.get_mut(c.0).text;
                while spaces_in_added_wf > 0 {
                    match text.find(' ') {
                        Some(pos) => {
                            text.remove(pos);
                            spaces_in_added_wf -= 1;
                        }
                        None => break,
                    }
                }
            }
            self.rr_rem_cohort(rnumber, c);
        }

        // Fix <<< on the new end.
        let current = st.current;
        let back = *self
            .store
            .single_windows
            .get(current.0)
            .cohorts
            .last()
            .unwrap();
        let back_front = self.store.cohorts.get(back.0).readings[0];
        let has_endtag = {
            let r = self.store.readings.get(back_front.0);
            r.tags.find(self.endtag.get()) != r.tags.end()
        };
        if !has_endtag {
            let len = self.store.single_windows.get(current.0).cohorts.len();
            let prev = self.store.single_windows.get(current.0).cohorts[len - 2];
            let endtag_id = self.tag_by_hash(self.endtag);
            let prs = self.store.cohorts.get(prev.0).readings.clone();
            for r in prs {
                self.del_tag_from_reading(r, endtag_id);
            }
            let brs = self.store.cohorts.get(back.0).readings.clone();
            for r in brs {
                self.add_tag_to_reading(r, endtag_id);
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, self.endtag.get(), r) {
                    st.iter_val = rnumber;
                }
            }
        }
        self.index_single_window(current);
        st.readings_changed = true;
        self.reset_cohorts_for_loop = true;
    }

    /// K_COPYCOHORT: resolve an `attach` cohort via `rule.dep_target` (+ dep_tests),
    /// clone the target cohort into a fresh cohort (readings copied, maplist tags
    /// added, `sublist`/getTagsMatching excepts stripped, `wread` copied), then
    /// splice the copy into the window relative to `attach`'s subtree
    /// (BEFORE/AFTER via `childset2`). RF_REVERSE swaps source/target and selects
    /// `childset1`. Faithful port of the C++ `K_COPYCOHORT` action.
    pub(crate) fn rr_copycohort(&mut self, st: &mut RRState, rule: RuleId, rnumber: u32) {
        let current = st.current;
        let cohort = self.context_stack.last().unwrap().target.cohort.unwrap();
        let c = self.store.cohorts.get(cohort.0).local_number;
        self.dep_deep_seen.clear();
        self.tmpl_cntx = crate::grammar_applicator::tmpl_context_t::default();
        {
            let f = self.context_stack.last_mut().unwrap();
            f.attach_to = crate::grammar_applicator::ReadingSpec::default();
        }
        let dep_target = match self.grammar.rule_by_number.get(rule.0).dep_target {
            Some(dt) => dt,
            None => return,
        };
        let mut attach_out: Option<CohortId> = None;
        let res =
            self.run_contextual_test(Some(current), c, dep_target, Some(&mut attach_out), None);
        if !(res.is_some() && attach_out.is_some()) {
            return;
        }
        let mut attach = attach_out.unwrap();
        self.profile_rule_context(true, rule, dep_target);
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
            self.context_stack.last_mut().unwrap().mark = Some(attach);
            self.dep_deep_seen.clear();
            self.tmpl_cntx = crate::grammar_applicator::tmpl_context_t::default();
            let (aparent, alocal) = {
                let cc = self.store.cohorts.get(attach.0);
                (cc.parent, cc.local_number)
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

        if !good || cohort == attach || self.store.cohorts.get(cohort.0).local_number == 0 {
            return;
        }

        let rflags = self.grammar.rule_by_number.get(rule.0).flags;
        // childset defaults to childset2; RF_REVERSE swaps source/target and uses
        // childset1.
        let mut cohort = cohort;
        let mut childset = self.grammar.rule_by_number.get(rule.0).childset2.get();
        if rflags.intersects(RF_REVERSE) {
            std::mem::swap(&mut cohort, &mut attach);
            childset = self.grammar.rule_by_number.get(rule.0).childset1.get();
        }

        let attach_parent = self.store.cohorts.get(attach.0).parent.unwrap();
        let ccohort = crate::cohort::alloc_cohort(&mut self.store, Some(attach_parent));
        {
            let gn = self.gWindow.cohort_counter;
            self.gWindow.cohort_counter = self.gWindow.cohort_counter.wrapping_add(1);
            let wf = self.store.cohorts.get(cohort.0).wordform;
            let cc = self.store.cohorts.get_mut(ccohort.0);
            cc.global_number = gn;
            cc.wordform = wf;
        }
        let sets_any = self.grammar.sets_any.clone();
        insert_if_exists(
            &mut self.store.cohorts.get_mut(ccohort.0).possible_sets,
            sets_any.as_ref(),
        );

        let the_tags = self.rr_maplist_tags(rule);

        // excepts: sublist tags matched on the apply-to subreading, plus the raw
        // sublist tags.
        let mut excepts = TagList::new();
        let sublist = self.grammar.rule_by_number.get(rule.0).sublist;
        if let Some(sl) = sublist {
            let tags = self.get_tag_list_of_set(sl, false);
            let subreading = self.get_apply_to().subreading.unwrap();
            self.get_tags_matching(subreading, &tags, &mut excepts);
            excepts.extend(tags.iter().copied());
        }

        let mapping_prefix = self.grammar.mapping_prefix;
        let tag_any = self.grammar.tag_any;
        let source_readings = self.store.cohorts.get(cohort.0).readings.clone();
        for r0 in source_readings {
            let mut rs: Vec<ReadingId> = Vec::new();
            let mut rc = Some(r0);
            while let Some(r) = rc {
                let creading = crate::reading::alloc_reading(&mut self.store, Some(ccohort));
                self.numReadings = self.numReadings.wrapping_add(1);
                {
                    let rr = self.store.readings.get_mut(creading.0);
                    rr.hit_by.push(rnumber);
                    rr.noprint = false;
                }
                let mut mappings = TagList::new();
                let src_tags = self.store.readings.get(r.0).tags_list.clone();
                for hash0 in src_tags {
                    let mut hash = TagHash(hash0);
                    let tter = self.tag_by_hash(hash);
                    let (ttype, first) = {
                        let t = self.grammar.single_tags_list.get(tter.0);
                        (t.r#type, t.tag.chars().next())
                    };
                    if ttype.intersects(T_MAPPING) || first == Some(mapping_prefix) {
                        mappings.push(tter);
                    } else {
                        hash = self.add_tag_to_reading(creading, tter);
                    }
                    if self.update_valid_rules(
                        &st.rules.clone(),
                        &mut st.intersects,
                        hash.get(),
                        creading,
                    ) {
                        st.iter_val = rnumber;
                    }
                }
                for &tter in &the_tags {
                    let mut hash = self.grammar.single_tags_list.get(tter.0).hash;
                    if hash.get() == tag_any {
                        continue;
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
                    if self.update_valid_rules(
                        &st.rules.clone(),
                        &mut st.intersects,
                        hash.get(),
                        creading,
                    ) {
                        st.iter_val = rnumber;
                    }
                }
                if !mappings.is_empty() {
                    self.split_mappings(&mut mappings, ccohort, creading, false);
                }
                rs.push(creading);
                rc = self.store.readings.get(r.0).next;
            }
            // Chain rs[0].next = rs[1] ... then append only the front.
            for j in 1..rs.len() {
                self.store.readings.get_mut(rs[j - 1].0).next = Some(rs[j]);
            }
            crate::cohort::append_reading(&mut self.store, ccohort, rs[0]);
        }

        if self.store.cohorts.get(ccohort.0).readings.is_empty() {
            self.init_empty_cohort(ccohort);
            if self.trace {
                let r = self.store.cohorts.get(ccohort.0).readings[0];
                self.store.readings.get_mut(r.0).hit_by.push(rnumber);
                self.store.readings.get_mut(r.0).noprint = false;
            }
        }

        // Strip except tags from every reading (following the .next chains).
        let ccreadings = self.store.cohorts.get(ccohort.0).readings.clone();
        for r0 in ccreadings {
            let mut rc = Some(r0);
            while let Some(r) = rc {
                for &tter in &excepts {
                    self.del_tag_from_reading(r, tter);
                }
                rc = self.store.readings.get(r.0).next;
            }
        }

        // Copy wread when present.
        if let Some(wread) = self.store.cohorts.get(cohort.0).wread {
            let cwread = crate::reading::alloc_reading(&mut self.store, Some(ccohort));
            self.store.cohorts.get_mut(ccohort.0).wread = Some(cwread);
            let wtags = self.store.readings.get(wread.0).tags_list.clone();
            for hash0 in wtags {
                let tter = self.tag_by_hash(TagHash(hash0));
                let hash = self.add_tag_to_reading(cwread, tter);
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash.get(), cwread) {
                    st.iter_val = rnumber;
                }
            }
        }

        let cgn = self.store.cohorts.get(ccohort.0).global_number;
        self.gWindow.cohort_map.insert(cgn, ccohort);
        self.gWindow.dep_window.insert(cgn, ccohort);

        let mut edges = CohortSet::new();
        self.rr_collect_subtree(attach_parent, &mut edges, attach, childset);

        if rflags.intersects(RF_BEFORE) {
            let front = edges.front();
            let ln = self.store.cohorts.get(front.0).local_number as usize;
            self.store
                .single_windows
                .get_mut(attach_parent.0)
                .cohorts
                .insert(ln, ccohort);
            self.rr_insert_into_all_cohorts_before(attach_parent, front, ccohort);
            self.attach_parent_child(front, ccohort, false, false);
        } else {
            let back = edges.back();
            let ln = self.store.cohorts.get(back.0).local_number as usize + 1;
            self.store
                .single_windows
                .get_mut(attach_parent.0)
                .cohorts
                .insert(ln, ccohort);
            self.rr_insert_into_all_cohorts_after(attach_parent, back, ccohort);
            self.attach_parent_child(back, ccohort, false, false);
        }

        self.rr_reindex(attach_parent);
        self.index_single_window(attach_parent);
        st.readings_changed = true;
        self.reset_cohorts_for_loop = true;
    }

    /// K_SPLITCOHORT: replace the apply-to cohort with a run of new cohorts built
    /// from the rule's `maplist` (wordform-delimited groups), each carrying
    /// baseform-led readings. The `u_sscanf`-parsed `%[0-9cd]->%[0-9pm]`
    /// dependency-mapping tags drive `cohort_dep` (self/parent indices, with `c`/`d`
    /// self meaning "keep old children" and `p`/`m` parent meaning "keep old
    /// parent"); the `R:*` tag (or the last cohort) receives the transferred named
    /// relations. Text is handed to the last new cohort, then the source cohort is
    /// removed. Faithful port of the C++ `K_SPLITCOHORT` action.
    pub(crate) fn rr_splitcohort(&mut self, st: &mut RRState, rule: RuleId) {
        self.index_ruleCohort_no.clear(0);
        let current = st.current;
        let rnumber = self.grammar.rule_by_number.get(rule.0).number;

        let the_tags = self.rr_maplist_tags(rule);

        // Partition into (cohort, readings) groups delimited by T_WORDFORM tags.
        // `cohorts` holds the new cohort ids; `groups` holds per-cohort reading
        // tag-lists (built in the second pass).
        let mut cohort_ids: Vec<CohortId> = Vec::new();
        let mut wf: Option<TagId> = None;
        for &tter in &the_tags {
            let ttype = self.grammar.single_tags_list.get(tter.0).r#type;
            if ttype.intersects(T_WORDFORM) {
                let cid = crate::cohort::alloc_cohort(&mut self.store, Some(current));
                let gn = self.gWindow.cohort_counter;
                self.gWindow.cohort_counter = self.gWindow.cohort_counter.wrapping_add(1);
                self.store.cohorts.get_mut(cid.0).global_number = gn;
                self.store.cohorts.get_mut(cid.0).wordform = Some(tter);
                cohort_ids.push(cid);
                wf = Some(tter);
                continue;
            }
            if wf.is_none() {
                // Error: wordform must precede other tags (I/O omitted).
                continue;
            }
        }

        let n = cohort_ids.len();
        // cohort_dep[i] = (self, parent) as global indices (into the new run) with
        // DEP_NO_PARENT sentinels.
        let mut rel_trg: u32 = DEP_NO_PARENT;
        let mut cohort_dep: Vec<(u32, u32)> = vec![(0, 0); n];
        if n > 0 {
            cohort_dep[0].1 = DEP_NO_PARENT;
            cohort_dep[n - 1].0 = DEP_NO_PARENT;
            cohort_dep[n - 1].1 = ui32(n - 1);
            // faithful port: `i` is both the index and the stored value `ui32(i)`.
            #[allow(clippy::needless_range_loop)]
            for i in 1..n.saturating_sub(1) {
                cohort_dep[i].1 = ui32(i);
            }
        }

        // Second pass: fill each cohort's reading tag-lists and parse dep mappings.
        let mut groups: Vec<Vec<TagList>> = vec![Vec::new(); n];
        let mut i: usize = 0;
        let mut bf: Option<TagId> = None;
        for &tter in &the_tags {
            let ttype = self.grammar.single_tags_list.get(tter.0).r#type;
            if ttype.intersects(T_WORDFORM) {
                i += 1;
                bf = None;
                continue;
            }
            if ttype.intersects(T_BASEFORM) {
                let wfid = self
                    .store
                    .cohorts
                    .get(cohort_ids[i - 1].0)
                    .wordform
                    .unwrap();
                groups[i - 1].push(vec![wfid]);
                bf = Some(tter);
            }
            if bf.is_none() {
                // Error: baseform must follow the wordform (I/O omitted); skip.
                continue;
            }

            // u_sscanf("%[0-9cd]->%[0-9pm]", &dep_self, &dep_parent) == 2
            let tagstr = self.grammar.single_tags_list.get(tter.0).tag.clone();
            if let Some((dep_self, dep_parent)) = split_dep_mapping(&tagstr) {
                let sc = dep_self.chars().next();
                if sc == Some('c') || sc == Some('d') {
                    cohort_dep[i - 1].0 = DEP_NO_PARENT;
                    if rel_trg == DEP_NO_PARENT {
                        rel_trg = ui32(i - 1);
                    }
                } else {
                    match parse_scanf_i(&dep_self) {
                        Some(v) => cohort_dep[i - 1].0 = v,
                        None => {
                            // Error: dep_self not valid (I/O omitted).
                        }
                    }
                }
                let pc = dep_parent.chars().next();
                if pc == Some('p') || pc == Some('m') {
                    cohort_dep[i - 1].1 = DEP_NO_PARENT;
                } else {
                    match parse_scanf_i(&dep_parent) {
                        Some(v) => cohort_dep[i - 1].1 = v,
                        None => {
                            // Error: dep_parent not valid (I/O omitted).
                        }
                    }
                }
                continue;
            }
            // R:* → relation transfer target.
            if tagstr.chars().count() == 3 && tagstr.starts_with("R:*") {
                rel_trg = ui32(i - 1);
                continue;
            }
            groups[i - 1].last_mut().unwrap().push(tter);
        }

        if rel_trg == DEP_NO_PARENT {
            rel_trg = ui32(n.saturating_sub(1));
        }

        // Build readings for each new cohort and splice them into the window.
        let apply = self.get_apply_to().cohort.unwrap();
        let mapping_prefix = self.grammar.mapping_prefix;
        let tag_any = self.grammar.tag_any;
        for idx in 0..n {
            let ccohort = cohort_ids[idx];
            let readings = groups[idx].clone();
            for mut tags in readings {
                let creading = crate::reading::alloc_reading(&mut self.store, Some(ccohort));
                self.numReadings = self.numReadings.wrapping_add(1);
                let sets_any = self.grammar.sets_any.clone();
                insert_if_exists(
                    &mut self.store.cohorts.get_mut(ccohort.0).possible_sets,
                    sets_any.as_ref(),
                );
                {
                    let rr = self.store.readings.get_mut(creading.0);
                    rr.hit_by.push(rnumber);
                    rr.noprint = false;
                }
                let mut mappings = TagList::new();

                // (*) expansion against the apply-to cohort's first reading.
                let mut k = 0usize;
                while k < tags.len() {
                    if self.grammar.single_tags_list.get(tags[k].0).hash.get() == tag_any {
                        let nt = self.store.cohorts.get(apply.0).readings[0];
                        let nt_list = self.store.readings.get(nt.0).tags_list.clone();
                        if nt_list.len() <= 2 {
                            k += 1;
                            continue;
                        }
                        tags[k] = self.tag_by_hash(TagHash(nt_list[2]));
                        let mut kk = 1usize;
                        for &nt_hash in &nt_list[3..] {
                            let tid = self.tag_by_hash(TagHash(nt_hash));
                            if self
                                .grammar
                                .single_tags_list
                                .get(tid.0)
                                .r#type
                                .intersects(T_DEPENDENCY)
                            {
                                continue;
                            }
                            tags.insert(k + kk, tid);
                            kk += 1;
                        }
                    }
                    k += 1;
                }

                for &tter in &tags {
                    let mut hash = self.grammar.single_tags_list.get(tter.0).hash;
                    let (ttype, first) = {
                        let t = self.grammar.single_tags_list.get(tter.0);
                        (t.r#type, t.tag.chars().next())
                    };
                    if ttype.intersects(T_MAPPING) || first == Some(mapping_prefix) {
                        mappings.push(tter);
                    } else {
                        hash = self.add_tag_to_reading(creading, tter);
                    }
                    if self.update_valid_rules(
                        &st.rules.clone(),
                        &mut st.intersects,
                        hash.get(),
                        creading,
                    ) {
                        st.iter_val = rnumber;
                    }
                }
                if !mappings.is_empty() {
                    self.split_mappings(&mut mappings, ccohort, creading, false);
                }
                crate::cohort::append_reading(&mut self.store, ccohort, creading);
            }

            if self.store.cohorts.get(ccohort.0).readings.is_empty() {
                self.init_empty_cohort(ccohort);
            }

            let cgn = self.store.cohorts.get(ccohort.0).global_number;
            self.gWindow.dep_window.insert(cgn, ccohort);
            self.gWindow.cohort_map.insert(cgn, ccohort);

            let apply_ln = self.store.cohorts.get(apply.0).local_number as usize;
            self.store
                .single_windows
                .get_mut(current.0)
                .cohorts
                .insert(apply_ln + idx + 1, ccohort);
            // all_cohorts.insert(find(begin + apply_ln, end, apply) + idx + 1)
            self.rr_splitcohort_insert_all(current, apply, apply_ln, idx, ccohort);
        }

        // Move text from the to-be-deleted cohort to the last new cohort.
        if n > 0 {
            let last = cohort_ids[n - 1];
            let src_text = std::mem::take(&mut self.store.cohorts.get_mut(apply.0).text);
            let last_text =
                std::mem::replace(&mut self.store.cohorts.get_mut(last.0).text, src_text);
            self.store.cohorts.get_mut(apply.0).text = last_text;
        }

        // Dependency + named-relation re-attachment.
        let front_gn = if n > 0 {
            self.store.cohorts.get(cohort_ids[0].0).global_number.get()
        } else {
            0
        };
        for idx in 0..n {
            let ccohort = cohort_ids[idx];

            if cohort_dep[idx].0 == DEP_NO_PARENT {
                // Forward the source cohort's children to this new cohort.
                loop {
                    let ch = {
                        let dc = &self.store.cohorts.get(apply.0).dep_children;
                        if dc.empty() {
                            break;
                        }
                        dc.back()
                    };
                    if let Some(&target) = self.gWindow.cohort_map.get(&GlobalNumber(ch)) {
                        self.attach_parent_child(ccohort, target, true, true);
                    }
                    self.store.cohorts.get_mut(apply.0).dep_children.erase(ch);
                }
            }

            if cohort_dep[idx].1 == DEP_NO_PARENT {
                let dp = self.store.cohorts.get(apply.0).dep_parent;
                if let Some(&parent) = dp.and_then(|v| self.gWindow.cohort_map.get(&v)) {
                    self.attach_parent_child(parent, ccohort, true, true);
                }
            } else {
                let key = front_gn.wrapping_add(cohort_dep[idx].1).wrapping_sub(1);
                if let Some(&parent) = self.gWindow.cohort_map.get(&GlobalNumber(key)) {
                    self.attach_parent_child(parent, ccohort, true, true);
                }
            }

            // Re-attach all named relations to the dependency tail / R:* cohort.
            if rel_trg == ui32(idx)
                && (self
                    .store
                    .cohorts
                    .get(apply.0)
                    .r#type
                    .intersects(CT_RELATED))
            {
                crate::cohort::set_related(&mut self.store, ccohort);
                {
                    let mut src_rels =
                        std::mem::take(&mut self.store.cohorts.get_mut(apply.0).relations);
                    std::mem::swap(
                        &mut self.store.cohorts.get_mut(ccohort.0).relations,
                        &mut src_rels,
                    );
                    self.store.cohorts.get_mut(apply.0).relations = src_rels;
                }
                let apply_gn = self.store.cohorts.get(apply.0).global_number.get();
                let ccohort_gn = self.store.cohorts.get(ccohort.0).global_number.get();
                let windows = self.rr_all_single_windows();
                for sw in windows {
                    let chs = self.store.single_windows.get(sw.0).cohorts.clone();
                    for ch in chs {
                        let keys: Vec<u32> = self
                            .store
                            .cohorts
                            .get(ch.0)
                            .relations
                            .keys()
                            .copied()
                            .collect();
                        for key in keys {
                            let rels = self
                                .store
                                .cohorts
                                .get_mut(ch.0)
                                .relations
                                .get_mut(&key)
                                .unwrap();
                            if rels.count(apply_gn) != 0 {
                                rels.erase(apply_gn);
                                rels.insert(ccohort_gn);
                            }
                        }
                    }
                }
            }
        }

        // Remove the source cohort.
        {
            let rs = self.store.cohorts.get(apply.0).readings.clone();
            for r in rs {
                self.store.readings.get_mut(r.0).hit_by.push(rnumber);
                self.store.readings.get_mut(r.0).deleted = true;
            }
        }
        self.store.cohorts.get_mut(apply.0).r#type |= CT_REMOVED;
        crate::cohort::detach(&mut self.store, apply);
        let apply_self = self
            .store
            .cohorts
            .get(apply.0)
            .dep_self
            .map_or(0, |g| g.get());
        let keys: Vec<GlobalNumber> = self.gWindow.cohort_map.keys().copied().collect();
        for k in keys {
            let cid = *self.gWindow.cohort_map.get(&k).unwrap();
            self.store
                .cohorts
                .get_mut(cid.0)
                .dep_children
                .erase(apply_self);
        }
        let apply_gn = self.store.cohorts.get(apply.0).global_number;
        self.gWindow.cohort_map.remove(&apply_gn);
        let apply_ln = self.store.cohorts.get(apply.0).local_number as usize;
        self.store
            .single_windows
            .get_mut(current.0)
            .cohorts
            .remove(apply_ln);
        // NOTE: C++ does NOT erase the source cohort from `all_cohorts` — it
        // stays there flagged CT_REMOVED so trace mode prints it as `;` lines.

        self.rr_reindex(current);
        self.index_single_window(current);
        st.readings_changed = true;
        self.reset_cohorts_for_loop = true;
    }

    /// SPLITCOHORT `all_cohorts` splice: reproduces
    /// `all_cohorts.insert(find(begin + apply_ln, end, apply) + idx + 1, ccohort)`
    /// — search for `apply` starting at `apply_ln`, insert `idx+1` slots past it.
    fn rr_splitcohort_insert_all(
        &mut self,
        current: SwId,
        apply: CohortId,
        apply_ln: usize,
        idx: usize,
        ccohort: CohortId,
    ) {
        let ac = &mut self.store.single_windows.get_mut(current.0).all_cohorts;
        let start = apply_ln.min(ac.len());
        let found = ac[start..]
            .iter()
            .position(|&x| x == apply)
            .map(|p| start + p);
        match found {
            Some(p) => {
                let at = (p + idx + 1).min(ac.len());
                ac.insert(at, ccohort);
            }
            None => ac.push(ccohort),
        }
    }

    /// All single-windows in `previous`+`current`+`next` order — the C++
    /// `swss[3]` triple iterated by the SPLITCOHORT relation rewrite.
    fn rr_all_single_windows(&self) -> Vec<SwId> {
        let mut out: Vec<SwId> = Vec::new();
        out.extend(self.gWindow.previous.iter().copied());
        if let Some(cur) = self.gWindow.current {
            out.push(cur);
        }
        out.extend(self.gWindow.next.iter().copied());
        out
    }
}
