//! Port of `src/flat_unordered_map.hpp` — an open-addressing hash map with
//! sentinel keys (`res_empty = T(-1)`, `res_del = T(-1)-1`).
//!
//! Wave 2: a literal, bug-for-bug 1:1 translation. The probing, sentinel
//! handling, load-factor / rehash triggers, and the
//! `size_ + deleted == capacity()` compaction guard are reproduced exactly,
//! **including** the faithfully-preserved quirks:
//!   - the unbounded `insert`/`erase`/`reserve` probe loops that skip
//!     `res_del` tombstones (never reusing a tombstoned slot);
//!   - `find`'s `capacity()*4` iteration cap;
//!   - iterator `operator--` not validating slot 0;
//!   - `insert` never overwriting an existing key's value.
//!
//! The C++ template `flat_unordered_map<T, V, res_empty = T(-1),
//! res_del = T(-1)-1>` becomes a generic struct whose key type `K` supplies
//! the two sentinels (and a widening `as_size`) through the [`Sentinel`]
//! trait, matching the C++ non-type template parameters.

// The C++ private members `hash_value_sz`/`hash_value` are the LCG probe
// hasher (`t * 3663850746527583589 + 11210403176660999867`), defined locally —
// `crate::inlines::hash_value_sz` is a DIFFERENT function (the 65599 mixer)
// and must not be used here (Wave 3 fix: delegating to it changed the probe
// sequence and physical slot / iteration order vs the C++).

/// Trait carrying the two reserved sentinel key values and the widening cast
/// the C++ code performs with `static_cast<size_type>(t)`.
///
/// In C++ these are the non-type template parameters `res_empty = T(-1)` and
/// `res_del = T(-1)-1`; here they are associated constants so the generic map
/// can name them without an extra runtime field. `as_size` reproduces the
/// `static_cast<size_type>(t)` (zero-extension for unsigned keys) used by
/// `hash_value`.
pub trait Sentinel: Copy + PartialEq {
    /// `res_empty` == `T(-1)`.
    const EMPTY: Self;
    /// `res_del` == `T(-1) - 1`.
    const DEL: Self;
    /// `static_cast<size_type>(self)`.
    fn as_size(self) -> usize;
}

impl Sentinel for u32 {
    const EMPTY: u32 = u32::MAX; // T(-1)
    const DEL: u32 = u32::MAX - 1; // T(-1) - 1
    fn as_size(self) -> usize {
        self as usize
    }
}

// [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map]
/// Open-addressing hash map. `size_` counts live entries (excludes empty and
/// tombstoned slots), `deleted` counts tombstones, `elements` is the physical
/// slot table (a slot's key is `Sentinel::EMPTY`, `Sentinel::DEL`, or a live
/// key).
pub struct FlatUnorderedMap<K, V> {
    size_: usize,
    deleted: usize,
    elements: Vec<(K, V)>,
}

impl<K, V> Default for FlatUnorderedMap<K, V> {
    fn default() -> Self {
        FlatUnorderedMap {
            size_: 0,
            deleted: 0,
            elements: Vec::new(),
        }
    }
}

// [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.const-iterator]
/// Bidirectional const iterator over live slots in physical (slot) order.
///
/// `fus == None` is the C++ `nullptr` (the singular / past-the-end value).
pub struct ConstIterator<'a, K, V> {
    fus: Option<&'a FlatUnorderedMap<K, V>>,
    i: usize,
}

// Copy/Clone mirror the C++ trivially-copyable iterator (copy ctor + copy
// assignment). Manual impls avoid spurious `K: Clone`/`V: Clone` bounds.
impl<'a, K, V> Clone for ConstIterator<'a, K, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, K, V> Copy for ConstIterator<'a, K, V> {}

/// Wave 4: the C++-style cursor is also a real [`Iterator`] (yields the
/// current `(key, value)` pair, then advances via the `pre_increment` walk).
impl<'a, K: Sentinel, V: Copy> Iterator for ConstIterator<'a, K, V> {
    type Item = (K, V);
    fn next(&mut self) -> Option<(K, V)> {
        let fum = self.fus?;
        let cur = fum.elements[self.i];
        self.pre_increment();
        Some(cur)
    }
}

impl<'a, K, V> Default for ConstIterator<'a, K, V> {
    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.const-iterator.const-iterator-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.const-iterator.const-iterator-fn]
    fn default() -> Self {
        ConstIterator { fus: None, i: 0 }
    }
}

// [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.const-iterator.operator-fn]
// [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.const-iterator.operator-fn]
impl<'a, K, V> PartialEq for ConstIterator<'a, K, V> {
    fn eq(&self, o: &Self) -> bool {
        // fus == o.fus is plain pointer comparison (nullptr == nullptr for two
        // end() iterators) && i == o.i.
        let a = self.fus.map(|r| r as *const FlatUnorderedMap<K, V>);
        let b = o.fus.map(|r| r as *const FlatUnorderedMap<K, V>);
        a == b && self.i == o.i
    }
}

impl<'a, K: Sentinel, V> ConstIterator<'a, K, V> {
    // Non-default constructor `const_iterator(const flat_unordered_map&,
    // size_t)`. Unannotated in the C++ header.
    fn new(fus: &'a FlatUnorderedMap<K, V>, i: usize) -> Self {
        ConstIterator { fus: Some(fus), i }
    }

    /// `operator++()`: advance to the next live slot, or become end()
    /// (`fus = None`, `i = 0`) once past the last live slot.
    pub fn pre_increment(&mut self) -> &mut Self {
        let fus = self.fus.unwrap();
        self.i += 1;
        while self.i < fus.capacity() {
            if fus.elements[self.i].0 != K::EMPTY && fus.elements[self.i].0 != K::DEL {
                break;
            }
            self.i += 1;
        }
        if self.i >= fus.capacity() {
            self.fus = None;
            self.i = 0;
        }
        self
    }

    /// `operator--()`: retreat to the previous live slot. QUIRK (faithful):
    /// the scan `for (--i; i > 0; --i)` never validates slot 0 — reaching
    /// `i == 0` in the loop stops without checking whether slot 0 is live.
    pub fn pre_decrement(&mut self) -> &mut Self {
        let fus = self.fus.unwrap();
        if self.i == 0 {
            self.fus = None;
            self.i = 0;
        } else {
            self.i -= 1;
            while self.i > 0 {
                if fus.elements[self.i].0 != K::EMPTY && fus.elements[self.i].0 != K::DEL {
                    break;
                }
                self.i -= 1;
            }
        }
        self
    }

    /// `operator*()` — the referenced live `(key, value)` slot.
    pub fn get(&self) -> &'a (K, V) {
        &self.fus.unwrap().elements[self.i]
    }
}

// [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.iterator]
/// `typedef const_iterator iterator;`
pub type Iter<'a, K, V> = ConstIterator<'a, K, V>;

// Read-only surface: needs only `K: Sentinel` (no `V` bounds).
impl<K: Sentinel, V> FlatUnorderedMap<K, V> {
    const DEFAULT_CAP: usize = 16;

    /// Empty map (C++ default constructor).
    pub fn new() -> Self {
        FlatUnorderedMap::default()
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.hash-value-sz-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.hash-value-sz-fn]
    // LCG probe hash `t * 3663850746527583589 + 11210403176660999867` in
    // size_t (usize) width with modular wraparound — the container's OWN
    // private member, NOT `crate::inlines::hash_value_sz` (that is the 65599
    // mixer; delegating to it produced a fixed-stride probe and a different
    // physical slot / iteration order than the C++).
    fn hash_value_sz(&self, t: usize) -> usize {
        t.wrapping_mul(3663850746527583589)
            .wrapping_add(11210403176660999867)
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.hash-value-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.hash-value-fn]
    // Widen the key (`static_cast<size_type>(t)`) then apply the LCG mix.
    fn hash_value(&self, t: K) -> usize {
        self.hash_value_sz(t.as_size())
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.size-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.size-fn]
    pub fn size(&self) -> usize {
        self.size_
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.capacity-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.capacity-fn]
    pub fn capacity(&self) -> usize {
        self.elements.len()
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.empty-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.empty-fn]
    pub fn empty(&self) -> bool {
        self.size_ == 0
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.end-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.end-fn]
    pub fn end(&self) -> ConstIterator<'_, K, V> {
        ConstIterator::default()
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.begin-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.begin-fn]
    /// Iterate the live `(key, value)` pairs (wave 4 — the idiomatic
    /// replacement for the C++ `begin()`/`pre_increment()`/`end()` walk).
    pub fn iter(&self) -> impl Iterator<Item = &(K, V)> + '_ {
        self.elements
            .iter()
            .filter(|e| e.0 != K::EMPTY && e.0 != K::DEL)
    }

    pub fn begin(&self) -> ConstIterator<'_, K, V> {
        if self.size_ == 0 {
            return self.end();
        }
        let ie = self.capacity();
        let mut i = 0;
        while i < ie {
            if self.elements[i].0 != K::EMPTY && self.elements[i].0 != K::DEL {
                return ConstIterator::new(self, i);
            }
            i += 1;
        }
        self.end()
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.find-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.find-fn]
    // The `const` overload. `res_del` tombstones are skipped (not stop
    // conditions); the `capacity()*4` cap guards against a cycling probe.
    pub fn find(&self, t: K) -> ConstIterator<'_, K, V> {
        debug_assert!(
            t != K::EMPTY && t != K::DEL,
            "Key cannot be res_empty or res_del!"
        );

        let mut it = ConstIterator::default();

        if self.size_ != 0 {
            let max = self.capacity() - 1;
            let mut spot = self.hash_value(t) & max;
            let mut i = 0;
            while i < self.capacity() * 4
                && self.elements[spot].0 != K::EMPTY
                && self.elements[spot].0 != t
            {
                spot = self.hash_value_sz(spot) & max;
                i += 1;
            }
            if self.elements[spot].0 == t {
                it.fus = Some(self);
                it.i = spot;
            }
        }

        it
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.count-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.count-fn]
    pub fn count(&self, t: K) -> usize {
        (self.find(t) != self.end()) as usize
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.contains-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.contains-fn]
    pub fn contains(&self, t: K) -> bool {
        self.find(t) != self.end()
    }
}

// Mutating surface: the C++ code default-constructs (`V()`) and copies values,
// so `V: Default + Clone`.
impl<K: Sentinel, V: Default + Clone> FlatUnorderedMap<K, V> {
    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.insert-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.insert-fn]
    pub fn insert(&mut self, t: (K, V)) -> usize {
        debug_assert!(
            t.0 != K::EMPTY && t.0 != K::DEL,
            "Key cannot be res_empty or res_del!"
        );

        // (1) Compact tombstones in place when no empty slots remain.
        if self.deleted != 0 && self.size_ + self.deleted == self.capacity() {
            self.reserve(self.capacity());
        }

        // (2) Load-factor growth (integer division throughout).
        if (self.size_ + 1) * 3 / 2 >= self.capacity() / 2 {
            self.reserve(std::cmp::max(Self::DEFAULT_CAP, self.capacity() * 2));
        }
        let max = self.capacity() - 1;
        let mut spot = self.hash_value(t.0) & max;
        // (4) Probe — skips res_del tombstones; unbounded.
        while self.elements[spot].0 != K::EMPTY && self.elements[spot].0 != t.0 {
            spot = self.hash_value_sz(spot) & max;
        }
        // (5) Only write when the key was not already present (never
        // overwrites an existing value).
        if self.elements[spot].0 != t.0 {
            self.elements[spot] = t;
            self.size_ += 1;
        }
        spot
    }

    // Range-insert overload `insert(It b, It e)`. Pre-grows capacity to fit
    // `size_ + distance(b, e)` before inserting each pair. Unannotated in the
    // C++ header. Requires a known length (C++ uses `std::distance`).
    pub fn insert_range<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
        I::IntoIter: ExactSizeIterator,
    {
        let it = iter.into_iter();
        let d = it.len();
        let mut c = self.capacity();
        while (self.size_ + d) * 3 / 2 >= c / 2 {
            c = std::cmp::max(Self::DEFAULT_CAP, c * 2);
        }
        if c != self.capacity() {
            self.reserve(c);
        }

        for item in it {
            self.insert(item);
        }
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.erase-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.erase-fn]
    pub fn erase(&mut self, t: K) {
        debug_assert!(
            t != K::EMPTY && t != K::DEL,
            "Key cannot be res_empty or res_del!"
        );

        if self.size_ == 0 {
            return;
        }
        let max = self.capacity() - 1;
        let mut spot = self.hash_value(t) & max;
        // Probe — skips res_del tombstones; unbounded.
        while self.elements[spot].0 != K::EMPTY && self.elements[spot].0 != t {
            spot = self.hash_value_sz(spot) & max;
        }
        if self.elements[spot].0 == t {
            self.elements[spot].0 = K::DEL;
            self.elements[spot].1 = V::default();
            self.size_ -= 1;
            if self.size_ == 0 && self.deleted != 0 {
                self.clear(0);
            } else {
                self.deleted += 1;
            }
        }
    }

    // `const_iterator erase(const_iterator)`. Because a `ConstIterator`
    // borrows the map immutably, it cannot be passed by value into a
    // `&mut self` method; the erase target is taken as the slot index `i`
    // (the iterator's `.i`). Unannotated in the C++ header.
    pub fn erase_iter(&mut self, mut i: usize) -> ConstIterator<'_, K, V> {
        self.elements[i].0 = K::DEL;
        self.elements[i].1 = V::default();
        // ++it (operator++): advance to the next live slot.
        i += 1;
        while i < self.capacity() {
            if self.elements[i].0 != K::EMPTY && self.elements[i].0 != K::DEL {
                break;
            }
            i += 1;
        }
        let past_end = i >= self.capacity();
        self.size_ -= 1;
        if self.size_ == 0 && self.deleted != 0 {
            self.clear(0);
            return self.end();
        } else {
            self.deleted += 1;
        }
        if past_end {
            self.end()
        } else {
            ConstIterator::new(self, i)
        }
    }

    // The non-`const` `find` overload: compacts tombstones first, then
    // delegates to the const `find`. Named `find_mut` (Rust cannot overload
    // on receiver mutability).
    pub fn find_mut(&mut self, t: K) -> ConstIterator<'_, K, V> {
        if self.deleted != 0 && self.size_ + self.deleted == self.capacity() {
            self.reserve(self.capacity());
        }
        self.find(t)
    }

    // `V& operator[](const T&)`: returns a mutable reference to the value for
    // `t`, inserting a default-constructed value if absent. Unannotated in the
    // C++ header.
    pub fn index_or_insert(&mut self, t: K) -> &mut V {
        debug_assert!(
            t != K::EMPTY && t != K::DEL,
            "Key cannot be res_empty or res_del!"
        );

        if self.deleted != 0 && self.size_ + self.deleted == self.capacity() {
            self.reserve(self.capacity());
        }

        let mut at = usize::MAX;
        if self.size_ != 0 {
            let max = self.capacity() - 1;
            let mut spot = self.hash_value(t) & max;
            while self.elements[spot].0 != K::EMPTY && self.elements[spot].0 != t {
                spot = self.hash_value_sz(spot) & max;
            }
            if self.elements[spot].0 == t {
                at = spot;
            }
        }
        if at == usize::MAX {
            at = self.insert((t, V::default()));
        }

        &mut self.elements[at].1
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.reserve-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.reserve-fn]
    pub fn reserve(&mut self, n: usize) {
        // (A) Initial-allocation path (raw resize — can only grow).
        if self.size_ == 0 {
            self.elements.resize(n, (K::EMPTY, V::default()));
            self.deleted = 0;
            return;
        }

        // (B) Rehash. The C++ uses a `thread_local static` scratch vector for
        // reuse; a local vector is behaviourally identical.
        let mut vals: Vec<(K, V)> = Vec::with_capacity(self.size_);
        for elem in &self.elements {
            if elem.0 != K::EMPTY && elem.0 != K::DEL {
                vals.push(elem.clone());
            }
        }

        self.clear(n);
        self.size_ = vals.len();
        let max = self.capacity() - 1;
        for val in &vals {
            let mut spot = self.hash_value(val.0) & max;
            while self.elements[spot].0 != K::EMPTY && self.elements[spot].0 != val.0 {
                spot = self.hash_value_sz(spot) & max;
            }
            self.elements[spot] = val.clone();
        }
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.assign-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.assign-fn]
    pub fn assign<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (K, V)>,
        I::IntoIter: ExactSizeIterator,
    {
        self.clear(0);
        self.insert_range(iter);
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.swap-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.swap-fn]
    pub fn swap(&mut self, other: &mut FlatUnorderedMap<K, V>) {
        std::mem::swap(&mut self.size_, &mut other.size_);
        std::mem::swap(&mut self.deleted, &mut other.deleted);
        std::mem::swap(&mut self.elements, &mut other.elements);
    }

    // [spec:cg3:def:flat-unordered-map.cg3.flat-unordered-map.clear-fn]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.clear-fn]
    pub fn clear(&mut self, n: usize) {
        self.size_ = self.elements.len(); // temporarily holds the old capacity
        self.elements.resize(0, (K::EMPTY, V::default()));
        self.elements
            .resize(std::cmp::max(self.size_, n), (K::EMPTY, V::default()));
        self.size_ = 0;
        self.deleted = 0;
    }

    // `container& get()` — access to the raw slot table. Unannotated.
    pub fn get(&mut self) -> &mut Vec<(K, V)> {
        &mut self.elements
    }
}

// [spec:cg3:def:flat-unordered-map.cg3.uint32-flat-hash-map]
/// `typedef flat_unordered_map<uint32_t, uint32_t> uint32FlatHashMap;`
pub type Uint32FlatHashMap = FlatUnorderedMap<u32, u32>;

#[cfg(test)]
mod tests {
    use super::*;

    // Collect the live (key, value) pairs by driving the container's own
    // begin()/end() and the iterator operator++ (pre_increment).
    fn collect(m: &Uint32FlatHashMap) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut it = m.begin();
        let end = m.end();
        while it != end {
            out.push(*it.get());
            it.pre_increment();
        }
        out.sort();
        out
    }

    // The private LCG probe hashers. `hash_value_sz` is the container's OWN
    // member `t*3663850746527583589 + 11210403176660999867` (usize wraparound)
    // and MUST differ from `crate::inlines::hash_value_sz` (the 65599 mixer).
    // `hash_value` widens the key via `static_cast<size_type>` then mixes.
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.hash-value-sz-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.hash-value-fn/test]
    #[test]
    fn lcg_probe_hash_formula() {
        let m: Uint32FlatHashMap = FlatUnorderedMap::new();
        // Exact LCG constants, in usize width with wrapping.
        let expect = |t: usize| {
            t.wrapping_mul(3663850746527583589usize)
                .wrapping_add(11210403176660999867usize)
        };
        assert_eq!(m.hash_value_sz(0), expect(0));
        assert_eq!(m.hash_value_sz(1), expect(1));
        assert_eq!(m.hash_value_sz(1234567), expect(1234567));
        // hash_value(key) == hash_value_sz(key as usize) (zero-extension).
        assert_eq!(m.hash_value(7u32), m.hash_value_sz(7));
        assert_eq!(m.hash_value(42u32), expect(42));

        // Bug-for-bug: this is NOT the crate 65599 mixer. Show they diverge
        // (the whole point of the Wave-3 fix note in the module docs).
        let mixer = crate::inlines::hash_value_sz(0x41, 0);
        assert_ne!(
            m.hash_value_sz(0x41),
            mixer,
            "container LCG must differ from inlines mixer"
        );
    }

    // Default ctor + insert + find/count/contains, begin/end iteration,
    // size/capacity/empty, and the default (end) ConstIterator equality
    // (operator== over nullptr + index). insert never overwrites an existing
    // key's value (documented quirk).
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.insert-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.find-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.count-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.contains-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.begin-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.end-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.size-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.capacity-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.empty-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.const-iterator.const-iterator-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.const-iterator.operator-fn/test]
    #[test]
    fn insert_find_iterate() {
        let mut m: Uint32FlatHashMap = FlatUnorderedMap::new();
        assert!(m.empty());
        assert_eq!(m.size(), 0);
        assert_eq!(m.capacity(), 0);
        // Two default (end) iterators compare equal (nullptr == nullptr, i==i).
        assert!(m.begin() == m.end());
        assert!(m.end() == ConstIterator::default());

        m.insert((10, 100));
        m.insert((20, 200));
        m.insert((30, 300));
        assert!(!m.empty());
        assert_eq!(m.size(), 3);
        assert!(m.capacity() >= 3); // grew to DEFAULT_CAP (16)

        // find lands on the live slot; contains/count agree.
        let it = m.find(20);
        assert!(it != m.end());
        assert_eq!(*it.get(), (20, 200));
        assert!(m.contains(30));
        assert_eq!(m.count(30), 1);
        // Absent key -> find == end(), count 0.
        assert!(m.find(99) == m.end());
        assert!(!m.contains(99));
        assert_eq!(m.count(99), 0);

        // insert never overwrites an existing key's value (documented quirk).
        m.insert((20, 999));
        assert_eq!(*m.find(20).get(), (20, 200));
        assert_eq!(m.size(), 3);

        // begin/end iteration recovers exactly the live pairs.
        assert_eq!(collect(&m), vec![(10, 100), (20, 200), (30, 300)]);
    }

    // erase tombstones a slot (find can no longer see it), reserve rehashes,
    // clear resets, swap exchanges whole state, assign clears + range-inserts.
    // Also confirms erasing the last live element with tombstones present
    // triggers the internal clear (deleted reset).
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.erase-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.reserve-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.clear-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.swap-fn/test]
    // [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.assign-fn/test]
    #[test]
    fn erase_reserve_clear_swap_assign() {
        let mut m: Uint32FlatHashMap = FlatUnorderedMap::new();
        for k in 1u32..=5 {
            m.insert((k, k * 10));
        }
        assert_eq!(m.size(), 5);

        // erase removes a key (leaves a tombstone) — find no longer sees it.
        m.erase(3);
        assert_eq!(m.size(), 4);
        assert!(m.find(3) == m.end());
        assert!(m.contains(4)); // probe still finds keys past the tombstone
        // Erasing an absent key is a no-op.
        m.erase(99);
        assert_eq!(m.size(), 4);

        // reserve rehashes into a larger table, preserving all live pairs.
        let before = collect(&m);
        m.reserve(64);
        assert!(m.capacity() >= 64);
        assert_eq!(m.size(), 4);
        assert_eq!(collect(&m), before);

        // swap exchanges the entire state of two maps.
        let mut other: Uint32FlatHashMap = FlatUnorderedMap::new();
        other.insert((100, 1));
        m.swap(&mut other);
        assert_eq!(collect(&m), vec![(100, 1)]);
        assert_eq!(other.size(), 4);
        assert_eq!(collect(&other), before);

        // assign clears then range-inserts the supplied pairs.
        m.assign([(7u32, 70u32), (8, 80), (7, 700)]);
        // Duplicate key 7 keeps its first value (insert never overwrites).
        assert_eq!(collect(&m), vec![(7, 70), (8, 80)]);

        // clear(0) empties the map.
        m.clear(0);
        assert!(m.empty());
        assert_eq!(m.size(), 0);
        assert!(m.begin() == m.end());

        // Erasing the last live element while a tombstone exists triggers the
        // internal clear() (size and deleted both reset to 0 cleanly).
        let mut t: Uint32FlatHashMap = FlatUnorderedMap::new();
        t.insert((1, 1));
        t.insert((2, 2));
        t.erase(1); // leaves a tombstone, size 1
        t.erase(2); // last live element removed with a tombstone present
        assert_eq!(t.size(), 0);
        assert!(t.empty());
        // After the internal compaction a fresh insert works normally.
        t.insert((5, 5));
        assert_eq!(t.size(), 1);
        assert!(t.contains(5));
    }
}
