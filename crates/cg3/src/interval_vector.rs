//! Port of `src/interval_vector.hpp` — a set of `T` (a `uint32_t`-like scalar)
//! stored as a sorted list of merged `[lb, ub]` intervals. Literal,
//! bug-for-bug 1:1 translation (Wave 2).
//!
//! **The `_size` drift bug is reproduced faithfully.** `insert` increments
//! `_size` ONLY on its general (non-empty, non-first-interval) path, while
//! `erase` ALWAYS decrements it, so `size()` systematically under-counts and
//! can underflow. To reproduce the C++ `size_t` wrap-around without panicking
//! in debug builds, `_size` is maintained with `wrapping_add`/`wrapping_sub`.
//!
//! **Scalar arithmetic** on `T` (`ub + 1`, `lb - 1`, `ub - lb + 1`, ...) is
//! done with wrapping ops via [`IntervalScalar`] to match C++ unsigned overflow
//! semantics.
//!
//! **Iterator representation.** The nested C++ `const_iterator` expands the
//! interval list into individual integers; it is ported as [`const_iterator`],
//! holding a borrow of the interval container, a `usize` index, and the current
//! value. `operator++`/`operator--`/`operator*` become
//! [`const_iterator::advance`]/[`const_iterator::retreat`]/
//! [`const_iterator::value`]; a bonus [`Iterator`] impl is also provided.

#![allow(non_camel_case_types)]

/// Scalar element trait: the operations `interval_vector<T>` needs from `T`.
///
/// C++ instantiates this only with `uint32_t`. A general numeric-trait crate
/// (e.g. `num-traits`) would provide these; with std-only we declare a minimal
/// local trait. Arithmetic is **wrapping** to match C++ unsigned overflow.
pub trait IntervalScalar: Copy + Default + Ord {
    fn one() -> Self;
    fn wrapping_add(self, rhs: Self) -> Self;
    fn wrapping_sub(self, rhs: Self) -> Self;
    /// Widen to `usize` (the C++ `size_t` widening in `_size += ub - lb + 1`).
    fn to_usize(self) -> usize;
}

impl IntervalScalar for u32 {
    fn one() -> Self {
        1
    }
    fn wrapping_add(self, rhs: Self) -> Self {
        u32::wrapping_add(self, rhs)
    }
    fn wrapping_sub(self, rhs: Self) -> Self {
        u32::wrapping_sub(self, rhs)
    }
    fn to_usize(self) -> usize {
        self as usize
    }
}

// [spec:cg3:def:interval-vector.cg3.interval-vector.interval]
/// One merged interval `[lb, ub]` (inclusive). Private, matching the C++
/// `private: struct interval`.
#[derive(Clone, Copy)]
struct interval<T> {
    lb: T,
    ub: T,
}

impl<T: IntervalScalar> interval<T> {
    // [spec:cg3:def:interval-vector.cg3.interval-vector.interval.interval-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.interval.interval-fn]
    /// `explicit interval(T lb = T())`: a degenerate one-point interval with
    /// `lb == ub`.
    fn new1(lb: T) -> Self {
        interval { lb, ub: lb }
    }

    /// `explicit interval(T lb, T ub)`: independent bounds.
    fn new2(lb: T, ub: T) -> Self {
        interval { lb, ub }
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.interval.operator-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.interval.operator-fn]
    /// `bool operator<(const T& o)`: `ub < o`. This is the overload used by
    /// `std::lower_bound(elements, t)` to find the first interval with
    /// `ub >= t`. (The sibling `operator<(const interval&)` == `ub < o.lb` is
    /// inlined where needed; unused by the port's binary search.)
    fn lt_val(&self, o: &T) -> bool {
        self.ub < *o
    }
}

// [spec:cg3:def:interval-vector.cg3.interval-vector.const-iterator]
/// Forward/backward iterator that expands the interval list into individual
/// integers. Ported from the nested C++ `interval_vector<T>::const_iterator`.
#[derive(Clone, Copy)]
pub struct const_iterator<'a, T: IntervalScalar> {
    // `nullptr` in the default C++ ctor → `None` here.
    elements: Option<&'a Vec<interval<T>>>,
    // C++ `ContConstIter it`; `it == elements.len()` is the end sentinel.
    it: usize,
    // C++ current scalar value `T t`.
    t: T,
}

impl<'a, T: IntervalScalar> const_iterator<'a, T> {
    // [spec:cg3:def:interval-vector.cg3.interval-vector.const-iterator.const-iterator-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.const-iterator.const-iterator-fn]
    /// Two-arg ctor `const_iterator(elements, it)`: `t = T()`, then if
    /// `it != end` set `t = it->lb`.
    fn new(elements: &'a Vec<interval<T>>, it: usize) -> Self {
        let mut t = T::default();
        if it != elements.len() {
            t = elements[it].lb;
        }
        const_iterator {
            elements: Some(elements),
            it,
            t,
        }
    }

    /// Three-arg ctor `const_iterator(elements, it, t)`: sets `t` directly
    /// (used by `find`/`lower_bound` to point at an exact value).
    fn with_value(elements: &'a Vec<interval<T>>, it: usize, t: T) -> Self {
        const_iterator {
            elements: Some(elements),
            it,
            t,
        }
    }

    /// `operator++`: walk integers within the interval up to `ub`, then step to
    /// the next interval's `lb` (or the end sentinel with `t = T()`).
    pub fn advance(&mut self) {
        let els = self.elements.expect("null const_iterator");
        if self.it == els.len() {
            self.t = T::default();
            return;
        }
        if self.t == els[self.it].ub {
            self.it += 1;
            if self.it == els.len() {
                self.t = T::default();
            } else {
                self.t = els[self.it].lb;
            }
        } else {
            self.t = self.t.wrapping_add(T::one());
        }
    }

    /// `operator--`: reverse of [`advance`]; from an interval's `lb` step to the
    /// previous interval's `ub`, or from `begin` to the end sentinel.
    pub fn retreat(&mut self) {
        let els = self.elements.expect("null const_iterator");
        if self.it == els.len() || self.t == els[self.it].lb {
            if self.it == 0 {
                self.t = T::default();
                self.it = els.len();
            } else {
                self.it -= 1;
                self.t = els[self.it].ub;
            }
        } else {
            self.t = self.t.wrapping_sub(T::one());
        }
    }

    /// `operator*`: the current expanded integer value.
    pub fn value(&self) -> T {
        self.t
    }
}

// [spec:cg3:def:interval-vector.cg3.interval-vector.const-iterator.operator-fn]
// [spec:cg3:sem:interval-vector.cg3.interval-vector.const-iterator.operator-fn]
/// `operator==`: `it == o.it && t == o.t` — BOTH the interval index AND the
/// scalar value must match. (LITERAL DEVIATION: C++ compares raw iterator
/// identity; here indices are compared, which is equivalent for iterators over
/// the same container — the only case that occurs.)
impl<'a, T: IntervalScalar> PartialEq for const_iterator<'a, T> {
    fn eq(&self, o: &Self) -> bool {
        self.it == o.it && self.t == o.t
    }
}

/// Bonus ergonomic `Iterator` (not in the C++ surface): yields each expanded
/// integer from the current position to the end sentinel.
impl<'a, T: IntervalScalar> Iterator for const_iterator<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        let els = self.elements?;
        if self.it >= els.len() {
            return None;
        }
        let cur = self.t;
        self.advance();
        Some(cur)
    }
}

// [spec:cg3:def:interval-vector.cg3.interval-vector]
/// A set of `T` stored as a sorted list of merged `[lb, ub]` intervals.
/// Ported bug-for-bug from `src/interval_vector.hpp`, including the `_size`
/// drift.
#[derive(Clone)]
pub struct interval_vector<T: IntervalScalar = u32> {
    elements: Vec<interval<T>>,
    _size: usize,
}

impl<T: IntervalScalar> interval_vector<T> {
    /// `interval_vector() : _size(0)`.
    pub fn new() -> Self {
        interval_vector {
            elements: Vec::new(),
            _size: 0,
        }
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.interval-vector-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.interval-vector-fn]
    /// Range constructor `interval_vector(Iter b, Iter e)`: `_size = 0`, then
    /// `insert(*b)` for each element. Inherits `insert`'s `_size` bug.
    pub fn from_range<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut rv = interval_vector::new();
        for x in iter {
            rv.insert(x);
        }
        rv
    }

    /// `std::lower_bound(elements, t)` (via `interval::operator<(T)`, i.e.
    /// `ub < t`) as an index — the first interval with `ub >= t`.
    fn lower_bound_idx(&self, t: T) -> usize {
        self.elements.partition_point(|iv| iv.lt_val(&t))
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.insert-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.insert-fn]
    /// Inserts the single integer `t`, coalescing with neighbours. Returns
    /// `true` iff newly added.
    ///
    /// BUG/QUIRK (reproduced): `_size` is incremented ONLY in the general case
    /// (step 4); the empty-container and `it == begin` paths return `true`
    /// without incrementing, so `size()` systematically under-counts.
    pub fn insert(&mut self, t: T) -> bool {
        if self.elements.is_empty() {
            self.elements.push(interval::new1(t));
            return true;
        }
        let it = self.lower_bound_idx(t);
        if it != self.elements.len() && t >= self.elements[it].lb && t <= self.elements[it].ub {
            return false;
        }
        if it == 0 {
            if self.elements[it].ub.wrapping_add(T::one()) == t {
                self.elements[it].ub = self.elements[it].ub.wrapping_add(T::one());
            } else if self.elements[it].lb.wrapping_sub(T::one()) == t {
                self.elements[it].lb = self.elements[it].lb.wrapping_sub(T::one());
            } else {
                self.elements.insert(it, interval::new1(t));
            }
            return true;
        }
        let pr = it - 1;
        if it != 0 && self.elements[pr].ub.wrapping_add(T::one()) == t {
            self.elements[pr].ub = self.elements[pr].ub.wrapping_add(T::one());
            if it != self.elements.len()
                && self.elements[pr].ub.wrapping_add(T::one()) == self.elements[it].lb
            {
                self.elements[pr].ub = self.elements[it].ub;
                self.elements.remove(it);
            }
        } else if it != self.elements.len() && self.elements[it].lb == t.wrapping_add(T::one()) {
            self.elements[it].lb = self.elements[it].lb.wrapping_sub(T::one());
            if it != 0 && self.elements[pr].ub.wrapping_add(T::one()) == self.elements[it].lb {
                self.elements[pr].ub = self.elements[it].ub;
                self.elements.remove(it);
            }
        } else {
            self.elements.insert(it, interval::new1(t));
        }
        self._size = self._size.wrapping_add(1);
        true
    }

    /// C++ range `insert(It b, It e)`: loops `insert(*b)`.
    pub fn insert_range<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for x in iter {
            self.insert(x);
        }
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.push-back-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.push-back-fn]
    /// C++ `push_back` — misleadingly named (wave 4 rename): not an append;
    /// returns `insert(t)` (interval-merging sorted insertion).
    pub fn insert_sorted(&mut self, t: T) -> bool {
        self.insert(t)
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.erase-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.erase-fn]
    /// Removes the single integer `t`; returns `true` iff it was present.
    /// Every success path does `--_size` (always), which combined with
    /// `insert`'s skipped increments makes `size()` drift/underflow.
    ///
    /// The split path (d) captures `it->ub` and mutates by index BEFORE the
    /// insert, per the spec BUG note — Rust indices don't dangle, so this
    /// realizes the intended (non-UB) behaviour the C++ has when the vector is
    /// not at capacity.
    pub fn erase(&mut self, t: T) -> bool {
        let it = self.lower_bound_idx(t);
        if it == self.elements.len() {
            return false;
        }
        if self.elements[it].ub < t || self.elements[it].lb > t {
            return false;
        }
        if self.elements[it].lb == t && self.elements[it].ub == t {
            self.elements.remove(it);
            self._size = self._size.wrapping_sub(1);
            return true;
        }
        if self.elements[it].ub == t {
            self.elements[it].ub = self.elements[it].ub.wrapping_sub(T::one());
            self._size = self._size.wrapping_sub(1);
            return true;
        }
        if self.elements[it].lb == t {
            self.elements[it].lb = self.elements[it].lb.wrapping_add(T::one());
            self._size = self._size.wrapping_sub(1);
            return true;
        }
        if self.elements[it].lb < t && self.elements[it].ub > t {
            let ub = self.elements[it].ub;
            self.elements
                .insert(it + 1, interval::new2(t.wrapping_add(T::one()), ub));
            self.elements[it].ub = t.wrapping_sub(T::one());
            self._size = self._size.wrapping_sub(1);
            return true;
        }

        debug_assert!(
            false,
            "interval_vector.erase() should never reach this place..."
        );
        false
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.find-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.find-fn]
    /// A `const_iterator` positioned exactly at `t`, or `end()` if absent.
    pub fn find(&self, t: T) -> const_iterator<'_, T> {
        let it = self.lower_bound_idx(t);
        if it == self.elements.len() {
            return self.end();
        }
        if self.elements[it].ub < t || self.elements[it].lb > t {
            return self.end();
        }
        const_iterator::with_value(&self.elements, it, t)
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.lower-bound-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.lower-bound-fn]
    /// A `const_iterator` to the smallest present value `>= t`, or `end()`.
    /// The `if (it->ub < t)` block is dead given `lower_bound`'s guarantee but
    /// is reproduced faithfully.
    pub fn lower_bound(&self, t: T) -> const_iterator<'_, T> {
        let mut t = t;
        let mut it = self.lower_bound_idx(t);
        if it == self.elements.len() {
            return self.end();
        }
        if self.elements[it].ub < t {
            it += 1;
            if it == self.elements.len() {
                return self.end();
            }
            t = self.elements[it].lb;
        }
        if self.elements[it].lb > t {
            t = self.elements[it].lb;
        }
        const_iterator::with_value(&self.elements, it, t)
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.contains-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.contains-fn]
    /// `true` iff `t` is in the set. Builds no iterator.
    pub fn contains(&self, t: T) -> bool {
        let it = self.lower_bound_idx(t);
        if it == self.elements.len() {
            return false;
        }
        if self.elements[it].ub < t || self.elements[it].lb > t {
            return false;
        }
        true
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.begin-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.begin-fn]
    /// Iterator at the first interval's `lb` (the minimum), or end if empty.
    pub fn begin(&self) -> const_iterator<'_, T> {
        const_iterator::new(&self.elements, 0)
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.end-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.end-fn]
    /// The end sentinel: index `elements.len()`, value `T()`.
    pub fn end(&self) -> const_iterator<'_, T> {
        const_iterator::new(&self.elements, self.elements.len())
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.front-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.front-fn]
    /// `elements.front().lb` — the minimum. UB if empty (here: panics).
    pub fn front(&self) -> T {
        self.elements[0].lb
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.back-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.back-fn]
    /// `elements.back().ub` — the maximum. UB if empty (here: panics).
    pub fn back(&self) -> T {
        self.elements[self.elements.len() - 1].ub
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.size-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.size-fn]
    /// The cached `_size` counter (drifts; see the type-level note).
    pub fn size(&self) -> usize {
        self._size
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.empty-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.empty-fn]
    /// `elements.empty()` — checks the interval container directly (NOT the
    /// possibly-drifted `_size`), so it stays reliable.
    pub fn empty(&self) -> bool {
        self.elements.is_empty()
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.clear-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.clear-fn]
    /// `elements.clear()` and `_size = 0` (also discarding any `_size` drift).
    pub fn clear(&mut self) {
        self.elements.clear();
        self._size = 0;
    }

    // [spec:cg3:def:interval-vector.cg3.interval-vector.intersect-fn]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.intersect-fn]
    /// Set intersection of `self` and `o`, leaving both unchanged. Two-pointer
    /// merge over the sorted, non-overlapping interval lists, appending
    /// coalesced overlaps directly to the result. Unlike `insert`, keeps
    /// `_size` correct.
    pub fn intersect(&self, o: &interval_vector<T>) -> interval_vector<T> {
        let mut rv: interval_vector<T> = interval_vector::new();
        if !self.empty() && !o.empty() {
            let ae = self.elements.len();
            let be = o.elements.len();
            let mut a = 0usize;
            let mut b = 0usize;
            while a != ae && b != be {
                while a != ae && b != be && self.elements[a].ub < o.elements[b].lb {
                    a += 1;
                }
                while a != ae && b != be && o.elements[b].ub < self.elements[a].lb {
                    b += 1;
                }
                while a != ae
                    && b != be
                    && self.elements[a].ub >= o.elements[b].lb
                    && o.elements[b].ub >= self.elements[a].lb
                {
                    let lb = self.elements[a].lb.max(o.elements[b].lb);
                    let ub = self.elements[a].ub.min(o.elements[b].ub);
                    if !rv.elements.is_empty()
                        && rv.elements[rv.elements.len() - 1].ub.wrapping_add(T::one()) == lb
                    {
                        let last = rv.elements.len() - 1;
                        rv.elements[last].ub = ub;
                    } else {
                        rv.elements.push(interval::new2(lb, ub));
                    }
                    rv._size = rv
                        ._size
                        .wrapping_add(ub.wrapping_sub(lb).wrapping_add(T::one()).to_usize());
                    if self.elements[a].ub < o.elements[b].ub {
                        a += 1;
                    } else {
                        b += 1;
                    }
                }
            }
        }
        rv
    }
}

impl<T: IntervalScalar> Default for interval_vector<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// C++ `using uint32IntervalVector = interval_vector<uint32_t>`.
pub type uint32IntervalVector = interval_vector<u32>;

#[cfg(test)]
mod tests {
    use super::*;

    // Constructing an interval_vector, inserting integers, and reading it back
    // exercises the constructors, insert coalescing, contains/find/lower_bound,
    // begin/end iteration, front/back extremes, empty, and the interval helper
    // ctors + the interval `operator<` used by the internal lower_bound.
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.interval-vector-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.interval.interval-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.interval.operator-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.insert-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.push-back-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.contains-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.find-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.lower-bound-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.begin-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.end-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.front-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.back-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.empty-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.const-iterator.const-iterator-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.const-iterator.operator-fn/test]
    #[test]
    fn insert_coalesce_and_query() {
        // interval helper ctors + operator<
        let iv1 = interval::<u32>::new1(5);
        assert!(iv1.lt_val(&6));
        assert!(!iv1.lt_val(&5));
        let iv2 = interval::<u32>::new2(2, 4);
        assert!(iv2.lt_val(&5));

        // Range ctor (from_range -> insert loop).
        let mut v = interval_vector::from_range([3u32, 4, 5, 1]);
        assert!(!v.empty());
        assert!(v.contains(1));
        assert!(v.contains(4));
        assert!(!v.contains(2));
        // 3,4,5 coalesced; 1 is separate -> front=1, back=5.
        assert_eq!(v.front(), 1);
        assert_eq!(v.back(), 5);

        // Adjacent insert coalesces (fills the 2 gap -> one interval 1..=5).
        assert!(v.insert_sorted(2));
        assert!(v.contains(2));
        assert_eq!(v.front(), 1);
        assert_eq!(v.back(), 5);
        // Re-inserting present value returns false (no change).
        assert!(!v.insert(3));

        // find lands exactly on the value; find of absent -> end().
        let f = v.find(4);
        assert_eq!(f.value(), 4);
        assert!(v.find(99) == v.end());

        // lower_bound returns smallest present >= t.
        assert_eq!(v.lower_bound(0).value(), 1);
        assert_eq!(v.lower_bound(3).value(), 3);
        assert!(v.lower_bound(6) == v.end());

        // begin/end + iterator advance expand the intervals into 1,2,3,4,5.
        let expanded: Vec<u32> = {
            let mut it = v.begin();
            let mut out = Vec::new();
            while it != v.end() {
                out.push(it.value());
                it.advance();
            }
            out
        };
        assert_eq!(expanded, vec![1, 2, 3, 4, 5]);
    }

    // erase splits/shrinks intervals and always decrements _size, while insert
    // skips the increment on the empty/first-interval paths: this drives the
    // documented `_size` drift bug and clear() resetting it. Also exercises the
    // iterator retreat + the bonus Iterator impl.
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.erase-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.size-fn/test]
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.clear-fn/test]
    #[test]
    fn erase_split_and_size_drift() {
        let mut v: interval_vector<u32> = interval_vector::new();
        // First insert into an empty container: returns true but does NOT bump
        // _size (the reproduced bug). So size() stays 0 after one element.
        assert!(v.insert(10));
        assert_eq!(v.size(), 0, "empty-path insert skips _size increment (bug)");

        // Grow the interval; the general path DOES bump _size.
        assert!(v.insert(11)); // coalesces onto [10,11]; general path -> +1
        assert_eq!(v.size(), 1);

        // Split the middle out of [10,11] via a fresh 3-wide interval.
        v.insert(20);
        v.insert(21);
        v.insert(22); // -> interval [20,22]
        // erase the middle -> split into [20,20] and [22,22]; _size -= 1 always.
        assert!(v.erase(21));
        assert!(v.contains(20) && v.contains(22) && !v.contains(21));

        // erase of absent value returns false and leaves size alone.
        assert!(!v.erase(999));

        // Iterator retreat walks backwards from end.
        let mut it = v.end();
        it.retreat();
        assert_eq!(it.value(), v.back());

        // Bonus Iterator impl collects all present integers ascending.
        let all: Vec<u32> = v.begin().collect();
        assert_eq!(all, vec![10, 11, 20, 22]);

        // clear() empties + resets any drifted _size to 0.
        v.clear();
        assert!(v.empty());
        assert_eq!(v.size(), 0);
    }

    // intersect merges two sorted interval lists and keeps _size correct.
    // [spec:cg3:sem:interval-vector.cg3.interval-vector.intersect-fn/test]
    #[test]
    fn intersect_overlaps() {
        let a = interval_vector::from_range([1u32, 2, 3, 4, 5, 10, 11, 12]);
        let b = interval_vector::from_range([3u32, 4, 11, 12, 13, 99]);
        let c = a.intersect(&b);
        let got: Vec<u32> = c.begin().collect();
        assert_eq!(got, vec![3, 4, 11, 12]);
        // intersect maintains _size correctly (unlike insert): 4 elements.
        assert_eq!(c.size(), 4);

        // Empty operand -> empty intersection.
        let empty: interval_vector<u32> = interval_vector::new();
        assert!(a.intersect(&empty).empty());
    }
}
