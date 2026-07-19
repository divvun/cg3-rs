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

        self.p.pop()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Minimal `Poolable`: records how many times it was `clear`ed, and bumps a
    /// shared counter when it is dropped so the pool's destructor loop can be
    /// observed.
    struct Widget {
        cleared: u32,
        payload: i32,
        drops: Rc<Cell<u32>>,
    }

    impl Poolable for Widget {
        fn clear(&mut self) {
            self.cleared += 1;
            self.payload = 0;
        }
    }

    impl Drop for Widget {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    // `new()`/`get()`/`put()` together: an empty pool yields `None`; `put`
    // clears the object then stores it; `get` returns the most-recently-put box
    // (LIFO / `Vec::pop`) — the documented deviation from the C++ address-sorted
    // `back()`. Also asserts `put` invoked `clear` exactly once.
    // [spec:cg3:sem:pool.cg3.pool.get-fn/test]
    // [spec:cg3:sem:pool.cg3.pool.put-fn/test]
    #[test]
    fn get_put_lifo_and_clear_on_put() {
        let drops = Rc::new(Cell::new(0u32));
        let mut pool: Pool<Widget> = Pool::new();

        // Empty pool -> get() is None.
        assert!(pool.get().is_none());

        // put() must clear() the object (resetting payload) before storing.
        pool.put(Box::new(Widget {
            cleared: 0,
            payload: 42,
            drops: drops.clone(),
        }));
        pool.put(Box::new(Widget {
            cleared: 0,
            payload: 99,
            drops: drops.clone(),
        }));

        // LIFO: the second-put widget comes back first. Its payload was zeroed
        // by clear(), and clear() ran exactly once.
        let a = pool.get().expect("one available");
        assert_eq!(a.payload, 0, "put() cleared the payload");
        assert_eq!(a.cleared, 1, "put() called clear() exactly once");
        // Return it; clear() runs again on put.
        pool.put(a);
        let a = pool.get().unwrap();
        assert_eq!(a.cleared, 2);

        // Drain the rest.
        let b = pool.get().expect("second available");
        assert!(pool.get().is_none());
        drop(a);
        drop(b);
    }

    // `Pool::drop` runs the C++ destructor `delete` loop: every object still
    // retained in the pool is dropped when the pool is dropped. Driven by
    // filling a pool, then dropping it and observing the shared drop counter.
    // [spec:cg3:sem:pool.cg3.pool.pool-fn/test]
    #[test]
    fn drop_releases_retained_objects() {
        let drops = Rc::new(Cell::new(0u32));
        {
            let mut pool: Pool<Widget> = Pool::default();
            pool.put(Box::new(Widget {
                cleared: 0,
                payload: 1,
                drops: drops.clone(),
            }));
            pool.put(Box::new(Widget {
                cleared: 0,
                payload: 2,
                drops: drops.clone(),
            }));
            pool.put(Box::new(Widget {
                cleared: 0,
                payload: 3,
                drops: drops.clone(),
            }));
            assert_eq!(drops.get(), 0, "nothing dropped while pool is alive");
        } // pool dropped here -> all three retained widgets dropped.
        assert_eq!(
            drops.get(),
            3,
            "pool destructor dropped every retained object"
        );
    }
}
