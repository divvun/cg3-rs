//! `GrammarApplicator` — runRulesOnSingleWindow — the per-section rule schedule and its relation/tag helpers.
//!
//! Split out of the wave-2 monolithic `run_rules.rs` (wave 4, w4-file-split-fmt).

use crate::arena::{CohortId, ReadingId, RuleId, SwId, TagId};
use crate::cohort::CohortSet;
use crate::inlines::ui32;
use crate::interval_vector::uint32IntervalVector;
use crate::reading::ReadingList;
use crate::rule::{RF_ENCL_FINAL, RF_NOITERATE, RF_REPEAT};
use crate::tag::{T_MAPPING, T_VARSTRING, TagList};
use crate::types::{TagHash, UString};

// C++ anonymous `enum { RV_NOTHING = 1, RV_SOMETHING = 2, RV_DELIMITED = 4,
// RV_TRACERULE = 8 };` — the return-value bit flags of runRulesOnSingleWindow.

use super::*;

impl crate::grammar_applicator::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn]
    /// C++ `uint32_t runRulesOnSingleWindow(SingleWindow& current, const
    /// uint32IntervalVector& rules)`.
    pub fn run_rules_on_single_window(
        &mut self,
        current: SwId,
        rules: &uint32IntervalVector,
    ) -> u32 {
        let mut retval = RV_NOTHING;
        let mut section_did_something = false;

        let intersects = self
            .store
            .single_windows
            .get(current.0)
            .valid_rules
            .intersect(rules);
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
        self.gWindow
            .cohort_map
            .insert(crate::types::GlobalNumber(0), front);

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
                if !self.apply_mappings && (rtype == K_MAP || rtype == K_ADD || rtype == K_REPLACE)
                {
                    break 'repeat;
                }
                if !self.apply_corrections && (rtype == K_SUBSTITUTE || rtype == K_APPEND) {
                    break 'repeat;
                }
                if has_enclosures {
                    if (rflags.intersects(RF_ENCL_FINAL)) && !self.did_final_enclosure {
                        break 'repeat;
                    }
                    if self.did_final_enclosure && (!rflags.intersects(RF_ENCL_FINAL)) {
                        break 'repeat;
                    }
                }

                st.readings_changed = false;
                st.should_repeat = false;
                st.should_bail = false;
                st.removed.clear();
                st.selected.clear();

                // C++ builds two RuleCallback closures aliasing this + the
                // shared state; the port threads `st` directly (wave 4 — the
                // raw-pointer trampolines are gone).
                let rv = self.run_single_rule(current, RuleId(j), &mut st);
                if rv || st.readings_changed {
                    if !((rflags.intersects(RF_NOITERATE)) && self.section_max_count != 1) {
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
                if rule_did_something && (rflags.intersects(RF_REPEAT)) {
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
        let n = self
            .store
            .single_windows
            .get(current.0)
            .rule_to_cohorts
            .len();
        for i in 0..n {
            // Extract, sort with the store-aware comparator, put back.
            let mut v: Vec<CohortId> = self.store.single_windows.get(current.0).rule_to_cohorts[i]
                .as_slice()
                .to_vec();
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
    pub(crate) fn rr_reindex(&mut self, which: SwId) {
        let n = self.store.single_windows.get(which.0).cohorts.len();
        for i in 0..n {
            let cid = self.store.single_windows.get(which.0).cohorts[i];
            self.store.cohorts.get_mut(cid.0).local_number = ui32(i);
        }
        let gw = &self.gWindow;
        gw.rebuild_cohort_links(&mut self.store);
    }

    /// `collect_subtree(cs, head, cset)`.
    pub(crate) fn rr_collect_subtree(
        &mut self,
        current: SwId,
        cs: &mut CohortSet,
        head: CohortId,
        cset: u32,
    ) {
        if cset != 0 {
            let head_gn = self.store.cohorts.get(head.0).global_number;
            let cohorts = self.store.single_windows.get(current.0).cohorts.clone();
            for iter in &cohorts {
                let (gn, dp) = {
                    let c = self.store.cohorts.get(iter.0);
                    (c.global_number, c.dep_parent)
                };
                if gn == head_gn
                    || (dp == Some(head_gn) && self.does_set_match_cohort_normal(*iter, cset, None))
                {
                    self.cohortset_insert(cs, *iter);
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
                        self.cohortset_insert(&mut more, *iter);
                    }
                }
            }
            for m in more.as_slice().to_vec() {
                self.cohortset_insert(cs, m);
            }
        } else {
            self.cohortset_insert(cs, head);
        }
    }

    /// `make_relation_rtag(tag, id)`: intern the textual `R:<tag>:<id>` tag.
    fn rr_make_relation_rtag(&mut self, tag: TagId, id: u32) -> TagId {
        let base = self.grammar.single_tags_list.get(tag.0).tag.clone();
        let tmp: UString = format!("R:{}:{}", base, id);
        // C++ `addTag(tmp)` is the `addTag(const UChar*)` convenience overload →
        // `addTag(str, 0)`.
        self.add_tag(&tmp, crate::tag::TagType::empty())
    }

    /// `add_relation_rtag(cohort, tag, id)`.
    pub(crate) fn rr_add_relation_rtag(&mut self, cohort: CohortId, tag: TagId, id: u32) {
        let nt = self.rr_make_relation_rtag(tag, id);
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            self.add_tag_to_reading(r, nt);
        }
    }

    /// `set_relation_rtag(cohort, tag, id)`: erase existing `R:<tag>:*` tags then
    /// add the new one.
    pub(crate) fn rr_set_relation_rtag(&mut self, cohort: CohortId, tag: TagId, id: u32) {
        let nt = self.rr_make_relation_rtag(tag, id);
        let base = self.grammar.single_tags_list.get(tag.0).tag.clone();
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for r in rs {
            let list = self.store.readings.get(r.0).tags_list.clone();
            let mut new_list: Vec<u32> = Vec::with_capacity(list.len());
            for h in list {
                let utag = self
                    .grammar
                    .single_tags_list
                    .get(self.tag_by_hash(TagHash(h)).0)
                    .tag
                    .clone();
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
    pub(crate) fn rr_rem_relation_rtag(&mut self, cohort: CohortId, tag: TagId, id: u32) {
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
    pub(crate) fn rr_insert_taglist_to_reading(
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
            if self
                .grammar
                .single_tags_list
                .get(tag.0)
                .r#type
                .intersects(T_VARSTRING)
            {
                tag = self.generate_varstring_tag_id(tag);
            }
            let (thash, ttype, first_char) = {
                let t = self.grammar.single_tags_list.get(tag.0);
                (t.hash, t.r#type, t.tag.chars().next())
            };
            if thash.get() == self.grammar.tag_any {
                break;
            }
            if ttype.intersects(T_MAPPING) || first_char == Some(mapping_prefix) {
                mappings.push(tag);
            } else {
                self.store
                    .readings
                    .get_mut(reading.0)
                    .tags_list
                    .insert(at, thash.get());
                at += 1;
            }
            let rule = st.rule.0;
            if self.update_valid_rules(&st.rules.clone(), &mut st.intersects, thash.get(), reading) {
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
    pub(crate) fn subs_any_push(&mut self, rid: ReadingId) {
        self.subs_any.push(rid);
    }

    /// `clear(subs_any)` — free every amalgamated sub-reading back to the readings
    /// arena and empty the tracking vector. RECONCILIATION: matches the required
    /// `Vec<ReadingId>` shape of `subs_any` (see [`Self::subs_any_push`]).
    pub(crate) fn subs_any_clear(&mut self) {
        let ids: Vec<ReadingId> = self.subs_any.to_vec();
        for rid in ids {
            let opt = Some(rid);
            crate::reading::free_reading(&mut self.store, opt);
        }
        self.subs_any.clear();
    }
}
