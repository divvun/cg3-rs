# src/sorted_vector.hpp

> [spec:cg3:def:sorted-vector.cg3.detail.is-sorted-fn]
> bool is_sorted(ForwardIt first, ForwardIt last, Comp comp)

> [spec:cg3:sem:sorted-vector.cg3.detail.is-sorted-fn]
> Custom reimplementation of std::is_sorted for the range [first, last) under the
> strict-weak-ordering predicate comp. If the range is empty (first == last),
> returns true immediately. Otherwise it walks adjacent pairs: set next = first,
> then repeatedly ++next; while next != last, evaluate comp(*next, *first) — "is
> the later element strictly less than the earlier one" — and return false the
> instant it holds (an out-of-order pair); otherwise advance first = next and
> continue. Returns true if no inversion is found (also true for single-element
> ranges). O(n) comparisons over a ForwardIt range; no side effects.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector]
> class sorted_vector {
>   container elements;
>   Comp comp;
> }

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.assign-fn]
> void assign(It b, It e)

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.assign-fn]
> Replaces the whole contents with the sorted, de-duplicated form of the input
> range [b, e). Implemented as clear() followed by the range insert(b, e). That
> range-insert works as: d = distance(b, e); if d == 1 it just does a single
> insert(*b) and returns. Otherwise it builds into a static thread_local buffer
> `merged` (reset to size 0, reserved to elements.size()+d): if
> detail::is_sorted(b,e,comp) it std::merge's the current elements with [b,e) into
> merged; otherwise it copies [b,e) into another static thread_local buffer
> `sorted`, std::sort's it with comp, then std::merge's elements with sorted into
> merged. It then swaps merged into elements and runs std::unique + erase to drop
> consecutive duplicates. Because assign clears first, the merge's left input is
> empty, so the net result is exactly [b,e) sorted by comp with adjacent duplicates
> removed. Note the reuse of per-thread thread_local scratch buffers. A separate
> assign(const_iterator, const_iterator) overload instead does a raw
> elements.assign(b,e), trusting the range is already sorted and unique. O(n log n)
> for unsorted input, else O(n).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.back-fn]
> T back() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.back-fn]
> Returns a copy of elements.back(), i.e. the largest element. Undefined behaviour
> if the container is empty. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.begin-fn]
> iterator begin()

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.begin-fn]
> Returns elements.begin(), a mutable iterator to the first (smallest) element, or
> end() if empty. Constant time. (A const overload returning a const_iterator also
> exists.)

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.capacity-fn]
> size_type capacity() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.capacity-fn]
> Returns elements.capacity(), the allocated storage capacity of the underlying
> vector (always >= size()). Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.cbegin-fn]
> const_iterator cbegin() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.cbegin-fn]
> Returns elements.cbegin(), a const_iterator to the first element (or cend() if
> empty). Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.cend-fn]
> const_iterator cend() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.cend-fn]
> Returns elements.cend(), the const one-past-last iterator. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.clear-fn]
> void clear()

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.clear-fn]
> Calls elements.clear(), removing all elements so size() becomes 0. Capacity is
> left unchanged. Linear in element destructor calls; for trivial T effectively
> O(1).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.const-iterator]
> typedef typename container::const_iterator const_iterator

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.const-reverse-iterator]
> typedef typename container::const_reverse_iterator const_reverse_iterator

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.container]
> typedef typename std::vector<T> container

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.contains-fn]
> bool contains(T t) const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.contains-fn]
> Returns true iff t is present, computed as (find(t) != end()). O(log n).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.count-fn]
> size_type count(T t) const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.count-fn]
> Returns 1 if t is present, else 0 — computed as (find(t) != end()) implicitly
> converted to size_type. Because the container holds unique elements the count is
> never greater than 1 (set semantics). O(log n).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.empty-fn]
> bool empty() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.empty-fn]
> Returns elements.empty(), true iff size() == 0. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.end-fn]
> iterator end()

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.end-fn]
> Returns elements.end(), the one-past-last iterator. Constant time. (A const
> overload returning a const_iterator also exists.)

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.erase-fn]
> bool erase(T t)

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.erase-fn]
> Removes the element equal to t if present; returns true iff something was erased.
> Returns false immediately when: elements is empty; comp(back(), t) is true (t is
> greater than the max, so absent); or comp(t, front()) is true (t is less than the
> min, so absent). Otherwise it = lower_bound(t) (the non-const member overload →
> std::lower_bound with comp). If it != end() and *it equals t (both !comp(*it,t)
> and !comp(t,*it) hold), erase that single element and return true; else return
> false. Erasing shifts all following elements one slot left. O(log n) search +
> O(n) shift. (A separate erase(const_iterator) overload erases by position, and a
> range erase(b,e) loops erase(*b) over the range.)

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.erase-n-fn]
> void erase_n(size_type i)

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.erase-n-fn]
> Erases the element at 0-based index i via elements.erase(begin()+i), removing
> that position and shifting every later element one slot left. No bounds check is
> performed: if i >= size() this is undefined behaviour (the port should treat i as
> a trusted in-range index, matching the C++). O(n) shift.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.find-fn]
> const_iterator find(T t) const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.find-fn]
> Looks up t and returns a const_iterator to it, or end() if absent. Returns end()
> immediately if elements is empty, if comp(back(), t) (t above the max), or if
> comp(t, front()) (t below the min). Otherwise it = lower_bound(t) (const
> overload). If it != end() and *it is not equal to t (comp(*it,t) || comp(t,*it),
> effectively t < *it), returns end(); otherwise returns it, pointing at the
> matching element. O(log n).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.find-n-fn]
> size_type find_n(T t) const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.find-n-fn]
> Returns the 0-based index of t as distance(begin(), find(t)). When t is present
> this is its position; when find(t) returns end() the distance equals size(), so
> find_n yields size() as the "not found" sentinel. O(log n).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.front-fn]
> T front() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.front-fn]
> Returns a copy of elements.front(), i.e. the smallest element. Undefined
> behaviour if the container is empty. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.insert-fn]
> std::pair<iterator, bool> insert(T t)

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.insert-fn]
> Inserts t while keeping `elements` sorted by comp and free of duplicates;
> returns std::pair<iterator,bool> whose bool is true iff a new element was added.
> If elements is empty, push_back(t) and return (begin(), true). Otherwise compute
> it = std::lower_bound(begin, end, t, comp) — the first position whose element is
> not less than t — and record at = distance(begin, it) BEFORE any mutation
> (because reallocation invalidates it). If it == end (t is greater than every
> element), emplace_back(t) and return (begin()+at, true). Else test comp(*it, t)
> || comp(t, *it): the first disjunct is always false at a lower_bound result, so
> this effectively means "t < *it" (t absent) — insert(it, t) and return
> (begin()+at, true). If neither comparison holds, *it equals t (already present):
> return (begin()+at, false) with no insertion. The returned iterator is recomputed
> as begin()+at because the original `it` may be invalidated by the insert/emplace.
> O(log n) search + O(n) shift. (There is also a separate range overload insert(b,e)
> that merges/sorts/uniques a whole range; see assign.)

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.iterator]
> typedef typename container::iterator iterator

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.key-type]
> typedef T key_type

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.lower-bound-fn]
> iterator lower_bound(T t)

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.lower-bound-fn]
> Returns std::lower_bound(begin, end, t, comp): the first position whose element
> is not less than t (the leftmost element >= t under comp), or end() if every
> element is less than t. Binary search, O(log n). Two overloads exist (a mutable
> `iterator` when the vector is non-const, a const_iterator when const); both call
> std::lower_bound with the member comp.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.pop-back-fn]
> void pop_back()

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.pop-back-fn]
> Removes the last (largest) element via elements.pop_back(), reducing size by one.
> Undefined behaviour if the container is empty. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.push-back-fn]
> void push_back(T t)

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.push-back-fn]
> Despite the name this does NOT append to the back; it simply calls insert(t),
> performing a sorted, duplicate-suppressing insertion into `elements`. The
> std::pair returned by insert is discarded (return type void). A faithful port
> must route push_back through the same sorted-insert logic, not a raw append.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.rbegin-fn]
> const_reverse_iterator rbegin() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.rbegin-fn]
> Returns elements.rbegin(), a const_reverse_iterator to the last (largest)
> element — the start of reverse traversal. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.rend-fn]
> const_reverse_iterator rend() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.rend-fn]
> Returns elements.rend(), the const_reverse_iterator one-before-first — the end
> of reverse traversal. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.size-fn]
> size_type size() const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.size-fn]
> Returns elements.size(), the number of stored elements. Constant time.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.size-type]
> typedef typename container::size_type size_type

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.sort-fn]
> void sort()

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.sort-fn]
> Sorts `elements` in place with std::sort using a freshly default-constructed
> Comp() — NOT the stored `comp` member (a distinction only relevant if Comp were
> stateful). Does not remove duplicates. Normally redundant because the invariant
> already keeps elements sorted; it exists to re-establish order after direct
> mutation via get()/at(). O(n log n).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.sorted-vector-fn]
> sorted_vector()

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.sorted-vector-fn]
> Default constructor. Produces an empty sorted_vector: the underlying `elements`
> std::vector is empty and the `comp` comparator member is default-constructed
> (std::less<T> by default). In normal builds this is the implicit/defaulted
> constructor with no body. Only when the CG_TRACE_OBJECTS macro is defined is it
> user-provided, in which case it additionally writes a debug line ("OBJECT:
> <this-ptr> <pretty-function>") to std::cerr (and the matching destructor logs
> the pointer plus elements.size()). A faithful port just yields an empty
> container; the tracing output is a debug-only side effect.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.swap-fn]
> void swap(sorted_vector& other)

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.swap-fn]
> Swaps the underlying storage with `other` via elements.swap(other.elements) —
> O(1), just exchanges the two vectors' internals. Note the `comp` comparator
> members are NOT swapped; each object keeps its own comp, which only matters if
> the two comparators differ in state (they are stateless std::less by default).

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.upper-bound-fn]
> const_iterator upper_bound(T t) const

> [spec:cg3:sem:sorted-vector.cg3.sorted-vector.upper-bound-fn]
> Returns std::upper_bound(begin, end, t, comp): the first position whose element
> is strictly greater than t (the leftmost element for which t < element), or end()
> if none. Binary search, O(log n), const_iterator result.

> [spec:cg3:def:sorted-vector.cg3.sorted-vector.value-type]
> typedef T value_type

> [spec:cg3:def:sorted-vector.cg3.uint32-sorted-vector]
> typedef sorted_vector<uint32_t> uint32SortedVector

