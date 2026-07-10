//! Port of `src/sorted_vector.hpp` — a `std::vector` kept sorted and used as a
//! set (unique elements). Literal, bug-for-bug 1:1 translation (Wave 2).
//!
//! **Iterator representation.** The C++ container hands out
//! `std::vector<T>::iterator`/`const_iterator` and does pointer arithmetic on
//! them (`begin() + at`, `std::distance(begin(), it)`). The faithful Rust
//! analog used here is a **`usize` index** into the underlying `Vec`: `begin()`
//! / `cbegin()` == `0`, `end()` / `cend()` == `size()`, `find`/`lower_bound`/
//! `upper_bound` return the index (or `size()` for the end position). This maps
//! the C++ pointer arithmetic exactly (`begin() + at` becomes just `at`,
//! `distance(begin(), it)` becomes `it`). For actual element traversal use
//! [`sorted_vector::iter`] / [`sorted_vector::as_slice`]; reverse traversal
//! (C++ `rbegin`/`rend`) collapses into [`sorted_vector::iter_rev`].
//!
//! **Comparator.** C++ `sorted_vector<T, Comp = std::less<T>>` takes a
//! stateless strict-weak-ordering functor `Comp` with `comp(a, b)` meaning
//! "a < b". The Rust analog is the [`Comparator`] trait; the default [`Less`]
//! uses `PartialOrd`. Custom comparators (later waves' `compare_Cohort`,
//! `compare_Tag`, ...) implement [`Comparator`] for their element type.

#![allow(non_camel_case_types)]

use std::cmp::Ordering;

/// Strict-weak-ordering predicate, the Rust analog of the C++ `Comp` functor.
///
/// `comp(a, b)` returns `true` iff `a` sorts strictly before `b` (i.e. the C++
/// `comp(a, b)` / `a < b`).
pub trait Comparator<T> {
    fn comp(&self, a: &T, b: &T) -> bool;
}

/// Default comparator: the analog of `std::less<T>`, ordering by `PartialOrd`.
#[derive(Default, Clone, Copy, Debug)]
pub struct Less;

impl<T: PartialOrd> Comparator<T> for Less {
    fn comp(&self, a: &T, b: &T) -> bool {
        a < b
    }
}

/// The C++ `namespace CG3::detail` free functions.
pub mod detail {
    use super::Comparator;

    // [spec:cg3:def:sorted-vector.cg3.detail.is-sorted-fn]
    // [spec:cg3:sem:sorted-vector.cg3.detail.is-sorted-fn]
    /// Custom reimplementation of `std::is_sorted` over the range `s` under the
    /// strict-weak-ordering predicate `comp`. Empty range → `true`. Walks
    /// adjacent pairs, returning `false` the instant a later element sorts
    /// strictly before its predecessor. `O(n)`.
    pub fn is_sorted<T, C: Comparator<T>>(s: &[T], comp: &C) -> bool {
        if !s.is_empty() {
            let mut first = 0usize;
            let mut next = first + 1;
            while next != s.len() {
                if comp.comp(&s[next], &s[first]) {
                    return false;
                }
                first = next;
                next += 1;
            }
        }
        true
    }
}

// [spec:cg3:def:sorted-vector.cg3.sorted-vector.container]
/// C++ `typedef std::vector<T> container`.
pub type Container<T> = Vec<T>;

// [spec:cg3:def:sorted-vector.cg3.sorted-vector.size-type]
/// C++ `typedef container::size_type size_type`.
pub type SizeType = usize;

// The remaining member typedefs of the C++ class map to Rust as follows (they
// are documented here rather than as standalone aliases because they either
// equal the type parameter or the chosen index representation):
//
//   [spec:cg3:def:sorted-vector.cg3.sorted-vector.iterator]
//     iterator               = usize (index into `elements`)
//   [spec:cg3:def:sorted-vector.cg3.sorted-vector.const-iterator]
//     const_iterator         = usize (index into `elements`)
//   [spec:cg3:def:sorted-vector.cg3.sorted-vector.const-reverse-iterator]
//     const_reverse_iterator = std::iter::Rev<slice::Iter<'_, T>> (see iter_rev)
//   [spec:cg3:def:sorted-vector.cg3.sorted-vector.value-type]
//     value_type             = T
//   [spec:cg3:def:sorted-vector.cg3.sorted-vector.key-type]
//     key_type               = T

// [spec:cg3:def:sorted-vector.cg3.sorted-vector]
/// A `std::vector<T>` kept sorted by `comp` and holding unique elements (a
/// sorted set). Ported bug-for-bug from `src/sorted_vector.hpp`.
#[derive(Clone)]
pub struct sorted_vector<T, Comp = Less> {
    elements: Container<T>,
    comp: Comp,
}

impl<T, Comp: Default> sorted_vector<T, Comp> {
    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.sorted-vector-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.sorted-vector-fn]
    /// Default constructor: an empty `elements` and a default-constructed
    /// `comp`. (The `CG_TRACE_OBJECTS` debug tracing is a debug-only side
    /// effect and is not reproduced.)
    pub fn new() -> Self {
        sorted_vector {
            elements: Container::new(),
            comp: Comp::default(),
        }
    }
}

impl<T, Comp: Default> Default for sorted_vector<T, Comp> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone, Comp: Comparator<T> + Default> sorted_vector<T, Comp> {
    /// `std::lower_bound(begin, end, t, comp)` as an index — the first position
    /// whose element is not less than `t`.
    fn lower_bound_idx(&self, t: &T) -> usize {
        let comp = &self.comp;
        self.elements.partition_point(|x| comp.comp(x, t))
    }

    /// `std::upper_bound(begin, end, t, comp)` as an index — the first position
    /// whose element is strictly greater than `t`.
    fn upper_bound_idx(&self, t: &T) -> usize {
        let comp = &self.comp;
        self.elements.partition_point(|x| !comp.comp(t, x))
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.insert-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.insert-fn]
    /// Sorted, duplicate-suppressing insert. Returns `(index, inserted)` — the
    /// Rust analog of `std::pair<iterator, bool>`, where the index is
    /// `begin() + at` recomputed after the (possibly reallocating) insert.
    pub fn insert(&mut self, t: T) -> (usize, bool) {
        if self.elements.is_empty() {
            self.elements.push(t);
            return (0, true);
        }
        let it = self.lower_bound_idx(&t);
        // Recorded BEFORE any mutation (reallocation would invalidate `it`).
        let at = it;
        if it == self.elements.len() {
            self.elements.push(t);
            return (at, true);
        }
        if self.comp.comp(&self.elements[it], &t) || self.comp.comp(&t, &self.elements[it]) {
            self.elements.insert(it, t);
            return (at, true);
        }
        (at, false)
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.push-back-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.push-back-fn]
    /// Despite the name this does NOT append; it routes through [`insert`],
    /// performing a sorted, duplicate-suppressing insertion. The `insert`
    /// return value is discarded.
    pub fn push_back(&mut self, t: T) {
        self.insert(t);
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.erase-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.erase-fn]
    /// Removes the element equal to `t` if present; returns `true` iff removed.
    pub fn erase(&mut self, t: T) -> bool {
        if self.elements.is_empty() {
            return false;
        }
        let last = self.elements.len() - 1;
        if self.comp.comp(&self.elements[last], &t) {
            return false;
        }
        if self.comp.comp(&t, &self.elements[0]) {
            return false;
        }
        let it = self.lower_bound_idx(&t);
        if it != self.elements.len()
            && !self.comp.comp(&self.elements[it], &t)
            && !self.comp.comp(&t, &self.elements[it])
        {
            self.elements.remove(it);
            return true;
        }
        false
    }

    /// C++ `const_iterator erase(const_iterator it)`: erase by position.
    /// Returns the index following the removed element (== `it`), the analog of
    /// `std::vector::erase`'s returned iterator.
    pub fn erase_it(&mut self, it: usize) -> usize {
        let o = it;
        self.elements.remove(o);
        o
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.erase-n-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.erase-n-fn]
    /// Erases the element at 0-based index `i`. No bounds check in C++ (UB when
    /// out of range); `Vec::remove` panics instead — `i` is a trusted in-range
    /// index.
    pub fn erase_n(&mut self, i: usize) {
        self.elements.remove(i);
    }

    /// C++ range `erase(b, e)`: loops `erase(*b)` over the range.
    pub fn erase_range(&mut self, items: &[T]) {
        for x in items {
            self.erase(x.clone());
        }
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.find-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.find-fn]
    /// Looks up `t`, returning its index or `end()` (== `size()`) if absent.
    pub fn find(&self, t: T) -> usize {
        if self.elements.is_empty() {
            return self.elements.len();
        }
        let last = self.elements.len() - 1;
        if self.comp.comp(&self.elements[last], &t) {
            return self.elements.len();
        }
        if self.comp.comp(&t, &self.elements[0]) {
            return self.elements.len();
        }
        let it = self.lower_bound_idx(&t);
        if it != self.elements.len()
            && (self.comp.comp(&self.elements[it], &t) || self.comp.comp(&t, &self.elements[it]))
        {
            return self.elements.len();
        }
        it
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.find-n-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.find-n-fn]
    /// 0-based index of `t` as `distance(begin(), find(t))`. Yields `size()` as
    /// the "not found" sentinel (since `find` returns `end()`).
    pub fn find_n(&self, t: T) -> SizeType {
        self.find(t)
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.count-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.count-fn]
    /// `1` if `t` is present, else `0` (set semantics; never > 1).
    pub fn count(&self, t: T) -> SizeType {
        (self.find(t) != self.end()) as SizeType
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.contains-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.contains-fn]
    /// `true` iff `t` is present (`find(t) != end()`).
    pub fn contains(&self, t: T) -> bool {
        self.find(t) != self.end()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.begin-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.begin-fn]
    /// Index of the first (smallest) element: always `0`.
    pub fn begin(&self) -> usize {
        0
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.end-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.end-fn]
    /// One-past-last index: `size()`.
    pub fn end(&self) -> usize {
        self.elements.len()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.cbegin-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.cbegin-fn]
    pub fn cbegin(&self) -> usize {
        0
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.cend-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.cend-fn]
    pub fn cend(&self) -> usize {
        self.elements.len()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.rbegin-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.rbegin-fn]
    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.rend-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.rend-fn]
    /// Reverse traversal from the last (largest) toward the first element.
    /// C++'s separate `rbegin()`/`rend()` collapse into this single reversed
    /// iterator (LITERAL DEVIATION: two iterator-endpoints → one Rust
    /// iterator, since a `usize` reverse position would collide numerically
    /// with forward positions).
    pub fn iter_rev(&self) -> std::iter::Rev<std::slice::Iter<'_, T>> {
        self.elements.iter().rev()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.front-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.front-fn]
    /// Copy of the smallest element. UB if empty (here: panics).
    pub fn front(&self) -> T {
        self.elements[0].clone()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.back-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.back-fn]
    /// Copy of the largest element. UB if empty (here: panics).
    pub fn back(&self) -> T {
        self.elements[self.elements.len() - 1].clone()
    }

    /// C++ `T& at(size_type i)`: mutable element access by index.
    pub fn at(&mut self, i: usize) -> &mut T {
        &mut self.elements[i]
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.lower-bound-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.lower-bound-fn]
    /// First index whose element is not less than `t` (leftmost `>= t`), or
    /// `end()`. Collapses the C++ mutable/const overloads into one.
    pub fn lower_bound(&self, t: T) -> usize {
        self.lower_bound_idx(&t)
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.upper-bound-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.upper-bound-fn]
    /// First index whose element is strictly greater than `t`, or `end()`.
    pub fn upper_bound(&self, t: T) -> usize {
        self.upper_bound_idx(&t)
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.size-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.size-fn]
    pub fn size(&self) -> SizeType {
        self.elements.len()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.capacity-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.capacity-fn]
    pub fn capacity(&self) -> SizeType {
        self.elements.capacity()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.empty-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.empty-fn]
    pub fn empty(&self) -> bool {
        self.elements.is_empty()
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.swap-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.swap-fn]
    /// Swaps the underlying storage with `other`. The `comp` members are NOT
    /// swapped (each keeps its own).
    pub fn swap(&mut self, other: &mut sorted_vector<T, Comp>) {
        std::mem::swap(&mut self.elements, &mut other.elements);
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.clear-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.clear-fn]
    /// Removes all elements; capacity unchanged.
    pub fn clear(&mut self) {
        self.elements.clear();
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.sort-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.sort-fn]
    /// Re-sorts `elements` in place with a freshly default-constructed `Comp`
    /// (NOT the stored `comp`). Does not de-duplicate. `std::sort` is
    /// unstable, so `sort_unstable_by` is used; the total order is derived from
    /// the strict-weak-ordering predicate.
    pub fn sort(&mut self) {
        let c = Comp::default();
        self.elements.sort_unstable_by(|a, b| {
            if c.comp(a, b) {
                Ordering::Less
            } else if c.comp(b, a) {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.pop-back-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.pop-back-fn]
    /// Removes the last (largest) element. UB if empty in C++; here a no-op.
    pub fn pop_back(&mut self) {
        self.elements.pop();
    }

    /// C++ `container& get()`: mutable access to the underlying vector.
    pub fn get(&mut self) -> &mut Container<T> {
        &mut self.elements
    }

    /// Read-only view of the underlying vector (faithful analog for
    /// `begin()..end()` traversal / element access).
    pub fn as_slice(&self) -> &[T] {
        &self.elements
    }

    /// Forward element iterator (faithful analog for `begin()`/`end()`
    /// traversal that dereferences).
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.elements.iter()
    }
}

impl<T: Clone + PartialEq, Comp: Comparator<T> + Default> sorted_vector<T, Comp> {
    /// C++ range `insert(It b, It e)`: merges/sorts/uniques a whole range into
    /// `elements`. LITERAL DEVIATION: the C++ uses per-thread `thread_local`
    /// scratch buffers `merged`/`sorted`, which cannot be generic in Rust;
    /// local `Vec`s are used instead (no behavioural difference).
    pub fn insert_range(&mut self, items: &[T]) {
        let d = items.len();
        if d == 1 {
            self.insert(items[0].clone());
            return;
        }

        let mut merged: Container<T> = Vec::with_capacity(self.elements.len() + d);

        if detail::is_sorted(items, &self.comp) {
            merge_into(&self.elements, items, &self.comp, &mut merged);
        } else {
            let mut sorted: Container<T> = items.to_vec();
            let comp = &self.comp;
            sorted.sort_unstable_by(|a, b| {
                if comp.comp(a, b) {
                    Ordering::Less
                } else if comp.comp(b, a) {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            });
            merge_into(&self.elements, &sorted, &self.comp, &mut merged);
        }

        std::mem::swap(&mut self.elements, &mut merged);
        // std::unique + erase(it, end): drop consecutive duplicates via `==`.
        self.elements.dedup();
    }

    // [spec:cg3:def:sorted-vector.cg3.sorted-vector.assign-fn]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.assign-fn]
    /// `clear()` then range-`insert`: the sorted, de-duplicated form of
    /// `items` (analog of the templated `assign(It, It)`).
    pub fn assign(&mut self, items: &[T]) {
        self.clear();
        self.insert_range(items);
    }

    /// C++ `assign(const_iterator, const_iterator)`: raw `elements.assign(b,e)`
    /// trusting the range is already sorted and unique. Callers pass a
    /// sub-slice of another `sorted_vector` (e.g. `&src.as_slice()[b..e]`).
    pub fn assign_sorted(&mut self, items: &[T]) {
        self.elements = items.to_vec();
    }
}

/// `std::merge(a, b, out, comp)`: stable merge of two sorted ranges, taking
/// from `a` on ties.
fn merge_into<T: Clone, C: Comparator<T>>(a: &[T], b: &[T], comp: &C, out: &mut Vec<T>) {
    let mut i = 0usize;
    let mut j = 0usize;
    while i < a.len() && j < b.len() {
        if comp.comp(&b[j], &a[i]) {
            out.push(b[j].clone());
            j += 1;
        } else {
            out.push(a[i].clone());
            i += 1;
        }
    }
    while i < a.len() {
        out.push(a[i].clone());
        i += 1;
    }
    while j < b.len() {
        out.push(b[j].clone());
        j += 1;
    }
}

// [spec:cg3:def:sorted-vector.cg3.uint32-sorted-vector]
/// C++ `typedef sorted_vector<uint32_t> uint32SortedVector`.
pub type uint32SortedVector = sorted_vector<u32>;

#[cfg(test)]
mod tests {
    use super::*;

    // A descending comparator to prove `Comp` is honoured (sort/insert/find all
    // route through `comp`, not raw `PartialOrd`), and to exercise the reverse
    // ordering in `is_sorted`, `swap`, `sort`, `assign`.
    #[derive(Default, Clone, Copy)]
    struct Greater;
    impl<T: PartialOrd> Comparator<T> for Greater {
        fn comp(&self, a: &T, b: &T) -> bool {
            a > b
        }
    }

    // Building a sorted_vector with the default ctor and populating it via the
    // duplicate-suppressing `insert`/`push_back`, then querying with
    // find/find_n/count/contains and the internal lower_bound/upper_bound.
    // Also exercises begin/end/cbegin/cend (index endpoints), front/back,
    // size/capacity/empty, and the `detail::is_sorted` free function that
    // insert_range consults.
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.sorted-vector-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.insert-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.push-back-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.find-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.find-n-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.count-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.contains-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.lower-bound-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.upper-bound-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.begin-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.end-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.cbegin-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.cend-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.front-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.back-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.size-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.capacity-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.empty-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.detail.is-sorted-fn/test]
    #[test]
    fn insert_query_and_bounds() {
        let mut v: sorted_vector<u32> = sorted_vector::new();
        assert!(v.empty());
        assert_eq!(v.size(), 0);
        assert_eq!(v.begin(), 0);
        assert_eq!(v.end(), 0);

        // insert keeps the vector sorted and returns (index, inserted).
        assert_eq!(v.insert(5), (0, true)); // empty-path
        assert_eq!(v.insert(2), (0, true)); // goes to the front
        assert_eq!(v.insert(8), (2, true)); // appended (push path)
        // Duplicate is suppressed: (index_of_existing, false).
        assert_eq!(v.insert(5), (1, false));
        // push_back routes through insert (does NOT append).
        v.push_back(3);
        v.push_back(3); // duplicate suppressed
        // Order is now [2,3,5,8].
        assert_eq!(v.as_slice(), &[2, 3, 5, 8]);

        assert!(!v.empty());
        assert_eq!(v.size(), 4);
        assert!(v.capacity() >= 4);
        assert_eq!(v.front(), 2);
        assert_eq!(v.back(), 8);
        // Index endpoints.
        assert_eq!(v.begin(), 0);
        assert_eq!(v.cbegin(), 0);
        assert_eq!(v.end(), 4);
        assert_eq!(v.cend(), 4);

        // find returns the element index; absent -> end() (== size()).
        assert_eq!(v.find(5), 2);
        assert_eq!(v.find(99), v.end());
        // find_n is distance(begin(), find(t)) == the same index / size sentinel.
        assert_eq!(v.find_n(3), 1);
        assert_eq!(v.find_n(99), v.size());
        // count is 0/1 (set semantics).
        assert_eq!(v.count(5), 1);
        assert_eq!(v.count(4), 0);
        assert!(v.contains(8));
        assert!(!v.contains(4));

        // lower_bound: leftmost >= t; upper_bound: leftmost > t.
        assert_eq!(v.lower_bound(3), 1); // element 3 at index 1
        assert_eq!(v.lower_bound(4), 2); // first >= 4 is 5 at index 2
        assert_eq!(v.upper_bound(3), 2); // first > 3 is 5 at index 2
        assert_eq!(v.upper_bound(8), 4); // nothing > 8 -> end()

        // detail::is_sorted under the default (ascending) comparator.
        assert!(detail::is_sorted(v.as_slice(), &Less));
        assert!(detail::is_sorted::<u32, Less>(&[], &Less)); // empty -> true
        assert!(!detail::is_sorted(&[3u32, 1, 2], &Less));
        // Under the reverse comparator a descending slice is "sorted".
        assert!(detail::is_sorted(&[8u32, 5, 2], &Greater));
    }

    // erase (by value, by iterator index, by position n), pop_back, clear, and
    // the reverse traversal that collapses C++ rbegin()/rend() into iter_rev.
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.erase-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.erase-n-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.pop-back-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.clear-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.rbegin-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.rend-fn/test]
    #[test]
    fn erase_pop_clear_and_reverse() {
        let mut v: sorted_vector<u32> = sorted_vector::new();
        for x in [1u32, 2, 3, 4, 5] {
            v.push_back(x);
        }
        assert_eq!(v.as_slice(), &[1, 2, 3, 4, 5]);

        // erase by value: present -> true, absent -> false.
        assert!(v.erase(3));
        assert!(!v.erase(3));
        assert!(!v.erase(99));
        assert_eq!(v.as_slice(), &[1, 2, 4, 5]);

        // erase_n removes the element at index 0.
        v.erase_n(0);
        assert_eq!(v.as_slice(), &[2, 4, 5]);

        // erase_it (by iterator/index) removes and returns the following index.
        let next = v.erase_it(1); // removes '4'
        assert_eq!(next, 1);
        assert_eq!(v.as_slice(), &[2, 5]);

        // Reverse traversal (rbegin..rend) yields largest -> smallest.
        let rev: Vec<u32> = v.iter_rev().copied().collect();
        assert_eq!(rev, vec![5, 2]);

        // pop_back removes the largest.
        v.pop_back();
        assert_eq!(v.as_slice(), &[2]);

        // clear empties but leaves capacity.
        let cap = v.capacity();
        v.clear();
        assert!(v.empty());
        assert_eq!(v.capacity(), cap);
        // pop_back on empty is a no-op (not UB here).
        v.pop_back();
        assert!(v.empty());
    }

    // sort re-sorts with a *freshly default-constructed* Comp (not the stored
    // comp), swap exchanges only the storage (comp stays per-instance), and
    // assign clears + range-inserts (sort/unique) — driving insert_range and
    // the merge helper indirectly.
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.sort-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.swap-fn/test]
    // [spec:cg3:sem:sorted-vector.cg3.sorted-vector.assign-fn/test]
    #[test]
    fn sort_swap_assign() {
        // assign de-duplicates and sorts an unsorted range.
        let mut v: sorted_vector<u32> = sorted_vector::new();
        v.assign(&[9, 1, 5, 1, 9, 3]);
        assert_eq!(v.as_slice(), &[1, 3, 5, 9]);

        // Manually scramble the storage then sort() — it re-sorts ascending
        // (default Comp) regardless of prior order, without de-duplicating.
        {
            let raw = v.get();
            raw.clear();
            raw.extend_from_slice(&[7, 2, 7, 4]);
        }
        v.sort();
        assert_eq!(v.as_slice(), &[2, 4, 7, 7]); // sorted, duplicate NOT removed

        // swap exchanges storage between two sorted_vectors.
        let mut a: sorted_vector<u32> = sorted_vector::new();
        a.assign(&[10, 20]);
        let mut b: sorted_vector<u32> = sorted_vector::new();
        b.assign(&[1, 2, 3]);
        a.swap(&mut b);
        assert_eq!(a.as_slice(), &[1, 2, 3]);
        assert_eq!(b.as_slice(), &[10, 20]);

        // A sorted_vector using a custom (descending) comparator: assign keeps
        // it in the comparator's order, and sort() re-derives that order.
        let mut d: sorted_vector<u32, Greater> = sorted_vector::new();
        d.assign(&[3, 1, 2, 3]);
        assert_eq!(d.as_slice(), &[3, 2, 1]);
        d.sort();
        assert_eq!(d.as_slice(), &[3, 2, 1]);
    }
}
