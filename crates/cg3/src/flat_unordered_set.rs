//! Port of `src/flat_unordered_set.hpp` — an open-addressing hash set with
//! sentinel values (`res_empty = T(-1)`, `res_del = T(-1)-1`), built on the
//! same design as [`crate::flat_unordered_map`].
//!
//! Wave 2: a literal, bug-for-bug 1:1 translation. The probing, sentinel
//! handling, load-factor / rehash triggers, and the
//! `size_ + deleted == capacity()` compaction guard are reproduced exactly,
//! **including** the faithfully-preserved quirks:
//!   - the unbounded `insert`/`erase`/`reserve` probe loops that skip
//!     `res_del` tombstones (never reusing a tombstoned slot);
//!   - `find`'s `capacity()*4` iteration cap;
//!   - iterator `operator--` not validating slot 0;
//!   - `insert` never rewriting an already-present value.
//!
//! The C++ template `flat_unordered_set<T, res_empty = T(-1),
//! res_del = T(-1)-1>` becomes a generic struct whose element type `T`
//! supplies the two sentinels (and a widening `as_size`) through the
//! [`Sentinel`] trait, matching the C++ non-type template parameters.

// The C++ private members `hash_value_sz`/`hash_value` are the LCG probe
// hasher (`t * 3663850746527583589 + 11210403176660999867`), defined locally —
// `crate::inlines::hash_value_sz` is a DIFFERENT function (the 65599 mixer)
// and must not be used here (Wave 3 fix: delegating to it changed the probe
// sequence and physical slot / iteration order vs the C++).

/// Trait carrying the two reserved sentinel values and the widening cast the
/// C++ code performs with `static_cast<size_type>(t)`.
///
/// In C++ these are the non-type template parameters `res_empty = T(-1)` and
/// `res_del = T(-1)-1`; here they are associated constants. `as_size`
/// reproduces the `static_cast<size_type>(t)` (zero-extension for unsigned
/// values) used by `hash_value`.
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

impl Sentinel for u64 {
    const EMPTY: u64 = u64::MAX; // T(-1)
    const DEL: u64 = u64::MAX - 1; // T(-1) - 1
    fn as_size(self) -> usize {
        self as usize
    }
}

// [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set]
/// Open-addressing hash set. `size_` counts live values (excludes empty and
/// tombstoned slots), `deleted` counts tombstones, `elements` is the physical
/// slot table (a slot is `Sentinel::EMPTY`, `Sentinel::DEL`, or a live value).
pub struct FlatUnorderedSet<T> {
    size_: usize,
    deleted: usize,
    elements: Vec<T>,
}

impl<T> Default for FlatUnorderedSet<T> {
    fn default() -> Self {
        FlatUnorderedSet {
            size_: 0,
            deleted: 0,
            elements: Vec::new(),
        }
    }
}

// [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.const-iterator]
/// Bidirectional const iterator over live slots in physical (slot) order.
///
/// `fus == None` is the C++ `nullptr` (the singular / past-the-end value).
pub struct ConstIterator<'a, T> {
    fus: Option<&'a FlatUnorderedSet<T>>,
    i: usize,
}

// Copy/Clone mirror the C++ trivially-copyable iterator (copy ctor + copy
// assignment). Manual impls avoid a spurious `T: Clone` bound.
impl<'a, T> Clone for ConstIterator<'a, T> {
    fn clone(&self) -> Self {
        ConstIterator {
            fus: self.fus,
            i: self.i,
        }
    }
}

impl<'a, T> Copy for ConstIterator<'a, T> {}

impl<'a, T> Default for ConstIterator<'a, T> {
    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.const-iterator.const-iterator-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.const-iterator.const-iterator-fn]
    fn default() -> Self {
        ConstIterator { fus: None, i: 0 }
    }
}

// The set's `operator==` is unannotated in the C++ header (unlike the map's).
impl<'a, T> PartialEq for ConstIterator<'a, T> {
    fn eq(&self, o: &Self) -> bool {
        // fus == o.fus is plain pointer comparison (nullptr == nullptr for two
        // end() iterators) && i == o.i.
        let a = self.fus.map(|r| r as *const FlatUnorderedSet<T>);
        let b = o.fus.map(|r| r as *const FlatUnorderedSet<T>);
        a == b && self.i == o.i
    }
}

impl<'a, T: Sentinel> ConstIterator<'a, T> {
    // Non-default constructor `const_iterator(const flat_unordered_set&,
    // size_t)`. Unannotated in the C++ header.
    fn new(fus: &'a FlatUnorderedSet<T>, i: usize) -> Self {
        ConstIterator { fus: Some(fus), i }
    }

    /// `operator++()`: advance to the next live slot, or become end()
    /// (`fus = None`, `i = 0`) once past the last live slot.
    pub fn pre_increment(&mut self) -> &mut Self {
        let fus = self.fus.unwrap();
        self.i += 1;
        while self.i < fus.capacity() {
            if fus.elements[self.i] != T::EMPTY && fus.elements[self.i] != T::DEL {
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

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.const-iterator.operator-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.const-iterator.operator-fn]
    /// `operator++(int)`: post-increment. Copies self, applies the
    /// pre-increment to `*self`, and returns the pre-advance copy.
    pub fn post_increment(&mut self) -> Self {
        let tmp = *self;
        self.pre_increment();
        tmp
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
                if fus.elements[self.i] != T::EMPTY && fus.elements[self.i] != T::DEL {
                    break;
                }
                self.i -= 1;
            }
        }
        self
    }

    /// `operator*()` — the referenced live value (returned by value, as in
    /// C++: `T operator*() const`).
    pub fn get(&self) -> T {
        self.fus.unwrap().elements[self.i]
    }
}

/// `using iterator = const_iterator;` (unannotated in the C++ header).
pub type Iter<'a, T> = ConstIterator<'a, T>;

impl<T: Sentinel> FlatUnorderedSet<T> {
    const DEFAULT_CAP: usize = 16;

    /// Empty set (C++ default constructor).
    pub fn new() -> Self {
        FlatUnorderedSet::default()
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.hash-value-sz-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.hash-value-sz-fn]
    // LCG probe hash `t * 3663850746527583589 + 11210403176660999867` in
    // size_t (usize) width with modular wraparound — the container's OWN
    // private member, NOT `crate::inlines::hash_value_sz` (that is the 65599
    // mixer; delegating to it produced a fixed-stride probe and a different
    // physical slot / iteration order than the C++).
    fn hash_value_sz(&self, t: usize) -> usize {
        t.wrapping_mul(3663850746527583589)
            .wrapping_add(11210403176660999867)
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.hash-value-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.hash-value-fn]
    // Widen the value (`static_cast<size_type>(t)`) then apply the LCG mix.
    fn hash_value(&self, t: T) -> usize {
        self.hash_value_sz(t.as_size())
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.insert-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.insert-fn]
    pub fn insert(&mut self, t: T) {
        debug_assert!(
            t != T::EMPTY && t != T::DEL,
            "Value cannot be res_empty or res_del!"
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
        let mut spot = self.hash_value(t) & max;
        // (4) Probe — skips res_del tombstones; unbounded.
        while self.elements[spot] != T::EMPTY && self.elements[spot] != t {
            spot = self.hash_value_sz(spot) & max;
        }
        // (5) Only write when the value was not already present.
        if self.elements[spot] != t {
            self.elements[spot] = t;
            self.size_ += 1;
        }
    }

    // Range-insert overload `insert(It b, It e)`. Pre-grows capacity to fit
    // `size_ + distance(b, e)` before inserting each value. Unannotated in the
    // C++ header. Requires a known length (C++ uses `std::distance`).
    pub fn insert_range<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
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

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.erase-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.erase-fn]
    pub fn erase(&mut self, t: T) {
        debug_assert!(
            t != T::EMPTY && t != T::DEL,
            "Value cannot be res_empty or res_del!"
        );

        if self.size_ == 0 {
            return;
        }
        let max = self.capacity() - 1;
        let mut spot = self.hash_value(t) & max;
        // Probe — skips res_del tombstones; unbounded.
        while self.elements[spot] != T::EMPTY && self.elements[spot] != t {
            spot = self.hash_value_sz(spot) & max;
        }
        if self.elements[spot] == t {
            self.elements[spot] = T::DEL;
            self.size_ -= 1;
            if self.size_ == 0 && self.deleted != 0 {
                self.clear(0);
            } else {
                self.deleted += 1;
            }
        }
    }

    // `const_iterator erase(const_iterator)`. Because a `ConstIterator`
    // borrows the set immutably, it cannot be passed by value into a
    // `&mut self` method; the erase target is taken as the slot index `i`
    // (the iterator's `.i`). Unannotated in the C++ header.
    pub fn erase_iter(&mut self, mut i: usize) -> ConstIterator<'_, T> {
        self.elements[i] = T::DEL;
        // ++it (operator++): advance to the next live slot.
        i += 1;
        while i < self.capacity() {
            if self.elements[i] != T::EMPTY && self.elements[i] != T::DEL {
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

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.find-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.find-fn]
    // The `const` overload. `res_del` tombstones are skipped (not stop
    // conditions); the `capacity()*4` cap guards against a cycling probe.
    pub fn find(&self, t: T) -> ConstIterator<'_, T> {
        debug_assert!(
            t != T::EMPTY && t != T::DEL,
            "Value cannot be res_empty or res_del!"
        );

        let mut it = ConstIterator::default();

        if self.size_ != 0 {
            let max = self.capacity() - 1;
            let mut spot = self.hash_value(t) & max;
            let mut i = 0;
            while i < self.capacity() * 4 && self.elements[spot] != T::EMPTY && self.elements[spot] != t {
                spot = self.hash_value_sz(spot) & max;
                i += 1;
            }
            if self.elements[spot] == t {
                it.fus = Some(self);
                it.i = spot;
            }
        }

        it
    }

    // The non-`const` `find` overload: compacts tombstones first, then
    // delegates to the const `find`. Named `find_mut` (Rust cannot overload
    // on receiver mutability).
    pub fn find_mut(&mut self, t: T) -> ConstIterator<'_, T> {
        if self.deleted != 0 && self.size_ + self.deleted == self.capacity() {
            self.reserve(self.capacity());
        }
        self.find(t)
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.count-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.count-fn]
    pub fn count(&self, t: T) -> usize {
        (self.find(t) != self.end()) as usize
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.contains-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.contains-fn]
    pub fn contains(&self, t: T) -> bool {
        self.find(t) != self.end()
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.begin-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.begin-fn]
    pub fn begin(&self) -> ConstIterator<'_, T> {
        if self.size_ == 0 {
            return self.end();
        }
        let ie = self.capacity();
        let mut i = 0;
        while i < ie {
            if self.elements[i] != T::EMPTY && self.elements[i] != T::DEL {
                return ConstIterator::new(self, i);
            }
            i += 1;
        }
        self.end()
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.end-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.end-fn]
    pub fn end(&self) -> ConstIterator<'_, T> {
        ConstIterator::default()
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.size-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.size-fn]
    pub fn size(&self) -> usize {
        self.size_
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.capacity-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.capacity-fn]
    pub fn capacity(&self) -> usize {
        self.elements.len()
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.reserve-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.reserve-fn]
    pub fn reserve(&mut self, n: usize) {
        // (A) Initial-allocation path (raw resize — can only grow).
        if self.size_ == 0 {
            self.elements.resize(n, T::EMPTY);
            self.deleted = 0;
            return;
        }

        // (B) Rehash. The C++ uses a `thread_local static` scratch vector for
        // reuse; a local vector is behaviourally identical.
        let mut vals: Vec<T> = Vec::new();
        vals.reserve(self.size_);
        for elem in &self.elements {
            if *elem != T::EMPTY && *elem != T::DEL {
                vals.push(*elem);
            }
        }

        self.clear(n);
        self.size_ = vals.len();
        let max = self.capacity() - 1;
        for val in &vals {
            let mut spot = self.hash_value(*val) & max;
            while self.elements[spot] != T::EMPTY && self.elements[spot] != *val {
                spot = self.hash_value_sz(spot) & max;
            }
            self.elements[spot] = *val;
        }
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.empty-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.empty-fn]
    pub fn empty(&self) -> bool {
        self.size_ == 0
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.assign-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.assign-fn]
    pub fn assign<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        self.clear(0);
        self.insert_range(iter);
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.swap-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.swap-fn]
    pub fn swap(&mut self, other: &mut FlatUnorderedSet<T>) {
        std::mem::swap(&mut self.size_, &mut other.size_);
        std::mem::swap(&mut self.deleted, &mut other.deleted);
        std::mem::swap(&mut self.elements, &mut other.elements);
    }

    // [spec:cg3:def:flat-unordered-set.cg3.flat-unordered-set.clear-fn]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.clear-fn]
    pub fn clear(&mut self, n: usize) {
        self.size_ = self.elements.len(); // temporarily holds the old capacity
        self.elements.resize(0, T::EMPTY);
        self.elements.resize(std::cmp::max(self.size_, n), T::EMPTY);
        self.size_ = 0;
        self.deleted = 0;
    }

    // `container& get()` — access to the raw slot table. Unannotated.
    pub fn get(&mut self) -> &mut Vec<T> {
        &mut self.elements
    }
}

/// `using uint32FlatHashSet = flat_unordered_set<uint32_t>;`
pub type Uint32FlatHashSet = FlatUnorderedSet<u32>;

/// `using uint64FlatHashSet = flat_unordered_set<uint64_t>;`
pub type Uint64FlatHashSet = FlatUnorderedSet<u64>;

#[cfg(test)]
mod tests {
    use super::*;

    // Collect the live values by driving begin()/end() and the iterator
    // post-increment (operator++(int)).
    fn collect(s: &Uint32FlatHashSet) -> Vec<u32> {
        let mut out = Vec::new();
        let mut it = s.begin();
        let end = s.end();
        while it != end {
            // post_increment returns the pre-advance copy; read from it.
            let prev = it.post_increment();
            out.push(prev.get());
        }
        out.sort();
        out
    }

    // The private LCG probe hashers. `hash_value_sz` is the container's OWN
    // member `t*3663850746527583589 + 11210403176660999867` (usize wraparound)
    // and MUST differ from `crate::inlines::hash_value_sz`. `hash_value` widens
    // the value via `static_cast<size_type>` (zero-extension) then mixes.
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.hash-value-sz-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.hash-value-fn/test]
    #[test]
    fn lcg_probe_hash_formula() {
        let s: Uint32FlatHashSet = FlatUnorderedSet::new();
        let expect = |t: usize| {
            t.wrapping_mul(3663850746527583589usize)
                .wrapping_add(11210403176660999867usize)
        };
        assert_eq!(s.hash_value_sz(0), expect(0));
        assert_eq!(s.hash_value_sz(3), expect(3));
        assert_eq!(s.hash_value_sz(987654321), expect(987654321));
        // hash_value(value) == hash_value_sz(value as usize).
        assert_eq!(s.hash_value(9u32), s.hash_value_sz(9));
        assert_eq!(s.hash_value(64u32), expect(64));

        // NOT the crate mixer.
        let mixer = crate::inlines::hash_value_sz(0x41, 0);
        assert_ne!(
            s.hash_value_sz(0x41),
            mixer,
            "container LCG must differ from inlines mixer"
        );

        // u64 values also widen and mix (Sentinel for u64).
        let s64: Uint64FlatHashSet = FlatUnorderedSet::new();
        assert_eq!(s64.hash_value(5u64), expect(5));
    }

    // Default ctor + insert + find/count/contains, begin/end iteration with
    // post-increment (the annotated operator-fn), size/capacity/empty, default
    // (end) ConstIterator. insert never rewrites an already-present value.
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.insert-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.find-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.count-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.contains-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.begin-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.end-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.size-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.capacity-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.empty-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.const-iterator.const-iterator-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.const-iterator.operator-fn/test]
    #[test]
    fn insert_find_iterate() {
        let mut s: Uint32FlatHashSet = FlatUnorderedSet::new();
        assert!(s.empty());
        assert_eq!(s.size(), 0);
        assert_eq!(s.capacity(), 0);
        assert!(s.begin() == s.end());
        assert!(s.end() == ConstIterator::default());

        s.insert(11);
        s.insert(22);
        s.insert(33);
        assert!(!s.empty());
        assert_eq!(s.size(), 3);
        assert!(s.capacity() >= 3);

        // find/contains/count.
        assert!(s.find(22) != s.end());
        assert_eq!(s.find(22).get(), 22);
        assert!(s.contains(33));
        assert_eq!(s.count(33), 1);
        assert!(s.find(99) == s.end());
        assert!(!s.contains(99));
        assert_eq!(s.count(99), 0);

        // Re-inserting a present value does not grow the set.
        s.insert(22);
        assert_eq!(s.size(), 3);

        // post_increment iteration recovers exactly the live values.
        assert_eq!(collect(&s), vec![11, 22, 33]);

        // post_increment semantics: returns the pre-advance value, advances self.
        let mut it = s.begin();
        let snapshot = it.post_increment();
        assert_eq!(snapshot.get(), s.begin().get()); // snapshot still at first
        // `it` has moved on (or reached end); the snapshot is unaffected.
        assert!(it != s.begin() || it == s.end());
    }

    // erase tombstones, reserve rehashes, clear resets, swap exchanges state,
    // assign clears + range-inserts. Also the erase-last-with-tombstone path.
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.erase-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.reserve-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.clear-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.swap-fn/test]
    // [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.assign-fn/test]
    #[test]
    fn erase_reserve_clear_swap_assign() {
        let mut s: Uint32FlatHashSet = FlatUnorderedSet::new();
        for v in 1u32..=5 {
            s.insert(v);
        }
        assert_eq!(s.size(), 5);

        // erase leaves a tombstone; find no longer sees it, others survive.
        s.erase(3);
        assert_eq!(s.size(), 4);
        assert!(s.find(3) == s.end());
        assert!(s.contains(4));
        s.erase(99); // no-op
        assert_eq!(s.size(), 4);

        // reserve rehashes into a bigger table, preserving live values.
        let before = collect(&s);
        s.reserve(64);
        assert!(s.capacity() >= 64);
        assert_eq!(collect(&s), before);

        // swap exchanges the whole state.
        let mut other: Uint32FlatHashSet = FlatUnorderedSet::new();
        other.insert(500);
        s.swap(&mut other);
        assert_eq!(collect(&s), vec![500]);
        assert_eq!(collect(&other), before);

        // assign clears then range-inserts (duplicates collapse to one value).
        s.assign([7u32, 8, 7, 9]);
        assert_eq!(collect(&s), vec![7, 8, 9]);

        // clear(0) empties.
        s.clear(0);
        assert!(s.empty());
        assert_eq!(s.size(), 0);
        assert!(s.begin() == s.end());

        // Erase-last-live-with-tombstone triggers the internal clear.
        let mut t: Uint32FlatHashSet = FlatUnorderedSet::new();
        t.insert(1);
        t.insert(2);
        t.erase(1);
        t.erase(2);
        assert_eq!(t.size(), 0);
        assert!(t.empty());
        t.insert(5);
        assert!(t.contains(5));
    }
}
