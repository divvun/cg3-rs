//! Port of `src/pool.hpp`.
//!
//! A LIFO object pool. The C++ `pool<T>` keeps raw `T*` in a
//! `sorted_vector<T*>` (pointers held in ascending address order, de-duped),
//! hands them back out via `get()`, and `delete`s all remaining ones in its
//! destructor. `put()` first calls `t->clear()` to reset the object.
//!
//! Faithful port as a generic owning free-list. Pooled ownership is a
//! `Vec<Box<T>>`: `get()` yields `Option<Box<T>>` (the boxed object, or `None`
//! when empty), `put(Box<T>)` clears then stores it, and dropping the pool
//! drops every retained object (the C++ destructor's `delete` loop).
//!
//! The `t->clear()` requirement is expressed by the [`Poolable`] trait bound;
//! the concrete `Cohort`/`Reading`/`Window` types (later layers) implement it.
//!
//! Literal deviations (noted for parity review):
//! - The backing store is a plain `Vec`, not a *sorted* set. `get()` returns
//!   the most-recently `put` object (true LIFO / `Vec::pop`), whereas the C++
//!   `back()` is the greatest *pointer address*, not necessarily the newest.
//! - The sorted_vector de-duplicated pointers so a double-`put` was silently
//!   ignored. With owned `Box<T>` a double-`put` of the *same* object is not
//!   representable (ownership is moved), so that de-dup / commented-out
//!   `throw` diagnostic has no analogue and is omitted.
//! - The `CG_TRACE_OBJECTS` stderr logging is compile-time optional and omitted.

// A type that can be reset before being returned to a pool.
pub trait Poolable {
    fn clear(&mut self);
}

// [spec:cg3:def:pool.cg3.pool]
pub struct Pool<T> {
    p: Vec<Box<T>>,
}

impl<T> Pool<T> {
    pub fn new() -> Self {
        Pool { p: Vec::new() }
    }

    // [spec:cg3:def:pool.cg3.pool.get-fn]
    // [spec:cg3:sem:pool.cg3.pool.get-fn]
    pub fn get(&mut self) -> Option<Box<T>> {
        // C++: var = nullptr; if (!p.empty()) { var = p.back(); p.pop_back(); }
        // `Vec::pop` removes and returns the last (LIFO) element, or `None`.
        let var = self.p.pop();
        var
    }

    // [spec:cg3:def:pool.cg3.pool.put-fn]
    // [spec:cg3:sem:pool.cg3.pool.put-fn]
    pub fn put(&mut self, mut t: Box<T>)
    where
        T: Poolable,
    {
        t.clear();
        self.p.push(t);
    }
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}

// [spec:cg3:def:pool.cg3.pool.pool-fn]
// [spec:cg3:sem:pool.cg3.pool.pool-fn]
impl<T> Drop for Pool<T> {
    fn drop(&mut self) {
        // C++: for (auto it : p) { delete it; }
        for it in self.p.drain(..) {
            drop(it);
        }
    }
}
