# src/interval_vector.hpp

> [spec:cg3:def:interval-vector.cg3.interval-vector]
> class interval_vector {
>   struct interval { T lb; T ub; explicit interval(T lb = T()) : lb(lb) , ub(lb) { } explicit interval(T lb, T ub) : lb(lb) , ub(ub) { } bool operator<(const in...;
>   Cont elements;
>   size_t _size;
>   class const_iterator { private: const Cont* elements; ContConstIter it; T t; public: using iterator_category = std::bidirectional_iterator_tag; using value_t...;
> }

> [spec:cg3:def:interval-vector.cg3.interval-vector.back-fn]
> T back() const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.back-fn]
> Returns elements.back().ub — the upper bound of the last interval, i.e. the
> maximum value in the set. Undefined behaviour if empty. Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.begin-fn]
> const_iterator begin() const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.begin-fn]
> Returns const_iterator(elements, elements.begin()): an iterator positioned at the
> first interval, whose current value is that interval's lb (the minimum element),
> or an end-equivalent iterator if the container is empty. Constant time. The
> iterator expands intervals into individual integers when advanced.

> [spec:cg3:def:interval-vector.cg3.interval-vector.clear-fn]
> void clear()

> [spec:cg3:sem:interval-vector.cg3.interval-vector.clear-fn]
> Empties the set: elements.clear() drops all intervals and _size = 0 resets the
> counter (also discarding any accumulated _size drift). Constant/linear time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.const-iterator]
> class const_iterator {
>   const Cont* elements;
>   ContConstIter it;
>   T t;
> }

> [spec:cg3:def:interval-vector.cg3.interval-vector.const-iterator.const-iterator-fn]
> const_iterator(const Cont& elements, ContConstIter it)

> [spec:cg3:sem:interval-vector.cg3.interval-vector.const-iterator.const-iterator-fn]
> Constructs a const_iterator over the interval container from a container
> reference and an interval iterator `it`. Stores &elements, it, and a current
> value t initialized to T(); then, if it != elements.end(), sets t = it->lb (the
> low bound of the pointed interval). Thus the dereferenced value (operator*)
> starts at the first integer of its interval, or stays T() (0) for an end
> iterator. Advancing (operator++) walks integers within the interval up to ub then
> steps to the next interval's lb (or to the end sentinel with t = T());
> operator-- reverses this, stepping to the previous interval's ub or, from begin,
> to the end sentinel. (A three-arg overload const_iterator(elements, it, t) sets t
> directly to a caller-supplied value, used by find/lower_bound to point at an
> exact value inside an interval.) Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.const-iterator.operator-fn]
> bool operator==(const const_iterator& o) const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.const-iterator.operator-fn]
> Equality of two const_iterators: returns (it == o.it && t == o.t) — BOTH the
> underlying interval iterator AND the current scalar value t must match. (operator!=
> is its negation.) Consequences: two iterators positioned at the same value but
> holding different interval iterators compare unequal, and an end iterator has
> it == elements.end() with t == T() (0). Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.contains-fn]
> bool contains(T t) const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.contains-fn]
> Returns true iff t is in the set. it = std::lower_bound(elements, t) using the
> interval-vs-scalar comparison (interval < t means ub < t), so it is the first
> interval with ub >= t. Returns false if it == end, or if t lies outside
> [it->lb, it->ub] (checked as it->ub < t — dead after lower_bound — or it->lb > t);
> otherwise true. O(log m) where m is interval count. Unlike find it builds no
> iterator.

> [spec:cg3:def:interval-vector.cg3.interval-vector.empty-fn]
> bool empty() const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.empty-fn]
> Returns elements.empty() — true iff there are no intervals. Note it checks the
> interval container directly, NOT the (potentially wrong) _size counter, so
> empty() stays reliable even when size() has drifted. Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.end-fn]
> const_iterator end() const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.end-fn]
> Returns const_iterator(elements, elements.end()): the end sentinel, with the
> interval iterator at elements.end() and current value T() (0). Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.erase-fn]
> bool erase(T t)

> [spec:cg3:sem:interval-vector.cg3.interval-vector.erase-fn]
> Removes the single integer t from the set; returns true iff t was present.
> it = std::lower_bound(elements, t) (first interval with ub >= t). Return false if
> it == end, or if t is outside [it->lb, it->ub] (checked as it->ub < t || it->lb >
> t). Otherwise, by where t sits in the interval: (a) interval is exactly [t,t]:
> erase the whole interval; (b) t == it->ub (top edge): decrement it->ub; (c) t ==
> it->lb (bottom edge): increment it->lb; (d) strictly interior (it->lb < t <
> it->ub): SPLIT — insert a new interval [t+1, it->ub] at position it+1, then set
> it->ub = t-1. Every success path does --_size and returns true; an unreachable
> assert(false) guards the impossible fall-through. Note _size is ALWAYS decremented
> on erase, whereas insert frequently skips its increment, so size() can drift and
> even underflow. BUG/EDGE: in the split path (d) the value it->ub is read to build
> the new interval BEFORE elements.insert runs; that insert may reallocate the
> vector and invalidate `it`, after which `it->ub = t-1` writes through a possibly
> dangling iterator (latent UB when the vector is at capacity). A faithful port
> should capture the interval's index and ub first, then mutate by index.
> O(log m) search + O(m) shift.

> [spec:cg3:def:interval-vector.cg3.interval-vector.find-fn]
> const_iterator find(T t) const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.find-fn]
> Returns a const_iterator positioned exactly at value t, or end() if t is absent.
> it = std::lower_bound(elements, t) (first interval with ub >= t). If it == end, or
> t is outside [it->lb, it->ub] (it->ub < t — dead after lower_bound — or it->lb >
> t), returns end(). Otherwise returns const_iterator(elements, it, t): the
> three-arg form sets the iterator's current value directly to t, so dereferencing
> it yields t. O(log m).

> [spec:cg3:def:interval-vector.cg3.interval-vector.front-fn]
> T front() const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.front-fn]
> Returns elements.front().lb — the lower bound of the first interval, i.e. the
> minimum value in the set. Undefined behaviour if empty. Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.insert-fn]
> bool insert(T t)

> [spec:cg3:sem:interval-vector.cg3.interval-vector.insert-fn]
> Inserts the single integer t into the set, coalescing with neighbouring intervals;
> returns true iff t was newly added (false if already present). Steps:
> (1) If elements is empty, emplace a new interval [t,t] and return true — WITHOUT
> incrementing _size (bug; see note).
> (2) Otherwise it = std::lower_bound(elements, t) via interval<T (ub < t), i.e. the
> first interval whose ub >= t. If it != end and it->lb <= t <= it->ub, t is already
> present → return false (no size change).
> (3) If it == begin: if it->ub+1 == t increment it->ub (this branch is effectively
> dead here, since not-present with ub >= t forces t < it->lb <= it->ub); else if
> it->lb-1 == t decrement it->lb (extend the first interval downward by one); else
> insert a fresh [t,t] before it. Return true — again WITHOUT incrementing _size.
> (4) General case (it != begin): let pr = it-1 (preceding interval, pr->ub < t). If
> pr->ub+1 == t, extend pr upward (++pr->ub); then if it != end and now pr->ub+1 ==
> it->lb (t exactly filled the gap) merge by pr->ub = it->ub and erase(it). Else if
> it != end and it->lb == t+1, extend it downward (--it->lb); then if pr->ub+1 ==
> it->lb merge pr into it (pr->ub = it->ub, erase(it)). Else insert a fresh [t,t]
> before it. Finally ++_size and return true.
> BUG/QUIRK: _size is incremented ONLY in the general case (step 4); the
> empty-container and it==begin paths return true without incrementing, so _size
> (and thus size()) systematically under-counts, and combined with erase always
> decrementing it can underflow (wrap as size_t). O(log m) search + O(m) shift.

> [spec:cg3:def:interval-vector.cg3.interval-vector.intersect-fn]
> interval_vector intersect(const interval_vector& o) const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.intersect-fn]
> Returns a new interval_vector holding the set intersection of *this and o, leaving
> both operands unchanged. If either is empty, returns an empty result. Otherwise it
> runs a two-pointer merge over the sorted, non-overlapping interval lists a
> (this->elements) and b (o.elements): in an outer loop while both still have
> intervals, first advance a past intervals lying entirely below b (a->ub < b->lb),
> then advance b past intervals entirely below a (b->ub < a->lb); then while the
> current a and b overlap (a->ub >= b->lb && b->ub >= a->lb) emit their overlap [lb =
> max(a->lb,b->lb), ub = min(a->ub,b->ub)]: if the result's last interval is
> adjacent (rv.elements.back().ub + 1 == lb) extend it to ub, otherwise push_back a
> new interval(lb, ub); add (ub - lb + 1) to rv._size; then advance whichever input
> interval ends first (a->ub < b->ub → ++a, else ++b). Unlike insert, intersect
> maintains rv._size correctly. Result intervals are appended directly to
> rv.elements (bypassing insert) and stay sorted and coalesced. O(|a| + |b|).

> [spec:cg3:def:interval-vector.cg3.interval-vector.interval]
> struct interval {
>   T lb;
>   T ub;
> }

> [spec:cg3:def:interval-vector.cg3.interval-vector.interval-vector-fn]
> interval_vector(Iter b, const Iter& e)

> [spec:cg3:sem:interval-vector.cg3.interval-vector.interval-vector-fn]
> Range constructor: initializes _size = 0, then iterates b..e calling insert(*b)
> for each element, building the compressed interval set incrementally. Input order
> does not affect the final set (insert places each value correctly). It inherits
> insert's _size accounting bug (see insert), so after construction _size / size()
> may under-count the true number of elements. O(n) inserts, each O(log m + shift)
> where m is the current interval count. (A matching insert(b,e) range method loops
> the same way.)

> [spec:cg3:def:interval-vector.cg3.interval-vector.interval.interval-fn]
> explicit interval(T lb = T())

> [spec:cg3:sem:interval-vector.cg3.interval-vector.interval.interval-fn]
> Single-argument (also default) constructor for the inner `interval` struct. Builds
> a degenerate one-point interval by setting both bounds equal: lb = the argument
> and ub = the same argument. The parameter defaults to T() (0 for uint32_t), so a
> default-constructed interval is [0,0]. A separate two-argument constructor
> interval(T lb, T ub) sets the bounds independently. Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.interval.operator-fn]
> bool operator<(const interval& o) const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.interval.operator-fn]
> Strict ordering of one interval before another: returns (ub < o.lb) — this
> interval compares "less" iff its upper bound is strictly below the other's lower
> bound, i.e. it lies entirely below o with no overlap and not even touching.
> Overlapping or equal-bounded intervals compare as not-less in both directions. A
> sibling overload operator<(const T& o) returns (ub < o) and is the one actually
> used by std::lower_bound(elements, t) to find the first interval with ub >= t.
> Constant time.

> [spec:cg3:def:interval-vector.cg3.interval-vector.lower-bound-fn]
> const_iterator lower_bound(T t) const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.lower-bound-fn]
> Returns a const_iterator to the smallest present value >= t, or end() if none.
> it = std::lower_bound(elements, t) (first interval with ub >= t); if it == end
> return end(). The `if (it->ub < t)` block is dead given lower_bound's guarantee,
> but faithfully: it would ++it, return end() if now past the last interval, else
> set t = it->lb. Then if it->lb > t (t falls in the gap below this interval), clamp
> t up to it->lb. Return const_iterator(elements, it, t). Net effect: if t is inside
> an interval the iterator's value is t itself; if t sits in a gap it snaps to the
> next interval's lb; if t exceeds all intervals it returns end(). O(log m).

> [spec:cg3:def:interval-vector.cg3.interval-vector.push-back-fn]
> bool push_back(T t)

> [spec:cg3:sem:interval-vector.cg3.interval-vector.push-back-fn]
> Not an append: returns insert(t), performing the same coalescing set insertion and
> returning true iff t was newly added.

> [spec:cg3:def:interval-vector.cg3.interval-vector.size-fn]
> size_type size() const

> [spec:cg3:sem:interval-vector.cg3.interval-vector.size-fn]
> Returns the cached _size counter (the number of individual integers, not the
> number of intervals). WARNING: _size is maintained inconsistently — insert
> increments it only on its general path, skipping the empty-container and
> first-interval branches, while erase always decrements it. Consequently size() can
> under-count the true element total and, after enough skipped-increment inserts
> followed by erases, can even underflow (wrap around as size_t). Constant time; a
> faithful port must replicate this drift, not "fix" it to the real count.

