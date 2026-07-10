# src/Cohort.cpp, src/Cohort.hpp

> [spec:cg3:def:cohort.cg3.alloc-cohort-fn]
> Cohort* alloc_cohort(SingleWindow* p)

> [spec:cg3:sem:cohort.cg3.alloc-cohort-fn]
> Recycles or allocates a Cohort. Calls the thread-local free-list
> `pool_cohorts.get()`: if the pool is empty that returns null/0, and a
> fresh `new Cohort(p)` is constructed (its constructor sets only
> `parent = p` and leaves every other member at its in-class default).
> Otherwise `get()` popped a previously freed cohort (already reset by
> `clear()` when it was put back), and this function reassigns its
> `parent = p`. Returns the cohort pointer.

> [spec:cg3:def:cohort.cg3.cohort]
> class Cohort {
>   uint8_t type = 0;
>   uint32_t global_number = 0;
>   uint32_t local_number = 0;
>   uint32_t enclosed = 0;
>   Tag* wordform = nullptr;
>   uint32_t dep_self = 0;
>   uint32_t dep_parent = DEP_NO_PARENT;
>   uint32_t is_pleft = 0;
>   uint32_t is_pright = 0;
>   SingleWindow* parent = nullptr;
>   UString text;
>   UString wblank;
>   Cohort* prev = nullptr;
>   Cohort* next = nullptr;
>   Reading* wread = nullptr;
>   ReadingList readings;
>   ReadingList deleted;
>   ReadingList delayed;
>   ReadingList ignored;
>   num_t num_max, num_min;
>   uint32SortedVector dep_children;
>   boost::dynamic_bitset<> possible_sets;
>   RelationCtn relations;
>   RelationCtn relations_input;
>   uint32_t line_number = 0;
> }

> [spec:cg3:def:cohort.cg3.cohort-set]
> typedef sorted_vector<Cohort*, compare_Cohort> CohortSet

> [spec:cg3:def:cohort.cg3.cohort-vector]
> typedef std::vector<Cohort*> CohortVector

> [spec:cg3:def:cohort.cg3.cohort.add-child-fn]
> void Cohort::addChild(uint32_t child)

> [spec:cg3:sem:cohort.cg3.cohort.add-child-fn]
> Inserts `child` (a dependency-child cohort's global_number) into this
> cohort's `dep_children` sorted uint32 vector. The sorted-vector insert
> deduplicates, so it is a no-op when `child` is already present. The
> insert's return value is discarded (void). Does not touch the child
> cohort's own `dep_parent` — the caller wires that separately.

> [spec:cg3:def:cohort.cg3.cohort.add-relation-fn]
> bool Cohort::addRelation(uint32_t rel, uint32_t cohort)

> [spec:cg3:sem:cohort.cg3.cohort.add-relation-fn]
> Adds target cohort `cohort` (a global_number) under relation-name hash
> `rel`. Obtains a mutable reference to `relations[rel]` (default-creating
> an empty sorted uint32 vector if `rel` is not yet a key), records its
> size, inserts `cohort`, and returns true iff the size grew (i.e. it was
> newly added), false if it was already present. Additive: leaves any
> pre-existing targets of `rel` in place.

> [spec:cg3:def:cohort.cg3.cohort.allocate-append-reading-fn]
> Reading* Cohort::allocateAppendReading()

> [spec:cg3:sem:cohort.cg3.cohort.allocate-append-reading-fn]
> Allocates a fresh reading with `alloc_reading(this)` (owned by this
> cohort), push_back's it onto the member `readings` list, and — only if
> the new reading's `number == 0` — sets `number = UI32(readings.size() *
> 1000 + 1000)` where `readings.size()` is measured AFTER the push (first
> append → 2000, second → 3000, and so on). Clears the CT_NUM_CURRENT bit
> of `type` (invalidates the numeric min/max cache). Returns the new
> reading pointer. A sibling overload `allocateAppendReading(Reading& r)`
> is identical except it seeds the new reading via `alloc_reading(r)`
> (copy of `r`).

> [spec:cg3:def:cohort.cg3.cohort.append-reading-fn]
> void Cohort::appendReading(Reading* read, ReadingList& readings)

> [spec:cg3:sem:cohort.cg3.cohort.append-reading-fn]
> Appends `read` to the passed-in `readings` list (the parameter shadows
> the member of the same name, so this can target any ReadingList):
> push_back(read); then if `read->number == 0`, assign `read->number =
> UI32(readings.size() * 1000 + 1000)` with `readings.size()` taken AFTER
> the push (first append → 2000, second → 3000, ...). Clears the
> CT_NUM_CURRENT bit of `type` (marks the numeric min/max cache stale).
> The sibling 1-arg overload `appendReading(read)` simply forwards to this
> with the member `readings` list.

> [spec:cg3:def:cohort.cg3.cohort.clear-fn]
> void Cohort::clear()

> [spec:cg3:sem:cohort.cg3.cohort.clear-fn]
> Resets the cohort so it can be reused from the pool. If BOTH `parent`
> and `parent->parent` are non-null, erases this cohort's `global_number`
> key from the owning Window's `cohort_map` and `dep_window` maps (reached
> as SingleWindow `parent` → Window `parent->parent`). Calls `detach()` to
> unlink from the sibling chain. Then resets scalars: type=0,
> global_number=0, local_number=0, enclosed=0, wordform=null, dep_self=0,
> dep_parent=DEP_NO_PARENT, is_pleft=0, is_pright=0, parent=null,
> line_number=0. Clears containers text, wblank, num_max, num_min,
> dep_children, possible_sets, relations, relations_input. Frees every
> reading in `readings`, `deleted`, `delayed`, and `ignored` (calling
> `free_reading` on each) plus `free_reading(wread)`. Finally clears the
> `readings`, `deleted`, and `delayed` lists and sets wread=null. QUIRK:
> the `ignored` list is NOT cleared after its readings are freed, so it is
> left holding dangling pointers to freed/recycled readings. Also note
> `global_number` is read for the map erases BEFORE it is zeroed, so this
> ordering must be preserved.

> [spec:cg3:def:cohort.cg3.cohort.cohort-fn]
> Cohort::~Cohort()

> [spec:cg3:sem:cohort.cg3.cohort.cohort-fn]
> Destructor. Frees every owned Reading in order: iterates `readings`,
> then `deleted`, then `delayed`, then `ignored`, calling `free_reading`
> on each element, then `free_reading(wread)`. If `parent` is non-null,
> erases this cohort's `global_number` from the owning Window's
> `cohort_map` and `dep_window` (reached as `parent->parent`); unlike
> `clear()`, it does NOT null-check `parent->parent` here, assuming the
> Window is valid whenever `parent` is set. Finally calls `detach()` to
> unlink from the prev/next sibling chain. (The default constructor
> `Cohort(SingleWindow* p)` only sets `parent = p`.)

> [spec:cg3:def:cohort.cg3.cohort.detach-fn]
> void Cohort::detach()

> [spec:cg3:sem:cohort.cg3.cohort.detach-fn]
> Unlinks this cohort from the doubly-linked sibling chain: if `prev` is
> non-null set `prev->next = next`; if `next` is non-null set
> `next->prev = prev`; then set this cohort's own `prev` and `next` to
> null. Does not modify `parent` or any SingleWindow cohort vector.

> [spec:cg3:def:cohort.cg3.cohort.get-max-fn]
> double Cohort::getMax(uint32_t key)

> [spec:cg3:sem:cohort.cg3.cohort.get-max-fn]
> Calls `updateMinMax()` to refresh the cache, then returns
> `num_max[key]` if `key` is present in the `num_max` map, otherwise the
> constant NUMERIC_MAX (= 2^48 − 1 as a double, i.e. 281474976710655.0).

> [spec:cg3:def:cohort.cg3.cohort.get-min-fn]
> double Cohort::getMin(uint32_t key)

> [spec:cg3:sem:cohort.cg3.cohort.get-min-fn]
> Calls `updateMinMax()` to refresh the cache, then returns
> `num_min[key]` if `key` is present in the `num_min` map, otherwise the
> constant NUMERIC_MIN (= −(2^48) as a double, i.e. −281474976710656.0).

> [spec:cg3:def:cohort.cg3.cohort.num-t]
> typedef bc::flat_map<uint32_t, double> num_t

> [spec:cg3:def:cohort.cg3.cohort.rem-child-fn]
> void Cohort::remChild(uint32_t child)

> [spec:cg3:sem:cohort.cg3.cohort.rem-child-fn]
> Erases `child` (a global_number) from this cohort's `dep_children`
> sorted uint32 vector; no-op if it is not present.

> [spec:cg3:def:cohort.cg3.cohort.rem-relation-fn]
> bool Cohort::remRelation(uint32_t rel, uint32_t cohort)

> [spec:cg3:sem:cohort.cg3.cohort.rem-relation-fn]
> Removes target `cohort` from relation `rel`. Looks up `rel` in
> `relations`; if absent, returns false. Otherwise records the target
> set's size, erases `cohort` from it, and — if `rel` is also a key in
> `relations_input` — erases `cohort` from that set too. Returns true iff
> the `relations[rel]` set actually shrank (something was removed).

> [spec:cg3:def:cohort.cg3.cohort.set-related-fn]
> void Cohort::setRelated()

> [spec:cg3:sem:cohort.cg3.cohort.set-related-fn]
> Sets the CT_RELATED bit in `type`, then iterates every reading in
> `readings` and sets each reading's `noprint = false` (forcing them to be
> printed).

> [spec:cg3:def:cohort.cg3.cohort.set-relation-fn]
> bool Cohort::setRelation(uint32_t rel, uint32_t cohort)

> [spec:cg3:sem:cohort.cg3.cohort.set-relation-fn]
> Makes relation `rel` a single-target relation pointing only at
> `cohort`. First erases key `rel` from `relations_input`. Then obtains a
> mutable reference to `relations[rel]` (default-creating it if needed);
> if that set already has exactly one element and it equals `cohort`,
> returns false (no change). Otherwise clears the set, inserts only
> `cohort`, and returns true.

> [spec:cg3:def:cohort.cg3.cohort.unignore-all-fn]
> void unignoreAll()

> [spec:cg3:sem:cohort.cg3.cohort.unignore-all-fn]
> If the `ignored` list is non-empty: sets `deleted = false` on each
> ignored reading, then appends the entire `ignored` list onto the end of
> `readings` (readings.insert(readings.end(), ignored.begin(),
> ignored.end())), then clears `ignored`. Moves previously-ignored
> readings back into the active set. No-op when `ignored` is empty. Header
> inline method.

> [spec:cg3:def:cohort.cg3.cohort.update-min-max-fn]
> void Cohort::updateMinMax()

> [spec:cg3:sem:cohort.cg3.cohort.update-min-max-fn]
> Recomputes the per-comparison-hash numeric min/max cache. If the
> CT_NUM_CURRENT bit is already set in `type`, returns immediately (cache
> valid). Otherwise clears `num_min` and `num_max`, then for every reading
> in `readings` (only `readings` — not deleted/delayed/ignored) iterates
> that reading's `tags_numerical` map (a flat_map keyed by tag id whose
> values are `Tag*`). For each such tag, using key `tag->comparison_hash`:
> if the key is absent from `num_min` OR `tag->comparison_val` is strictly
> less than the stored value, store `comparison_val` as the new min; and,
> independently, if the key is absent from `num_max` OR `comparison_val`
> is strictly greater than the stored value, store it as the new max.
> Finally sets the CT_NUM_CURRENT bit. Private method.

> [spec:cg3:def:cohort.cg3.free-cohort-fn]
> void free_cohort(Cohort*& c)

> [spec:cg3:sem:cohort.cg3.free-cohort-fn]
> Returns a cohort to the pool. If `c` is null/0, returns immediately.
> Otherwise calls `pool_cohorts.put(c)` — which invokes `c->clear()`
> (resetting the cohort and unlinking it from the Window maps and sibling
> chain) and inserts the pointer into the pool's sorted free-list,
> silently ignoring the insert if the pointer is already present (the
> duplicate-detection throw is commented out). Then nulls the caller's
> pointer via the reference parameter (`c = 0`). The cohort is NOT
> deleted; it is retained for later reuse (the pool's own destructor
> deletes all pooled cohorts).

> [spec:cg3:def:cohort.cg3.relation-ctn]
> typedef bc::flat_map<uint32_t, uint32SortedVector> RelationCtn

> [spec:cg3:def:cohort.cg3.uint32-to-cohorts-map]
> typedef std::unordered_map<uint32_t, CohortSet> uint32ToCohortsMap

