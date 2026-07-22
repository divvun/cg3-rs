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
//! stateless and cannot reach the runtime arenas, so the sorted-set operations
//! are reproduced by the arena-aware `cs_*` helpers below.
//!
//! SIGNATURE CONVENTION: the C++ `operator++`/`operator*`/`reset`/ctors become
//! methods (`advance`/`current`/`reset`/`new`) that take the arena(s) they
//! actually dereference — `cohorts: &GenArena<Cohort>` (+ `windows:
//! &GenArena<SingleWindow>` where window `number`s are compared, + `grammar:
//! &Grammar` to resolve `m_test->pos`, + `registry: &CohortRegistry` to resolve
//! `dep_parent`/`dep_children` global-numbers through `cohort_map`) — the
//! iterator only holds ids, so `self` (iterator state) and the passed arenas
//! never alias, and a caller holding `&mut` on the readings arena (the
//! `Matcher` view) can still drive them. (Stage-B: the C++ `Window*` narrowed
//! to the `CohortRegistry` view that owns `cohort_map`.) The dependency
//! iterators dereference all four views, so their `new`/`advance`/`reset` take
//! them bundled as one [`IterArenas`].

use crate::arena::{CohortId, CtxId, GenArena, SwId};
use crate::cohort::{CT_ENCLOSED, CT_REMOVED, Cohort};
use crate::contextual_test::{
    POS_LEFT, POS_RIGHT, POS_RIGHTMOST, POS_SELF, POS_SPAN_BOTH, POS_SPAN_LEFT, POS_SPAN_RIGHT,
};
use crate::grammar::Grammar;
use crate::single_window::SingleWindow;
use crate::window::CohortRegistry;

/// The read-only views the dependency iterators dereference, bundled as one
/// borrowed view: the cohort arena (`local_number`/`parent`/`dep_*`), the
/// single-window arena (window `number` comparisons), the grammar
/// (`m_test->pos`), and the cohort registry (`cohort_map`, resolving
/// `dep_parent`/`dep_children` global-numbers). The C++ iterators reach all
/// four through the `Cohort*`/`Window*` object graph.
#[derive(Copy, Clone)]
pub struct IterArenas<'a> {
    pub cohorts: &'a GenArena<Cohort>,
    pub windows: &'a GenArena<SingleWindow>,
    pub grammar: &'a Grammar,
    pub registry: &'a CohortRegistry,
}

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
// arenas to resolve a cohort's `local_number`/owning-window `number`, so the
// sorted, duplicate-suppressing set operations are reproduced here against the
// arenas rather than via the stateless `sorted_vector` comparator.

/// C++ `less_Cohort(a, b)` (SingleWindow.hpp): order by `local_number`, ties
/// broken by the owning SingleWindow `number`.
fn less_cohort(
    cohorts: &GenArena<Cohort>,
    windows: &GenArena<SingleWindow>,
    a: CohortId,
    b: CohortId,
) -> bool {
    let ca = &cohorts[a.0];
    let cb = &cohorts[b.0];
    if ca.local_number == cb.local_number {
        let na = windows[ca.parent.unwrap().0].number;
        let nb = windows[cb.parent.unwrap().0].number;
        na < nb
    } else {
        ca.local_number < cb.local_number
    }
}

/// `sorted_vector::lower_bound` — first index whose element is not less than `t`.
fn cs_lower_bound(
    cohorts: &GenArena<Cohort>,
    windows: &GenArena<SingleWindow>,
    v: &[CohortId],
    t: CohortId,
) -> usize {
    v.partition_point(|&x| less_cohort(cohorts, windows, x, t))
}

/// `sorted_vector::insert` — sorted, duplicate-suppressing. Returns `true` iff
/// `t` was inserted (the `.second` of the C++ `std::pair<iterator, bool>`).
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

/// `sorted_vector::find` — index of `t`, or `v.len()` (== `end()`) if absent.
fn cs_find(
    cohorts: &GenArena<Cohort>,
    windows: &GenArena<SingleWindow>,
    v: &[CohortId],
    t: CohortId,
) -> usize {
    if v.is_empty() {
        return v.len();
    }
    let last = v.len() - 1;
    if less_cohort(cohorts, windows, v[last], t) {
        return v.len();
    }
    if less_cohort(cohorts, windows, t, v[0]) {
        return v.len();
    }
    let it = cs_lower_bound(cohorts, windows, v, t);
    if it != v.len()
        && (less_cohort(cohorts, windows, v[it], t) || less_cohort(cohorts, windows, t, v[it]))
    {
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
    cohorts: &GenArena<Cohort>,
    windows: &GenArena<SingleWindow>,
    pos: crate::contextual_test::PosFlags,
    current: CohortId,
    cohort_parent: Option<SwId>,
    cohort_win: u32,
) -> bool {
    let cur_parent = cohorts[current.0].parent;
    if cur_parent != cohort_parent {
        let cur_win = windows[cur_parent.unwrap().0].number;
        if (!pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT) && cur_win < cohort_win)
            || (!pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT) && cur_win > cohort_win)
        {
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
        CohortIterator {
            m_span: span,
            m_cohort: cohort,
            m_test: test,
        }
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
        TopologyLeftIter {
            base: CohortIterator::new(cohort, test, span),
        }
    }

    // [spec:cg3:def:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
    /// C++ `operator++`: walk LEFT along the sibling chain, stopping at a window
    /// boundary the test may not cross and skipping `CT_ENCLOSED` cohorts.
    pub fn advance(&mut self, cohorts: &GenArena<Cohort>, grammar: &Grammar) {
        if self.base.m_cohort.is_none() || self.base.m_test.is_none() {
            return;
        }
        let cur_id = self.base.m_cohort.unwrap();
        let test_id = self.base.m_test.unwrap();
        let cur_parent = cohorts[cur_id.0].parent;
        let pos = grammar.contexts_arena[test_id.0].pos;
        let boundary = match cohorts[cur_id.0].prev {
            Some(prev) => {
                cohorts[prev.0].parent != cur_parent
                    && !(pos.intersects(POS_SPAN_BOTH | POS_SPAN_LEFT) || self.base.m_span)
            }
            None => false,
        };
        if boundary {
            self.base.m_cohort = None;
        } else {
            let mut mc = self.base.m_cohort;
            loop {
                mc = cohorts[mc.unwrap().0].prev;
                match mc {
                    Some(id) if cohorts[id.0].r#type.intersects(CT_ENCLOSED) => continue,
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
        TopologyRightIter {
            base: CohortIterator::new(cohort, test, span),
        }
    }

    // [spec:cg3:def:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
    /// C++ `operator++`: mirror of `TopologyLeftIter::advance`, walking RIGHT via
    /// `next` and using `POS_SPAN_RIGHT`.
    pub fn advance(&mut self, cohorts: &GenArena<Cohort>, grammar: &Grammar) {
        if self.base.m_cohort.is_none() || self.base.m_test.is_none() {
            return;
        }
        let cur_id = self.base.m_cohort.unwrap();
        let test_id = self.base.m_test.unwrap();
        let cur_parent = cohorts[cur_id.0].parent;
        let pos = grammar.contexts_arena[test_id.0].pos;
        let boundary = match cohorts[cur_id.0].next {
            Some(next) => {
                cohorts[next.0].parent != cur_parent
                    && !(pos.intersects(POS_SPAN_BOTH | POS_SPAN_RIGHT) || self.base.m_span)
            }
            None => false,
        };
        if boundary {
            self.base.m_cohort = None;
        } else {
            let mut mc = self.base.m_cohort;
            loop {
                mc = cohorts[mc.unwrap().0].next;
                match mc {
                    Some(id) if cohorts[id.0].r#type.intersects(CT_ENCLOSED) => continue,
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
        arenas: IterArenas<'_>,
    ) -> Self {
        let mut it = DepParentIter {
            base: CohortIterator::new(cohort, test, span),
            m_seen: Vec::new(),
        };
        it.advance(arenas);
        it
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
    /// C++ `operator++`: one step up the dep tree. The cycle guard `m_seen`
    /// stores the chain of previously-CURRENT cohorts (the child, not `p`).
    pub fn advance(&mut self, arenas: IterArenas<'_>) {
        let IterArenas {
            cohorts,
            windows,
            grammar,
            registry,
        } = arenas;
        if self.base.m_cohort.is_none() || self.base.m_test.is_none() {
            return;
        }
        let cur_id = self.base.m_cohort.unwrap();
        let test_id = self.base.m_test.unwrap();
        let pos = grammar.contexts_arena[test_id.0].pos;
        let dep_parent = cohorts[cur_id.0].dep_parent;
        if dep_parent.is_some()
            && let Some(&p_id) = registry.cohort_map.get(&dep_parent.unwrap())
        {
            if cohorts[p_id.0].r#type.intersects(CT_REMOVED) {
                self.base.m_cohort = None;
                return;
            }
            if cs_find(cohorts, windows, &self.m_seen, p_id) == self.m_seen.len() {
                cs_insert(cohorts, windows, &mut self.m_seen, cur_id);
                let cur_parent = cohorts[cur_id.0].parent;
                let p_parent = cohorts[p_id.0].parent;
                if p_parent == cur_parent || pos.intersects(POS_SPAN_BOTH) || self.base.m_span {
                    self.base.m_cohort = Some(p_id);
                } else {
                    let cur_win = windows[cur_parent.unwrap().0].number;
                    let p_win = windows[p_parent.unwrap().0].number;
                    if (p_win < cur_win && pos.intersects(POS_SPAN_LEFT))
                        || (p_win > cur_win && pos.intersects(POS_SPAN_RIGHT))
                    {
                        self.base.m_cohort = Some(p_id);
                    } else {
                        self.base.m_cohort = None;
                    }
                }
                return;
            }
        }
        self.base.m_cohort = None;
    }

    // [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter.reset-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.reset-fn]
    /// The C++ reset signature plus the [`IterArenas`] view the re-advance
    /// dereferences.
    pub fn reset(
        &mut self,
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        arenas: IterArenas<'_>,
    ) {
        self.base.reset(cohort, test, span);
        self.m_seen.clear();
        self.advance(arenas);
    }
}

impl DepDescendentIter {
    // [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
    pub fn new(
        cohort: Option<CohortId>,
        test: Option<CtxId>,
        span: bool,
        arenas: IterArenas<'_>,
    ) -> Self {
        let mut it = DepDescendentIter {
            base: CohortIterator::new(cohort, test, span),
            m_descendents: Vec::new(),
            m_ai: 0,
        };
        it.reset(cohort, test, span, arenas);
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
        arenas: IterArenas<'_>,
    ) {
        let IterArenas {
            cohorts,
            windows,
            grammar,
            registry,
        } = arenas;
        self.base.reset(cohort, test, span);
        self.m_descendents.clear();
        self.base.m_cohort = None;

        if let (Some(cohort_id), Some(test_id)) = (cohort, test) {
            let pos = grammar.contexts_arena[test_id.0].pos;
            let cohort_parent = cohorts[cohort_id.0].parent;
            let cohort_win = windows[cohort_parent.unwrap().0].number;

            // Seed with the direct children.
            let dch0 = cohorts[cohort_id.0].dep_children.clone();
            for dter in dch0.as_slice() {
                let current = match registry.cohort_map.get(&crate::types::GlobalNumber(*dter)) {
                    None => continue,
                    Some(&c) => c,
                };
                if span_good(cohorts, windows, pos, current, cohort_parent, cohort_win) {
                    cs_insert(cohorts, windows, &mut self.m_descendents, current);
                }
            }

            // BFS transitive closure; `seen` guards cycles (each expanded once).
            let mut seen: Vec<CohortId> = Vec::new();
            cs_insert(cohorts, windows, &mut seen, cohort_id);
            loop {
                let mut added = false;
                let mut to_add: Vec<CohortId> = Vec::new();
                let len = self.m_descendents.len();
                for i in 0..len {
                    let cohort_inner = self.m_descendents[i];
                    if cs_find(cohorts, windows, &seen, cohort_inner) != seen.len() {
                        continue;
                    }
                    cs_insert(cohorts, windows, &mut seen, cohort_inner);
                    let dch = cohorts[cohort_inner.0].dep_children.clone();
                    for dter in dch.as_slice() {
                        let current =
                            match registry.cohort_map.get(&crate::types::GlobalNumber(*dter)) {
                                None => continue,
                                Some(&c) => c,
                            };
                        // The span test is always measured against the ORIGINAL
                        // `cohort`'s window, not `cohort_inner`'s.
                        if span_good(cohorts, windows, pos, current, cohort_parent, cohort_win) {
                            cs_insert(cohorts, windows, &mut to_add, current);
                            added = true;
                        }
                    }
                }
                for &iter in &to_add {
                    cs_insert(cohorts, windows, &mut self.m_descendents, iter);
                }
                if !added {
                    break;
                }
            }

            // Position filtering (separate `if`s, applied in order).
            if pos.intersects(POS_LEFT) {
                let lb = cs_lower_bound(cohorts, windows, &self.m_descendents, cohort_id);
                self.m_descendents = self.m_descendents[..lb].to_vec();
            }
            if pos.intersects(POS_RIGHT) {
                let lb = cs_lower_bound(cohorts, windows, &self.m_descendents, cohort_id);
                self.m_descendents = self.m_descendents[lb..].to_vec();
            }
            if pos.intersects(POS_SELF) {
                cs_insert(cohorts, windows, &mut self.m_descendents, cohort_id);
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
        arenas: IterArenas<'_>,
    ) -> Self {
        let mut it = DepAncestorIter {
            base: CohortIterator::new(cohort, test, span),
            m_ancestors: Vec::new(),
            m_ai: 0,
        };
        it.reset(cohort, test, span, arenas);
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
        arenas: IterArenas<'_>,
    ) {
        let IterArenas {
            cohorts,
            windows,
            grammar,
            registry,
        } = arenas;
        self.base.reset(cohort, test, span);
        self.m_ancestors.clear();
        self.base.m_cohort = None;

        if let (Some(cohort_id), Some(test_id)) = (cohort, test) {
            let pos = grammar.contexts_arena[test_id.0].pos;
            let cohort_parent = cohorts[cohort_id.0].parent;
            let cohort_win = windows[cohort_parent.unwrap().0].number;

            let mut current = cohort_id;
            loop {
                let dep_parent = cohorts[current.0].dep_parent;
                // C++ looks the raw value up unconditionally; DEP_NO_PARENT
                // simply misses the map, exactly like None here.
                current = match dep_parent.and_then(|dp| registry.cohort_map.get(&dp)) {
                    None => break,
                    Some(&c) => c,
                };
                if span_good(cohorts, windows, pos, current, cohort_parent, cohort_win) {
                    // A failed (duplicate) insert means we've looped back.
                    if !cs_insert(cohorts, windows, &mut self.m_ancestors, current) {
                        break;
                    }
                }
            }

            if pos.intersects(POS_LEFT) {
                let lb = cs_lower_bound(cohorts, windows, &self.m_ancestors, cohort_id);
                self.m_ancestors = self.m_ancestors[..lb].to_vec();
            }
            if pos.intersects(POS_RIGHT) {
                let lb = cs_lower_bound(cohorts, windows, &self.m_ancestors, cohort_id);
                self.m_ancestors = self.m_ancestors[lb..].to_vec();
            }
            if pos.intersects(POS_SELF) {
                cs_insert(cohorts, windows, &mut self.m_ancestors, cohort_id);
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
    pub fn add_cohort(
        &mut self,
        cohorts: &GenArena<Cohort>,
        windows: &GenArena<SingleWindow>,
        cohort: CohortId,
    ) {
        cs_insert(cohorts, windows, &mut self.m_cohortset, cohort);
        self.m_cohortsetiter = 0; // begin()
    }

    // [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
    /// C++ `operator++`. RE-YIELD BUG (reproduced, NOT fixed): on a match it
    /// breaks WITHOUT advancing `m_cohortsetiter`, so the cursor still points AT
    /// the matched element and a subsequent `advance` re-yields it. Harmless —
    /// the type is dead code.
    pub fn advance(
        &mut self,
        cohorts: &GenArena<Cohort>,
        windows: &GenArena<SingleWindow>,
        grammar: &Grammar,
    ) {
        self.base.m_cohort = None;
        while self.m_cohortsetiter != self.m_cohortset.len() {
            let c = self.m_cohortset[self.m_cohortsetiter];
            let c_parent = cohorts[c.0].parent;
            let orig_parent = cohorts[self.m_origcohort.unwrap().0].parent;
            let pos = grammar.contexts_arena[self.base.m_test.unwrap().0].pos;
            if c_parent == orig_parent || pos.intersects(POS_SPAN_BOTH) || self.base.m_span {
                self.base.m_cohort = Some(c);
                break;
            } else {
                let c_win = windows[c_parent.unwrap().0].number;
                let orig_win = windows[orig_parent.unwrap().0].number;
                if (c_win < orig_win && pos.intersects(POS_SPAN_LEFT))
                    || (c_win > orig_win && pos.intersects(POS_SPAN_RIGHT))
                {
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
        ChildrenIterator {
            base: MultiCohortIterator::new(cohort, test, span),
            m_depth: 0,
        }
    }

    // [spec:cg3:def:cohort-iterator.cg3.children-iterator.children-iterator-fn]
    // [spec:cg3:sem:cohort-iterator.cg3.children-iterator.children-iterator-fn]
    /// C++ `operator++` (ToDo: iterative deepening DFS). BUGS (reproduced, NOT
    /// fixed): dereferences `m_cohort` with no null check, and even when
    /// `dep_children` is non-empty it installs a fresh `CohortSetIter` WITHOUT
    /// populating it via `add_cohort` and never advances `m_cohort` — so it does
    /// not actually walk children. Dead code.
    pub fn advance(&mut self, cohorts: &GenArena<Cohort>) {
        self.base.m_cohortiter = None; // m_cohortiter.reset()
        self.m_depth += 1;
        if !cohorts[self.base.m_cohort.unwrap().0].dep_children.empty() {
            self.base.m_cohortiter = Some(Box::new(CohortSetIter::new(
                self.base.m_cohort,
                self.base.m_test,
                self.base.m_span,
            )));
        }
    }
}
