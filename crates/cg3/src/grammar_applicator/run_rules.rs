//! `src/GrammarApplicator_runRules.cpp` impl of GrammarApplicator.
//!
//! LITERAL bug-for-bug port of the rule-application engine. The flagged CG-3
//! quirks are reproduced deliberately:
//!   * `run_single_rule` self-reorders `rule.tests` on a failing context test
//!     (moves the failing test to the front) — a mutation of the "const" rule
//!     via C++ `mutable`; here it writes back into the grammar arena.
//!   * `update_rule_to_cohorts` performs a live-iterator-safe insert into a
//!     `CohortSet` that is currently being iterated by an active `run_single_rule`
//!     frame (the `cohortsets`/`rocits` raw-pointer bookkeeping).
//!
//! RECONCILIATION NOTES (see crate report): this file assumes the applicator
//! grows a `store: RuntimeStore` field, that `SingleWindow::rule_to_cohorts`
//! becomes `Vec<CohortSet>` (and `nested_rule_to_cohorts` `Option<Box<CohortSet>>`),
//! that `CohortSet` sort/insert can resolve the store-aware `compare_Cohort`, and
//! calls the sibling engine methods (matchSet / runContextualTest / reflow /
//! context / core) by their C++-matching signatures — none of which exist yet.

use crate::arena::{CohortId, CtxId, ReadingId, RuleId, SetId, SwId, TagId};
use super::regexgrps_t;
use crate::cohort::{
    CT_ENCLOSED, CT_IGNORED, CT_NUM_CURRENT, CT_RELATED, CT_REMOVED, CohortSet, DEP_NO_PARENT,
};
use crate::contextual_test::{POS_NO_PASS_ORIGIN, POS_PASS_ORIGIN};
use crate::inlines::{hash_value, insert_if_exists, ui32};
use crate::interval_vector::uint32IntervalVector;
use crate::reading::{Reading, ReadingList};
use crate::rule::{
    RF_AFTER, RF_ALLOWCROSS, RF_ALLOWLOOP, RF_BEFORE, RF_DELAYED, RF_DETACH, RF_ENCL_FINAL,
    RF_ENCL_INNER, RF_ENCL_OUTER, RF_IGNORED, RF_KEEPORDER, RF_NEAREST, RF_NOITERATE, RF_NOMAPPED,
    RF_NOPARENT, RF_OUTPUT, RF_REMEMBERX, RF_REPEAT, RF_RESETX, RF_REVERSE, RF_SAFE, RF_UNMAPLAST,
    RF_UNSAFE,
};
use crate::set::{ST_CHILD_UNIFY, ST_MAPPING, ST_SET_UNIFY, ST_SPECIAL, ST_TAG_UNIFY, Set};
use crate::strings::KEYWORDS::{self, *};
use crate::tag::{T_BASEFORM, T_DEPENDENCY, T_MAPPING, T_SPECIAL, T_VARSTRING, T_WORDFORM, TagList};
use crate::types::UString;

// C++ anonymous `enum { RV_NOTHING = 1, RV_SOMETHING = 2, RV_DELIMITED = 4,
// RV_TRACERULE = 8 };` — the return-value bit flags of runRulesOnSingleWindow.
const RV_NOTHING: u32 = 1;
const RV_SOMETHING: u32 = 2;
const RV_DELIMITED: u32 = 4;
const RV_TRACERULE: u32 = 8;

/// C++ `constexpr int GSR_ANY = 32767` — the "amalgamate all sub-readings"
/// sentinel for `get_sub_reading` / `rule.sub_reading`.
const GSR_ANY: i32 = 32767;

/// Expand a `uint32IntervalVector` to the ascending list of its member values
/// (the C++ `for (auto v : iv)`). Not a manifest symbol — iteration helper.
fn iv_to_vec(iv: &uint32IntervalVector) -> Vec<u32> {
    let mut out = Vec::new();
    let mut it = iv.begin();
    let end = iv.end();
    while it != end {
        out.push(it.value());
        it.advance();
    }
    out
}

impl super::GrammarApplicator {
    // ---- small helpers (not manifest symbols) --------------------------------

    /// Resolve a tag *hash* to its `TagId` via `grammar.single_tags`
    /// (`grammar->single_tags.find(h)->second`).
    #[inline]
    fn tag_by_hash(&self, h: u32) -> TagId {
        self.grammar.single_tags.find(h).get().1
    }

    /// By-id wrapper for the sibling `generate_varstring_tag(&mut self, &Tag)`:
    /// clone the `Tag` out of the grammar arena (so the `&mut self` matcher does
    /// not alias the grammar borrow) and delegate. The C++ `generateVarstringTag`
    /// takes a `const Tag*`; the arena port takes `&Tag`, so run_rules — which
    /// threads `TagId`s — needs this shim.
    #[inline]
    fn generate_varstring_tag_id(&mut self, tag: TagId) -> TagId {
        let t = self.grammar.single_tags_list.get(tag.0).clone();
        self.generate_varstring_tag(&t)
    }

    /// C++ `TRACE` macro: push `rule->number` onto the apply-to subreading's
    /// `hit_by`, and — when the rule targets the whole reading (`sub_reading ==
    /// 32767`) — onto the apply-to reading's `hit_by` too.
    fn trace(&mut self, rule_number: u32, rule_sub_reading: i32) {
        let at = self.get_apply_to();
        if let Some(sr) = at.subreading {
            self.store.readings.get_mut(sr.0).hit_by.push(rule_number);
        }
        if rule_sub_reading == GSR_ANY {
            if let Some(r) = at.reading {
                self.store.readings.get_mut(r.0).hit_by.push(rule_number);
            }
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
        if let Some(rw) = rword {
            if Some(rw) != cword {
                // `rword` (a Tag in the grammar arena) is cloned out so the
                // `&mut self` matcher calls do not alias the grammar borrow.
                let rword_tag = self.grammar.single_tags_list.get(rw.0).clone();
                let chash = cword.map(|c| self.grammar.single_tags_list.get(c.0).hash).unwrap_or(0);
                if rword_tag.r#type & crate::tag::T_REGEXP != 0 {
                    if self.does_tag_match_regexp(chash, &rword_tag, false) == 0 {
                        return false;
                    }
                } else if rword_tag.r#type & crate::tag::T_CASE_INSENSITIVE != 0 {
                    if self.does_tag_match_icase(chash, &rword_tag, false) == 0 {
                        return false;
                    }
                } else {
                    return false;
                }
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
    pub fn update_rule_to_cohorts(&mut self, c: CohortId, rsit: u32) -> bool {
        // --rule(s) cmdline filter.
        if !self.valid_rules.empty() && !self.valid_rules.contains(rsit) {
            return false;
        }
        let current = self.store.cohorts.get(c.0).parent.unwrap();
        let r = RuleId(rsit); // grammar->rule_by_number[rsit]
        let cword = self.store.cohorts.get(c.0).wordform;
        let rword = self.grammar.rule_by_number.get(r.0).wordform;
        if !self.does_wordforms_match(cword, rword) {
            return false;
        }
        if (self.store.single_windows.get(current.0).rule_to_cohorts.len() as u32) < rsit + 1 {
            self.index_single_window(current);
        }

        // cohortset = &current->rule_to_cohorts[rsit]. We identify it by (window,
        // rule) rather than by a raw pointer to reproduce the aliasing check.
        // Scan the active-iterator stack for entries pointing at this cohortset.
        let cohortset_ptr: *mut CohortSet = {
            let sw = self.store.single_windows.get_mut(current.0);
            &mut sw.rule_to_cohorts[rsit as usize] as *mut CohortSet
        };
        let mut csi: Vec<usize> = Vec::new();
        for i in 0..self.cohortsets.len() {
            if self.cohortsets[i] != cohortset_ptr {
                continue;
            }
            csi.push(i);
        }

        if !csi.is_empty() {
            // Snapshot capacity, then split the active iterators into "parked at
            // end" and "(position, cohort-at-position)".
            let cap = unsafe { (*cohortset_ptr).capacity() };
            let mut ends: Vec<*mut usize> = Vec::new();
            let mut chs: Vec<(*mut usize, CohortId)> = Vec::new();
            for &i in &csi {
                let rocit = self.rocits[i];
                let pos = unsafe { *rocit };
                let size = unsafe { (*cohortset_ptr).size() };
                if pos >= size {
                    ends.push(rocit);
                } else {
                    let at = *unsafe { (*cohortset_ptr).at(pos) };
                    chs.push((rocit, at));
                }
            }
            self.cohortset_insert(cohortset_ptr, c);
            let new_size = unsafe { (*cohortset_ptr).size() };
            for it in ends {
                unsafe {
                    *it = new_size;
                }
            }
            if cap != unsafe { (*cohortset_ptr).capacity() } {
                for (pos_ptr, cohort) in chs {
                    let n = self.cohortset_find_n(cohortset_ptr, cohort);
                    unsafe {
                        *pos_ptr = n;
                    }
                }
            }
        } else {
            self.cohortset_insert(cohortset_ptr, c);
        }

        self.store.single_windows.get_mut(current.0).valid_rules.insert(rsit)
    }

    /// Store-aware `CohortSet::insert` — the `sorted_vector<Cohort*,
    /// compare_Cohort>` sorted insert needs the runtime store to order two
    /// `CohortId`s (via [`crate::single_window::less_cohort`]). Not a manifest
    /// symbol; the engine-side realisation of the deferred `compare_Cohort`.
    fn cohortset_insert(&mut self, cs: *mut CohortSet, c: CohortId) {
        let cs = unsafe { &mut *cs };
        // Find sorted position with the store-aware comparator.
        let slice = cs.as_slice();
        let mut lo = 0usize;
        let mut hi = slice.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if crate::single_window::less_cohort(&self.store, slice[mid], c) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // Dedup: C++ sorted_vector insert is a set-insert (no dup).
        if lo < slice.len() && slice[lo] == c {
            return;
        }
        cs.get().insert(lo, c);
    }

    /// Store-aware `CohortSet::lower_bound` — first index whose element is not
    /// `less_cohort`-less than `c` (the C++ `std::lower_bound` with
    /// `compare_Cohort`). Must be used on sets built via [`Self::cohortset_insert`]
    /// (sorted by `(local_number, window number)`, NOT raw `CohortId`).
    fn cohortset_lower_bound(&self, cs: *const CohortSet, c: CohortId) -> usize {
        let slice = unsafe { (*cs).as_slice() };
        let mut lo = 0usize;
        let mut hi = slice.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if crate::single_window::less_cohort(&self.store, slice[mid], c) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Store-aware `CohortSet::find_n` — mirrors C++ `sorted_vector::find`
    /// (front/back early-outs, `lower_bound`, comparator-equivalence check)
    /// with `compare_Cohort`; returns the index or `size()` when absent.
    fn cohortset_find_n(&self, cs: *const CohortSet, c: CohortId) -> usize {
        let slice = unsafe { (*cs).as_slice() };
        if slice.is_empty() {
            return 0;
        }
        let store = &self.store;
        let last = slice.len() - 1;
        if crate::single_window::less_cohort(store, slice[last], c) {
            return slice.len();
        }
        if crate::single_window::less_cohort(store, c, slice[0]) {
            return slice.len();
        }
        let it = self.cohortset_lower_bound(cs, c);
        if it != slice.len()
            && (crate::single_window::less_cohort(store, slice[it], c)
                || crate::single_window::less_cohort(store, c, slice[it]))
        {
            return slice.len();
        }
        it
    }

    /// Store-aware `CohortSet::erase` — mirrors C++ `sorted_vector::erase` with
    /// `compare_Cohort` (comparator-equivalence, not id-equality, exactly as the
    /// C++ does). Returns `true` iff an element was removed.
    fn cohortset_erase(&mut self, cs: *mut CohortSet, c: CohortId) -> bool {
        let n = self.cohortset_find_n(cs, c);
        let csr = unsafe { &mut *cs };
        if n != csr.size() {
            csr.erase_n(n);
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
            let c = self.store.readings.get(reading.0).parent.unwrap();
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
            let sw = self.store.single_windows.get_mut(current.0);
            sw.valid_rules.clear();
            sw.rule_to_cohorts.resize_with(nrules, CohortSet::new);
            for cs in sw.rule_to_cohorts.iter_mut() {
                cs.clear();
            }
        }

        let cohorts = self.store.single_windows.get(current.0).cohorts.clone();
        for c in cohorts {
            let psize = self.store.cohorts.get(c.0).possible_sets.len();
            for psit in 0..psize as u32 {
                if !self.store.cohorts.get(c.0).possible_sets[psit as usize] {
                    continue;
                }
                // grammar->rules_by_set.find(psit)
                let rules_of_set: Option<Vec<u32>> = self.grammar.rules_by_set.get(&psit).map(iv_to_vec);
                if let Some(rsits) = rules_of_set {
                    for rsit in rsits {
                        self.update_rule_to_cohorts(c, rsit);
                    }
                }
            }
        }
    }

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
    fn get_tag_list_of_set(&self, set: SetId, unif_mode: bool) -> TagList {
        let the_set = &self.grammar.sets_list[set.0];
        self.get_tag_list_ret(the_set, unif_mode)
    }

    /// As [`Self::get_tag_list_of_set`] but keyed by the raw set *number* (the C++
    /// `grammar->sets_list[number]` — resolved through `sets_list_order`).
    fn get_tag_list_of_set_number(&self, number: u32, unif_mode: bool) -> TagList {
        let the_set = self.grammar.set_by_number(number);
        self.get_tag_list_ret(the_set, unif_mode)
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
    /// C++ `void getTagList(const Set& theSet, TagList& theTags, bool unif_mode)
    /// const` — the by-reference overload (appends to `the_tags`).
    pub fn get_tag_list(&self, the_set: &Set, the_tags: &mut TagList, unif_mode: bool) {
        if the_set.r#type & ST_SET_UNIFY != 0 {
            // usets = (*context_stack.back().unif_sets)[theSet.number]
            let unif_sets = self.context_stack.last().unwrap().unif_sets.unwrap();
            let usets = unsafe { &(*unif_sets) }.get(&the_set.number);
            let p_set = self.grammar.set_by_number(the_set.sets[0]);
            for &iter in &p_set.sets {
                let present = usets.map(|s| s.count(iter) != 0).unwrap_or(false);
                if present {
                    self.get_tag_list(self.grammar.set_by_number(iter), the_tags, false);
                }
            }
        } else if the_set.r#type & ST_TAG_UNIFY != 0 {
            for &iter in &the_set.sets {
                self.get_tag_list(self.grammar.set_by_number(iter), the_tags, true);
            }
        } else if !the_set.sets.is_empty() {
            for &iter in &the_set.sets {
                self.get_tag_list(self.grammar.set_by_number(iter), the_tags, unif_mode);
            }
        } else if unif_mode {
            let unif_tags = self.context_stack.last().unwrap().unif_tags.unwrap();
            let val = unsafe { &(*unif_tags) }.get(&the_set.number).copied();
            if let Some(node) = val {
                crate::tag_trie::trie_get_tag_list_find(
                    &the_set.trie,
                    the_tags,
                    node as *const core::ffi::c_void,
                    &self.grammar,
                );
                crate::tag_trie::trie_get_tag_list_find(
                    &the_set.trie_special,
                    the_tags,
                    node as *const core::ffi::c_void,
                    &self.grammar,
                );
            }
        } else {
            crate::tag_trie::trie_get_tag_list_append(&the_set.trie, the_tags, &self.grammar);
            crate::tag_trie::trie_get_tag_list_append(&the_set.trie_special, the_tags, &self.grammar);
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
            if self.store.readings.get(tr.0).next.is_none() {
                return Some(tr);
            }
            // reading = fresh; *reading = *tr; reading->next = nullptr.
            let amalgam = self.clone_reading_value(tr);
            let rid = ReadingId(self.store.readings.alloc(amalgam));
            self.store.readings.get_mut(rid.0).next = None;
            self.subs_any_push(rid);

            let mut cur = tr;
            while let Some(next) = self.store.readings.get(cur.0).next {
                cur = next;
                // tags_list: push 0 then extend with cur.tags_list
                let cur_tags_list = self.store.readings.get(cur.0).tags_list.clone();
                {
                    let r = self.store.readings.get_mut(rid.0);
                    r.tags_list.push(0);
                    r.tags_list.extend(cur_tags_list.iter().copied());
                }
                let (tags, tags_plain, tags_textual) = {
                    let cr = self.store.readings.get(cur.0);
                    (
                        cr.tags.as_slice().to_vec(),
                        cr.tags_plain.as_slice().to_vec(),
                        cr.tags_textual.as_slice().to_vec(),
                    )
                };
                {
                    let r = self.store.readings.get_mut(rid.0);
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
                let cur_num = self.store.readings.get(cur.0).tags_numerical.clone();
                let (mapped, mapping, mt, mtst) = {
                    let cr = self.store.readings.get(cur.0);
                    (cr.mapped, cr.mapping, cr.matched_target, cr.matched_tests)
                };
                let r = self.store.readings.get_mut(rid.0);
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
            crate::reading::reading_rehash(&mut self.store, &self.grammar, rid);
            return Some(rid);
        }

        if sub_reading > 0 {
            let mut cur = Some(tr);
            let mut i = 0;
            while i < sub_reading && cur.is_some() {
                cur = self.store.readings.get(cur.unwrap().0).next;
                i += 1;
            }
            return cur;
        }

        // sub_reading < 0
        let mut ntr = 0i32;
        let mut ttr = Some(tr);
        while let Some(t) = ttr {
            ttr = self.store.readings.get(t.0).next;
            ntr -= 1;
        }
        let mut cur = Some(tr);
        if self.store.readings.get(tr.0).next.is_none() {
            cur = None;
        }
        let mut i = ntr;
        while i < sub_reading && cur.is_some() {
            cur = self.store.readings.get(cur.unwrap().0).next;
            i += 1;
        }
        cur
    }

    /// Verbatim field copy of a stored `Reading` (the C++ `*reading = *tr`).
    /// `Reading` derives only `Default`, so the fields are copied explicitly.
    fn clone_reading_value(&self, id: ReadingId) -> Reading {
        let r = self.store.readings.get(id.0);
        Reading {
            mapped: r.mapped,
            deleted: r.deleted,
            noprint: r.noprint,
            matched_target: r.matched_target,
            matched_tests: r.matched_tests,
            immutable: r.immutable,
            active: r.active,
            baseform: r.baseform,
            hash: r.hash,
            hash_plain: r.hash_plain,
            number: r.number,
            tags_bloom: r.tags_bloom,
            tags_plain_bloom: r.tags_plain_bloom,
            tags_textual_bloom: r.tags_textual_bloom,
            mapping: r.mapping,
            parent: r.parent,
            next: r.next,
            hit_by: r.hit_by.clone(),
            tags_list: r.tags_list.clone(),
            tags: r.tags.clone(),
            tags_plain: r.tags_plain.clone(),
            tags_textual: r.tags_textual.clone(),
            tags_numerical: r.tags_numerical.clone(),
            tags_string: r.tags_string.clone(),
            tags_string_hash: r.tags_string_hash,
        }
    }

    // [spec:cg3:def:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-single-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-grammar-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-single-window-fn]
    /// C++ `uint32_t runGrammarOnSingleWindow(SingleWindow& current)`.
    pub fn run_grammar_on_single_window(&mut self, current: SwId) -> u32 {
        if !self.grammar.before_sections.is_empty() && !self.no_before_sections {
            let rules = self.runsections.get(&-1).cloned().unwrap_or_default();
            let rv = self.run_rules_on_single_window(current, &rules);
            if rv & (RV_DELIMITED | RV_TRACERULE) != 0 {
                return rv;
            }
        }

        if !self.grammar.rules.is_empty() && !self.no_sections {
            let mut counter: std::collections::BTreeMap<i32, u32> = std::collections::BTreeMap::new();
            // Iterate runsections (ordered by section key). Callbacks can change
            // window state but not the runsections map; a plain key cursor mirrors
            // the C++ `iter`/`++iter`.
            let keys: Vec<i32> = self.runsections.keys().copied().collect();
            let mut idx = 0usize;
            let mut pass = 0usize;
            while idx < keys.len() {
                let key = keys[idx];
                if key < 0 || (self.section_max_count != 0 && *counter.get(&key).unwrap_or(&0) >= self.section_max_count) {
                    idx += 1;
                    pass = 0;
                    continue;
                }
                let rules = self.runsections.get(&key).cloned().unwrap();
                let rv = self.run_rules_on_single_window(current, &rules);
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

        if !self.grammar.after_sections.is_empty() && !self.no_after_sections {
            let rules = self.runsections.get(&-2).cloned().unwrap_or_default();
            let rv = self.run_rules_on_single_window(current, &rules);
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
        let cohorts = self.store.single_windows.get(current.0).cohorts.clone();
        for ci in (0..cohorts.len()).rev() {
            let c = cohorts[ci];
            let is_pleft = self.store.cohorts.get(c.0).is_pleft;
            if is_pleft == 0 {
                continue;
            }
            let pright = self.grammar.parentheses.get(&is_pleft).copied();
            if let Some(pright) = pright {
                let mut found = false;
                let mut encs: Vec<CohortId> = Vec::new();
                let cur_cohorts = self.store.single_windows.get(current.0).cohorts.clone();
                let mut right = ci;
                while right < cur_cohorts.len() {
                    let s = cur_cohorts[right];
                    encs.push(s);
                    if self.store.cohorts.get(s.0).is_pright == pright {
                        found = true;
                        break;
                    }
                    right += 1;
                }
                if found {
                    // Remove enclosed span from `cohorts`, shifting left.
                    let left = ci;
                    let lc = self.store.cohorts.get(cur_cohorts[left].0).local_number;
                    let mut writ = left;
                    let mut lc = lc;
                    let mut rd = right + 1;
                    {
                        let sw = self.store.single_windows.get_mut(current.0);
                        while rd < sw.cohorts.len() {
                            sw.cohorts[writ] = sw.cohorts[rd];
                            writ += 1;
                            rd += 1;
                        }
                    }
                    // Renumber the moved cohorts.
                    let moved: Vec<CohortId> =
                        self.store.single_windows.get(current.0).cohorts[left..writ].to_vec();
                    for cid in moved {
                        self.store.cohorts.get_mut(cid.0).local_number = lc;
                        lc += 1;
                    }
                    let new_len = self.store.single_windows.get(current.0).cohorts.len() - encs.len();
                    self.store.single_windows.get_mut(current.0).cohorts.truncate(new_len);
                    // C++ walks the CONTIGUOUS all_cohorts range from
                    // encs.front() to encs.back() inclusive — also
                    // bumping `enclosed` on previously-wrapped cohorts
                    // sandwiched in the span; that encodes nesting depth.
                    {
                        let front = encs[0];
                        let back = *encs.last().unwrap();
                        let start_ln =
                            self.store.cohorts.get(front.0).local_number as usize;
                        let all = self.store.single_windows.get(current.0).all_cohorts.clone();
                        let mut ec = all[start_ln..]
                            .iter()
                            .position(|&x| x == front)
                            .map(|p| p + start_ln)
                            .expect("enclosure front in all_cohorts");
                        loop {
                            let c = self.store.cohorts.get_mut(all[ec].0);
                            c.r#type |= CT_ENCLOSED;
                            c.enclosed += 1;
                            if all[ec] == back {
                                break;
                            }
                            ec += 1;
                        }
                    }
                    self.store.single_windows.get_mut(current.0).has_enclosures = true;
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
            if self.store.single_windows.get(current.0).has_enclosures {
                let nc = self.store.single_windows.get(current.0).all_cohorts.len();
                let mut handled = false;
                let mut i = 0usize;
                while i < nc {
                    let c = self.store.single_windows.get(current.0).all_cohorts[i];
                    if self.store.cohorts.get(c.0).enclosed == 1 {
                        let mut la = i;
                        while la > 0 {
                            let prev = self.store.single_windows.get(current.0).all_cohorts[la - 1];
                            if self.store.cohorts.get(prev.0).r#type & (CT_ENCLOSED | CT_REMOVED | CT_IGNORED) == 0 {
                                la -= 1;
                                break;
                            }
                            la -= 1;
                        }
                        let ni = {
                            let lac = self.store.single_windows.get(current.0).all_cohorts[la];
                            self.store.cohorts.get(lac.0).local_number as usize
                        };

                        let mut ra = i;
                        let mut ne = 0usize;
                        while ra < nc {
                            let rac = self.store.single_windows.get(current.0).all_cohorts[ra];
                            if self.store.cohorts.get(rac.0).r#type & (CT_ENCLOSED | CT_REMOVED | CT_IGNORED) == 0 {
                                break;
                            }
                            {
                                let c = self.store.cohorts.get_mut(rac.0);
                                c.enclosed -= 1;
                                if c.enclosed == 0 {
                                    c.r#type &= !CT_ENCLOSED;
                                    ne += 1;
                                }
                            }
                            ra += 1;
                        }

                        {
                            let clen = self.store.single_windows.get(current.0).cohorts.len();
                            let sw = self.store.single_windows.get_mut(current.0);
                            sw.cohorts.resize(clen + ne, CohortId(u32::MAX));
                        }
                        {
                            let clen = self.store.single_windows.get(current.0).cohorts.len();
                            let mut j = clen - 1;
                            while j > ni + ne {
                                let moved = self.store.single_windows.get(current.0).cohorts[j - ne];
                                self.store.single_windows.get_mut(current.0).cohorts[j] = moved;
                                self.store.cohorts.get_mut(moved.0).local_number = ui32(j);
                                self.store.single_windows.get_mut(current.0).cohorts[j - ne] = CohortId(u32::MAX);
                                j -= 1;
                            }
                        }
                        {
                            let mut j = 0usize;
                            while i < ra {
                                let ac = self.store.single_windows.get(current.0).all_cohorts[i];
                                if self.store.cohorts.get(ac.0).enclosed == 0 {
                                    self.store.single_windows.get_mut(current.0).cohorts[ni + j + 1] = ac;
                                    self.store.cohorts.get_mut(ac.0).local_number = ui32(ni + j + 1);
                                    self.store.cohorts.get_mut(ac.0).parent = Some(current);
                                    j += 1;
                                }
                                i += 1;
                            }
                        }
                        self.par_left_tag = {
                            let ac = self.store.single_windows.get(current.0).all_cohorts[la + 1];
                            self.store.cohorts.get(ac.0).is_pleft
                        };
                        self.par_right_tag = {
                            let ac = self.store.single_windows.get(current.0).all_cohorts[ra - 1];
                            self.store.cohorts.get(ac.0).is_pright
                        };
                        self.par_left_pos = ui32(ni + 1);
                        self.par_right_pos = ui32(ni + ne);
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
                if !self.did_final_enclosure {
                    self.par_left_tag = 0;
                    self.par_right_tag = 0;
                    self.par_left_pos = 0;
                    self.par_right_pos = 0;
                    self.did_final_enclosure = true;
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
        F: super::stream_format::StreamFormat,
        W: std::io::Write,
    {
        while !self.gWindow.previous.is_empty() && self.gWindow.previous.len() as u32 > self.num_windows {
            let tmp = self.gWindow.previous[0];
            // C++ `printSingleWindow(tmp, *ux_stdout)` — print to the live
            // output writer threaded in by the driver, in the most-derived
            // applicator's format.
            fmt.print_single_window(self, tmp, output, false);
            let opt = Some(tmp);
            crate::single_window::free_swindow(&mut self.gWindow, &mut self.store, opt);
            self.gWindow.previous.remove(0);
        }

        self.rule_hits.clear();
        self.index_ruleCohort_no.clear(0);
        let current = self.gWindow.current.unwrap();
        self.index_single_window(current);
        self.store.single_windows.get_mut(current.0).hit_external.clear();
        let gw = &mut self.gWindow;
        gw.rebuild_cohort_links(&mut self.store);

        *pass += 1;
        if *pass > 1000 {
            // Endless-loop warning (I/O pass omitted).
            return std::ops::ControlFlow::Break(());
        }

        if self.trace_encl {
            let hitpass = u32::MAX - *pass;
            let cohorts = self.store.single_windows.get(current.0).cohorts.clone();
            for c in cohorts {
                let rs = self.store.cohorts.get(c.0).readings.clone();
                for rit in rs {
                    self.store.readings.get_mut(rit.0).hit_by.push(hitpass);
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
        let mut i = self.store.single_windows.get(current.0).all_cohorts.len();
        while i > 0 {
            let cohort = self.store.single_windows.get(current.0).all_cohorts[i - 1];
            if self.store.cohorts.get(cohort.0).r#type & CT_IGNORED != 0 {
                let mut ins = i;
                while ins > 0 {
                    let prev = self.store.single_windows.get(current.0).all_cohorts[ins - 1];
                    if self.store.cohorts.get(prev.0).r#type & (CT_REMOVED | CT_ENCLOSED | CT_IGNORED) == 0 {
                        let pos = self.store.cohorts.get(prev.0).local_number as usize + 1;
                        self.store.single_windows.get_mut(current.0).cohorts.insert(pos, cohort);
                        self.store.cohorts.get_mut(cohort.0).r#type &= !CT_IGNORED;
                        let gn = self.store.cohorts.get(cohort.0).global_number;
                        self.gWindow.cohort_map.insert(gn, cohort);
                        should_reflow = true;
                        break;
                    }
                    ins -= 1;
                }
            }
            i -= 1;
        }
        if should_reflow {
            let clen = self.store.single_windows.get(current.0).cohorts.len();
            for k in 0..clen {
                let cid = self.store.single_windows.get(current.0).cohorts[k];
                self.store.cohorts.get_mut(cid.0).local_number = ui32(k);
            }
            self.reflow_dependency_window(0);
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
        self.run_grammar_on_window_with(&mut super::stream_format::CgFormat, output)
    }

    /// [`run_grammar_on_window`](Self::run_grammar_on_window) with an explicit
    /// [`StreamFormat`](super::stream_format::StreamFormat) strategy (the C++
    /// virtual print dispatch — the retired-window flush must print in the
    /// most-derived applicator's output format).
    pub fn run_grammar_on_window_with<F, W>(&mut self, fmt: &mut F, output: &mut W)
    where
        F: super::stream_format::StreamFormat,
        W: std::io::Write,
    {
        let current = self.gWindow.current.unwrap();
        self.did_final_enclosure = false;

        // Apply the window's variable deltas onto the global `variables` map.
        // The raw slot tables include EMPTY/DEL sentinel slots — filter them
        // (the flat containers panic on sentinel keys).
        let (vset, vrem): (Vec<(u32, u32)>, Vec<u32>) = {
            let sw = self.store.single_windows.get_mut(current.0);
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
            *self.variables.index_or_insert(k) = v;
        }
        for k in vrem {
            self.variables.erase(k);
        }
        let (mk, mv) = (self.mprefix_key, self.mprefix_value);
        *self.variables.index_or_insert(mk) = mv;

        if self.has_dep {
            self.reflow_dependency_window(0);
            if !self.input_eof
                && !self.gWindow.next.is_empty()
                && self.store.single_windows.get(self.gWindow.next.last().unwrap().0).cohorts.len() > 1
            {
                let nb = *self.gWindow.next.last().unwrap();
                let cohorts = self.store.single_windows.get(nb.0).cohorts.clone();
                for cohort in cohorts {
                    let gn = self.store.cohorts.get(cohort.0).global_number;
                    self.gWindow.dep_window.insert(gn, cohort);
                }
            }
        }
        if self.has_relations {
            self.reflow_relation_window();
        }

        // Enclosure wrapping: C++ `goto scanParentheses` — re-scan from
        // scratch after every wrap until a full pass changes nothing.
        if !self.grammar.parentheses.is_empty() {
            while self.rr_wrap_one_enclosure(current) {}
        }

        self.par_left_tag = 0;
        self.par_right_tag = 0;
        self.par_left_pos = 0;
        self.par_right_pos = 0;
        let mut pass: u32 = 0;
        // C++ `runGrammarOnWindow_begin:` — loop until a pass runs to the end.
        while self.rr_window_pass(fmt, output, &mut pass).is_continue() {}
    }
}

/// The shared, mutable per-`runRulesOnSingleWindow` state that C++ captures by
/// reference into the `reading_cb`/`cohort_cb` closures and the helper lambdas.
/// The C++ closures alias `this` and these locals through the stack; the port
/// threads this struct explicitly (`&mut RRState`) into the dispatch/helper
/// methods, while the two `RuleCallback` trampolines carry raw `*mut Self` +
/// `*mut RRState` (reproducing the C++ aliasing, matching the raw-pointer design
/// the applicator struct already uses for `cohortsets`/`rocits`). Not a manifest
/// symbol.
struct RRState {
    current: SwId,
    /// The `rules` parameter (read-only working set).
    rules: uint32IntervalVector,
    /// `current.valid_rules.intersect(rules)` — grows as tags are added.
    intersects: uint32IntervalVector,
    /// The current rule (`rule`); WITH temporarily reassigns it.
    rule: RuleId,
    /// The re-seatable outer cursor value (`*iter_rules`).
    iter_val: u32,
    removed: ReadingList,
    selected: ReadingList,
    readings_changed: bool,
    should_repeat: bool,
    should_bail: bool,
    delimited: bool,
    /// `Sorter::do_sort` — re-sort every rule_to_cohorts when the rule finishes.
    do_sort: bool,
}

/// First member value of an interval set, or `None` when empty.
fn iv_first(iv: &uint32IntervalVector) -> Option<u32> {
    if iv.empty() { None } else { Some(iv.front()) }
}

/// First member value strictly greater than `v` (the C++ `++iter_rules`).
fn iv_next_after(iv: &uint32IntervalVector, v: u32) -> Option<u32> {
    let lb = iv.lower_bound(v.wrapping_add(1));
    if lb == iv.end() { None } else { Some(lb.value()) }
}

impl super::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn]
    /// C++ `uint32_t runRulesOnSingleWindow(SingleWindow& current, const
    /// uint32IntervalVector& rules)`.
    pub fn run_rules_on_single_window(&mut self, current: SwId, rules: &uint32IntervalVector) -> u32 {
        let mut retval = RV_NOTHING;
        let mut section_did_something = false;

        let intersects = self.store.single_windows.get(current.0).valid_rules.intersect(rules);
        let mut st = Box::new(RRState {
            current,
            rules: rules.clone(),
            intersects,
            rule: RuleId(0),
            iter_val: 0,
            removed: ReadingList::new(),
            selected: ReadingList::new(),
            readings_changed: false,
            should_repeat: false,
            should_bail: false,
            delimited: false,
            do_sort: false,
        });

        // current.parent->cohort_map[0] = current.cohorts.front()
        let front = self.store.single_windows.get(current.0).cohorts[0];
        self.gWindow.cohort_map.insert(0, front);

        let mut cursor = iv_first(&st.intersects);
        'outer: while let Some(start) = cursor {
            st.do_sort = false;
            let mut cur = start;
            let mut brk_outer = false;

            'repeat: loop {
                let mut rule_did_something = false;
                let j = cur;
                st.iter_val = j;

                if !self.valid_rules.empty() && !self.valid_rules.contains(j) {
                    break 'repeat;
                }
                self.current_rule = Some(RuleId(j));
                st.rule = RuleId(j);
                let (rtype, rflags, has_enclosures) = {
                    let r = self.grammar.rule_by_number.get(j);
                    (
                        r.r#type,
                        r.flags,
                        self.store.single_windows.get(current.0).has_enclosures,
                    )
                };
                if rtype == K_IGNORE {
                    break 'repeat;
                }
                if !self.apply_mappings && (rtype == K_MAP || rtype == K_ADD || rtype == K_REPLACE) {
                    break 'repeat;
                }
                if !self.apply_corrections && (rtype == K_SUBSTITUTE || rtype == K_APPEND) {
                    break 'repeat;
                }
                if has_enclosures {
                    if (rflags & RF_ENCL_FINAL != 0) && !self.did_final_enclosure {
                        break 'repeat;
                    }
                    if self.did_final_enclosure && (rflags & RF_ENCL_FINAL == 0) {
                        break 'repeat;
                    }
                }

                st.readings_changed = false;
                st.should_repeat = false;
                st.should_bail = false;
                st.removed.clear();
                st.selected.clear();

                // Build the two callback trampolines aliasing self + st.
                let this_ptr = self as *mut Self;
                let st_ptr: *mut RRState = &mut *st;
                let reading_cb: super::RuleCallback =
                    Box::new(move || unsafe { (*this_ptr).reading_cb_dispatch(&mut *st_ptr) });
                let cohort_cb: super::RuleCallback =
                    Box::new(move || unsafe { (*this_ptr).cohort_cb_dispatch(&mut *st_ptr) });

                let rv = self.run_single_rule(current, RuleId(j), reading_cb, cohort_cb);
                if rv || st.readings_changed {
                    if !((rflags & RF_NOITERATE != 0) && self.section_max_count != 1) {
                        section_did_something = true;
                    }
                    rule_did_something = true;
                }

                if st.should_bail {
                    // bailout: rule_hits[rule->number] = 0; index_ruleCohort_no.clear();
                    self.rule_hits.insert(j, 0);
                    self.index_ruleCohort_no.clear(0);
                    if retval & RV_TRACERULE != 0 {
                        brk_outer = true;
                    }
                    break 'repeat;
                }
                if st.should_repeat {
                    cur = st.iter_val; // JUMP re-seated iter_rules
                    continue 'repeat;
                }

                if rule_did_something {
                    st.iter_val = j; // iter_rules = intersects.find(rule->number)
                    let line = self.grammar.rule_by_number.get(j).line;
                    if self.trace_rules.contains(line) {
                        retval |= RV_TRACERULE;
                    }
                }
                if st.delimited {
                    brk_outer = true;
                    break 'repeat;
                }
                if rule_did_something && (rflags & RF_REPEAT != 0) {
                    self.index_ruleCohort_no.clear(0);
                    cur = j;
                    continue 'repeat;
                }
                if retval & RV_TRACERULE != 0 {
                    brk_outer = true;
                }
                break 'repeat;
            }

            // Sorter dtor.
            if st.do_sort {
                self.rr_sort_all_cohortsets(current);
            }
            if brk_outer {
                break 'outer;
            }
            cursor = iv_next_after(&st.intersects, st.iter_val);
        }

        if section_did_something {
            retval |= RV_SOMETHING;
        }
        if st.delimited {
            retval |= RV_DELIMITED;
        }
        retval
    }

    /// Sorter dtor body: re-sort every rule_to_cohorts CohortSet with the
    /// store-aware `compare_Cohort`.
    fn rr_sort_all_cohortsets(&mut self, current: SwId) {
        let n = self.store.single_windows.get(current.0).rule_to_cohorts.len();
        for i in 0..n {
            // Extract, sort with the store-aware comparator, put back.
            let mut v: Vec<CohortId> =
                self.store.single_windows.get(current.0).rule_to_cohorts[i].as_slice().to_vec();
            v.sort_by(|&a, &b| {
                if crate::single_window::less_cohort(&self.store, a, b) {
                    std::cmp::Ordering::Less
                } else if crate::single_window::less_cohort(&self.store, b, a) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            self.store.single_windows.get_mut(current.0).rule_to_cohorts[i].assign_sorted(&v);
        }
    }

    // ---- runRulesOnSingleWindow helper lambdas (as methods over RRState) ------

    /// `reindex(which)`: renumber a window's cohorts to their index, then
    /// `gWindow->rebuildCohortLinks()`.
    fn rr_reindex(&mut self, which: SwId) {
        let n = self.store.single_windows.get(which.0).cohorts.len();
        for i in 0..n {
            let cid = self.store.single_windows.get(which.0).cohorts[i];
            self.store.cohorts.get_mut(cid.0).local_number = ui32(i);
        }
        let gw = &self.gWindow;
        gw.rebuild_cohort_links(&mut self.store);
    }

    /// `collect_subtree(cs, head, cset)`.
    fn rr_collect_subtree(&mut self, current: SwId, cs: &mut CohortSet, head: CohortId, cset: u32) {
        if cset != 0 {
            let head_gn = self.store.cohorts.get(head.0).global_number;
            let cohorts = self.store.single_windows.get(current.0).cohorts.clone();
            for iter in &cohorts {
                let (gn, dp) = {
                    let c = self.store.cohorts.get(iter.0);
                    (c.global_number, c.dep_parent)
                };
                if gn == head_gn {
                    self.cohortset_insert(cs as *mut CohortSet, *iter);
                } else if dp == head_gn && self.does_set_match_cohort_normal(*iter, cset, None) {
                    self.cohortset_insert(cs as *mut CohortSet, *iter);
                }
            }
            let mut more: CohortSet = CohortSet::new();
            let cs_snapshot: Vec<CohortId> = cs.as_slice().to_vec();
            for iter in &cohorts {
                for &cht in &cs_snapshot {
                    if self.store.cohorts.get(cht.0).global_number == head_gn {
                        continue;
                    }
                    if self.is_child_of(*iter, cht) {
                        self.cohortset_insert(&mut more as *mut CohortSet, *iter);
                    }
                }
            }
            for m in more.as_slice().to_vec() {
                self.cohortset_insert(cs as *mut CohortSet, m);
            }
        } else {
            self.cohortset_insert(cs as *mut CohortSet, head);
        }
    }

    /// `make_relation_rtag(tag, id)`: intern the textual `R:<tag>:<id>` tag.
    fn rr_make_relation_rtag(&mut self, tag: TagId, id: u32) -> TagId {
        let base = self.grammar.single_tags_list.get(tag.0).tag.clone();
        let tmp: UString = format!("R:{}:{}", base, id);
        // C++ `addTag(tmp)` is the `addTag(const UChar*)` convenience overload →
        // `addTag(str, 0)`.
        self.add_tag(&tmp, 0)
    }

    /// `add_relation_rtag(cohort, tag, id)`.
    fn rr_add_relation_rtag(&mut self, cohort: CohortId, tag: TagId, id: u32) {
        let nt = self.rr_make_relation_rtag(tag, id);
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            self.add_tag_to_reading(r, nt);
        }
    }

    /// `set_relation_rtag(cohort, tag, id)`: erase existing `R:<tag>:*` tags then
    /// add the new one.
    fn rr_set_relation_rtag(&mut self, cohort: CohortId, tag: TagId, id: u32) {
        let nt = self.rr_make_relation_rtag(tag, id);
        let base = self.grammar.single_tags_list.get(tag.0).tag.clone();
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            let list = self.store.readings.get(r.0).tags_list.clone();
            let mut new_list: Vec<u32> = Vec::with_capacity(list.len());
            for h in list {
                let utag = self.grammar.single_tags_list.get(self.tag_by_hash(h).0).tag.clone();
                let matches = utag.starts_with("R:")
                    && utag.len() > 2 + base.len()
                    && utag.as_bytes().get(2 + base.len()) == Some(&b':')
                    && utag[2..2 + base.len()] == base;
                if matches {
                    let rr = self.store.readings.get_mut(r.0);
                    rr.tags.erase(h);
                    rr.tags_textual.erase(h);
                    rr.tags_numerical.remove(&h);
                    rr.tags_plain.erase(h);
                } else {
                    new_list.push(h);
                }
            }
            self.store.readings.get_mut(r.0).tags_list = new_list;
            self.add_tag_to_reading(r, nt);
        }
    }

    /// `rem_relation_rtag(cohort, tag, id)`.
    fn rr_rem_relation_rtag(&mut self, cohort: CohortId, tag: TagId, id: u32) {
        let nt = self.rr_make_relation_rtag(tag, id);
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            self.del_tag_from_reading(r, nt);
        }
    }

    /// `insert_taglist_to_reading(iter, taglist, reading, mappings)` — insert
    /// varstring-expanded tags at position `at` (stop at `*`), routing mapping
    /// tags to `mappings`, calling updateValidRules, then reflowReading. Returns
    /// the resulting insertion index.
    fn rr_insert_taglist_to_reading(
        &mut self,
        st: &mut RRState,
        mut at: usize,
        taglist: &TagList,
        reading: ReadingId,
        mappings: &mut TagList,
    ) {
        let mapping_prefix = self.grammar.mapping_prefix;
        for &tag0 in taglist {
            let mut tag = tag0;
            if self.grammar.single_tags_list.get(tag.0).r#type & T_VARSTRING != 0 {
                tag = self.generate_varstring_tag_id(tag);
            }
            let (thash, ttype, first_char) = {
                let t = self.grammar.single_tags_list.get(tag.0);
                (t.hash, t.r#type, t.tag.chars().next())
            };
            if thash == self.grammar.tag_any {
                break;
            }
            if ttype & T_MAPPING != 0 || first_char == Some(mapping_prefix) {
                mappings.push(tag);
            } else {
                self.store.readings.get_mut(reading.0).tags_list.insert(at, thash);
                at += 1;
            }
            let rule = st.rule.0;
            if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, thash, reading) {
                st.iter_val = self.grammar.rule_by_number.get(rule).number;
            }
        }
        self.reflow_reading(reading);
    }

    /// C++ `subs_any.emplace_back(...)` bookkeeping helper.
    ///
    /// RECONCILIATION (see `get_sub_reading` doc + report): the C++ `subs_any` is
    /// a `std::deque<Reading>` of amalgamated sub-readings; in the arena model the
    /// amalgam is allocated in `self.store.readings` and only its `ReadingId` is
    /// tracked here, so `subs_any` must become `Vec<ReadingId>` (a NOTED mod.rs
    /// field change — currently `VecDeque<Reading>`). `clear(subs_any)` at each
    /// cohort frees these ids back to the readings arena.
    fn subs_any_push(&mut self, rid: ReadingId) {
        self.subs_any.push(rid);
    }

    /// `clear(subs_any)` — free every amalgamated sub-reading back to the readings
    /// arena and empty the tracking vector. RECONCILIATION: matches the required
    /// `Vec<ReadingId>` shape of `subs_any` (see [`Self::subs_any_push`]).
    fn subs_any_clear(&mut self) {
        let ids: Vec<ReadingId> = self.subs_any.iter().copied().collect();
        for rid in ids {
            let opt = Some(rid);
            crate::reading::free_reading(&mut self.store, opt);
        }
        self.subs_any.clear();
    }

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
    pub fn run_single_rule(
        &mut self,
        current: SwId,
        rule: RuleId,
        mut reading_cb: super::RuleCallback,
        mut cohort_cb: super::RuleCallback,
    ) -> bool {
        self.finish_cohort_loop = true;
        let rnumber = self.grammar.rule_by_number.get(rule.0).number;

        // cohortset = &current.rule_to_cohorts[rule.number]; override_cohortset()
        // may re-seat it to the nested set (in_nested). `nested` records which one.
        let nested = self.rr_override_cohortset(current, rnumber);
        let cohortset_ptr: *mut CohortSet = self.rr_cohortset_ptr(current, rnumber, nested);
        self.cohortsets.push(cohortset_ptr);
        // `rocit` lives in a heap box so a stable `*mut usize` can be parked in
        // `rocits` (the C++ parks `&rocit`, a stack local). C++ pushes `nullptr`
        // then re-seats `rocits.back() = &rocit` each cohort iteration; the box's
        // address is stable so it is parked once here.
        let mut rocit_box: Box<usize> = Box::new(0);
        self.rocits.push(&mut *rocit_box as *mut usize);

        // Run the body; the scope_guard `popper` (pop cohortsets/rocits) runs on
        // EVERY exit path, so it is applied here after the body returns.
        let anything_changed = self.run_single_rule_body(
            current,
            rule,
            rnumber,
            nested,
            cohortset_ptr,
            &mut rocit_box,
            &mut reading_cb,
            &mut cohort_cb,
        );

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
            let crp = t.dep_parent; // context_ref_pos aliases dep_parent
            if t.r#type & crate::tag::T_CONTEXT != 0 && (crp as usize) <= ctx_len {
                if let Some(Some(c)) = self
                    .context_stack
                    .last()
                    .map(|f| f.context.get((crp - 1) as usize).copied().flatten())
                {
                    ctx_cohorts.push(c);
                }
            }
        }
        let apply = self.get_apply_to().cohort;
        let sw = self.store.single_windows.get_mut(current.0);
        if sw.nested_rule_to_cohorts.is_none() {
            sw.nested_rule_to_cohorts = Some(Box::new(CohortSet::new()));
        }
        let nested = sw.nested_rule_to_cohorts.as_mut().unwrap();
        nested.clear();
        if let Some(a) = apply {
            let np: *mut CohortSet = &mut **nested;
            // insert apply-to + context cohorts with the store-aware comparator.
            drop(nested);
            self.cohortset_insert(np, a);
            for c in ctx_cohorts {
                self.cohortset_insert(np, c);
            }
        }
        true
    }

    /// Resolve the active cohortset pointer for `run_single_rule`: the nested set
    /// when `nested`, else `current.rule_to_cohorts[rule_number]`.
    ///
    /// RECONCILIATION: both must be `CohortSet` (NOTED single_window.rs change).
    fn rr_cohortset_ptr(&mut self, current: SwId, rule_number: u32, nested: bool) -> *mut CohortSet {
        let sw = self.store.single_windows.get_mut(current.0);
        if nested {
            &mut **sw.nested_rule_to_cohorts.as_mut().unwrap() as *mut CohortSet
        } else {
            &mut sw.rule_to_cohorts[rule_number as usize] as *mut CohortSet
        }
    }

    /// Bridge to the sibling `print_debug_rule`, whose signature threads
    /// `store: &mut RuntimeStore` separately from `&mut self`. Swap the store out
    /// so both borrows can be satisfied, then restore it. Diagnostic-only (gated
    /// on `debug_rules`).
    fn rr_print_debug_rule(&mut self, rule: RuleId, target: bool, cntx: bool) {
        let mut store = std::mem::take(&mut self.store);
        self.print_debug_rule(&mut store, rule, target, cntx);
        self.store = store;
    }

    /// `reset_cohorts` lambda body of `runSingleRule`: re-seat the active
    /// cohortset (and the outer `rocit` cursor) after a window-restructuring
    /// action. Returns the (possibly re-seated) cohortset pointer.
    fn rr_reset_cohorts(
        &mut self,
        current: SwId,
        rule_number: u32,
        rocit: &mut usize,
    ) -> *mut CohortSet {
        let nested = self.rr_override_cohortset(current, rule_number);
        let cs = self.rr_cohortset_ptr(current, rule_number, nested);
        *self.cohortsets.last_mut().unwrap() = cs;
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
            let lb = self.cohortset_lower_bound(cs, front_at_local);
            let size = unsafe { (*cs).size() };
            if lb == size {
                *rocit = size;
            } else {
                let at = unsafe { *(*cs).at(lb) };
                *rocit = self.cohortset_find_n(cs, at);
            }
            let gac_type = self.store.cohorts.get(gac.0).r#type;
            let new_size = unsafe { (*cs).size() };
            if gac_type & (CT_REMOVED | CT_IGNORED) == 0 && *rocit < new_size {
                *rocit += 1;
            }
        }
        cs
    }

    /// The body of [`Self::run_single_rule`] (everything inside the `popper`
    /// scope guard). Split out so the guard's `cohortsets`/`rocits` pop runs on
    /// every early-return path. See [`Self::run_single_rule`] for the markers.
    #[allow(clippy::too_many_arguments)]
    fn run_single_rule_body(
        &mut self,
        current: SwId,
        rule: RuleId,
        rnumber: u32,
        nested: bool,
        mut cohortset: *mut CohortSet,
        rocit_box: &mut Box<usize>,
        reading_cb: &mut super::RuleCallback,
        cohort_cb: &mut super::RuleCallback,
    ) -> bool {
        let mut anything_changed = false;
        let (rtype0, rflags, rsub_reading, rtarget, rline) = {
            let r = self.grammar.rule_by_number.get(rule.0);
            (r.r#type, r.flags, r.sub_reading, r.target, r.line)
        };
        let set_type = self.grammar.set_by_number(rtarget).r#type;
        let _ = (nested, rline);

        let mut rocit: usize = 0;
        while rocit < unsafe { (*cohortset).size() } {
            *self.rocits.last_mut().unwrap() = &mut **rocit_box as *mut usize;
            **rocit_box = rocit;
            let cohort = unsafe { *(*cohortset).at(rocit) };
            rocit += 1;
            **rocit_box = rocit;

            self.finish_reading_loop = true;

            // Skip the initial >>> cohort.
            if self.store.cohorts.get(cohort.0).local_number == 0 {
                continue;
            }
            // Skip removed/ignored cohorts.
            if self.store.cohorts.get(cohort.0).r#type & (CT_REMOVED | CT_IGNORED) != 0 {
                continue;
            }
            let c = self.store.cohorts.get(cohort.0).local_number;
            // Skip parentheses-enclosed or foreign-parented cohorts.
            if self.store.cohorts.get(cohort.0).r#type & CT_ENCLOSED != 0
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
                if (rflags & RF_DELAYED != 0) && cc.delayed.is_empty() {
                    continue;
                } else if (rflags & RF_IGNORED != 0) && cc.ignored.is_empty() {
                    continue;
                } else if rflags & (RF_DELAYED | RF_IGNORED) == 0 && cc.deleted.is_empty() {
                    continue;
                }
            }
            // Target-set possibility pre-check.
            if rsub_reading == 0 {
                let ps = &self.store.cohorts.get(cohort.0).possible_sets;
                if rtarget as usize >= ps.len() || !ps[rtarget as usize] {
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
                    if (!self.r#unsafe || (rflags & RF_SAFE != 0)) && rflags & RF_UNSAFE == 0 {
                        continue;
                    }
                }
            } else if r#type == K_UNMAP && rflags & RF_SAFE != 0 {
                continue;
            }
            // Delimit at final cohort.
            if r#type == K_DELIMIT
                && c == (self.store.single_windows.get(current.0).cohorts.len() as u32) - 1
            {
                continue;
            }

            // Enclosure inner/outer gating.
            if rflags & RF_ENCL_INNER != 0 {
                if self.par_left_pos == 0 {
                    continue;
                }
                let ln = self.store.cohorts.get(cohort.0).local_number;
                if ln < self.par_left_pos || ln > self.par_right_pos {
                    continue;
                }
            } else if rflags & RF_ENCL_OUTER != 0 {
                let ln = self.store.cohorts.get(cohort.0).local_number;
                if self.par_left_pos != 0 && ln >= self.par_left_pos && ln <= self.par_right_pos {
                    continue;
                }
            }

            // SETPARENT SAFE / NOPARENT with existing parent.
            let dep_parent = self.store.cohorts.get(cohort.0).dep_parent;
            if r#type == K_SETPARENT && (rflags & RF_SAFE != 0) && dep_parent != DEP_NO_PARENT {
                continue;
            }
            if (rflags & RF_NOPARENT != 0) && dep_parent != DEP_NO_PARENT {
                continue;
            }
            // REMPARENT / SWITCHPARENT with no parent.
            if (r#type == K_REMPARENT || r#type == K_SWITCHPARENT) && dep_parent == DEP_NO_PARENT {
                continue;
            }

            // rule/cohort no-match cache.
            let gn = self.store.cohorts.get(cohort.0).global_number;
            let ih = hash_value(rnumber, gn);
            if self.index_ruleCohort_no.contains(ih) {
                continue;
            }
            self.index_ruleCohort_no.insert(ih);

            let mut num_active: usize = 0;
            let mut num_iff: usize = 0;
            let mut num_immutable: usize = 0;
            let mut reading_contexts: Vec<super::Rule_Context> = Vec::new();

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
                self.unif_tags_store.resize_with(nread + 1, Default::default);
            }
            if self.unif_sets_store.len() < nread + 1 {
                self.unif_sets_store.resize_with(nread + 1, Default::default);
            }

            // Push the per-cohort context frame.
            {
                let mut ctx = super::Rule_Context::default();
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
                    (r.mapped, r.noprint, r.immutable, r.hash_plain, r.hash, r.number)
                };
                if r_mapped && (rtype0 == K_MAP || rtype0 == K_ADD || rtype0 == K_REPLACE) {
                    i += 1;
                    continue;
                }
                if r_mapped && (rflags & RF_NOMAPPED != 0) {
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
                        K_PROTECT | K_ADD | K_MAP | K_REPLACE | K_SELECT | K_REMOVE | K_IFF
                            | K_SUBSTITUTE | K_UNMAP
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
                if set_type & (ST_SPECIAL | ST_MAPPING | ST_CHILD_UNIFY) == 0
                    && !self.readings_plain.is_empty()
                {
                    if let Some(&cached) = self.readings_plain.get(&r_hash_plain) {
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
                }

                // Fresh per-reading regex/unif state.
                {
                    let ur = self.used_regex;
                    let rgs: *mut regexgrps_t = &mut self.regexgrps_store[ur] as *mut regexgrps_t;
                    let uts: *mut super::unif_tags_t =
                        &mut self.unif_tags_store[used_unif] as *mut super::unif_tags_t;
                    let uss: *mut super::unif_sets_t =
                        &mut self.unif_sets_store[used_unif] as *mut super::unif_sets_t;
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
                    unsafe {
                        (*uts).clear();
                        (*uss).clear();
                    }
                }

                self.unif_last_wordform = 0;
                self.unif_last_baseform = 0;
                self.unif_last_textual = 0;
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
                let target_matches = rtarget != 0 && {
                    let bypass = set_type & (ST_CHILD_UNIFY | ST_SPECIAL) != 0;
                    self.does_set_match_reading(reading, rtarget, bypass, false)
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
                        let tests: Vec<CtxId> =
                            self.grammar.rule_by_number.get(rule.0).tests.iter().copied().collect();
                        let mut ti = 0usize;
                        while ti < tests.len() {
                            let test = tests[ti];
                            if rflags & RF_RESETX != 0 || rflags & RF_REMEMBERX == 0 {
                                self.set_mark(Some(cohort));
                            }
                            self.seen_barrier = false;
                            self.dep_deep_seen.clear();
                            for d in self.ci_depths.iter_mut() {
                                *d = 0;
                            }
                            self.tmpl_cntx = super::tmpl_context_t::default();
                            let tpos = self.grammar.contexts_arena[test.0].pos;
                            let mut result: Option<CohortId> = None;
                            let with_deep = rtype0 == K_WITH;
                            if with_deep {
                                self.merge_with = None;
                            }
                            let deep_ptr: Option<*mut Option<CohortId>> =
                                if with_deep { Some(&mut result as *mut _) } else { None };
                            let next_test = if tpos & POS_PASS_ORIGIN == 0
                                && (self.no_pass_origin || (tpos & POS_NO_PASS_ORIGIN != 0))
                            {
                                self.run_contextual_test(Some(current), c, test, deep_ptr, Some(cohort))
                            } else {
                                self.run_contextual_test(Some(current), c, test, deep_ptr, None)
                            };
                            let ctx_push = if self.merge_with.is_some() {
                                self.merge_with
                            } else {
                                result
                            };
                            self.context_stack.last_mut().unwrap().context.push(ctx_push);
                            test_good = next_test.is_some();
                            self.profile_rule_context(test_good, rule, test);
                            if !test_good {
                                good = false;
                                // Self-reorder quirk: move failing test to front.
                                if ti != 0 && rflags & RF_KEEPORDER == 0 {
                                    let r = self.grammar.rule_by_number.get_mut(rule.0);
                                    r.tests.remove(ti);
                                    r.tests.push_front(test);
                                }
                                break;
                            }
                            let (ut_empty, us_empty) = {
                                let f = self.context_stack.last().unwrap();
                                (
                                    f.unif_tags.map(|p| unsafe { (*p).is_empty() }).unwrap_or(true),
                                    f.unif_sets.map(|p| unsafe { (*p).is_empty() }).unwrap_or(true),
                                )
                            };
                            did_test = set_type & (ST_CHILD_UNIFY | ST_SPECIAL) == 0
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
                                    if let Some(sr) = self.get_sub_reading(rj, rsub_reading) {
                                        if self.store.readings.get(sr.0).immutable {
                                            let r = self.store.readings.get_mut(sr.0);
                                            r.matched_target = true;
                                            r.matched_tests = true;
                                            num_active += 1;
                                            num_iff += 1;
                                        }
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
                                let mut store = std::mem::take(&mut self.store);
                                self.add_profiling_example(&mut store, k);
                                self.store = store;
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
                (cc.readings.len(), cc.deleted.len(), cc.delayed.len(), cc.ignored.len())
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
                    rocit -= 1;
                    unsafe { (*cohortset).erase_n(rocit) };
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
                    && (!self.r#unsafe || (rflags & RF_SAFE != 0))
                    && rflags & RF_UNSAFE == 0
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
                reading_cb();
                if !self.finish_cohort_loop {
                    self.context_stack.pop();
                    return anything_changed;
                }
                if self.reset_cohorts_for_loop {
                    cohortset = self.rr_reset_cohorts(current, rnumber, &mut rocit);
                    broke = true;
                    break;
                }
                if !self.finish_reading_loop {
                    break;
                }
            }
            let _ = broke;

            self.reset_cohorts_for_loop = false;
            cohort_cb();
            if !self.finish_cohort_loop {
                self.context_stack.pop();
                return anything_changed;
            }
            if self.reset_cohorts_for_loop {
                cohortset = self.rr_reset_cohorts(current, rnumber, &mut rocit);
            }
            self.context_stack.pop();
        }
        anything_changed
    }

    /// `ignore_cohort(cohort)` lambda of `runSingleRule`: mark a cohort
    /// `CT_IGNORED`, hit_by its readings, erase it from every rule's cohortset,
    /// detach it, and remove it from the window's `cohorts` (kept in `all_cohorts`).
    fn rr_ignore_cohort(&mut self, rule_number: u32, cohort: CohortId) {
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
        self.store.single_windows.get_mut(current.0).cohorts.remove(ln);
    }

    /// Erase `cohort` from every `current.rule_to_cohorts[i]` (the C++
    /// `for (auto& cs : current.rule_to_cohorts) cs.erase(cohort);`).
    /// RECONCILIATION: `rule_to_cohorts` must be `Vec<CohortSet>` (NOTED).
    fn rr_erase_from_all_cohortsets(&mut self, current: SwId, cohort: CohortId) {
        let n = self.store.single_windows.get(current.0).rule_to_cohorts.len();
        for i in 0..n {
            let cs: *mut CohortSet =
                &mut self.store.single_windows.get_mut(current.0).rule_to_cohorts[i];
            self.cohortset_erase(cs, cohort);
        }
    }

    /// `rem_cohort(cohort)` lambda of `runSingleRule`: fully remove a cohort —
    /// hit_by + mark deleted its readings, erase it from all rule cohortsets,
    /// forward its dependency children to its parent, mark `CT_REMOVED`, detach,
    /// prune it from every `dep_children`, drop it from `cohort_map` and the
    /// window's `cohorts`, renumber, and (when that empties a non-current window)
    /// splice the window out. Finally `rebuildCohortLinks()`.
    fn rr_rem_cohort(&mut self, rule_number: u32, cohort: CohortId) {
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
            let parent_key = if dp == DEP_NO_PARENT { 0 } else { dp };
            let (pc, cc) = (
                self.gWindow.cohort_map.get(&parent_key).copied(),
                self.gWindow.cohort_map.get(&ch).copied(),
            );
            if let (Some(pc), Some(cc)) = (pc, cc) {
                self.attach_parent_child(pc, cc, true, true);
            }
            self.store.cohorts.get_mut(cohort.0).dep_children.erase(ch);
        }
        self.store.cohorts.get_mut(cohort.0).r#type |= CT_REMOVED;
        crate::cohort::detach(&mut self.store, cohort);
        let dep_self = self.store.cohorts.get(cohort.0).dep_self;
        let keys: Vec<u32> = self.gWindow.cohort_map.keys().copied().collect();
        for k in keys {
            let cid = *self.gWindow.cohort_map.get(&k).unwrap();
            self.store.cohorts.get_mut(cid.0).dep_children.erase(dep_self);
        }
        let gn = self.store.cohorts.get(cohort.0).global_number;
        self.gWindow.cohort_map.remove(&gn);
        let ln = self.store.cohorts.get(cohort.0).local_number as usize;
        self.store.single_windows.get_mut(current.0).cohorts.remove(ln);
        self.rr_renumber(current);

        // Window emptied (only >>> left) and not the active window → drop it.
        if self.store.single_windows.get(current.0).cohorts.len() == 1
            && Some(current) != self.gWindow.current
        {
            let empty_cohort = self.store.single_windows.get(current.0).cohorts[0];
            self.rr_erase_from_all_cohortsets(current, empty_cohort);
            crate::cohort::detach(&mut self.store, empty_cohort);
            let ds = self.store.cohorts.get(empty_cohort.0).dep_self;
            let keys: Vec<u32> = self.gWindow.cohort_map.keys().copied().collect();
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
            self.store.single_windows.get_mut(current.0).all_cohorts.clear();
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
    fn rr_renumber(&mut self, current: SwId) {
        let n = self.store.single_windows.get(current.0).cohorts.len();
        for k in 0..n {
            let cid = self.store.single_windows.get(current.0).cohorts[k];
            self.store.cohorts.get_mut(cid.0).local_number = ui32(k);
        }
    }

    /// Snapshot the global `variables` map's live `(key, value)` entries in slot
    /// order (the C++ `for (auto& kv : variables)` iteration). Lets the REMVARIABLE
    /// branch scan while mutating `self`.
    fn variables_entries(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::with_capacity(self.variables.size());
        let end = self.variables.end();
        let mut it = self.variables.begin();
        while it != end {
            out.push(*it.get());
            it.pre_increment();
        }
        out
    }

    /// C++ `getTagList(*set).front()`-style first-tag helper with varstring
    /// resolution — returns the first tag of a set's expanded tag list, varstring-
    /// generated. Used by JUMP / SETVARIABLE.
    fn rr_first_taglist_tag(&mut self, set: SetId) -> Option<TagId> {
        let list = self.get_tag_list_of_set(set, false);
        let first = list.first().copied()?;
        let ttype = self.grammar.single_tags_list.get(first.0).r#type;
        if ttype & T_VARSTRING != 0 {
            Some(self.generate_varstring_tag_id(first))
        } else {
            Some(first)
        }
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
    /// C++ `cohort_cb` lambda of `runRulesOnSingleWindow` — the per-cohort action
    /// invoked once after all matched readings have been through `reading_cb`.
    /// Dispatches SELECT/REMOVE finalisation, IFF, JUMP, REM/SETVARIABLE, DELIMIT,
    /// EXTERNAL, and REMCOHORT. `&mut RRState` carries the mutable rule-loop state.
    pub fn cohort_cb_dispatch(&mut self, st: &mut RRState) {
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
                for ri in 0..readings.len() {
                    let mut rd = readings[ri];
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
                        self.get_sub_reading(readings[ri], rsub_reading)
                    } else {
                        Some(rd)
                    };
                    if let Some(sr) = sr_opt {
                        self.store.readings.get_mut(sr.0).hit_by.push(rnumber);
                    }
                    if si < st.selected.len() && readings[ri] == st.selected[si] {
                        si += 1;
                    } else {
                        self.store.readings.get_mut(readings[ri].0).deleted = true;
                        drop.push(readings[ri]);
                    }
                }
                // target->readings.swap(selected)
                {
                    let sel = st.selected.clone();
                    self.store.cohorts.get_mut(target.0).readings = sel;
                }
                if rflags & RF_DELAYED != 0 {
                    self.store.cohorts.get_mut(target.0).delayed.extend(drop.iter().copied());
                } else if rflags & RF_IGNORED != 0 {
                    self.store.cohorts.get_mut(target.0).ignored.extend(drop.iter().copied());
                } else {
                    self.store.cohorts.get_mut(target.0).deleted.extend(drop.iter().copied());
                }
                st.readings_changed = true;
            }
            st.selected.clear();
        } else if rtype == K_REMOVE || rtype == K_IFF {
            let target = self.get_apply_to().cohort.unwrap();
            let treadings = self.store.cohorts.get(target.0).readings.len();
            let cond = !st.removed.is_empty()
                && (st.removed.len() < treadings
                    || (self.r#unsafe && rflags & RF_SAFE == 0)
                    || rflags & RF_UNSAFE != 0);
            if cond {
                if rflags & RF_DELAYED != 0 {
                    self.store.cohorts.get_mut(target.0).delayed.extend(st.removed.iter().copied());
                } else if rflags & RF_IGNORED != 0 {
                    self.store.cohorts.get_mut(target.0).ignored.extend(st.removed.iter().copied());
                } else {
                    self.store.cohorts.get_mut(target.0).deleted.extend(st.removed.iter().copied());
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
                let anchor = self.grammar.anchors.find(to_hash);
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
                    if ttype & T_VARSTRING != 0 {
                        tag = self.generate_varstring_tag_id(tag);
                    }
                    let (tt, th) = {
                        let t = self.grammar.single_tags_list.get(tag.0);
                        (t.r#type, t.hash)
                    };
                    let found: Option<u32> = if tt & crate::tag::T_REGEXP != 0 {
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
                    } else if tt & crate::tag::T_CASE_INSENSITIVE != 0 {
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
                    } else if self.variables.find(th) != self.variables.end() {
                        Some(th)
                    } else {
                        None
                    };
                    if let Some(key) = found {
                        if rflags & RF_OUTPUT != 0 {
                            self.store.single_windows.get_mut(current.0).variables_output.insert(key);
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
                *self.variables.index_or_insert(nh) = vh;
                if rflags & RF_OUTPUT != 0 {
                    self.store.single_windows.get_mut(st.current.0).variables_output.insert(nh);
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
                if !self.store.single_windows.get_mut(current.0).hit_external.insert(rline).1 {
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
            let mut store = std::mem::take(&mut self.store);
            self.pipe_out_single_window(&store, current, &mut es);
            self.pipe_in_single_window(&mut store, current, &mut es);
            self.store = store;
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
            st.iter_val = if lb == st.intersects.end() { st.iter_val } else { lb.value() };
            self.reset_cohorts_for_loop = true;
        } else if rtype == K_REMCOHORT {
            let apply = self.get_apply_to().cohort.unwrap();
            if rflags & RF_IGNORED != 0 {
                let childset1 = self.grammar.rule_by_number.get(rule.0).childset1;
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
                r.tags.find(self.endtag) != r.tags.end()
            };
            if has_endtag {
                let back = *self.store.single_windows.get(st.current.0).cohorts.last().unwrap();
                let rs = self.store.cohorts.get(back.0).readings.clone();
                let endtag = self.tag_by_hash(self.endtag);
                for r in rs {
                    self.add_tag_to_reading(r, endtag);
                    if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, self.endtag, r) {
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
    pub fn reading_cb_dispatch(&mut self, st: &mut RRState) {
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
            let cohort_readings =
                self.store.cohorts.get(self.get_apply_to().cohort.unwrap().0).readings.len();
            if rtype == K_REMOVE
                && (rflags & RF_UNMAPLAST != 0)
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
            self.store.readings.get_mut(self.get_apply_to().subreading.unwrap().0).immutable = true;
        } else if rtype == K_UNPROTECT {
            self.trace(rnumber, rsub_reading);
            self.store.readings.get_mut(self.get_apply_to().subreading.unwrap().0).immutable = false;
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
        rflags: u64,
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
                self.store.cohorts.get_mut(self.get_apply_to().cohort.unwrap().0).dep_parent =
                    DEP_NO_PARENT;
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
    fn rr_maplist_tags(&mut self, rule: RuleId) -> TagList {
        let maplist = self.grammar.rule_by_number.get(rule.0).maplist;
        let mut out = TagList::new();
        if let Some(ml) = maplist {
            let raw = self.get_tag_list_of_set(ml, false);
            for &t0 in &raw {
                let mut t = t0;
                while self.grammar.single_tags_list.get(t.0).r#type & T_VARSTRING != 0 {
                    t = self.generate_varstring_tag_id(t);
                }
                out.push(t);
            }
        }
        out
    }

    /// K_WITH: mark TRACE, then run each sub-rule (repeating while `RF_REPEAT`),
    /// aggregating `readings_changed`. `in_nested` is toggled around the block.
    fn rr_with(&mut self, st: &mut RRState, rule: RuleId, _rflags: u64) {
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
                // Rebuild trampolines aliasing self+st for the nested call.
                let this_ptr = self as *mut Self;
                let st_ptr: *mut RRState = st;
                let reading_cb: super::RuleCallback =
                    Box::new(move || unsafe { (*this_ptr).reading_cb_dispatch(&mut *st_ptr) });
                let cohort_cb: super::RuleCallback =
                    Box::new(move || unsafe { (*this_ptr).cohort_cb_dispatch(&mut *st_ptr) });
                let result = self.run_single_rule(current, sr, reading_cb, cohort_cb);
                any_readings_changed = any_readings_changed || result || st.readings_changed;
                if !((result || st.readings_changed) && (sr_flags & RF_REPEAT != 0)) {
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
        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1;
        let child = self.get_apply_to().cohort.unwrap();
        let current = self.store.cohorts.get(child.0).parent.unwrap();
        let child_dp = self.store.cohorts.get(child.0).dep_parent;
        let parent = *self.gWindow.cohort_map.get(&child_dp).unwrap();
        let parent_gn = self.store.cohorts.get(parent.0).global_number;
        let grandparent_number = self.store.cohorts.get(parent.0).dep_parent;
        let mut siblings: Vec<CohortId> = Vec::new();
        let cohorts = self.store.single_windows.get(current.0).cohorts.clone();
        for c in cohorts {
            if self.store.cohorts.get(c.0).dep_parent == parent_gn
                && self.does_set_match_cohort_normal(c, childset1, None)
            {
                siblings.push(c);
            }
        }
        self.store.cohorts.get_mut(child.0).dep_parent = DEP_NO_PARENT;
        self.store.cohorts.get_mut(parent.0).dep_parent = DEP_NO_PARENT;
        for &s in &siblings {
            self.store.cohorts.get_mut(s.0).dep_parent = DEP_NO_PARENT;
        }
        if let Some(&gp) = self.gWindow.cohort_map.get(&grandparent_number) {
            self.attach_parent_child(gp, child, false, false);
        }
        self.attach_parent_child(child, parent, false, false);
        for s in siblings {
            self.attach_parent_child(child, s, false, false);
        }
    }

    /// K_RESTORE: move readings back from `deleted`/`delayed`/`ignored` whose
    /// analysis matches `rule.maplist`.
    fn rr_restore(&mut self, rule: RuleId, rflags: u64, rnumber: u32, rsub_reading: i32) {
        let cohort = self.get_apply_to().cohort.unwrap();
        let maplist_num = self
            .grammar
            .rule_by_number
            .get(rule.0)
            .maplist
            .map(|s| self.grammar.sets_list[s.0].number)
            .unwrap_or(0);
        let which = if rflags & RF_DELAYED != 0 {
            0
        } else if rflags & RF_IGNORED != 0 {
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
            while self.grammar.single_tags_list.get(tter.0).r#type & T_VARSTRING != 0 {
                tter = self.generate_varstring_tag_id(tter);
            }
            let (ttype, thash, first) = {
                let t = self.grammar.single_tags_list.get(tter.0);
                (t.r#type, t.hash, t.tag.chars().next())
            };
            let mut hash = thash;
            if ttype & T_MAPPING != 0 || first == Some(mapping_prefix) {
                mappings.push(tter);
            } else {
                hash = self.add_tag_to_reading(reading, tter);
            }
            if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash, reading) {
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
                r.tags.find(thash) != r.tags.end()
            };
            if present {
                out.push(tt);
            } else if ttype & T_SPECIAL != 0 {
                let tagv = self.grammar.single_tags_list.get(tt.0).clone();
                let stag = self.does_tag_match_reading(reading, &tagv, false, true);
                if stag != 0 {
                    out.push(self.tag_by_hash(stag));
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
        rflags: u64,
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

        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1;
        let mut did_insert = false;
        if childset1 != 0 {
            let mut spot_tags = self.get_tag_list_of_set_number(childset1, false);
            self.rr_fill_tag_list(&mut spot_tags);
            // Find the spot in reading.tags_list matching all of spot_tags.
            let tags_list = self.store.readings.get(reading.0).tags_list.clone();
            let spot_hashes: Vec<u32> =
                spot_tags.iter().map(|t| self.grammar.single_tags_list.get(t.0).hash).collect();
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
                if rflags & RF_AFTER != 0 {
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
            r.tags_list.push(wf_hash);
        }
        let bform = self.store.readings.get(reading.0).baseform;
        self.store.readings.get_mut(reading.0).baseform = 0;
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
        if self.store.readings.get(reading.0).baseform == 0 {
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
            if ttype & T_BASEFORM != 0 {
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
                while self.grammar.single_tags_list.get(tter.0).r#type & T_VARSTRING != 0 {
                    tter = self.generate_varstring_tag_id(tter);
                }
                let (ttype, first) = {
                    let t = self.grammar.single_tags_list.get(tter.0);
                    (t.r#type, t.tag.chars().next())
                };
                if ttype & T_MAPPING != 0 || first == Some(mapping_prefix) {
                    mappings.push(tter);
                } else {
                    hash = self.add_tag_to_reading(creading, tter);
                }
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash, creading) {
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
        let creading = crate::cohort::allocate_append_reading_copy(&mut self.store, cohort, &src_snapshot);
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

        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1;
        let rflags = self.grammar.rule_by_number.get(rule.0).flags;
        let mut did_insert = false;
        if childset1 != 0 {
            let mut spot_tags = self.get_tag_list_of_set_number(childset1, false);
            self.rr_fill_tag_list(&mut spot_tags);
            let tags_list = self.store.readings.get(creading.0).tags_list.clone();
            let spot_hashes: Vec<u32> =
                spot_tags.iter().map(|t| self.grammar.single_tags_list.get(t.0).hash).collect();
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
            if rflags & RF_AFTER != 0 {
                let mut cnt = 0;
                while at < self.store.readings.get(creading.0).tags_list.len() && cnt != spot_tags.len() {
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
            && self.grammar.single_tags_list.get(the_tags[0].0).comparison_hash == self.grammar.tag_any;

        // FILL_TAG_LIST equivalent on the subreading.
        self.rr_fill_tag_list_of(sr, &mut the_tags);
        let the_hashes: Vec<u32> =
            the_tags.iter().map(|t| self.grammar.single_tags_list.get(t.0).hash).collect();
        let substtag = self.substtag;

        let mut tpos: usize = usize::MAX;
        let mut plain = true;
        let mut i = 0usize;
        while i < self.store.readings.get(sr.0).tags_list.len() {
            let remter = self.store.readings.get(sr.0).tags_list[i];
            if plain && !the_hashes.is_empty() && remter == the_hashes[0] {
                if self.store.readings.get(sr.0).baseform == remter {
                    self.store.readings.get_mut(sr.0).baseform = 0;
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
                    if self.store.readings.get(sr.0).baseform == tter {
                        self.store.readings.get_mut(sr.0).baseform = 0;
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
                if self.store.readings.get(sr.0).baseform == th {
                    self.store.readings.get_mut(sr.0).baseform = 0;
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
                        if self.grammar.single_tags_list.get(tag.0).r#type & T_VARSTRING != 0 {
                            tag = self.generate_varstring_tag_id(tag);
                        }
                        let (thash, ttype, first) = {
                            let t = self.grammar.single_tags_list.get(tag.0);
                            (t.hash, t.r#type, t.tag.chars().next())
                        };
                        if thash == self.grammar.tag_any {
                            break;
                        }
                        if ttype & T_MAPPING != 0 || first == Some(mapping_prefix) {
                            mappings.push(tag);
                        } else {
                            if ttype & T_WORDFORM != 0 {
                                wf = Some(tag);
                            }
                            self.store.readings.get_mut(sr.0).tags_list.insert(tpos, thash);
                            tpos += 1;
                        }
                        if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, thash, sr) {
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
            if let Some(wf) = wf {
                if Some(wf) != parent_wf {
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
                        if self.does_wordforms_match(Some(wf), rw) {
                            let cs: *mut CohortSet = &mut self
                                .store
                                .single_windows
                                .get_mut(current.0)
                                .rule_to_cohorts[rn as usize];
                            self.cohortset_insert(cs, cohort);
                            st.intersects.insert(rn);
                        } else {
                            let cs: *mut CohortSet = &mut self
                                .store
                                .single_windows
                                .get_mut(current.0)
                                .rule_to_cohorts[rn as usize];
                            self.cohortset_erase(cs, cohort);
                        }
                    }
                    let wf_hash = self.grammar.single_tags_list.get(wf.0).hash;
                    self.update_valid_rules(&st.rules.clone(), &mut st.intersects, wf_hash, sr);
                    st.iter_val = rnumber;
                }
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
                r.tags.find(thash) != r.tags.end()
            };
            if present {
                out.push(tt);
            } else if ttype & T_SPECIAL != 0 {
                let tagv = self.grammar.single_tags_list.get(tt.0).clone();
                let stag = self.does_tag_match_reading(reading, &tagv, false, true);
                if stag != 0 {
                    out.push(self.tag_by_hash(stag));
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
            let target_gn = self.store.cohorts.get(target.0).global_number;
            seen_targets.push(target_gn);
            self.dep_deep_seen.clear();
            self.tmpl_cntx = super::tmpl_context_t::default();
            {
                let f = self.context_stack.last_mut().unwrap();
                f.attach_to = super::ReadingSpec::default();
            }
            self.seen_barrier = false;
            let (tparent, tlocal) = {
                let c = self.store.cohorts.get(target.0);
                (c.parent, c.local_number)
            };
            let mut attach_out: Option<CohortId> = None;
            let res = self.run_contextual_test(
                tparent,
                tlocal,
                dep_target,
                Some(&mut attach_out as *mut _),
                None,
            );
            if res.is_some() && attach_out.is_some() {
                let mut attach = attach_out.unwrap();
                self.profile_rule_context(true, rule, dep_target);
                let break_after = self.seen_barrier || (rflags & RF_NEAREST != 0);
                if let Some(at) = self.get_attach_to().cohort {
                    attach = at;
                }
                self.context_target = Some(attach);
                let mut good = true;
                let dep_tests: Vec<CtxId> =
                    self.grammar.rule_by_number.get(rule.0).dep_tests.iter().copied().collect();
                for it in dep_tests {
                    self.set_mark_frame(attach);
                    self.dep_deep_seen.clear();
                    self.tmpl_cntx = super::tmpl_context_t::default();
                    let (aparent, alocal) = {
                        let c = self.store.cohorts.get(attach.0);
                        (c.parent, c.local_number)
                    };
                    let tg = self.run_contextual_test(aparent, alocal, it, None, None).is_some();
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
                let attach_gn = self.store.cohorts.get(attach.0).global_number;
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
    fn set_mark_frame(&mut self, cohort: CohortId) {
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
        if rflags & RF_REVERSE != 0 {
            std::mem::swap(&mut target, &mut attach);
        }

        if rtype == K_SETPARENT || rtype == K_SETCHILD {
            self.has_dep = true;
            let attached = if rtype == K_SETPARENT {
                self.attach_parent_child(
                    attach,
                    target,
                    rflags & RF_ALLOWLOOP != 0,
                    rflags & RF_ALLOWCROSS != 0,
                )
            } else {
                self.attach_parent_child(
                    target,
                    attach,
                    rflags & RF_ALLOWLOOP != 0,
                    rflags & RF_ALLOWCROSS != 0,
                )
            };
            if attached {
                self.index_ruleCohort_no.clear(0);
                let at_was = self.context_stack.last().unwrap().attach_to.cohort;
                self.context_stack.last_mut().unwrap().attach_to.cohort = None;
                self.trace(rnumber, rsub_reading);
                self.context_stack.last_mut().unwrap().attach_to.cohort = at_was;
                let sr = self.context_stack.last().unwrap().target.subreading.unwrap();
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
            while self.grammar.single_tags_list.get(tter.0).r#type & T_VARSTRING != 0 {
                tter = self.generate_varstring_tag_id(tter);
            }
            let thash = self.grammar.single_tags_list.get(tter.0).hash;
            let attach_gn = self.store.cohorts.get(attach.0).global_number;
            match rtype {
                K_ADDRELATION | K_ADDRELATIONS => {
                    if !is_plural {
                        crate::cohort::set_related(&mut self.store, attach);
                    }
                    crate::cohort::set_related(&mut self.store, target);
                    rel_did_anything |=
                        self.store.cohorts.get_mut(target.0).add_relation(thash, attach_gn);
                    self.rr_add_relation_rtag(target, tter, attach_gn);
                }
                K_SETRELATION | K_SETRELATIONS => {
                    if !is_plural {
                        crate::cohort::set_related(&mut self.store, attach);
                    }
                    crate::cohort::set_related(&mut self.store, target);
                    rel_did_anything |=
                        self.store.cohorts.get_mut(target.0).set_relation(thash, attach_gn);
                    self.rr_set_relation_rtag(target, tter, attach_gn);
                }
                _ => {
                    rel_did_anything |=
                        self.store.cohorts.get_mut(target.0).rem_relation(thash, attach_gn);
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
            let target_gn = self.store.cohorts.get(target.0).global_number;
            for t0 in sub_tags {
                let mut tter = t0;
                while self.grammar.single_tags_list.get(tter.0).r#type & T_VARSTRING != 0 {
                    tter = self.generate_varstring_tag_id(tter);
                }
                let thash = self.grammar.single_tags_list.get(tter.0).hash;
                match rtype {
                    K_ADDRELATIONS => {
                        crate::cohort::set_related(&mut self.store, attach);
                        rel_did_anything |=
                            self.store.cohorts.get_mut(attach.0).add_relation(thash, target_gn);
                        self.rr_add_relation_rtag(attach, tter, target_gn);
                    }
                    K_SETRELATIONS => {
                        crate::cohort::set_related(&mut self.store, attach);
                        rel_did_anything |=
                            self.store.cohorts.get_mut(attach.0).set_relation(thash, target_gn);
                        self.rr_set_relation_rtag(attach, tter, target_gn);
                    }
                    _ => {
                        rel_did_anything |=
                            self.store.cohorts.get_mut(attach.0).rem_relation(thash, target_gn);
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
            let sr = self.context_stack.last().unwrap().target.subreading.unwrap();
            self.store.readings.get_mut(sr.0).noprint = false;
            st.readings_changed = true;
        }
        // Relation rules never scan onward.
        true
    }

    /// K_MOVE_AFTER/K_MOVE_BEFORE/K_SWITCH: relocate the target cohort (and its
    /// subtree) relative to a contextual attach target within the same window.
    /// Reproduces the endless-loop bail (hash comparison + `rule_hits` counter).
    fn rr_move_switch(&mut self, st: &mut RRState, rule: RuleId, rtype: KEYWORDS, rnumber: u32) {
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
        self.tmpl_cntx = super::tmpl_context_t::default();
        self.context_stack.last_mut().unwrap().attach_to = super::ReadingSpec::default();
        let mut attach_out: Option<CohortId> = None;
        let res =
            self.run_contextual_test(Some(current), c, dep_target, Some(&mut attach_out as *mut _), None);
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
        let dep_tests: Vec<CtxId> =
            self.grammar.rule_by_number.get(rule.0).dep_tests.iter().copied().collect();
        for it in dep_tests {
            self.set_mark_frame(attach);
            self.dep_deep_seen.clear();
            self.tmpl_cntx = super::tmpl_context_t::default();
            let (aparent, alocal) = {
                let cc = self.store.cohorts.get(attach.0);
                (cc.parent, cc.local_number)
            };
            let tg = self.run_contextual_test(aparent, alocal, it, None, None).is_some();
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
        if rflags & RF_REVERSE != 0 {
            std::mem::swap(&mut a, &mut b);
        }
        let (attach, cohort) = (a, b);

        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1;
        let childset2 = self.grammar.rule_by_number.get(rule.0).childset2;

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
                self.store.single_windows.get_mut(current.0).cohorts.remove(ln);
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
                self.store.single_windows.get_mut(current.0).cohorts.insert(spot, cc);
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
            let gn = self.store.cohorts.get(cc.0).global_number;
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
        let anchor = self.store.single_windows.get(current.0).cohorts.get(spot + 1).copied();
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
            if ttype & T_WORDFORM != 0 {
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
            if ttype & T_BASEFORM != 0 {
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
                if self.grammar.single_tags_list.get(tags[k].0).hash == self.grammar.tag_any {
                    let nt = self.store.cohorts.get(insertion.0).readings[0];
                    let nt_list = self.store.readings.get(nt.0).tags_list.clone();
                    if nt_list.len() <= 2 {
                        k += 1;
                        continue;
                    }
                    tags[k] = self.tag_by_hash(nt_list[2]);
                    let mut kk = 1usize;
                    for j in 3..nt_list.len() {
                        let tid = self.tag_by_hash(nt_list[j]);
                        if self.grammar.single_tags_list.get(tid.0).r#type & T_DEPENDENCY != 0 {
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
                r.hit_by.push(self.grammar.rule_by_number.get(rule.0).number);
                r.noprint = false;
            }
            let rnumber = self.grammar.rule_by_number.get(rule.0).number;
            let mut mappings = TagList::new();
            for t0 in rit {
                let mut tter = t0;
                let mut hash = self.grammar.single_tags_list.get(tter.0).hash;
                while self.grammar.single_tags_list.get(tter.0).r#type & T_VARSTRING != 0 {
                    tter = self.generate_varstring_tag_id(tter);
                }
                let (ttype, first) = {
                    let t = self.grammar.single_tags_list.get(tter.0);
                    (t.r#type, t.tag.chars().next())
                };
                if ttype & T_MAPPING != 0 || first == Some(mapping_prefix) {
                    mappings.push(tter);
                } else {
                    hash = self.add_tag_to_reading(creading, tter);
                }
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash, creading) {
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
        } else if rtype == K_MERGECOHORTS && self.grammar.rule_by_number.get(rule.0).flags & RF_DETACH == 0 {
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
        let childset1 = self.grammar.rule_by_number.get(rule.0).childset1;
        let mut cohorts = CohortSet::new();
        self.rr_collect_subtree(current, &mut cohorts, insertion, childset1);
        if rtype == K_ADDCOHORT_BEFORE {
            let ln = self.store.cohorts.get(cohorts.front().0).local_number as usize;
            self.store.single_windows.get_mut(current.0).cohorts.insert(ln, ccohort);
            self.rr_insert_into_all_cohorts_before(current, cohorts.front(), ccohort);
        } else {
            let ln = self.store.cohorts.get(cohorts.back().0).local_number as usize + 1;
            self.store.single_windows.get_mut(current.0).cohorts.insert(ln, ccohort);
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
    fn rr_mergecohorts_attach(&mut self, insertion: CohortId, ccohort: CohortId, withs: Option<&CohortSet>) {
        let target = self.context_stack.last().unwrap().target.cohort.unwrap();
        let dp = self.store.cohorts.get(target.0).dep_parent;
        let has_parent = dp != DEP_NO_PARENT && self.gWindow.cohort_map.contains_key(&dp);
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
                        if let Some(n) = next {
                            if self.store.cohorts.get(n.0).parent != ins_parent {
                                next = None;
                            }
                        }
                        if let Some(n) = next {
                            if !withs.map(|w| w.contains(n)).unwrap_or(false) {
                                self.attach_parent_child(n, ccohort, false, false);
                                break;
                            }
                            next = self.store.cohorts.get(n.0).next;
                        }
                        if let Some(p) = prev {
                            if self.store.cohorts.get(p.0).parent != ins_parent {
                                prev = None;
                            }
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
            let parent = *self.gWindow.cohort_map.get(&dp).unwrap();
            self.attach_parent_child(parent, ccohort, false, false);
        }

        // Relation/child transfer across `withs` (C++ lines 1135-1158). `ps` is
        // the set of merged-in cohorts' global_numbers; every relation whose set
        // held any of those numbers is rewritten to point at `cCohort`, and every
        // cohort whose dep_parent is a merged cohort becomes a child of `cCohort`.
        let mut ps: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        if let Some(withs) = withs {
            for &c in withs.as_slice() {
                let cgn = self.store.cohorts.get(c.0).global_number;
                ps.insert(cgn);
                if self.store.cohorts.get(c.0).r#type & CT_RELATED != 0 {
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
                        let dst = self.store.cohorts.get_mut(ccohort.0).relations.entry(k).or_default();
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
        let ccohort_gn = self.store.cohorts.get(ccohort.0).global_number;
        let current = self.store.cohorts.get(insertion.0).parent.unwrap();
        let all_cohorts = self.store.single_windows.get(current.0).all_cohorts.clone();
        for c in all_cohorts {
            let cdp = self.store.cohorts.get(c.0).dep_parent;
            if ps.contains(&cdp) {
                self.attach_parent_child(ccohort, c, false, false);
            }
            let keys: Vec<u32> = self.store.cohorts.get(c.0).relations.keys().copied().collect();
            for key in keys {
                let mut changed = false;
                for &r in ps.iter() {
                    let rels = self.store.cohorts.get_mut(c.0).relations.get_mut(&key).unwrap();
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
    fn rr_addcohort(&mut self, st: &mut RRState, rule: RuleId) {
        let apply = self.get_apply_to().cohort.unwrap();
        // (spaces_in_added_wf: C++ "not used here")
        let (ccohort, _spaces_in_added_wf) = self.rr_add_cohort(st, rule, apply, None);
        let current = st.current;
        let rnumber = self.grammar.rule_by_number.get(rule.0).number;
        let last = *self.store.single_windows.get(current.0).cohorts.last().unwrap();
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
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, self.endtag, r) {
                    st.iter_val = rnumber;
                }
            }
        }
        self.index_single_window(current);
    }

    /// K_MERGECOHORTS: resolve the `withs` set via the rule's dep tests, add the
    /// merged cohort, then remove every merged-in cohort. Fixes the `<<<` end tag.
    fn rr_mergecohorts(&mut self, st: &mut RRState, rule: RuleId) {
        self.index_ruleCohort_no.clear(0);
        let target = self.get_apply_to().cohort.unwrap();
        let mut withs = CohortSet::new();
        withs.insert(target);
        let mut merge_at = target;

        let dep_tests: Vec<CtxId> =
            self.grammar.rule_by_number.get(rule.0).dep_tests.iter().copied().collect();
        for it in dep_tests {
            {
                let f = self.context_stack.last_mut().unwrap();
                f.attach_to = super::ReadingSpec::default();
            }
            self.merge_with = None;
            self.set_mark_frame(target);
            self.dep_deep_seen.clear();
            self.tmpl_cntx = super::tmpl_context_t::default();
            let (tparent, tlocal) = {
                let c = self.store.cohorts.get(target.0);
                (c.parent, c.local_number)
            };
            let mut attach: Option<CohortId> = None;
            let tg = self.run_contextual_test(tparent, tlocal, it, Some(&mut attach as *mut _), None).is_some()
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
        let back = *self.store.single_windows.get(current.0).cohorts.last().unwrap();
        let back_front = self.store.cohorts.get(back.0).readings[0];
        let has_endtag = {
            let r = self.store.readings.get(back_front.0);
            r.tags.find(self.endtag) != r.tags.end()
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
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, self.endtag, r) {
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
    fn rr_copycohort(&mut self, st: &mut RRState, rule: RuleId, rnumber: u32) {
        let current = st.current;
        let cohort = self.context_stack.last().unwrap().target.cohort.unwrap();
        let c = self.store.cohorts.get(cohort.0).local_number;
        self.dep_deep_seen.clear();
        self.tmpl_cntx = super::tmpl_context_t::default();
        {
            let f = self.context_stack.last_mut().unwrap();
            f.attach_to = super::ReadingSpec::default();
        }
        let dep_target = match self.grammar.rule_by_number.get(rule.0).dep_target {
            Some(dt) => dt,
            None => return,
        };
        let mut attach_out: Option<CohortId> = None;
        let res = self.run_contextual_test(
            Some(current),
            c,
            dep_target,
            Some(&mut attach_out as *mut _),
            None,
        );
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
        let dep_tests: Vec<CtxId> =
            self.grammar.rule_by_number.get(rule.0).dep_tests.iter().copied().collect();
        for it in dep_tests {
            self.context_stack.last_mut().unwrap().mark = Some(attach);
            self.dep_deep_seen.clear();
            self.tmpl_cntx = super::tmpl_context_t::default();
            let (aparent, alocal) = {
                let cc = self.store.cohorts.get(attach.0);
                (cc.parent, cc.local_number)
            };
            let tg = self.run_contextual_test(aparent, alocal, it, None, None).is_some();
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
        let mut attach = attach;
        let mut childset = self.grammar.rule_by_number.get(rule.0).childset2;
        if rflags & RF_REVERSE != 0 {
            std::mem::swap(&mut cohort, &mut attach);
            childset = self.grammar.rule_by_number.get(rule.0).childset1;
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
                    let mut hash = hash0;
                    let tter = self.tag_by_hash(hash);
                    let (ttype, first) = {
                        let t = self.grammar.single_tags_list.get(tter.0);
                        (t.r#type, t.tag.chars().next())
                    };
                    if ttype & T_MAPPING != 0 || first == Some(mapping_prefix) {
                        mappings.push(tter);
                    } else {
                        hash = self.add_tag_to_reading(creading, tter);
                    }
                    if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash, creading) {
                        st.iter_val = rnumber;
                    }
                }
                for &tter in &the_tags {
                    let mut hash = self.grammar.single_tags_list.get(tter.0).hash;
                    if hash == tag_any {
                        continue;
                    }
                    let (ttype, first) = {
                        let t = self.grammar.single_tags_list.get(tter.0);
                        (t.r#type, t.tag.chars().next())
                    };
                    if ttype & T_MAPPING != 0 || first == Some(mapping_prefix) {
                        mappings.push(tter);
                    } else {
                        hash = self.add_tag_to_reading(creading, tter);
                    }
                    if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash, creading) {
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
                let tter = self.tag_by_hash(hash0);
                let hash = self.add_tag_to_reading(cwread, tter);
                if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash, cwread) {
                    st.iter_val = rnumber;
                }
            }
        }

        let cgn = self.store.cohorts.get(ccohort.0).global_number;
        self.gWindow.cohort_map.insert(cgn, ccohort);
        self.gWindow.dep_window.insert(cgn, ccohort);

        let mut edges = CohortSet::new();
        self.rr_collect_subtree(attach_parent, &mut edges, attach, childset);

        if rflags & RF_BEFORE != 0 {
            let front = edges.front();
            let ln = self.store.cohorts.get(front.0).local_number as usize;
            self.store.single_windows.get_mut(attach_parent.0).cohorts.insert(ln, ccohort);
            self.rr_insert_into_all_cohorts_before(attach_parent, front, ccohort);
            self.attach_parent_child(front, ccohort, false, false);
        } else {
            let back = edges.back();
            let ln = self.store.cohorts.get(back.0).local_number as usize + 1;
            self.store.single_windows.get_mut(attach_parent.0).cohorts.insert(ln, ccohort);
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
    fn rr_splitcohort(&mut self, st: &mut RRState, rule: RuleId) {
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
            if ttype & T_WORDFORM != 0 {
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
            if ttype & T_WORDFORM != 0 {
                i += 1;
                bf = None;
                continue;
            }
            if ttype & T_BASEFORM != 0 {
                let wfid = self.store.cohorts.get(cohort_ids[i - 1].0).wordform.unwrap();
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
            if tagstr.chars().count() == 3
                && tagstr.starts_with("R:*")
            {
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
                    if self.grammar.single_tags_list.get(tags[k].0).hash == tag_any {
                        let nt = self.store.cohorts.get(apply.0).readings[0];
                        let nt_list = self.store.readings.get(nt.0).tags_list.clone();
                        if nt_list.len() <= 2 {
                            k += 1;
                            continue;
                        }
                        tags[k] = self.tag_by_hash(nt_list[2]);
                        let mut kk = 1usize;
                        for j in 3..nt_list.len() {
                            let tid = self.tag_by_hash(nt_list[j]);
                            if self.grammar.single_tags_list.get(tid.0).r#type & T_DEPENDENCY != 0 {
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
                    if ttype & T_MAPPING != 0 || first == Some(mapping_prefix) {
                        mappings.push(tter);
                    } else {
                        hash = self.add_tag_to_reading(creading, tter);
                    }
                    if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, hash, creading) {
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
            self.store.single_windows.get_mut(current.0).cohorts.insert(apply_ln + idx + 1, ccohort);
            // all_cohorts.insert(find(begin + apply_ln, end, apply) + idx + 1)
            self.rr_splitcohort_insert_all(current, apply, apply_ln, idx, ccohort);
        }

        // Move text from the to-be-deleted cohort to the last new cohort.
        if n > 0 {
            let last = cohort_ids[n - 1];
            let src_text = std::mem::take(&mut self.store.cohorts.get_mut(apply.0).text);
            let last_text = std::mem::replace(&mut self.store.cohorts.get_mut(last.0).text, src_text);
            self.store.cohorts.get_mut(apply.0).text = last_text;
        }

        // Dependency + named-relation re-attachment.
        let front_gn = if n > 0 {
            self.store.cohorts.get(cohort_ids[0].0).global_number
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
                    if let Some(&target) = self.gWindow.cohort_map.get(&ch) {
                        self.attach_parent_child(ccohort, target, true, true);
                    }
                    self.store.cohorts.get_mut(apply.0).dep_children.erase(ch);
                }
            }

            if cohort_dep[idx].1 == DEP_NO_PARENT {
                let dp = self.store.cohorts.get(apply.0).dep_parent;
                if let Some(&parent) = self.gWindow.cohort_map.get(&dp) {
                    self.attach_parent_child(parent, ccohort, true, true);
                }
            } else {
                let key = front_gn.wrapping_add(cohort_dep[idx].1).wrapping_sub(1);
                if let Some(&parent) = self.gWindow.cohort_map.get(&key) {
                    self.attach_parent_child(parent, ccohort, true, true);
                }
            }

            // Re-attach all named relations to the dependency tail / R:* cohort.
            if rel_trg == ui32(idx) && (self.store.cohorts.get(apply.0).r#type & CT_RELATED != 0) {
                crate::cohort::set_related(&mut self.store, ccohort);
                {
                    let mut src_rels = std::mem::take(&mut self.store.cohorts.get_mut(apply.0).relations);
                    std::mem::swap(&mut self.store.cohorts.get_mut(ccohort.0).relations, &mut src_rels);
                    self.store.cohorts.get_mut(apply.0).relations = src_rels;
                }
                let apply_gn = self.store.cohorts.get(apply.0).global_number;
                let ccohort_gn = self.store.cohorts.get(ccohort.0).global_number;
                let windows = self.rr_all_single_windows();
                for sw in windows {
                    let chs = self.store.single_windows.get(sw.0).cohorts.clone();
                    for ch in chs {
                        let keys: Vec<u32> =
                            self.store.cohorts.get(ch.0).relations.keys().copied().collect();
                        for key in keys {
                            let rels = self.store.cohorts.get_mut(ch.0).relations.get_mut(&key).unwrap();
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
        let apply_self = self.store.cohorts.get(apply.0).dep_self;
        let keys: Vec<u32> = self.gWindow.cohort_map.keys().copied().collect();
        for k in keys {
            let cid = *self.gWindow.cohort_map.get(&k).unwrap();
            self.store.cohorts.get_mut(cid.0).dep_children.erase(apply_self);
        }
        let apply_gn = self.store.cohorts.get(apply.0).global_number;
        self.gWindow.cohort_map.remove(&apply_gn);
        let apply_ln = self.store.cohorts.get(apply.0).local_number as usize;
        self.store.single_windows.get_mut(current.0).cohorts.remove(apply_ln);
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
        let found = ac[start..].iter().position(|&x| x == apply).map(|p| start + p);
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

/// C++ `u_sscanf(str, "%[0-9cd]->%[0-9pm]", &dep_self, &dep_parent) == 2`.
/// Splits on the literal `"->"` and validates each side against its scanset:
/// the left side accepts only `[0-9cd]`, the right only `[0-9pm]`. A scanset
/// match consumes the maximal leading run of accepted chars (may be empty →
/// the scanf field is empty but still "matched"); both fields must be present
/// (two conversions) for the whole match to succeed. Any char outside the
/// scanset simply terminates that field (later chars are ignored by `%[...]`),
/// but here the `->` delimiter and end-of-string bound the fields, so an
/// out-of-scanset char before `->` means the arrow won't be found at that point
/// — reproduced by requiring the ENTIRE side to be within the scanset.
fn split_dep_mapping(s: &str) -> Option<(String, String)> {
    let idx = s.find("->")?;
    let left = &s[..idx];
    let right = &s[idx + 2..];
    // `%[0-9cd]` consumes the leading run of accepted chars; the field matches
    // (possibly empty) only if the run reaches the `->` (i.e. every char before
    // the arrow is in-scanset — otherwise the arrow is past a rejected char and
    // scanf's `%[...]` would have stopped, so `->` never lines up).
    if !left.chars().all(|c| c.is_ascii_digit() || c == 'c' || c == 'd') {
        return None;
    }
    // `%[0-9pm]` consumes to end-of-string (bounded by NUL); it always "matches"
    // (empty run allowed) but the second conversion only counts if scanning got
    // this far, which it did. Extra out-of-scanset chars after the run are left
    // unconsumed but the conversion still succeeded.
    let run_end = right
        .char_indices()
        .find(|&(_, c)| !(c.is_ascii_digit() || c == 'p' || c == 'm'))
        .map(|(i, _)| i)
        .unwrap_or(right.len());
    Some((left.to_string(), right[..run_end].to_string()))
}

/// C++ `u_sscanf(field, "%i", &out) == 1` for the dep-mapping numeric fields.
/// `%i` accepts an optional sign and (via C strtol base 0) `0x`/`0` prefixes;
/// the fields here only ever hold `[0-9]` runs (the scanset filtered the rest),
/// so a plain unsigned decimal parse of the leading digit run is faithful. An
/// empty/non-numeric field fails the conversion (returns `None`).
fn parse_scanf_i(field: &str) -> Option<u32> {
    let digits: String = field.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}
