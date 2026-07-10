# src/flat_unordered_map.hpp

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map]
> class flat_unordered_map {
>   class const_iterator { private: friend class flat_unordered_map; const flat_unordered_map* fus; size_t i; public: using iterator_category = std::bidirectiona...;
>   enum { DEFAULT_CAP = static_cast<size_type>(16u), };
>   size_type size_ = 0;
>   size_type deleted = 0;
>   container elements;
> }

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.assign-fn]
> void assign(It b, It e)

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.assign-fn]
> Replaces the whole contents with the iterator range [b, e). Calls
> clear() (resets to all-empty slots while preserving current capacity)
> then insert(b, e) — the range-insert overload, which pre-grows capacity
> to fit size_+distance(b,e) before inserting each pair. No return value.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.begin-fn]
> const_iterator begin() const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.begin-fn]
> Returns a const_iterator to the first live entry in physical slot order.
> If size_ == 0, returns end(). Otherwise scans slots i = 0 .. capacity()-1
> and returns const_iterator(*this, i) for the first slot whose key is
> neither res_empty nor res_del. If none is found (should not happen when
> size_ > 0), returns end(). Iteration order is thus slot order, not
> insertion order.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.capacity-fn]
> size_type capacity() const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.capacity-fn]
> Returns elements.size(), the number of physical table slots. Capacity is
> 0 for a fresh map and otherwise a power of two (starts at 16 and doubles
> on growth), which is what lets the code mask a hash into a slot with
> `& (capacity()-1)`. This is the slot count, not the live-entry count.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.clear-fn]
> void clear(size_type n = 0)

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.clear-fn]
> Empties the table, keeping capacity at least max(old capacity, n) (n
> defaults to 0). Steps exactly: set size_ = elements.size() (temporary use
> of size_ to hold the old capacity), resize elements to 0, then resize to
> std::max(size_, n) with every slot = make_pair(res_empty, V()), then set
> size_ = 0 and deleted = 0. Net effect: all slots become the empty
> sentinel; capacity becomes max(old capacity, n) — clear never shrinks
> below the old capacity, and clear() with default n=0 preserves it.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.const-iterator]
> class const_iterator {
>   const flat_unordered_map* fus;
>   size_t i;
> }

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.const-iterator.const-iterator-fn]
> const_iterator()

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.const-iterator.const-iterator-fn]
> Default constructor: initializes fus = nullptr and i = 0. This is the
> singular/past-the-end iterator value returned by end() and used as the
> not-found sentinel by find().

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.const-iterator.operator-fn]
> bool operator==(const const_iterator& o) const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.const-iterator.operator-fn]
> Equality operator==. Returns true iff both iterators reference the same
> container (fus == o.fus, plain pointer comparison, where nullptr==nullptr
> means both are end()) AND the same slot index (i == o.i). operator!= is
> defined as the negation of this.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.contains-fn]
> bool contains(T t) const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.contains-fn]
> Returns (find(t) != end()) as a bool — true iff a live entry with key t
> exists. Uses the const find(), so it does not mutate the table.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.count-fn]
> size_t count(T t) const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.count-fn]
> Returns (find(t) != end()) coerced to size_t — 0 if key t is absent, 1
> if present (keys are unique, so never more than 1). Uses the const find().

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.empty-fn]
> bool empty() const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.empty-fn]
> Returns (size_ == 0) — true when the map holds no live entries. Ignores
> tombstoned and empty slots (which are not counted in size_).

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.end-fn]
> const_iterator end() const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.end-fn]
> Returns a default-constructed const_iterator (fus = nullptr, i = 0), the
> past-the-end sentinel. Two end() iterators compare equal, and an advancing
> iterator becomes exactly this value when it runs past the last live slot.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.erase-fn]
> void erase(T t)

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.erase-fn]
> Removes the entry with key t (t must not equal res_empty or res_del;
> asserted in debug builds). If size_ == 0, returns immediately. Otherwise
> set max = capacity()-1, spot = hash_value(t) & max, then probe: while
> elements[spot].first is neither res_empty nor equal to t, advance
> spot = hash_value_sz(spot) & max. NOTE this stops only at an empty slot or
> a key match — res_del tombstones are skipped over, and the loop is
> unbounded. If elements[spot].first == t: mark it deleted by setting
> elements[spot].first = res_del and elements[spot].second = V(), then
> --size_. If that made size_ == 0 while deleted != 0, call clear() (a full
> reset that also drops all tombstones); otherwise ++deleted. If the key is
> not found, nothing changes. No return value.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.find-fn]
> const_iterator find(T t) const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.find-fn]
> Looks up key t (must not be res_empty/res_del; asserted). Starts with a
> default (end()) iterator. If size_ > 0: max = capacity()-1, spot =
> hash_value(t) & max, then loop for up to capacity()*4 iterations while
> elements[spot].first is neither res_empty nor equal to t, advancing
> spot = hash_value_sz(spot) & max each time. The capacity*4 iteration cap
> exists to avoid an infinite loop should the probe sequence cycle without
> reaching an empty slot; res_del tombstones are skipped (they are not stop
> conditions). After the loop, if elements[spot].first == t, set the result
> iterator's fus = this and i = spot; else leave it as end(). Returns that
> iterator. (The non-const overload first compacts tombstones by calling
> reserve(capacity()) when deleted && size_+deleted == capacity(), then
> delegates to this const find.)

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.hash-value-fn]
> size_type hash_value(T t) const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.hash-value-fn]
> Hashes a key: casts t to size_type and returns hash_value_sz(
> static_cast<size_type>(t)), i.e. applies the same LCG mix to the
> zero-extended key. For unsigned key types this is a plain widening
> (zero-extension) followed by the multiply-add.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.hash-value-sz-fn]
> size_type hash_value_sz(size_type t) const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.hash-value-sz-fn]
> Computes the probe hash of a slot index / key value t: returns
> t * 3663850746527583589 + 11210403176660999867, where both constants are
> unsigned 64-bit (ull) literals and the whole expression is evaluated in
> size_type (size_t) width with modular wraparound. This is a linear-
> congruential mix used both to seed the open-addressing probe (via
> hash_value) and to advance it (spot = hash_value_sz(spot) & max). PARITY:
> the Rust port must do wrapping multiply-then-add in the platform size_t
> width (typically u64) with these exact constants; results differ if a
> 32-bit size_t is used.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.insert-fn]
> size_t insert(const value_type& t)

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.insert-fn]
> Inserts pair t (t.first must not equal res_empty or res_del; asserted in
> debug). Returns the slot index size_t. Steps: (1) If deleted != 0 and
> size_ + deleted == capacity() (no empty slots remain, only live+tombstone),
> call reserve(capacity()) to compact tombstones in place. (2) Load-factor
> growth: if (size_+1)*3/2 >= capacity()/2 (all integer division), call
> reserve(std::max(size_type(16), capacity()*2)); on an initially empty
> table (capacity 0) this yields capacity 16. (3) max = capacity()-1; spot =
> hash_value(t.first) & max. (4) Probe: while elements[spot].first is neither
> res_empty nor equal to t.first, advance spot = hash_value_sz(spot) & max.
> NOTE the probe stops only at an empty slot or a matching key — res_del
> tombstones are skipped, so insert never reuses a tombstoned slot (they are
> reclaimed only by rehash/compaction); the loop is unbounded. (5) If
> elements[spot].first != t.first (landed on an empty slot, key not already
> present), store elements[spot] = t and ++size_. If the key was already
> present, the existing value is NOT overwritten. (6) Return spot (the slot,
> whether newly written or pre-existing).

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.iterator]
> typedef const_iterator iterator

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.reserve-fn]
> void reserve(size_type n)

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.reserve-fn]
> Allocates/rehashes the table for n slots. Two paths: (A) If size_ == 0:
> resize elements to n slots all = make_pair(res_empty, V()), set deleted =
> 0, and return — this is the initial-allocation path (and, since it just
> resizes, it can only grow the raw vector). (B) Otherwise (rehash): gather
> every live entry (key != res_empty and != res_del) into a thread_local
> static scratch vector `vals`; call clear(n) (which sets capacity =
> max(old capacity, n) and zeroes size_/deleted); set size_ = vals.size();
> then for each saved val, probe from spot = hash_value(val.first) & max
> (max = capacity()-1), advancing spot = hash_value_sz(spot) & max while the
> slot's key is neither res_empty nor equal to val.first, and store
> elements[spot] = val. This rebuild drops all tombstones. Because clear(n)
> uses max(old capacity, n), reserve(capacity()) compacts in place (same
> size) and reserve(capacity()*2) doubles. No return value.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.size-fn]
> size_type size() const

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.size-fn]
> Returns size_, the count of live entries (excludes both empty and
> tombstoned slots). Distinct from capacity(), which is the slot count.

> [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.swap-fn]
> void swap(flat_unordered_map& other)

> [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.swap-fn]
> Exchanges all state with `other` in O(1): std::swap of size_, std::swap of
> deleted, and elements.swap(other.elements) (vector buffer swap). No
> reallocation or rehashing occurs.

> [spec:cg3:def:flat-unordered-map.cg3.uint32-flat-hash-map]
> typedef flat_unordered_map<uint32_t, uint32_t> uint32FlatHashMap

