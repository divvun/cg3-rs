//! `GrammarApplicator` — runGrammarOnWindow + the rule_to_cohorts bookkeeping (the C++ update/index machinery).
//!
//! Split out of the wave-2 monolithic `run_rules.rs` (wave 4, w4-file-split-fmt).

use crate::arena::{CohortId, ReadingId, RuleId, SetId, SwId, TagId};
use crate::cohort::{CT_ENCLOSED, CT_IGNORED, CT_REMOVED, CohortSet};
use crate::inlines::ui32;
use crate::interval_vector::uint32IntervalVector;
use crate::reading::Reading;
use crate::set::{ST_SET_UNIFY, ST_TAG_UNIFY, Set};
use crate::tag::TagList;
use crate::types::{SetNumber, TagHash};

// C++ anonymous `enum { RV_NOTHING = 1, RV_SOMETHING = 2, RV_DELIMITED = 4,
// RV_TRACERULE = 8 };` — the return-value bit flags of runRulesOnSingleWindow.

use super::*;

impl crate::grammar_applicator::Engine<'_> {
    // ---- small helpers (not manifest symbols) --------------------------------

    /// Resolve a tag *hash* to its `TagId` via `grammar.single_tags`
    /// (`grammar->single_tags.find(h)->second`).
    #[inline]
    pub(crate) fn tag_by_hash(&self, h: TagHash) -> TagId {
        self.grammar.single_tags.find(h.get()).get().1
    }

    /// By-id wrapper for the sibling `generate_varstring_tag(&mut self, &Tag)`:
    /// clone the `Tag` out of the grammar arena (so the `&mut self` matcher does
    /// not alias the grammar borrow) and delegate. The C++ `generateVarstringTag`
    /// takes a `const Tag*`; the arena port takes `&Tag`, so run_rules — which
    /// threads `TagId`s — needs this shim.
    #[inline]
    pub(crate) fn generate_varstring_tag_id(&mut self, tag: TagId) -> TagId {
        let t = self.grammar.single_tags_list.get(tag.0).clone();
        self.generate_varstring_tag(&t)
    }

    /// C++ `TRACE` macro: push `rule->number` onto the apply-to subreading's
    /// `hit_by`, and — when the rule targets the whole reading (`sub_reading ==
    /// 32767`) — onto the apply-to reading's `hit_by` too.
    pub(crate) fn trace(&mut self, rule_number: u32, rule_sub_reading: i32) {
        let at = self.get_apply_to();
        if let Some(sr) = at.subreading {
            self.doc
                .store
                .readings
                .get_mut(sr.0)
                .hit_by
                .push(rule_number);
        }
        if rule_sub_reading == GSR_ANY
            && let Some(r) = at.reading
        {
            self.doc
                .store
                .readings
                .get_mut(r.0)
                .hit_by
                .push(rule_number);
        }
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.does-wordforms-match-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.does-wordforms-match-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-wordforms-match-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-wordforms-match-fn]
    /// C++ `bool doesWordformsMatch(const Tag* cword, const Tag* rword)`.
    /// `cword`/`rword` are the cohort's and rule's wordform tags (nullable →
    /// `Option<TagId>`).
    pub fn does_wordforms_match(&mut self, cword: Option<TagId>, rword: Option<TagId>) -> bool {
        if let Some(rw) = rword
            && Some(rw) != cword
        {
            // `rword` (a Tag in the grammar arena) is cloned out so the
            // `&mut self` matcher calls do not alias the grammar borrow.
            let rword_tag = self.grammar.single_tags_list.get(rw.0).clone();
            let chash = cword
                .map(|c| self.grammar.single_tags_list.get(c.0).hash)
                .map_or(0, |h| h.get());
            if rword_tag.r#type.intersects(crate::tag::T_REGEXP) {
                if self.does_tag_match_regexp(chash, &rword_tag, false) == 0 {
                    return false;
                }
            } else if rword_tag.r#type.intersects(crate::tag::T_CASE_INSENSITIVE) {
                if self.does_tag_match_icase(chash, &rword_tag, false) == 0 {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.update-rule-to-cohorts-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.update-rule-to-cohorts-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.update-rule-to-cohorts-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.update-rule-to-cohorts-fn]
    /// C++ `bool updateRuleToCohorts(Cohort& c, const uint32_t& rsit)`.
    ///
    /// Registers cohort `c` as a candidate for rule `rsit`, keeping any active
    /// `run_single_rule` iterators (`cohortsets`/`rocits`) valid across the sorted
    /// insert (the live-iterator-safe insert quirk, reproduced via the raw-pointer
    /// bookkeeping the struct already carries).
    /// Resolve a [`CsRef`] descriptor to the live cohort set (wave 4 — the
    /// safe replacement for the parked `CohortSet*`).
    pub(crate) fn cs_ref(&self, r: crate::grammar_applicator::CsRef) -> &CohortSet {
        match r {
            crate::grammar_applicator::CsRef::Window { sw, rule } => {
                &self.doc.store.single_windows.get(sw.0).rule_to_cohorts[rule as usize]
            }
            crate::grammar_applicator::CsRef::Nested { sw } => self
                .doc
                .store
                .single_windows
                .get(sw.0)
                .nested_rule_to_cohorts
                .as_deref()
                .expect("CsRef::Nested resolved with no nested_rule_to_cohorts"),
        }
    }

    /// Mutable [`Self::cs_ref`].
    pub(crate) fn cs_mut(&mut self, r: crate::grammar_applicator::CsRef) -> &mut CohortSet {
        match r {
            crate::grammar_applicator::CsRef::Window { sw, rule } => {
                &mut self.doc.store.single_windows.get_mut(sw.0).rule_to_cohorts[rule as usize]
            }
            crate::grammar_applicator::CsRef::Nested { sw } => self
                .doc
                .store
                .single_windows
                .get_mut(sw.0)
                .nested_rule_to_cohorts
                .as_deref_mut()
                .expect("CsRef::Nested resolved with no nested_rule_to_cohorts"),
        }
    }

    pub fn update_rule_to_cohorts(&mut self, c: CohortId, rsit: u32) -> bool {
        // --rule(s) cmdline filter.
        if !self.cfg.valid_rules.empty() && !self.cfg.valid_rules.contains(rsit) {
            return false;
        }
        let current = self.doc.store.cohorts.get(c.0).parent.unwrap();
        let r = RuleId(rsit); // grammar->rule_by_number[rsit]
        let cword = self.doc.store.cohorts.get(c.0).wordform;
        let rword = self.grammar.rule_by_number.get(r.0).wordform;
        if !self.does_wordforms_match(cword, rword) {
            return false;
        }
        if (self
            .doc
            .store
            .single_windows
            .get(current.0)
            .rule_to_cohorts
            .len() as u32)
            < rsit + 1
        {
            self.index_single_window(current);
        }

        // cohortset = &current->rule_to_cohorts[rsit], identified by (window,
        // rule) descriptor. Scan the active-iterator stack for frames iterating
        // this same set (the C++ pointer-identity check, now CsRef equality).
        let r_ref = crate::grammar_applicator::CsRef::Window {
            sw: current,
            rule: rsit,
        };
        let mut csi: Vec<usize> = Vec::new();
        for i in 0..self.scratch.cohortsets.len() {
            if self.scratch.cohortsets[i] != r_ref {
                continue;
            }
            csi.push(i);
        }

        if !csi.is_empty() {
            // Snapshot capacity, then split the active iterators into "parked at
            // end" and "(frame, cohort-at-position)".
            let cap = self.cs_ref(r_ref).capacity();
            let mut ends: Vec<usize> = Vec::new();
            let mut chs: Vec<(usize, CohortId)> = Vec::new();
            for &i in &csi {
                let pos = self.scratch.rocits[i];
                let size = self.cs_ref(r_ref).size();
                if pos >= size {
                    ends.push(i);
                } else {
                    let at = self.cs_ref(r_ref).as_slice()[pos];
                    chs.push((i, at));
                }
            }
            self.cohortset_insert_at(r_ref, c);
            let new_size = self.cs_ref(r_ref).size();
            for i in ends {
                self.scratch.rocits[i] = new_size;
            }
            if cap != self.cs_ref(r_ref).capacity() {
                for (i, cohort) in chs {
                    self.scratch.rocits[i] = self.cohortset_find_n_at(r_ref, cohort);
                }
            }
        } else {
            self.cohortset_insert_at(r_ref, c);
        }

        self.doc
            .store
            .single_windows
            .get_mut(current.0)
            .valid_rules
            .insert(rsit)
    }

    /// The `std::lower_bound` core over a cohort-set slice with the store-aware
    /// `compare_Cohort` ([`crate::single_window::less_cohort`]).
    fn cs_lower_bound_slice(&self, slice: &[CohortId], c: CohortId) -> usize {
        let mut lo = 0usize;
        let mut hi = slice.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if crate::single_window::less_cohort(&self.doc.store, slice[mid], c) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// The C++ `sorted_vector::find` core (front/back early-outs, lower_bound,
    /// comparator-equivalence check) over a slice; index or `len()` if absent.
    fn cs_find_n_slice(&self, slice: &[CohortId], c: CohortId) -> usize {
        if slice.is_empty() {
            return 0;
        }
        let store = &self.doc.store;
        let last = slice.len() - 1;
        if crate::single_window::less_cohort(store, slice[last], c) {
            return slice.len();
        }
        if crate::single_window::less_cohort(store, c, slice[0]) {
            return slice.len();
        }
        let it = self.cs_lower_bound_slice(slice, c);
        if it != slice.len()
            && (crate::single_window::less_cohort(store, slice[it], c)
                || crate::single_window::less_cohort(store, c, slice[it]))
        {
            return slice.len();
        }
        it
    }

    /// Store-aware `CohortSet::insert` on a LOCAL set (a set not owned by the
    /// store, e.g. the collect_subtree scratch sets). Two-phase: position with
    /// `&self`, then mutate the caller's set — no aliasing.
    pub(crate) fn cohortset_insert(&self, cs: &mut CohortSet, c: CohortId) {
        let lo = self.cs_lower_bound_slice(cs.as_slice(), c);
        // Dedup: C++ sorted_vector insert is a set-insert (no dup).
        if lo < cs.as_slice().len() && cs.as_slice()[lo] == c {
            return;
        }
        cs.get().insert(lo, c);
    }

    /// Store-aware `CohortSet::insert` on a store-owned set (by descriptor).
    pub(crate) fn cohortset_insert_at(&mut self, r: crate::grammar_applicator::CsRef, c: CohortId) {
        let lo = self.cs_lower_bound_slice(self.cs_ref(r).as_slice(), c);
        let cs = self.cs_mut(r);
        if lo < cs.as_slice().len() && cs.as_slice()[lo] == c {
            return;
        }
        cs.get().insert(lo, c);
    }

    /// Store-aware `CohortSet::lower_bound` on a store-owned set. Must be used
    /// on sets built via the sorted inserts above (`(local_number, window
    /// number)` order, NOT raw `CohortId`).
    pub(crate) fn cohortset_lower_bound_at(
        &self,
        r: crate::grammar_applicator::CsRef,
        c: CohortId,
    ) -> usize {
        self.cs_lower_bound_slice(self.cs_ref(r).as_slice(), c)
    }

    /// Store-aware `CohortSet::find_n` on a store-owned set.
    pub(crate) fn cohortset_find_n_at(
        &self,
        r: crate::grammar_applicator::CsRef,
        c: CohortId,
    ) -> usize {
        self.cs_find_n_slice(self.cs_ref(r).as_slice(), c)
    }

    /// Store-aware `CohortSet::erase` on a store-owned set — mirrors C++
    /// `sorted_vector::erase` with `compare_Cohort` (comparator-equivalence,
    /// not id-equality, exactly as the C++ does). `true` iff removed.
    pub(crate) fn cohortset_erase_at(
        &mut self,
        r: crate::grammar_applicator::CsRef,
        c: CohortId,
    ) -> bool {
        let n = self.cohortset_find_n_at(r, c);
        let cs = self.cs_mut(r);
        if n != cs.size() {
            cs.erase_n(n);
            return true;
        }
        false
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.update-valid-rules-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.update-valid-rules-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.update-valid-rules-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.update-valid-rules-fn]
    /// C++ `bool updateValidRules(const uint32IntervalVector& rules,
    /// uint32IntervalVector& intersects, const uint32_t& hash, Reading& reading)`.
    ///
    /// `reading` → `ReadingId` (used only for `reading.parent`).
    pub fn update_valid_rules(
        &mut self,
        rules: &uint32IntervalVector,
        intersects: &mut uint32IntervalVector,
        hash: u32,
        reading: ReadingId,
    ) -> bool {
        let os = intersects.size();
        // grammar->rules_by_tag.find(hash)
        let rsits: Option<Vec<u32>> = self.grammar.rules_by_tag.get(&hash).map(iv_to_vec);
        if let Some(rsits) = rsits {
            let c = self.doc.store.readings.get(reading.0).parent.unwrap();
            for rsit in rsits {
                if self.update_rule_to_cohorts(c, rsit) && rules.contains(rsit) {
                    intersects.insert(rsit);
                }
            }
        }
        os != intersects.size()
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.index-single-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.index-single-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.index-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.index-single-window-fn]
    /// C++ `void indexSingleWindow(SingleWindow& current)` (`current` → `SwId`).
    pub fn index_single_window(&mut self, current: SwId) {
        let nrules = self.grammar.rule_by_number.capacity() as usize;
        {
            let sw = self.doc.store.single_windows.get_mut(current.0);
            sw.valid_rules.clear();
            sw.rule_to_cohorts.resize_with(nrules, CohortSet::new);
            for cs in sw.rule_to_cohorts.iter_mut() {
                cs.clear();
            }
        }

        let cohorts = self.doc.store.single_windows.get(current.0).cohorts.clone();
        for c in cohorts {
            let psize = self.doc.store.cohorts.get(c.0).possible_sets.len();
            for psit in 0..psize as u32 {
                if !self.doc.store.cohorts.get(c.0).possible_sets[psit as usize] {
                    continue;
                }
                // grammar->rules_by_set.find(psit)
                let rules_of_set: Option<Vec<u32>> =
                    self.grammar.rules_by_set.get(&psit).map(iv_to_vec);
                if let Some(rsits) = rules_of_set {
                    for rsit in rsits {
                        self.update_rule_to_cohorts(c, rsit);
                    }
                }
            }
        }
    }

}

// ===========================================================================
// Stage-C decomposition: the `getTagList` read family + `get_sub_reading` /
// `clone_reading_value`, reached from the contextual matcher knot
// (`generate_varstring_tag` → `get_tag_list`; `does_set_match_*` →
// `get_sub_reading`), converted onto the split-borrow `Engine<'_>` view.
// The `getTagList` overloads form a `&self` call chain and so peel as a unit;
// unpeeled `&mut self` callers split at the call site via
// `self.<method>(...)`.
// ===========================================================================
impl crate::grammar_applicator::Engine<'_> {
    /// C++ `TagList getTagList(const Set& theSet, bool unif_mode) const` — the
    /// returning overload: constructs a fresh list, fills it, returns it.
    pub fn get_tag_list_ret(&self, the_set: &Set, unif_mode: bool) -> TagList {
        let mut the_tags = TagList::new();
        self.get_tag_list(the_set, &mut the_tags, unif_mode);
        the_tags
    }

    /// Convenience: `getTagList(*grammar->sets_list[set])` by `SetId`, resolving
    /// the `&Set` borrow internally so callers need not clone the (non-Clone)
    /// `Set`. `Set` is looked up by arena index (`sets_list[set.0]`).
    pub(crate) fn get_tag_list_of_set(&self, set: SetId, unif_mode: bool) -> TagList {
        let the_set = &self.grammar.sets_list[set.0];
        self.get_tag_list_ret(the_set, unif_mode)
    }

    /// As [`Self::get_tag_list_of_set`] but keyed by the raw set *number* (the C++
    /// `grammar->sets_list[number]` — resolved through `sets_list_order`).
    pub(crate) fn get_tag_list_of_set_number(&self, number: u32, unif_mode: bool) -> TagList {
        let the_set = self.grammar.set_by_number(SetNumber(number));
        self.get_tag_list_ret(the_set, unif_mode)
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
    /// C++ `void getTagList(const Set& theSet, TagList& theTags, bool unif_mode)
    /// const` — the by-reference overload (appends to `the_tags`).
    pub fn get_tag_list(&self, the_set: &Set, the_tags: &mut TagList, unif_mode: bool) {
        if the_set.r#type.intersects(ST_SET_UNIFY) {
            // usets = (*context_stack.back().unif_sets)[theSet.number]
            let unif_sets = self
                .scratch
                .context_stack
                .last()
                .unwrap()
                .unif_sets
                .unwrap();
            let usets = self.scratch.unif_sets_store[unif_sets].get(&the_set.number.get());
            let p_set = self.grammar.set_by_number(SetNumber(the_set.sets[0]));
            for &iter in &p_set.sets {
                let present = usets.map(|s| s.count(iter) != 0).unwrap_or(false);
                if present {
                    self.get_tag_list(self.grammar.set_by_number(SetNumber(iter)), the_tags, false);
                }
            }
        } else if the_set.r#type.intersects(ST_TAG_UNIFY) {
            for &iter in &the_set.sets {
                self.get_tag_list(self.grammar.set_by_number(SetNumber(iter)), the_tags, true);
            }
        } else if !the_set.sets.is_empty() {
            for &iter in &the_set.sets {
                self.get_tag_list(
                    self.grammar.set_by_number(SetNumber(iter)),
                    the_tags,
                    unif_mode,
                );
            }
        } else if unif_mode {
            let unif_tags = self
                .scratch
                .context_stack
                .last()
                .unwrap()
                .unif_tags
                .unwrap();
            let val = self.scratch.unif_tags_store[unif_tags]
                .get(&the_set.number.get())
                .copied();
            if let Some(node) = val {
                crate::tag_trie::trie_get_tag_list_find(
                    &the_set.trie,
                    the_tags,
                    node as *const core::ffi::c_void,
                    self.grammar,
                );
                crate::tag_trie::trie_get_tag_list_find(
                    &the_set.trie_special,
                    the_tags,
                    node as *const core::ffi::c_void,
                    self.grammar,
                );
            }
        } else {
            crate::tag_trie::trie_get_tag_list_append(&the_set.trie, the_tags, self.grammar);
            crate::tag_trie::trie_get_tag_list_append(
                &the_set.trie_special,
                the_tags,
                self.grammar,
            );
        }

        // Eliminate CONSECUTIVE duplicates only (non-adjacent dups are kept, for
        // AddCohort/Append repeated tags across readings).
        let mut oti = 0usize;
        while the_tags.len() > 1 && oti < the_tags.len() {
            let mut it = oti + 1;
            while it < the_tags.len() && it - oti == 1 {
                if the_tags[oti] == the_tags[it] {
                    the_tags.remove(it);
                } else {
                    it += 1;
                }
            }
            oti += 1;
        }
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.get-sub-reading-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.get-sub-reading-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-sub-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-sub-reading-fn]
    /// C++ `Reading* get_sub_reading(Reading* tr, int sub_reading)`
    /// (`Reading*` → `Option<ReadingId>`).
    ///
    /// RECONCILIATION: the `GSR_ANY` amalgam is a fresh Reading. C++ stores it in
    /// `subs_any` (a `std::deque<Reading>`) and returns a bare `Reading*`. In the
    /// arena model the amalgam must live in the readings arena to have a
    /// `ReadingId` the matchers can consume, so it is allocated there and its id
    /// tracked in `subs_any` — which therefore needs to become `Vec<ReadingId>`
    /// (cleared/freed by `clear(subs_any)` at each cohort). See report.
    pub fn get_sub_reading(&mut self, tr: ReadingId, sub_reading: i32) -> Option<ReadingId> {
        if sub_reading == 0 {
            return Some(tr);
        }

        if sub_reading == GSR_ANY {
            if self.doc.store.readings.get(tr.0).next.is_none() {
                return Some(tr);
            }
            // reading = fresh; *reading = *tr; reading->next = nullptr.
            let amalgam = self.clone_reading_value(tr);
            let rid = ReadingId(self.doc.store.readings.alloc(amalgam));
            self.doc.store.readings.get_mut(rid.0).next = None;
            self.subs_any_push(rid);

            let mut cur = tr;
            while let Some(next) = self.doc.store.readings.get(cur.0).next {
                cur = next;
                // tags_list: push 0 then extend with cur.tags_list
                let cur_tags_list = self.doc.store.readings.get(cur.0).tags_list.clone();
                {
                    let r = self.doc.store.readings.get_mut(rid.0);
                    r.tags_list.push(0);
                    r.tags_list.extend(cur_tags_list.iter().copied());
                }
                let (tags, tags_plain, tags_textual) = {
                    let cr = self.doc.store.readings.get(cur.0);
                    (
                        cr.tags.as_slice().to_vec(),
                        cr.tags_plain.as_slice().to_vec(),
                        cr.tags_textual.as_slice().to_vec(),
                    )
                };
                {
                    let r = self.doc.store.readings.get_mut(rid.0);
                    for t in tags {
                        r.tags.insert(t);
                        r.tags_bloom.insert(t);
                    }
                    for t in tags_plain {
                        r.tags_plain.insert(t);
                        r.tags_plain_bloom.insert(t);
                    }
                    for t in tags_textual {
                        r.tags_textual.insert(t);
                        r.tags_textual_bloom.insert(t);
                    }
                }
                let cur_num = self.doc.store.readings.get(cur.0).tags_numerical.clone();
                let (mapped, mapping, mt, mtst) = {
                    let cr = self.doc.store.readings.get(cur.0);
                    (cr.mapped, cr.mapping, cr.matched_target, cr.matched_tests)
                };
                let r = self.doc.store.readings.get_mut(rid.0);
                for (k, v) in cur_num {
                    r.tags_numerical.insert(k, v);
                }
                if mapped {
                    r.mapped = true;
                }
                if mapping.is_some() {
                    r.mapping = mapping;
                }
                if mt {
                    r.matched_target = true;
                }
                if mtst {
                    r.matched_tests = true;
                }
            }
            crate::reading::reading_rehash(&mut self.doc.store, self.grammar, rid);
            return Some(rid);
        }

        if sub_reading > 0 {
            let mut cur = Some(tr);
            let mut i = 0;
            while i < sub_reading && cur.is_some() {
                cur = self.doc.store.readings.get(cur.unwrap().0).next;
                i += 1;
            }
            return cur;
        }

        // sub_reading < 0
        let mut ntr = 0i32;
        let mut ttr = Some(tr);
        while let Some(t) = ttr {
            ttr = self.doc.store.readings.get(t.0).next;
            ntr -= 1;
        }
        let mut cur = Some(tr);
        if self.doc.store.readings.get(tr.0).next.is_none() {
            cur = None;
        }
        let mut i = ntr;
        while i < sub_reading && cur.is_some() {
            cur = self.doc.store.readings.get(cur.unwrap().0).next;
            i += 1;
        }
        cur
    }

    /// Verbatim field copy of a stored `Reading` (the C++ `*reading = *tr`).
    /// `Reading` derives only `Default`, so the fields are copied explicitly.
    pub(crate) fn clone_reading_value(&self, id: ReadingId) -> Reading {
        crate::reading::clone_verbatim(self.doc.store.readings.get(id.0))
    }
}

impl crate::grammar_applicator::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-single-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-grammar-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-single-window-fn]
    /// C++ `uint32_t runGrammarOnSingleWindow(SingleWindow& current)`.
    pub fn run_grammar_on_single_window(&mut self, current: SwId) -> u32 {
        if !self.grammar.before_sections.is_empty() && !self.cfg.no_before_sections {
            let rules = self.cfg.runsections.get(&-1).cloned().unwrap_or_default();
            let rv = self.engine().run_rules_on_single_window(current, &rules);
            if rv & (RV_DELIMITED | RV_TRACERULE) != 0 {
                return rv;
            }
        }

        if !self.grammar.rules.is_empty() && !self.cfg.no_sections {
            let mut counter: std::collections::BTreeMap<i32, u32> =
                std::collections::BTreeMap::new();
            // Iterate runsections (ordered by section key). Callbacks can change
            // window state but not the runsections map; a plain key cursor mirrors
            // the C++ `iter`/`++iter`.
            let keys: Vec<i32> = self.cfg.runsections.keys().copied().collect();
            let mut idx = 0usize;
            let mut pass = 0usize;
            while idx < keys.len() {
                let key = keys[idx];
                if key < 0
                    || (self.cfg.section_max_count != 0
                        && *counter.get(&key).unwrap_or(&0) >= self.cfg.section_max_count)
                {
                    idx += 1;
                    pass = 0;
                    continue;
                }
                let rules = self.cfg.runsections.get(&key).cloned().unwrap();
                let rv = self.engine().run_rules_on_single_window(current, &rules);
                *counter.entry(key).or_insert(0) += 1;
                if rv & (RV_DELIMITED | RV_TRACERULE) != 0 {
                    return rv;
                }
                if rv & RV_SOMETHING == 0 {
                    idx += 1;
                    pass = 0;
                } else {
                    pass += 1;
                }
                if pass >= 1000 {
                    // Endless-loop warning (window wordform dump omitted — I/O pass).
                    break;
                }
            }
        }

        if !self.grammar.after_sections.is_empty() && !self.cfg.no_after_sections {
            let rules = self.cfg.runsections.get(&-2).cloned().unwrap_or_default();
            let rv = self.engine().run_rules_on_single_window(current, &rules);
            if rv & (RV_DELIMITED | RV_TRACERULE) != 0 {
                return rv;
            }
        }

        0
    }

    /// One pass of the `runGrammarOnWindow` enclosure-wrapping scan (the C++
    /// `goto scanParentheses` loop body, wave 4: extracted). Returns `true` if
    /// an enclosure was wrapped (the caller re-scans from scratch), `false`
    /// when a full pass changed nothing.
    fn rr_wrap_one_enclosure(&mut self, current: SwId) -> bool {
        let cohorts = self.doc.store.single_windows.get(current.0).cohorts.clone();
        for ci in (0..cohorts.len()).rev() {
            let c = cohorts[ci];
            let is_pleft = self.doc.store.cohorts.get(c.0).is_pleft;
            if is_pleft == 0 {
                continue;
            }
            let pright = self.grammar.parentheses.get(&is_pleft).copied();
            if let Some(pright) = pright {
                let mut found = false;
                let mut encs: Vec<CohortId> = Vec::new();
                let cur_cohorts = self.doc.store.single_windows.get(current.0).cohorts.clone();
                let mut right = ci;
                while right < cur_cohorts.len() {
                    let s = cur_cohorts[right];
                    encs.push(s);
                    if self.doc.store.cohorts.get(s.0).is_pright == pright {
                        found = true;
                        break;
                    }
                    right += 1;
                }
                if found {
                    // Remove enclosed span from `cohorts`, shifting left.
                    let left = ci;
                    let lc = self.doc.store.cohorts.get(cur_cohorts[left].0).local_number;
                    let mut writ = left;
                    let mut lc = lc;
                    let mut rd = right + 1;
                    {
                        let sw = self.doc.store.single_windows.get_mut(current.0);
                        while rd < sw.cohorts.len() {
                            sw.cohorts[writ] = sw.cohorts[rd];
                            writ += 1;
                            rd += 1;
                        }
                    }
                    // Renumber the moved cohorts.
                    let moved: Vec<CohortId> =
                        self.doc.store.single_windows.get(current.0).cohorts[left..writ].to_vec();
                    for cid in moved {
                        self.doc.store.cohorts.get_mut(cid.0).local_number = lc;
                        lc += 1;
                    }
                    let new_len =
                        self.doc.store.single_windows.get(current.0).cohorts.len() - encs.len();
                    self.doc
                        .store
                        .single_windows
                        .get_mut(current.0)
                        .cohorts
                        .truncate(new_len);
                    // C++ walks the CONTIGUOUS all_cohorts range from
                    // encs.front() to encs.back() inclusive — also
                    // bumping `enclosed` on previously-wrapped cohorts
                    // sandwiched in the span; that encodes nesting depth.
                    {
                        let front = encs[0];
                        let back = *encs.last().unwrap();
                        let start_ln = self.doc.store.cohorts.get(front.0).local_number as usize;
                        let all = self
                            .doc
                            .store
                            .single_windows
                            .get(current.0)
                            .all_cohorts
                            .clone();
                        let mut ec = all[start_ln..]
                            .iter()
                            .position(|&x| x == front)
                            .map(|p| p + start_ln)
                            .expect("enclosure front in all_cohorts");
                        loop {
                            let c = self.doc.store.cohorts.get_mut(all[ec].0);
                            c.r#type |= CT_ENCLOSED;
                            c.enclosed += 1;
                            if all[ec] == back {
                                break;
                            }
                            ec += 1;
                        }
                    }
                    self.doc
                        .store
                        .single_windows
                        .get_mut(current.0)
                        .has_enclosures = true;
                    return true;
                }
            }
        }
        false
    }

    /// The enclosure-unpacking step of `runGrammarOnWindow` (the C++ `label_unpackEnclosures`
    /// block, wave 4: extracted from the `'run_begin`/`'unpack` label pair).
    /// Returns `true` when the window was restructured and the whole window
    /// pass must restart (the C++ `goto reflowDependencyWindow` /
    /// `goto runGrammarOnWindow_begin`), `false` to fall through to the
    /// ignored-cohort restore.
    fn rr_unpack_enclosures(&mut self, current: SwId, rv: u32) -> bool {
        loop {
            if self.doc.store.single_windows.get(current.0).has_enclosures {
                let nc = self
                    .doc
                    .store
                    .single_windows
                    .get(current.0)
                    .all_cohorts
                    .len();
                let mut handled = false;
                let mut i = 0usize;
                while i < nc {
                    let c = self.doc.store.single_windows.get(current.0).all_cohorts[i];
                    if self.doc.store.cohorts.get(c.0).enclosed == 1 {
                        let mut la = i;
                        while la > 0 {
                            let prev =
                                self.doc.store.single_windows.get(current.0).all_cohorts[la - 1];
                            if !self
                                .doc
                                .store
                                .cohorts
                                .get(prev.0)
                                .r#type
                                .intersects(CT_ENCLOSED | CT_REMOVED | CT_IGNORED)
                            {
                                la -= 1;
                                break;
                            }
                            la -= 1;
                        }
                        let ni = {
                            let lac = self.doc.store.single_windows.get(current.0).all_cohorts[la];
                            self.doc.store.cohorts.get(lac.0).local_number as usize
                        };

                        let mut ra = i;
                        let mut ne = 0usize;
                        while ra < nc {
                            let rac = self.doc.store.single_windows.get(current.0).all_cohorts[ra];
                            if !self
                                .doc
                                .store
                                .cohorts
                                .get(rac.0)
                                .r#type
                                .intersects(CT_ENCLOSED | CT_REMOVED | CT_IGNORED)
                            {
                                break;
                            }
                            {
                                let c = self.doc.store.cohorts.get_mut(rac.0);
                                c.enclosed -= 1;
                                if c.enclosed == 0 {
                                    c.r#type &= !CT_ENCLOSED;
                                    ne += 1;
                                }
                            }
                            ra += 1;
                        }

                        {
                            let clen = self.doc.store.single_windows.get(current.0).cohorts.len();
                            let sw = self.doc.store.single_windows.get_mut(current.0);
                            sw.cohorts.resize(clen + ne, CohortId(u32::MAX));
                        }
                        {
                            let clen = self.doc.store.single_windows.get(current.0).cohorts.len();
                            let mut j = clen - 1;
                            while j > ni + ne {
                                let moved =
                                    self.doc.store.single_windows.get(current.0).cohorts[j - ne];
                                self.doc.store.single_windows.get_mut(current.0).cohorts[j] = moved;
                                self.doc.store.cohorts.get_mut(moved.0).local_number = ui32(j);
                                self.doc.store.single_windows.get_mut(current.0).cohorts[j - ne] =
                                    CohortId(u32::MAX);
                                j -= 1;
                            }
                        }
                        {
                            let mut j = 0usize;
                            while i < ra {
                                let ac =
                                    self.doc.store.single_windows.get(current.0).all_cohorts[i];
                                if self.doc.store.cohorts.get(ac.0).enclosed == 0 {
                                    self.doc.store.single_windows.get_mut(current.0).cohorts
                                        [ni + j + 1] = ac;
                                    self.doc.store.cohorts.get_mut(ac.0).local_number =
                                        ui32(ni + j + 1);
                                    self.doc.store.cohorts.get_mut(ac.0).parent = Some(current);
                                    j += 1;
                                }
                                i += 1;
                            }
                        }
                        self.scratch.par_left_tag = {
                            let ac =
                                self.doc.store.single_windows.get(current.0).all_cohorts[la + 1];
                            TagHash(self.doc.store.cohorts.get(ac.0).is_pleft)
                        };
                        self.scratch.par_right_tag = {
                            let ac =
                                self.doc.store.single_windows.get(current.0).all_cohorts[ra - 1];
                            TagHash(self.doc.store.cohorts.get(ac.0).is_pright)
                        };
                        self.scratch.par_left_pos = ui32(ni + 1);
                        self.scratch.par_right_pos = ui32(ni + ne);
                        if rv & RV_TRACERULE != 0 {
                            continue;
                        }
                        handled = true;
                        break;
                    }
                    i += 1;
                }
                if handled {
                    return true;
                }
                if !self.scratch.did_final_enclosure {
                    self.scratch.par_left_tag = TagHash(0);
                    self.scratch.par_right_tag = TagHash(0);
                    self.scratch.par_left_pos = 0;
                    self.scratch.par_right_pos = 0;
                    self.scratch.did_final_enclosure = true;
                    if rv & RV_TRACERULE != 0 {
                        continue;
                    }
                    return true;
                }
            }
            break;
        }
        false
    }

    /// One pass of `runGrammarOnWindow`'s main loop (the C++
    /// `runGrammarOnWindow_begin:` label body, wave 4: extracted).
    /// `Continue` is the C++ `goto runGrammarOnWindow_begin` (delimit /
    /// enclosure restart); `Break` ends the window (normal fall-through or the
    /// 1000-pass endless-loop bail).
    fn rr_window_pass<F, W>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        pass: &mut u32,
    ) -> std::ops::ControlFlow<()>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        W: std::io::Write,
    {
        while !self.doc.stream.previous.is_empty()
            && self.doc.stream.previous.len() as u32 > self.cfg.num_windows
        {
            let tmp = self.doc.stream.previous[0];
            // C++ `printSingleWindow(tmp, *ux_stdout)` — print to the live
            // output writer threaded in by the driver, in the most-derived
            // applicator's format.
            fmt.print_single_window(self, tmp, output, false);
            let opt = Some(tmp);
            crate::single_window::free_swindow(
                &mut self.doc.store,
                &mut self.doc.cohorts,
                &mut self.doc.deps,
                opt,
            );
            self.doc.stream.previous.remove(0);
        }

        self.scratch.rule_hits.clear();
        self.scratch.index_ruleCohort_no.clear(0);
        let current = self.doc.stream.current.unwrap();
        self.engine().index_single_window(current);
        self.doc
            .store
            .single_windows
            .get_mut(current.0)
            .hit_external
            .clear();
        self.doc.stream.rebuild_cohort_links(&mut self.doc.store);

        *pass += 1;
        if *pass > 1000 {
            // Endless-loop warning (I/O pass omitted).
            return std::ops::ControlFlow::Break(());
        }

        if self.cfg.trace_encl {
            let hitpass = u32::MAX - *pass;
            let cohorts = self.doc.store.single_windows.get(current.0).cohorts.clone();
            for c in cohorts {
                let rs = self.doc.store.cohorts.get(c.0).readings.clone();
                for rit in rs {
                    self.doc.store.readings.get_mut(rit.0).hit_by.push(hitpass);
                }
            }
        }

        let rv = self.run_grammar_on_single_window(current);
        if rv & RV_DELIMITED != 0 {
            return std::ops::ControlFlow::Continue(());
        }

        // Unpack enclosures.
        // Unpack enclosures (C++ label_unpackEnclosures).
        if self.rr_unpack_enclosures(current, rv) {
            return std::ops::ControlFlow::Continue(());
        }

        // Restore CT_IGNORED cohorts.
        let mut should_reflow = false;
        let mut i = self
            .doc
            .store
            .single_windows
            .get(current.0)
            .all_cohorts
            .len();
        while i > 0 {
            let cohort = self.doc.store.single_windows.get(current.0).all_cohorts[i - 1];
            if self
                .doc
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_IGNORED)
            {
                let mut ins = i;
                while ins > 0 {
                    let prev = self.doc.store.single_windows.get(current.0).all_cohorts[ins - 1];
                    if !self
                        .doc
                        .store
                        .cohorts
                        .get(prev.0)
                        .r#type
                        .intersects(CT_REMOVED | CT_ENCLOSED | CT_IGNORED)
                    {
                        let pos = self.doc.store.cohorts.get(prev.0).local_number as usize + 1;
                        self.doc
                            .store
                            .single_windows
                            .get_mut(current.0)
                            .cohorts
                            .insert(pos, cohort);
                        self.doc.store.cohorts.get_mut(cohort.0).r#type &= !CT_IGNORED;
                        let gn = self.doc.store.cohorts.get(cohort.0).global_number;
                        self.doc.cohorts.cohort_map.insert(gn, cohort);
                        should_reflow = true;
                        break;
                    }
                    ins -= 1;
                }
            }
            i -= 1;
        }
        if should_reflow {
            let clen = self.doc.store.single_windows.get(current.0).cohorts.len();
            for k in 0..clen {
                let cid = self.doc.store.single_windows.get(current.0).cohorts[k];
                self.doc.store.cohorts.get_mut(cid.0).local_number = ui32(k);
            }
            self.engine().reflow_dependency_window(0);
        }
        std::ops::ControlFlow::Break(())
    }

    // [spec:cg3:def:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-grammar-on-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-window-fn]
    /// C++ `void runGrammarOnWindow()`. The retired-window flush prints to
    /// `*ux_stdout` in C++; the port threads the live output writer in.
    pub fn run_grammar_on_window<W: std::io::Write>(&mut self, output: &mut W) {
        self.run_grammar_on_window_with(
            &mut crate::grammar_applicator::stream_format::CgFormat,
            output,
        )
    }

    /// [`run_grammar_on_window`](Self::run_grammar_on_window) with an explicit
    /// [`StreamFormat`](crate::grammar_applicator::stream_format::StreamFormat) strategy (the C++
    /// virtual print dispatch — the retired-window flush must print in the
    /// most-derived applicator's output format).
    pub fn run_grammar_on_window_with<F, W>(&mut self, fmt: &mut F, output: &mut W)
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        W: std::io::Write,
    {
        let current = self.doc.stream.current.unwrap();
        self.scratch.did_final_enclosure = false;

        // Apply the window's variable deltas onto the global `variables` map.
        // The raw slot tables include EMPTY/DEL sentinel slots — filter them
        // (the flat containers panic on sentinel keys).
        let (vset, vrem): (Vec<(u32, u32)>, Vec<u32>) = {
            let sw = self.doc.store.single_windows.get_mut(current.0);
            (
                sw.variables_set
                    .get()
                    .iter()
                    .copied()
                    .filter(|(k, _)| *k != u32::MAX && *k != u32::MAX - 1)
                    .collect(),
                sw.variables_rem
                    .get()
                    .iter()
                    .copied()
                    .filter(|k| *k != u32::MAX && *k != u32::MAX - 1)
                    .collect(),
            )
        };
        for (k, v) in vset {
            // C++ `variables[k] = v` overwrites; flat `insert()` does not.
            *self.doc.variables.index_or_insert(k) = v;
        }
        for k in vrem {
            self.doc.variables.erase(k);
        }
        let (mk, mv) = (self.cfg.mprefix_key, self.cfg.mprefix_value);
        *self.doc.variables.index_or_insert(mk.get()) = mv.get();

        if self.doc.deps.has_dep {
            self.engine().reflow_dependency_window(0);
            if !self.doc.input_eof
                && !self.doc.stream.next.is_empty()
                && self
                    .doc
                    .store
                    .single_windows
                    .get(self.doc.stream.next.last().unwrap().0)
                    .cohorts
                    .len()
                    > 1
            {
                let nb = *self.doc.stream.next.last().unwrap();
                let cohorts = self.doc.store.single_windows.get(nb.0).cohorts.clone();
                for cohort in cohorts {
                    let gn = self.doc.store.cohorts.get(cohort.0).global_number;
                    self.doc.deps.dep_window.insert(gn, cohort);
                }
            }
        }
        if self.doc.deps.has_relations {
            self.reflow_relation_window();
        }

        // Enclosure wrapping: C++ `goto scanParentheses` — re-scan from
        // scratch after every wrap until a full pass changes nothing.
        if !self.grammar.parentheses.is_empty() {
            while self.rr_wrap_one_enclosure(current) {}
        }

        self.scratch.par_left_tag = TagHash(0);
        self.scratch.par_right_tag = TagHash(0);
        self.scratch.par_left_pos = 0;
        self.scratch.par_right_pos = 0;
        let mut pass: u32 = 0;
        // C++ `runGrammarOnWindow_begin:` — loop until a pass runs to the end.
        while self.rr_window_pass(fmt, output, &mut pass).is_continue() {}
    }
}
