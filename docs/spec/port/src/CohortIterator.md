# src/CohortIterator.cpp, src/CohortIterator.hpp

> [spec:cg3:def:cohort-iterator.cg3.children-iterator]
> class ChildrenIterator : public MultiCohortIterator {
>   ChildrenIterator& operator++();
>   uint32_t m_depth;
> }

> [spec:cg3:def:cohort-iterator.cg3.children-iterator.children-iterator-fn]
> ChildrenIterator::ChildrenIterator(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.children-iterator.children-iterator-fn]
> Constructs a ChildrenIterator: delegates to the MultiCohortIterator base
> constructor (storing m_span, m_cohort=cohort, m_test=test; m_seen empty,
> m_cohortiter a null unique_ptr) and sets m_depth = 0. Its `operator++`
> resets `m_cohortiter` (destroying any current inner iterator),
> increments m_depth, and — if `m_cohort->dep_children` is non-empty —
> installs a freshly constructed `new CohortSetIter(m_cohort, m_test,
> m_span)` into m_cohortiter; but it never populates that CohortSetIter
> via addCohort and never advances m_cohort, so it does not actually walk
> children (see the "Iterative deepening depth-first search" ToDo). It
> also dereferences `m_cohort` with no null check. NOTE: this whole family
> (ChildrenIterator, MultiCohortIterator, CohortSetIter and addCohort) is
> dead code — nothing outside CohortIterator.* references it — so these
> quirks are currently unexercised.

> [spec:cg3:def:cohort-iterator.cg3.cohort-iterator]
> class CohortIterator {
>   virtual CohortIterator& operator++();
>   bool m_span = false;
>   Cohort* m_cohort = nullptr;
>   const ContextualTest* m_test = nullptr;
> }

> [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
> CohortIterator::CohortIterator(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.cohort-iterator-fn]
> Base CohortIterator constructor: stores m_span=span, m_cohort=cohort,
> m_test=test. The iterator initially "points at" `cohort`; `operator*`
> returns m_cohort. All three params default to nullptr/nullptr/false, so
> `CohortIterator(0)` is the canonical end/sentinel iterator (m_cohort ==
> null). The plain base `operator++` is single-shot: it just sets
> m_cohort = null and returns *this (subclasses override it). This base
> `operator++`/`operator*`/`reset` behavior is the contract the driver
> loop `for (; *it != CohortIterator(0); ++(*it))` relies on.

> [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.operator-fn]
> bool CohortIterator::operator==(const CohortIterator& other) const

> [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.operator-fn]
> Equality compares ONLY the current cohort pointer: returns `m_cohort ==
> other.m_cohort`. (The sibling `operator!=` returns `m_cohort !=
> other.m_cohort`.) This is how the drive loop detects the end sentinel —
> comparing against a default-constructed `CohortIterator(0)` whose
> m_cohort is null. m_test and m_span are ignored by comparison.

> [spec:cg3:def:cohort-iterator.cg3.cohort-iterator.reset-fn]
> void CohortIterator::reset(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.cohort-iterator.reset-fn]
> Base reset: assigns m_span=span, m_cohort=cohort, m_test=test,
> re-seating the iterator to point at `cohort` without allocating. Virtual
> — DepParentIter/DepDescendentIter/DepAncestorIter override it to also
> rebuild their traversal state (and, for DepParentIter, pre-advance).

> [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter]
> class CohortSetIter : public CohortIterator {
>   CohortSetIter& operator++();
>   Cohort* m_origcohort;
>   CohortSet m_cohortset;
>   CohortSet::const_iterator m_cohortsetiter;
> }

> [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter.add-cohort-fn]
> void CohortSetIter::addCohort(Cohort* cohort)

> [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.add-cohort-fn]
> Inserts `cohort` into the sorted `m_cohortset` (CohortSet, dedup by
> compare_Cohort ordering) and RESETS `m_cohortsetiter` to
> `m_cohortset.begin()`, so every add rewinds the internal cursor to the
> front. Dead code: no caller exists anywhere in the codebase.

> [spec:cg3:def:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
> CohortSetIter::CohortSetIter(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.cohort-set-iter.cohort-set-iter-fn]
> Constructs a CohortSetIter: delegates to the base CohortIterator
> constructor (m_cohort=cohort, m_test=test, m_span=span), records
> `m_origcohort = cohort`, leaves `m_cohortset` empty, and sets
> `m_cohortsetiter = m_cohortset.end()`. Cohorts are added later via
> addCohort. Its `operator++` sets m_cohort=null then scans from
> m_cohortsetiter to the set end, stopping (break) at the first cohort
> that passes the span test: accepted if `cohort->parent ==
> m_origcohort->parent` (same SingleWindow) OR (m_test->pos &
> POS_SPAN_BOTH) OR m_span; else if the cohort's window number is less
> than m_origcohort's window number and POS_SPAN_LEFT is set; else if
> greater and POS_SPAN_RIGHT is set. On a match it breaks WITHOUT
> advancing past the matched element, so m_cohortsetiter still points AT
> that element — a subsequent `operator++` re-tests and re-yields the same
> cohort (a re-yield bug), though harmless since the type is unused. If
> nothing passes, m_cohort stays null. Dead code (see addCohort).

> [spec:cg3:def:cohort-iterator.cg3.dep-ancestor-iter]
> class DepAncestorIter : public CohortIterator {
>   DepAncestorIter& operator++();
>   CohortSet m_ancestors;
>   CohortSet::const_iterator m_ai;
> }

> [spec:cg3:def:cohort-iterator.cg3.dep-ancestor-iter.dep-ancestor-iter-fn]
> DepAncestorIter::DepAncestorIter(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.dep-ancestor-iter.dep-ancestor-iter-fn]
> Constructs a DepAncestorIter: delegates to the base CohortIterator
> constructor then calls `reset(cohort, test, span)`, which precomputes
> the whole ancestor chain into `m_ancestors` and seats m_cohort on the
> first one. `operator++` walks the precomputed set: `++m_ai;
> m_cohort = null; if (m_ai != m_ancestors.end()) m_cohort = *m_ai;`
> returning *this.

> [spec:cg3:def:cohort-iterator.cg3.dep-ancestor-iter.reset-fn]
> void DepAncestorIter::reset(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.dep-ancestor-iter.reset-fn]
> Rebuilds `m_ancestors` (a CohortSet ordered by compare_Cohort — by
> local_number then owning-window number) as the chain of dependency
> ancestors of `cohort`, filtered by span/position flags. Steps:
> (1) base reset; clear m_ancestors; m_cohort = null. (2) Only if BOTH
> cohort and test are non-null: loop starting `current = cohort`. Each
> iteration looks up `current->dep_parent` in the owning Window's
> cohort_map (always reached via the ORIGINAL `cohort->parent->parent`);
> if that key is absent, break. Otherwise set `current` to the parent
> cohort found. Compute good=true, and if `current->parent !=
> cohort->parent` (different window): set good=false when current's window
> number is < cohort's and neither POS_SPAN_BOTH nor POS_SPAN_LEFT is set,
> or when it is > cohort's and neither POS_SPAN_BOTH nor POS_SPAN_RIGHT is
> set. If good, `m_ancestors.insert(current)`; if that insert FAILS
> (element already present → we've looped back onto a known ancestor),
> break. QUIRK/cycle risk: when good is false the ancestor is skipped but
> the loop still climbs upward through it, and the ONLY loop terminators
> are a cohort_map miss or a failed (duplicate) insert — so a cross-window
> cycle whose nodes are all span-filtered (thus never inserted) would loop
> forever. (3) Position filtering, in order: if POS_LEFT, keep only
> entries before cohort's sort position — build a temp via
> assign(m_ancestors.begin(), m_ancestors.lower_bound(cohort)) and swap it
> in; if POS_RIGHT, keep entries at/after cohort via
> assign(lower_bound(cohort), end); if POS_SELF, insert cohort itself; if
> POS_RIGHTMOST and non-empty, reverse the underlying vector in place.
> (4) m_ai = m_ancestors.begin(); m_cohort = *m_ai if non-empty else null.

> [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter]
> class DepDescendentIter : public CohortIterator {
>   DepDescendentIter& operator++();
>   CohortSet m_descendents;
>   CohortSet::const_iterator m_ai;
> }

> [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
> DepDescendentIter::DepDescendentIter(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.dep-descendent-iter-fn]
> Constructs a DepDescendentIter: delegates to the base CohortIterator
> constructor then calls `reset(cohort, test, span)`, which precomputes
> the full transitive descendant set into `m_descendents` and seats
> m_cohort on the first one. `operator++` walks that precomputed set:
> `++m_ai; m_cohort = null; if (m_ai != m_descendents.end())
> m_cohort = *m_ai;` returning *this.

> [spec:cg3:def:cohort-iterator.cg3.dep-descendent-iter.reset-fn]
> void DepDescendentIter::reset(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.dep-descendent-iter.reset-fn]
> Rebuilds `m_descendents` (a CohortSet ordered by compare_Cohort — by
> local_number then owning-window number) as the transitive dependency
> descendants of `cohort`, filtered by span/position flags. Steps:
> (1) base reset; clear m_descendents; m_cohort = null. (2) Only if BOTH
> cohort and test are non-null: seed with direct children — for each
> global_number `dter` in `cohort->dep_children`, skip it if absent from
> the Window cohort_map; else fetch child `current`. If `current->parent
> != cohort->parent` (different window), reject (good=false) when current
> is in an earlier window (number <) and neither POS_SPAN_BOTH nor
> POS_SPAN_LEFT is set, or in a later window (number >) and neither
> POS_SPAN_BOTH nor POS_SPAN_RIGHT is set; insert each accepted child into
> m_descendents. (3) BFS transitive closure: a local `m_seen` CohortSet is
> initialized with {cohort}; repeat (do/while `added`) — for each
> `cohort_inner` currently in m_descendents that is not yet in m_seen,
> mark it seen and scan ITS dep_children the same way, EXCEPT the
> different-window/span test is always measured against the ORIGINAL
> `cohort`'s window (not cohort_inner's); collect accepted grandchildren
> into a `to_add` CohortSet and set added=true whenever an accepted child
> is found; after the pass, merge all of to_add into m_descendents. The
> m_seen set guards cycles (each cohort is expanded at most once), so the
> loop terminates (at most one extra no-op pass). (4) Position filtering,
> in order: if POS_LEFT, keep only entries before cohort — reuse m_seen
> via assign(m_descendents.begin(), m_descendents.lower_bound(cohort)) and
> swap; if POS_RIGHT, keep entries at/after cohort via
> assign(lower_bound(cohort), end) and swap; if POS_SELF, insert cohort
> itself; if POS_RIGHTMOST and non-empty, reverse the underlying vector in
> place. (5) m_ai = m_descendents.begin(); m_cohort = *m_ai if non-empty
> else null.

> [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter]
> class DepParentIter : public CohortIterator {
>   DepParentIter& operator++();
>   CohortSet m_seen;
> }

> [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
> DepParentIter::DepParentIter(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.dep-parent-iter-fn]
> Constructs a DepParentIter: delegates to the base CohortIterator
> constructor (m_seen starts empty), then immediately calls `++(*this)` so
> the iterator lands on the first dependency parent at construction time.
> `operator++` advances one step up the dep tree: returns unchanged if
> m_cohort or m_test is null. If `m_cohort->dep_parent != DEP_NO_PARENT`,
> it looks that global_number up in the owning Window's cohort_map
> (`m_cohort->parent->parent->cohort_map`). If found, let `p` be that
> parent cohort: if `p` has CT_REMOVED set, m_cohort=null and return; else
> if `p` is NOT already in m_seen, it first inserts the CURRENT m_cohort
> (the child, NOT `p`) into m_seen — this is the cycle guard — then
> accepts `p` as the new m_cohort iff `p->parent == m_cohort->parent`
> (same window) OR (m_test->pos & POS_SPAN_BOTH) OR m_span; else if p's
> window number < m_cohort's and POS_SPAN_LEFT is set; else if p's window
> number > m_cohort's and POS_SPAN_RIGHT is set; otherwise m_cohort=null;
> and returns. In every other case (dep_parent is DEP_NO_PARENT, parent
> not in cohort_map, or `p` already in m_seen) it falls through to
> m_cohort=null and returns. Cycle handling: because m_seen stores the
> chain of previously-current cohorts, a cycle A→B→A terminates — after
> stepping to B, B's parent lookup yields A which is already in m_seen, so
> the unseen-branch is skipped and m_cohort becomes null.

> [spec:cg3:def:cohort-iterator.cg3.dep-parent-iter.reset-fn]
> void DepParentIter::reset(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.dep-parent-iter.reset-fn]
> Calls the base CohortIterator::reset (assigns m_cohort/m_test/m_span),
> clears `m_seen`, then calls `++(*this)` to advance onto the first
> dependency parent — mirroring the constructor.

> [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator]
> class MultiCohortIterator {
>   virtual MultiCohortIterator& operator++();
>   bool m_span;
>   Cohort* m_cohort;
>   const ContextualTest* m_test;
>   CohortSet m_seen;
>   std::unique_ptr<CohortSetIter> m_cohortiter;
> }

> [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
> MultiCohortIterator::MultiCohortIterator(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.multi-cohort-iterator-fn]
> Base MultiCohortIterator constructor: stores m_span=span,
> m_cohort=cohort, m_test=test; `m_seen` is an empty CohortSet and
> `m_cohortiter` a null unique_ptr<CohortSetIter>. `operator*` returns
> `m_cohortiter.get()` (the inner CohortSetIter*, i.e. this is an iterator
> OF iterators), and the base `operator++` simply nulls m_cohort and
> returns *this. Dead code together with ChildrenIterator/CohortSetIter.

> [spec:cg3:def:cohort-iterator.cg3.multi-cohort-iterator.operator-fn]
> bool MultiCohortIterator::operator==(const MultiCohortIterator& other) const

> [spec:cg3:sem:cohort-iterator.cg3.multi-cohort-iterator.operator-fn]
> Returns `m_cohort == other.m_cohort` (pointer equality on the current
> cohort). The sibling `operator!=` returns the negation. Same
> end-sentinel comparison pattern as CohortIterator; m_test/m_span/m_seen/
> m_cohortiter are ignored by comparison.

> [spec:cg3:def:cohort-iterator.cg3.topology-left-iter]
> class TopologyLeftIter : public CohortIterator {
>   TopologyLeftIter& operator++();
> }

> [spec:cg3:def:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
> TopologyLeftIter::TopologyLeftIter(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.topology-left-iter.topology-left-iter-fn]
> Constructs a TopologyLeftIter by delegating to the base CohortIterator
> constructor (m_cohort=cohort, m_test=test, m_span=span); adds no state.
> Its `operator++` walks LEFT along the sibling chain. If m_cohort or
> m_test is null it returns unchanged. If the immediate `prev` exists AND
> belongs to a DIFFERENT SingleWindow (`prev->parent != m_cohort->parent`)
> AND the test does not permit crossing left — i.e. `!((m_test->pos &
> (POS_SPAN_BOTH | POS_SPAN_LEFT)) || m_span)` — it sets m_cohort=null
> (stop at the window boundary). Otherwise it steps `m_cohort =
> m_cohort->prev` inside a do/while that keeps stepping left while the
> landed cohort is non-null and has CT_ENCLOSED set, thereby skipping
> enclosed cohorts and stopping on the first non-enclosed cohort or null.

> [spec:cg3:def:cohort-iterator.cg3.topology-right-iter]
> class TopologyRightIter : public CohortIterator {
>   TopologyRightIter& operator++();
> }

> [spec:cg3:def:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
> TopologyRightIter::TopologyRightIter(Cohort* cohort, const ContextualTest* test, bool span)

> [spec:cg3:sem:cohort-iterator.cg3.topology-right-iter.topology-right-iter-fn]
> Constructs a TopologyRightIter by delegating to the base CohortIterator
> constructor; adds no state. Its `operator++` is the mirror image of
> TopologyLeftIter, walking RIGHT via `next`: null-guards m_cohort/m_test;
> if `next` exists in a DIFFERENT SingleWindow (`next->parent !=
> m_cohort->parent`) AND `!((m_test->pos & (POS_SPAN_BOTH |
> POS_SPAN_RIGHT)) || m_span)`, sets m_cohort=null (stop at the boundary);
> otherwise steps `m_cohort = m_cohort->next` in a do/while that skips any
> CT_ENCLOSED cohorts, stopping on the first non-enclosed cohort or null.

