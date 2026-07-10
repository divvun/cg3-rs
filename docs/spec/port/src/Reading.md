# src/Reading.cpp, src/Reading.hpp

> [spec:cg3:def:reading.cg3.alloc-reading-fn]
> Reading* alloc_reading(const Reading& o)

> [spec:cg3:sem:reading.cg3.alloc-reading-fn]
> Obtains a `Reading*` that is a copy of `o`, preferring to recycle one from the
> thread-local `pool_readings` pool. Calls `pool_readings.get()`: if the pool is
> empty it returns nullptr, and the function returns `new Reading(o)` (the copy
> constructor). Otherwise it reuses the popped, already-cleared pooled object and
> manually copies fields from `o`:
> - `mapped`, `deleted`, `noprint` copied as-is;
> - `matched_target`, `matched_tests`, `immutable`, `active` all FORCED to
>   false;
> - `baseform`, `hash`, `hash_plain` copied;
> - `number = o.number + 100`;
> - the three blooms `tags_bloom`, `tags_plain_bloom`, `tags_textual_bloom`;
> - `mapping`, `parent`, `next` (raw pointer copies);
> - `hit_by`, `tags_list`, `tags`, `tags_plain`, `tags_textual`,
>   `tags_numerical`, `tags_string`, `tags_string_hash`.
> Then if `r->next` is non-null, deep-clones the chain: `r->next =
> alloc_reading(*r->next)`, recursing over the whole `next` list. Returns `r`.
> QUIRK/divergence: the pooled branch forces `immutable = false` and `active =
> false`, but the copy-constructor branch (taken when the pool is empty) COPIES
> `immutable` and `active` from `o`. Both branches set `number = o.number + 100`
> and deep-clone `next`.

> [spec:cg3:def:reading.cg3.free-reading-fn]
> void free_reading(Reading*& r)

> [spec:cg3:sem:reading.cg3.free-reading-fn]
> Returns a Reading to the pool and nulls the caller's pointer. If `r` is
> nullptr, returns immediately. Otherwise calls `pool_readings.put(r)` — which
> internally calls `r->clear()` (resetting the object and, via clear, recursively
> freeing its own `next` chain back into the pool) and inserts `r` into the
> pool's `sorted_vector` (insertion is a no-op if the pointer is somehow already
> present). Then sets the reference parameter `r = 0` (nullptr). Net effect: `r`
> and its entire `next` chain are recycled into the pool and the caller's pointer
> becomes null.

> [spec:cg3:def:reading.cg3.reading]
> class Reading {
>   uint8_t mapped : 1;
>   uint8_t deleted : 1;
>   uint8_t noprint : 1;
>   uint8_t matched_target : 1;
>   uint8_t matched_tests : 1;
>   uint8_t immutable : 1;
>   uint8_t active : 1;
>   uint32_t baseform = 0;
>   uint32_t hash = 0;
>   uint32_t hash_plain = 0;
>   uint32_t number = 0;
>   uint32Bloomish tags_bloom;
>   uint32Bloomish tags_plain_bloom;
>   uint32Bloomish tags_textual_bloom;
>   Tag* mapping = nullptr;
>   Cohort* parent = nullptr;
>   Reading* next = nullptr;
>   uint32Vector hit_by;
>   tags_list_t tags_list;
>   uint32SortedVector tags;
>   uint32SortedVector tags_plain;
>   uint32SortedVector tags_textual;
>   tags_numerical_t tags_numerical;
>   UString tags_string;
>   uint32_t tags_string_hash = 0;
>   Reading& operator=(const Reading& r);
> }

> [spec:cg3:def:reading.cg3.reading-list]
> typedef std::vector<Reading*> ReadingList

> [spec:cg3:def:reading.cg3.reading.allocate-reading-fn]
> Reading* Reading::allocateReading(Cohort* p)

> [spec:cg3:sem:reading.cg3.reading.allocate-reading-fn]
> Thin instance-method wrapper that returns `alloc_reading(p)` (the `Cohort*`
> free-function overload); it does NOT use `this`. `alloc_reading(p)` pops from
> the thread-local `pool_readings`: if the pool is empty it returns `new
> Reading(p)`; otherwise it reuses the popped (already-cleared) object, setting
> `number = p ? (p->readings.size() * 1000 + 1000) : 0` and `parent = p`, and
> returns it. Net effect: a blank Reading parented to `p`, with `number` staged
> from `p`'s current reading count (or 0 when `p` is null). (There is a sibling
> overload `allocateReading(const Reading&)` that instead delegates to
> `alloc_reading(const Reading&)` to produce a copy.)

> [spec:cg3:def:reading.cg3.reading.clear-fn]
> void Reading::clear()

> [spec:cg3:sem:reading.cg3.reading.clear-fn]
> Resets the Reading to a blank, reusable state. Sets all seven bitfields
> (`mapped`, `deleted`, `noprint`, `matched_target`, `matched_tests`,
> `immutable`, `active`) to false; sets `baseform = hash = hash_plain = number =
> 0`; clears the three blooms `tags_bloom`, `tags_plain_bloom`,
> `tags_textual_bloom` (each zeroing its 4 buckets); `mapping = nullptr`;
> `parent = nullptr`; calls `free_reading(next)` (which recursively returns the
> `next` chain to the pool and nulls the pointer) then redundantly sets `next =
> nullptr`; clears `hit_by`, `tags_list`, `tags`, `tags_plain`, `tags_textual`,
> `tags_numerical`, `tags_string`; sets `tags_string_hash = 0`.

> [spec:cg3:def:reading.cg3.reading.cmp-number-fn]
> bool Reading::cmp_number(Reading* a, Reading* b)

> [spec:cg3:sem:reading.cg3.reading.cmp-number-fn]
> Static strict-weak comparator over two `Reading*`. If `a->number ==
> b->number`, returns `a->hash < b->hash`; otherwise returns `a->number <
> b->number`. I.e. orders readings ascending by `number`, tie-broken ascending
> by `hash`.

> [spec:cg3:def:reading.cg3.reading.reading-fn]
> Reading::Reading(const Reading& r)

> [spec:cg3:sem:reading.cg3.reading.reading-fn]
> Copy constructor `Reading(const Reading& r)`. Initializer list: copies bitfields
> `mapped`, `deleted`, `noprint` from `r`; FORCES `matched_target = false` and
> `matched_tests = false`; copies `immutable` and `active` from `r`; copies
> `baseform`, `hash`, `hash_plain`; sets `number = r.number + 100`; copies the
> three blooms, `mapping`, `parent`, `next` (raw pointer), `hit_by`, `tags_list`,
> `tags`, `tags_plain`, `tags_textual`, `tags_numerical`, `tags_string`,
> `tags_string_hash`. Body: if `next` is non-null, deep-clones the chain via
> `next = allocateReading(*next)` (→ `alloc_reading(const Reading&)`, recursing
> down the list). Note: hashes are copied verbatim (not recomputed); `number` is
> bumped by 100 and the `matched_*` flags are cleared, but `immutable`/`active`
> are preserved. (Contrast `operator=`, which copies ALL fields including
> `matched_*` and leaves `number` unchanged, and does a shallow `next` pointer
> copy with no deep clone.)

> [spec:cg3:def:reading.cg3.reading.rehash-fn]
> uint32_t Reading::rehash()

> [spec:cg3:sem:reading.cg3.reading.rehash-fn]
> Recomputes and caches `hash` and `hash_plain` from `tags`, `mapping`, and the
> `next` chain; returns the new `hash`. Steps:
> 1. `hash = 0`, `hash_plain = 0`.
> 2. Iterate the sorted `tags` set (a `uint32SortedVector` of tag hashes) in
>    order; for each element `iter`, if there is no `mapping` OR `mapping->hash
>    != iter`, fold it in: `hash = hash_value(iter, hash)` (uint32 integer-mix
>    overload; the first call with `hash == 0` seeds CG3_HASH_SEED). This
>    excludes the mapping tag's own hash from the accumulation.
> 3. `hash_plain = hash` — snapshot after tags, before mapping/next.
> 4. If `mapping` is set, `hash = hash_value(mapping->hash, hash)`.
> 5. If `next` is set, recursively call `next->rehash()` and then `hash =
>    hash_value(next->hash, hash)`.
> 6. Return `hash`.

> [spec:cg3:def:reading.cg3.reading.tags-list-t]
> typedef uint32Vector tags_list_t

> [spec:cg3:def:reading.cg3.reading.tags-numerical-t]
> typedef bc::flat_map<uint32_t, Tag*> tags_numerical_t

