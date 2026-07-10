# src/SingleWindow.cpp, src/SingleWindow.hpp

> [spec:cg3:def:single-window.cg3.alloc-swindow-fn]
> SingleWindow* alloc_swindow(Window* p)

> [spec:cg3:sem:single-window.cg3.alloc-swindow-fn]
> Obtains a `SingleWindow` for parent window `p`. Calls `pool_swindows.get()` (the
> thread-local `pool<SingleWindow>`); if it returns null (pool empty), constructs a
> `new SingleWindow(p)`; otherwise reuses the pooled object and just sets its
> `parent = p` (a pooled object was already `clear()`-ed when returned). Returns the
> pointer.

> [spec:cg3:def:single-window.cg3.compare-cohort]
> struct compare_Cohort

> [spec:cg3:def:single-window.cg3.compare-cohort.operator-fn]
> bool operator()(const Cohort* a, const Cohort* b) const

> [spec:cg3:sem:single-window.cg3.compare-cohort.operator-fn]
> Functor `operator()(a, b)`: returns `less_Cohort(a, b)` — the strict-weak
> ordering for sorting `Cohort*` (by `local_number`, tie-broken by owning window
> `number`). `const`.

> [spec:cg3:def:single-window.cg3.free-swindow-fn]
> void free_swindow(SingleWindow*& s)

> [spec:cg3:sem:single-window.cg3.free-swindow-fn]
> Returns a single-window to the pool. Takes the pointer by reference. If `s` is
> null, returns immediately. Otherwise `pool_swindows.put(s)` — which calls
> `s->clear()` and inserts `s` into the sorted pool (a duplicate insert is silently
> ignored) — then sets the caller's reference `s = 0` (null).

> [spec:cg3:def:single-window.cg3.less-cohort-fn]
> inline bool less_Cohort(const Cohort* a, const Cohort* b)

> [spec:cg3:sem:single-window.cg3.less-cohort-fn]
> Strict-less comparator for two `Cohort*`. If `a->local_number ==
> b->local_number`, order by owning single-window number
> (`a->parent->number < b->parent->number`); otherwise order by
> `a->local_number < b->local_number`. Returns the bool.

> [spec:cg3:def:single-window.cg3.single-window]
> class SingleWindow {
>   uint32_t number = 0;
>   bool has_enclosures = false;
>   bool flush_after = false;
>   SingleWindow *next = nullptr, *previous = nullptr;
>   Window* parent = nullptr;
>   UString text;
>   UString text_post;
>   CohortVector all_cohorts;
>   CohortVector cohorts;
>   uint32IntervalVector valid_rules;
>   uint32SortedVector hit_external;
>   std::vector<CohortSet> rule_to_cohorts;
>   std::unique_ptr<CohortSet> nested_rule_to_cohorts;
>   uint32FlatHashMap variables_set;
>   uint32FlatHashSet variables_rem;
>   uint32SortedVector variables_output;
>   Reading bag_of_tags;
> }

> [spec:cg3:def:single-window.cg3.single-window.append-cohort-fn]
> void SingleWindow::appendCohort(Cohort* cohort)

> [spec:cg3:sem:single-window.cg3.single-window.append-cohort-fn]
> Appends `cohort` as the new last cohort of this window and wires up all links.
> Sets `cohort->local_number = UI32(cohorts.size())` (its index BEFORE insertion)
> and `cohort->parent = this`. If `cohort->dep_self != 0`, records it as the highest
> dependency seen: `parent->parent->dep_highest_seen = cohort->dep_self` (`parent`
> is the owning Window, `parent->parent` the GrammarApplicator). Backward link: if
> `cohorts` is currently empty AND the `previous` single-window exists and has
> cohorts, link to that window's last cohort
> (`previous->cohorts.back()->next = cohort; cohort->prev = previous->cohorts.back()`);
> otherwise, if `cohorts` is non-empty, link to this window's current last cohort
> (`cohort->prev = cohorts.back(); cohorts.back()->next = cohort`). Forward link: if
> the `next` single-window exists and has cohorts, splice before its first cohort
> (`next->cohorts.front()->prev = cohort; cohort->next = next->cohorts.front()`).
> Push `cohort` onto both `cohorts` and `all_cohorts`. Register it in the parent
> Window maps: `cohort_map[cohort->global_number] = cohort` and
> `dep_window[cohort->global_number] = cohort`. If `cohort->local_number == 0` (the
> window's first cohort), also set `cohort_map[0] = cohort`. No return value.

> [spec:cg3:def:single-window.cg3.single-window.clear-fn]
> void SingleWindow::clear()

> [spec:cg3:sem:single-window.cg3.single-window.clear-fn]
> Resets the single-window to a pristine, reusable state (used by the pool). First
> performs the SAME two teardown steps as the destructor: if `cohorts.size() > 1`,
> iterate `parent->relation_map` and erase every entry whose value (`iter->second`)
> is `<= cohorts.back()->global_number`, advancing otherwise; then call
> `free_cohort` on every cohort in `all_cohorts`; then splice this window out of the
> sibling doubly-linked list (if both `next` and `previous` exist, link them
> together; else null the surviving side's back-pointer). Then zero/clear every
> field: `number = 0`, `has_enclosures = false`, `flush_after = false`,
> `next = nullptr`, `previous = nullptr`, `parent = nullptr`; clear `text`,
> `text_post`, `cohorts`, `all_cohorts`, `valid_rules`, `hit_external`; clear each
> `CohortSet` inside `rule_to_cohorts` (the OUTER vector keeps its size/capacity —
> only the per-index sets are emptied); clear `variables_set`, `variables_rem`,
> `variables_output`, and `bag_of_tags`. Quirk: `nested_rule_to_cohorts` (the
> `unique_ptr<CohortSet>`) is NOT reset here, so a stale nested set can survive a
> `clear()` and be reused via the pool.

> [spec:cg3:def:single-window.cg3.single-window.single-window-fn]
> SingleWindow::~SingleWindow()

> [spec:cg3:sem:single-window.cg3.single-window.single-window-fn]
> Destructor. If `cohorts.size() > 1`, prune the parent Window's `relation_map`:
> iterate it and erase every entry whose value (`iter->second`, a global cohort
> number) is `<= cohorts.back()->global_number` (the last cohort's global number),
> advancing the iterator otherwise. Then call `free_cohort(iter)` on every cohort in
> `all_cohorts` (recycling each to the cohort pool). Finally splice this window out
> of the sibling doubly-linked list: if both `next` and `previous` exist, link them
> (`next->previous = previous; previous->next = next`); else null the surviving
> side's back-pointer (`next->previous = nullptr` and/or `previous->next = nullptr`).
> (Any `CG_TRACE_OBJECTS` diagnostics are compile-time only.)

