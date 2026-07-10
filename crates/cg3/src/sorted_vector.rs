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
