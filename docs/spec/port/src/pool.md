# src/pool.hpp

> [spec:cg3:def:pool.cg3.pool]
> struct pool {
>   pool_t p;
> }

> [spec:cg3:def:pool.cg3.pool.get-fn]
> auto get()

> [spec:cg3:sem:pool.cg3.pool.get-fn]
> Removes and returns one pooled pointer, or null if the pool is empty.
> The pool `p` is a `sorted_vector<T*>` (pointers kept in ascending sorted
> order). Initialize `var = nullptr`; if `p` is not empty, set
> `var = p.back()` (the last / greatest-valued pointer) and `p.pop_back()`
> to remove it. Return `var`. When `CG_TRACE_OBJECTS` is defined it also
> logs the pointer and function signature to stderr. Caller reuses the
> returned object as-is; a null result means the caller must allocate a
> fresh object. Because the backing container is sorted, "back" is the
> greatest pointer address, not necessarily the most-recently inserted.

> [spec:cg3:def:pool.cg3.pool.pool-fn]
> ~pool()

> [spec:cg3:sem:pool.cg3.pool.pool-fn]
> Destructor. Iterates every pointer currently held in `p` and `delete`s
> each one, freeing all pooled objects. The pool owns the objects it
> retains, so destroying the pool destroys them. (In a Rust port the pool
> owns `Box<T>`/objects and dropping it drops them all.)

> [spec:cg3:def:pool.cg3.pool.put-fn]
> void put(T* t)

> [spec:cg3:sem:pool.cg3.pool.put-fn]
> Returns object `t` to the pool for reuse. First calls `t->clear()` to
> reset the object's state (so `T` must provide `clear()`). Then inserts
> the pointer into the sorted_vector `p` via `p.insert(t)`, which keeps `p`
> sorted and returns `{iterator, bool inserted}`. If `ins.second == false`
> (the pointer was already present — a double-put), the code checks for it
> but does nothing: the diagnostic `throw` is commented out, so double-puts
> are silently ignored and the pointer is not stored twice (sorted_vector
> de-duplicates). When `CG_TRACE_OBJECTS` is defined it logs the pointer
> and function signature to stderr. No return value.

