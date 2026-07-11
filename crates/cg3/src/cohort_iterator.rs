//! Iterator types ported from `src/CohortIterator.hpp`.
//!
//! C++ single-inheritance is modelled by composition: each derived iterator
//! embeds its base as a `base` field (`CohortIterator` for the topology/dep/set
//! iterators, `MultiCohortIterator` for `ChildrenIterator`). Arena model:
//! `Cohort*` → [`CohortId`], `const ContextualTest*` → [`CtxId`]. A
//! `CohortSet` (`sorted_vector<Cohort*, compare_Cohort>`) → `Vec<CohortId>`, and
//! a `CohortSet::const_iterator` cursor → a `usize` index into that vector.
//!
//! The advance/reset method bodies (this pass) are ported bug-for-bug from
//! `src/CohortIterator.cpp`. A `CohortSet` (`sorted_vector<Cohort*,
//! compare_Cohort>`) stays a `Vec<CohortId>`: the `compare_Cohort` ordering
//! (`less_Cohort` — by `local_number`, tie-broken by owning-window `number`)
//! must dereference a cohort, but the port's `compare_Cohort` comparator is
//! stateless and cannot reach the [`RuntimeStore`], so the sorted-set operations
//! are reproduced by the store-aware `cs_*` helpers below.
//!
//! SIGNATURE CONVENTION: the C++ `operator++`/`operator*`/`reset`/ctors become
//! methods (`advance`/`current`/`reset`/`new`) that take `store: &RuntimeStore`
//! (+ `grammar: &Grammar` to resolve `m_test->pos`, + `window: &Window` to
//! resolve `dep_parent`/`dep_children` global-numbers through `cohort_map`) — the
//! iterator only holds ids, so `self` (iterator state) and the passed stores
//! never alias.

use crate::arena::{CohortId, CtxId, SwId};
use crate::cohort::{CT_ENCLOSED, CT_REMOVED};
use crate::contextual_test::{
    POS_LEFT, POS_RIGHT, POS_RIGHTMOST, POS_SELF, POS_SPAN_BOTH, POS_SPAN_LEFT, POS_SPAN_RIGHT,
};
use crate::grammar::Grammar;
use crate::store::RuntimeStore;
use crate::window::Window;

// [spec:cg3:def:cohort-iterator.cg3.cohort-iterator]
/// C++ `class CohortIterator` — the base input-iterator over cohorts.
#[derive(Default, Clone, Debug)]
pub struct CohortIterator {
    pub m_span: bool,
    /// C++ `Cohort* m_cohort` — the cohort currently pointed at.
    pub m_cohort: Option<CohortId>,
    /// C++ `const ContextualTest* m_test`.
    pub m_test: Option<CtxId>,
}

// [spec:cg3:def:cohort-iterator.cg3.topology-left-iter]
/// C++ `class TopologyLeftIter : public CohortIterator` — walks left along the
/// sibling chain. Adds no state.
#[derive(Default, Clone, Debug)]
pub struct TopologyLeftIter {
    pub base: CohortIterator,
}

// [spec:cg3:def:cohort-iterator.cg3.topology-right-iter]
/// C++ `class TopologyRightIter : public CohortIterator` — walks right along the
/// sibling chain. Adds no state.
#[derive(Default, Clone, Debug)]
pub struct TopologyRightIter {
    pub base: CohortIterator,
}

// [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter]
/// C++ `class DepParentIter : public CohortIterator` — climbs the dependency
/// parent chain.
#[derive(Default, Clone, Debug)]
pub struct DepParentIter {
    pub base: CohortIterator,
    /// C++ `CohortSet m_seen` — the cycle guard.
    pub m_seen: Vec<CohortId>,
}

// [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter]
/// C++ `class DepDescendentIter : public CohortIterator` — walks the precomputed
/// transitive descendant set.
#[derive(Default, Clone, Debug)]
pub struct DepDescendentIter {
    pub base: CohortIterator,
    /// C++ `CohortSet m_descendents`.
    pub m_descendents: Vec<CohortId>,
    /// C++ `CohortSet::const_iterator m_ai` — cursor index into `m_descendents`.
    pub m_ai: usize,
}

// [spec:cg3:def:cohort-iterator.cg3.dep-ancestor-iter]
/// C++ `class DepAncestorIter : public CohortIterator` — walks the precomputed
/// ancestor chain.
#[derive(Default, Clone, Debug)]
pub struct DepAncestorIter {
    pub base: CohortIterator,
    /// C++ `CohortSet m_ancestors`.
    pub m_ancestors: Vec<CohortId>,
    /// C++ `CohortSet::const_iterator m_ai` — cursor index into `m_ancestors`.
    pub m_ai: usize,
}

// [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter]
/// C++ `class CohortSetIter : public CohortIterator` — iterates an explicit,
/// span-filtered cohort set. (Dead code in the C++ source, ported for parity.)
#[derive(Default, Clone, Debug)]
pub struct CohortSetIter {
    pub base: CohortIterator,
    /// C++ `Cohort* m_origcohort`.
    pub m_origcohort: Option<CohortId>,
    /// C++ `CohortSet m_cohortset`.
    pub m_cohortset: Vec<CohortId>,
    /// C++ `CohortSet::const_iterator m_cohortsetiter` — cursor index into
    /// `m_cohortset`.
    pub m_cohortsetiter: usize,
}

// [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator]
/// C++ `class MultiCohortIterator` — an iterator OF iterators (independent base,
/// not derived from `CohortIterator`). Dead code in the C++ source.
#[derive(Default, Clone, Debug)]
pub struct MultiCohortIterator {
    pub m_span: bool,
    /// C++ `Cohort* m_cohort`.
    pub m_cohort: Option<CohortId>,
    /// C++ `const ContextualTest* m_test`.
    pub m_test: Option<CtxId>,
    /// C++ `CohortSet m_seen`.
    pub m_seen: Vec<CohortId>,
    /// C++ `std::unique_ptr<CohortSetIter> m_cohortiter` — the inner iterator.
    pub m_cohortiter: Option<Box<CohortSetIter>>,
}

// [spec:cg3:def:cohort-iterator.cg3.children-iterator]
/// C++ `class ChildrenIterator : public MultiCohortIterator`. Dead code in the
/// C++ source.
#[derive(Default, Clone, Debug)]
pub struct ChildrenIterator {
    pub base: MultiCohortIterator,
    pub m_depth: u32,
}

// --- Store-aware `CohortSet` helpers ---------------------------------------
//
// A C++ `CohortSet` (`sorted_vector<Cohort*, compare_Cohort>`) is a
// `Vec<CohortId>` in the port. `compare_Cohort` (== `less_Cohort`) needs the
// store to resolve a cohort's `local_number`/owning-window `number`, so the
// sorted, duplicate-suppressing set operations are reproduced here against the
// store rather than via the stateless `sorted_vector` comparator.

/// C++ `less_Cohort(a, b)` (SingleWindow.hpp): order by `local_number`, ties
/// broken by the owning SingleWindow `number`.
fn less_cohort(store: &RuntimeStore, a: CohortId, b: CohortId) -> bool {
    let ca = &store.cohorts[a.0];
    let cb = &store.cohorts[b.0];
    if ca.local_number == cb.local_number {
        let na = store.single_windows[ca.parent.unwrap().0].number;
        let nb = store.single_windows[cb.parent.unwrap().0].number;
        na < nb
    } else {
        ca.local_number < cb.local_number
    }
}

/// `sorted_vector::lower_bound` — first index whose element is not less than `t`.
fn cs_lower_bound(store: &RuntimeStore, v: &[CohortId], t: CohortId) -> usize {
    v.partition_point(|&x| less_cohort(store, x, t))
}

/// `sorted_vector::insert` — sorted, duplicate-suppressing. Returns `true` iff
/// `t` was inserted (the `.second` of the C++ `std::pair<iterator, bool>`).
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

/// `sorted_vector::find` — index of `t`, or `v.len()` (== `end()`) if absent.
fn cs_find(store: &RuntimeStore, v: &[CohortId], t: CohortId) -> usize {
    if v.is_empty() {
        return v.len();
    }
    let last = v.len() - 1;
    if less_cohort(store, v[last], t) {
        return v.len();
    }
    if less_cohort(store, t, v[0]) {
        return v.len();
    }
    let it = cs_lower_bound(store, v, t);
    if it != v.len() && (less_cohort(store, v[it], t) || less_cohort(store, t, v[it])) {
        return v.len();
    }
    it
}

/// The shared span/position accept test used by `DepDescendentIter::reset` and
/// `DepAncestorIter::reset` — mirrors the inline C++ `good` logic, always
/// measured against the ORIGINAL cohort's window (`cohort_parent`/`cohort_win`).
/// `current->parent->number` is only read when the windows differ, matching the
/// C++ deref pattern.
fn span_good(
    store: &RuntimeStore,
    pos: crate::contextual_test::PosFlags,
    current: CohortId,
    cohort_parent: Option<SwId>,
    cohort_win: u32,
) -> bool {
    let cur_parent = store.cohorts[current.0].parent;
    if cur_parent != cohort_parent {
        let cur_win = store.single_windows[cur_parent.unwrap().0].number;
        if !pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT) && cur_win < cohort_win {
            return false;
        } else if !pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT) && cur_win > cohort_win {
            return false;
        }
    }
    true
}

impl CohortIterator {
    // [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
    /// Base ctor: stores `m_span`/`m_cohort`/`m_test`. `new(None, None, false)`
    /// is the end/sentinel iterator (`m_cohort == None`).
    pub fn new(cohort: Option<CohortId>, test: Option<CtxId>, span: bool) -> Self {
        CohortIterator { m_span: span, m_cohort: cohort, m_test: test }
    }

    // [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
    /// C++ `operator++` — the single-shot base advance: nulls `m_cohort`.
    pub fn advance(&mut self) {
        self.m_cohort = None;
    }

    // [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
    /// C++ `Cohort* operator*()` — returns the current cohort.
    pub fn current(&self) -> Option<CohortId> {
        self.m_cohort
    }

    // [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.operator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.operator-fn]
    /// C++ `operator==` — compares ONLY the current cohort (end-sentinel check).
    pub fn equals(&self, other: &CohortIterator) -> bool {
        self.m_cohort == other.m_cohort
    }

    // [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.reset-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.reset-fn]
    /// Base reset: re-seats the iterator without allocating.
    pub fn reset(&mut self, cohort: Option<CohortId>, test: Option<CtxId>, span: bool) {
        self.m_span = span;
        self.m_cohort = cohort;
        self.m_test = test;
    }
}

impl TopologyLeftIter {
    // [spec:cg3:def:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
    pub fn new(cohort: Option<CohortId>, test: Option<CtxId>, span: bool) -> Self {
        TopologyLeftIter { base: CohortIterator::new(cohort, test, span) }
    }

    // [spec:cg3:def:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
    /// C++ `operator++`: walk LEFT along the sibling chain, stopping at a window
    /// boundary the test may not cross and skipping `CT_ENCLOSED` cohorts.
    pub fn advance(&mut self, store: &RuntimeStore, grammar: &Grammar) {
        if self.base.m_cohort.is_none() || self.base.m_test.is_none() {
            return;
        }
        let cur_id = self.base.m_cohort.unwrap();
        let test_id = self.base.m_test.unwrap();
        let cur_parent = store.cohorts[cur_id.0].parent;
        let pos = grammar.contexts_arena[test_id.0].pos;
        let boundary = match store.cohorts[cur_id.0].prev {
            Some(prev) => {
                store.cohorts[prev.0].parent != cur_parent
                    && !(pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT) || self.base.m_span)
            }
            None => false,
        };
        if boundary {
            self.base.m_cohort = None;
        } else {
            let mut mc = self.base.m_cohort;
            loop {
                mc = store.cohorts[mc.unwrap().0].prev;
                match mc {
                    Some(id) if store.cohorts[id.0].r#type.intersects(CT_ENCLOSED) => continue,
                    _ => break,
                }
            }
            self.base.m_cohort = mc;
        }
    }
}

impl TopologyRightIter {
    // [spec:cg3:def:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
    pub fn new(cohort: Option<CohortId>, test: Option<CtxId>, span: bool) -> Self {
        TopologyRightIter { base: CohortIterator::new(cohort, test, span) }
    }

    // [spec:cg3:def:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
    /// C++ `operator++`: mirror of `TopologyLeftIter::advance`, walking RIGHT via
    /// `next` and using `POS_SPAN_RIGHT`.
    pub fn advance(&mut self, store: &RuntimeStore, grammar: &Grammar) {
        if self.base.m_cohort.is_none() || self.base.m_test.is_none() {
            return;
        }
        let cur_id = self.base.m_cohort.unwrap();
        let test_id = self.base.m_test.unwrap();
        let cur_parent = store.cohorts[cur_id.0].parent;
        let pos = grammar.contexts_arena[test_id.0].pos;
        let boundary = match store.cohorts[cur_id.0].next {
            Some(next) => {
                store.cohorts[next.0].parent != cur_parent
                    && !(pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT) || self.base.m_span)
            }
            None => false,
        };
        if boundary {
            self.base.m_cohort = None;
        } else {
            let mut mc = self.base.m_cohort;
            loop {
                mc = store.cohorts[mc.unwrap().0].next;
                match mc {
                    Some(id) if store.cohorts[id.0].r#type.intersects(CT_ENCLOSED) => continue,
                    _ => break,
                }
            }
            self.base.m_cohort = mc;
        }
    }
}

impl DepParentIter {
    // [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
    /// Ctor: delegates to the base then immediately advances onto the first
    /// dependency parent (mirroring the C++ `++(*this)` in the ctor body).
    pub fn new(
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        store: &RuntimeStore,
        grammar: &Grammar,
        window: &Window,
    ) -> Self {
        let mut it = DepParentIter { base: CohortIterator::new(cohort, test, span), m_seen: Vec::new() };
        it.advance(store, grammar, window);
        it
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
    /// C++ `operator++`: one step up the dep tree. The cycle guard `m_seen`
    /// stores the chain of previously-CURRENT cohorts (the child, not `p`).
    pub fn advance(&mut self, store: &RuntimeStore, grammar: &Grammar, window: &Window) {
        if self.base.m_cohort.is_none() || self.base.m_test.is_none() {
            return;
        }
        let cur_id = self.base.m_cohort.unwrap();
        let test_id = self.base.m_test.unwrap();
        let pos = grammar.contexts_arena[test_id.0].pos;
        let dep_parent = store.cohorts[cur_id.0].dep_parent;
        if dep_parent.is_some() {
            if let Some(&p_id) = window.cohort_map.get(&dep_parent.unwrap()) {
                if store.cohorts[p_id.0].r#type.intersects(CT_REMOVED) {
                    self.base.m_cohort = None;
                    return;
                }
                if cs_find(store, &self.m_seen, p_id) == self.m_seen.len() {
                    cs_insert(store, &mut self.m_seen, cur_id);
                    let cur_parent = store.cohorts[cur_id.0].parent;
                    let p_parent = store.cohorts[p_id.0].parent;
                    if p_parent == cur_parent || pos.intersects(POS_SPAN_BOTH) || self.base.m_span {
                        self.base.m_cohort = Some(p_id);
                    } else {
                        let cur_win = store.single_windows[cur_parent.unwrap().0].number;
                        let p_win = store.single_windows[p_parent.unwrap().0].number;
                        if p_win < cur_win && pos.intersects(POS_SPAN_LEFT) {
                            self.base.m_cohort = Some(p_id);
                        } else if p_win > cur_win && pos.intersects(POS_SPAN_RIGHT) {
                            self.base.m_cohort = Some(p_id);
                        } else {
                            self.base.m_cohort = None;
                        }
                    }
                    return;
                }
            }
        }
        self.base.m_cohort = None;
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter.reset-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.reset-fn]
    pub fn reset(
        &mut self,
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        store: &RuntimeStore,
        grammar: &Grammar,
        window: &Window,
    ) {
        self.base.reset(cohort, test, span);
        self.m_seen.clear();
        self.advance(store, grammar, window);
    }
}

impl DepDescendentIter {
    // [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
    pub fn new(
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        store: &RuntimeStore,
        grammar: &Grammar,
        window: &Window,
    ) -> Self {
        let mut it = DepDescendentIter {
            base: CohortIterator::new(cohort, test, span),
            m_descendents: Vec::new(),
            m_ai: 0,
        };
        it.reset(cohort, test, span, store, grammar, window);
        it
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
    /// C++ `operator++`: walk the precomputed descendant set.
    pub fn advance(&mut self) {
        self.m_ai += 1;
        self.base.m_cohort = None;
        if self.m_ai != self.m_descendents.len() {
            self.base.m_cohort = Some(self.m_descendents[self.m_ai]);
        }
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter.reset-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.reset-fn]
    pub fn reset(
        &mut self,
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        store: &RuntimeStore,
        grammar: &Grammar,
        window: &Window,
    ) {
        self.base.reset(cohort, test, span);
        self.m_descendents.clear();
        self.base.m_cohort = None;

        if let (Some(cohort_id), Some(test_id)) = (cohort, test) {
            let pos = grammar.contexts_arena[test_id.0].pos;
            let cohort_parent = store.cohorts[cohort_id.0].parent;
            let cohort_win = store.single_windows[cohort_parent.unwrap().0].number;

            // Seed with the direct children.
            let dch0 = store.cohorts[cohort_id.0].dep_children.clone();
            for dter in dch0.as_slice() {
                let current = match window.cohort_map.get(dter) {
                    None => continue,
                    Some(&c) => c,
                };
                if span_good(store, pos, current, cohort_parent, cohort_win) {
                    cs_insert(store, &mut self.m_descendents, current);
                }
            }

            // BFS transitive closure; `seen` guards cycles (each expanded once).
            let mut seen: Vec<CohortId> = Vec::new();
            cs_insert(store, &mut seen, cohort_id);
            loop {
                let mut added = false;
                let mut to_add: Vec<CohortId> = Vec::new();
                let len = self.m_descendents.len();
                for i in 0..len {
                    let cohort_inner = self.m_descendents[i];
                    if cs_find(store, &seen, cohort_inner) != seen.len() {
                        continue;
                    }
                    cs_insert(store, &mut seen, cohort_inner);
                    let dch = store.cohorts[cohort_inner.0].dep_children.clone();
                    for dter in dch.as_slice() {
                        let current = match window.cohort_map.get(dter) {
                            None => continue,
                            Some(&c) => c,
                        };
                        // The span test is always measured against the ORIGINAL
                        // `cohort`'s window, not `cohort_inner`'s.
                        if span_good(store, pos, current, cohort_parent, cohort_win) {
                            cs_insert(store, &mut to_add, current);
                            added = true;
                        }
                    }
                }
                for &iter in &to_add {
                    cs_insert(store, &mut self.m_descendents, iter);
                }
                if !added {
                    break;
                }
            }

            // Position filtering (separate `if`s, applied in order).
            if pos.intersects(POS_LEFT) {
                let lb = cs_lower_bound(store, &self.m_descendents, cohort_id);
                self.m_descendents = self.m_descendents[..lb].to_vec();
            }
            if pos.intersects(POS_RIGHT) {
                let lb = cs_lower_bound(store, &self.m_descendents, cohort_id);
                self.m_descendents = self.m_descendents[lb..].to_vec();
            }
            if pos.intersects(POS_SELF) {
                cs_insert(store, &mut self.m_descendents, cohort_id);
            }
            if pos.intersects(POS_RIGHTMOST) && !self.m_descendents.is_empty() {
                self.m_descendents.reverse();
            }
        }

        self.m_ai = 0;
        if self.m_ai != self.m_descendents.len() {
            self.base.m_cohort = Some(self.m_descendents[self.m_ai]);
        }
    }
}

impl DepAncestorIter {
    // [spec:cg3:def:cohort-iterator.cg3.dep-ancestor-iter.dep-ancestor-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-ancestor-iter.dep-ancestor-iter-fn]
    pub fn new(
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        store: &RuntimeStore,
        grammar: &Grammar,
        window: &Window,
    ) -> Self {
        let mut it = DepAncestorIter {
            base: CohortIterator::new(cohort, test, span),
            m_ancestors: Vec::new(),
            m_ai: 0,
        };
        it.reset(cohort, test, span, store, grammar, window);
        it
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-ancestor-iter.dep-ancestor-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-ancestor-iter.dep-ancestor-iter-fn]
    /// C++ `operator++`: walk the precomputed ancestor chain.
    pub fn advance(&mut self) {
        self.m_ai += 1;
        self.base.m_cohort = None;
        if self.m_ai != self.m_ancestors.len() {
            self.base.m_cohort = Some(self.m_ancestors[self.m_ai]);
        }
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-ancestor-iter.reset-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-ancestor-iter.reset-fn]
    /// Rebuilds `m_ancestors`. QUIRK/cycle risk (reproduced, NOT fixed): when a
    /// node is span-filtered (`good == false`) it is skipped but the loop still
    /// climbs through it; the only terminators are a `cohort_map` miss or a
    /// duplicate insert, so an all-span-filtered cross-window cycle loops forever.
    pub fn reset(
        &mut self,
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        store: &RuntimeStore,
        grammar: &Grammar,
        window: &Window,
    ) {
        self.base.reset(cohort, test, span);
        self.m_ancestors.clear();
        self.base.m_cohort = None;

        if let (Some(cohort_id), Some(test_id)) = (cohort, test) {
            let pos = grammar.contexts_arena[test_id.0].pos;
            let cohort_parent = store.cohorts[cohort_id.0].parent;
            let cohort_win = store.single_windows[cohort_parent.unwrap().0].number;

            let mut current = cohort_id;
            loop {
                let dep_parent = store.cohorts[current.0].dep_parent;
                // C++ looks the raw value up unconditionally; DEP_NO_PARENT
                // simply misses the map, exactly like None here.
                current = match dep_parent.and_then(|dp| window.cohort_map.get(&dp)) {
                    None => break,
                    Some(&c) => c,
                };
                if span_good(store, pos, current, cohort_parent, cohort_win) {
                    // A failed (duplicate) insert means we've looped back.
                    if !cs_insert(store, &mut self.m_ancestors, current) {
                        break;
                    }
                }
            }

            if pos.intersects(POS_LEFT) {
                let lb = cs_lower_bound(store, &self.m_ancestors, cohort_id);
                self.m_ancestors = self.m_ancestors[..lb].to_vec();
            }
            if pos.intersects(POS_RIGHT) {
                let lb = cs_lower_bound(store, &self.m_ancestors, cohort_id);
                self.m_ancestors = self.m_ancestors[lb..].to_vec();
            }
            if pos.intersects(POS_SELF) {
                cs_insert(store, &mut self.m_ancestors, cohort_id);
            }
            if pos.intersects(POS_RIGHTMOST) && !self.m_ancestors.is_empty() {
                self.m_ancestors.reverse();
            }
        }

        self.m_ai = 0;
        if self.m_ai != self.m_ancestors.len() {
            self.base.m_cohort = Some(self.m_ancestors[self.m_ai]);
        }
    }
}

impl CohortSetIter {
    // [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
    pub fn new(cohort: Option<CohortId>, test: Option<CtxId>, span: bool) -> Self {
        CohortSetIter {
            base: CohortIterator::new(cohort, test, span),
            m_origcohort: cohort,
            m_cohortset: Vec::new(),
            m_cohortsetiter: 0, // m_cohortset.end() == 0 while empty
        }
    }

    // [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter.add-cohort-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.add-cohort-fn]
    /// Sorted, deduped insert; rewinds the cursor to `begin()` every time.
    /// Dead code — no caller exists.
    pub fn add_cohort(&mut self, store: &RuntimeStore, cohort: CohortId) {
        cs_insert(store, &mut self.m_cohortset, cohort);
        self.m_cohortsetiter = 0; // begin()
    }

    // [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
    /// C++ `operator++`. RE-YIELD BUG (reproduced, NOT fixed): on a match it
    /// breaks WITHOUT advancing `m_cohortsetiter`, so the cursor still points AT
    /// the matched element and a subsequent `advance` re-yields it. Harmless —
    /// the type is dead code.
    pub fn advance(&mut self, store: &RuntimeStore, grammar: &Grammar) {
        self.base.m_cohort = None;
        while self.m_cohortsetiter != self.m_cohortset.len() {
            let c = self.m_cohortset[self.m_cohortsetiter];
            let c_parent = store.cohorts[c.0].parent;
            let orig_parent = store.cohorts[self.m_origcohort.unwrap().0].parent;
            let pos = grammar.contexts_arena[self.base.m_test.unwrap().0].pos;
            if c_parent == orig_parent || pos.intersects(POS_SPAN_BOTH) || self.base.m_span {
                self.base.m_cohort = Some(c);
                break;
            } else {
                let c_win = store.single_windows[c_parent.unwrap().0].number;
                let orig_win = store.single_windows[orig_parent.unwrap().0].number;
                if c_win < orig_win && pos.intersects(POS_SPAN_LEFT) {
                    self.base.m_cohort = Some(c);
                    break;
                } else if c_win > orig_win && pos.intersects(POS_SPAN_RIGHT) {
                    self.base.m_cohort = Some(c);
                    break;
                }
            }
            self.m_cohortsetiter += 1;
        }
    }
}

impl MultiCohortIterator {
    // [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
    pub fn new(cohort: Option<CohortId>, test: Option<CtxId>, span: bool) -> Self {
        MultiCohortIterator {
            m_span: span,
            m_cohort: cohort,
            m_test: test,
            m_seen: Vec::new(),
            m_cohortiter: None,
        }
    }

    // [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
    /// C++ base `operator++`: nulls `m_cohort`.
    pub fn advance(&mut self) {
        self.m_cohort = None;
    }

    // [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
    /// C++ `CohortIterator* operator*()` — the inner iterator (an iterator OF
    /// iterators).
    pub fn current(&self) -> Option<&CohortSetIter> {
        self.m_cohortiter.as_deref()
    }

    // [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator.operator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.operator-fn]
    pub fn equals(&self, other: &MultiCohortIterator) -> bool {
        self.m_cohort == other.m_cohort
    }
}

impl ChildrenIterator {
    // [spec:cg3:def:cohort-iterator.cg3.children-iterator.children-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.children-iterator.children-iterator-fn]
    pub fn new(cohort: Option<CohortId>, test: Option<CtxId>, span: bool) -> Self {
        ChildrenIterator { base: MultiCohortIterator::new(cohort, test, span), m_depth: 0 }
    }

    // [spec:cg3:def:cohort-iterator.cg3.children-iterator.children-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.children-iterator.children-iterator-fn]
    /// C++ `operator++` (ToDo: iterative deepening DFS). BUGS (reproduced, NOT
    /// fixed): dereferences `m_cohort` with no null check, and even when
    /// `dep_children` is non-empty it installs a fresh `CohortSetIter` WITHOUT
    /// populating it via `add_cohort` and never advances `m_cohort` — so it does
    /// not actually walk children. Dead code.
    pub fn advance(&mut self, store: &RuntimeStore) {
        self.base.m_cohortiter = None; // m_cohortiter.reset()
        self.m_depth += 1;
        if !store.cohorts[self.base.m_cohort.unwrap().0].dep_children.empty() {
            self.base.m_cohortiter = Some(Box::new(CohortSetIter::new(
                self.base.m_cohort,
                self.base.m_test,
                self.base.m_span,
            )));
        }
    }
}
