//! Port of `src/scoped_stack.hpp` — wave 4 (safe) form.
//!
//! The C++ scoped_stack hands out reusable temporary `C` objects via an RAII
//! proxy holding a raw back-pointer (`scoped_stack* ss`): `get()` reserves the
//! next slot, the proxy derefs to it, and `~proxy()` clears the slot and pops
//! the depth. The observable contract at every call site is simply "give me a
//! CLEARED scratch `C`, and recycle its capacity afterwards" — proxies are
//! strictly scope-local. Wave 4 (`w4-unsafe-elimination`) replaces the
//! raw-pointer proxy with a safe spare-list: [`ScopedStack::get`] returns an
//! OWNED cleared `C` (recycling a spare's capacity when one is available) and
//! [`ScopedStack::put`] clears and returns it. A value that is dropped instead
//! of `put` back merely forgoes the capacity reuse — semantics are unchanged
//! (acquired objects are always empty, exactly as the C++ clear-on-release
//! guaranteed).
//!
//! Dissolved with the raw pointer: the `proxy` type itself and its
//! `operator->`/`operator*`/`operator C&`/destructor
//! ([spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy]
//! [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy.proxy-fn]
//! [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.proxy-fn]
//! [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy.operator-fn]
//! [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.operator-fn]) — the owned
//! value IS the proxy now; `put`/drop is the destructor.
//!
//! The `C: clear()` requirement reuses [`crate::pool::Poolable`] so a concrete
//! type usable in both a `Pool` and a `ScopedStack` needs only one `clear`.

use crate::pool::Poolable;

// [spec:cg3:def:scoped-stack.cg3.scoped-stack]
pub struct ScopedStack<C> {
    /// Cleared spare objects (capacity recycling).
    cs: Vec<C>,
}

impl<C: Poolable> ScopedStack<C> {
    // [spec:cg3:def:scoped-stack.cg3.scoped-stack.scoped-stack-fn]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.scoped-stack-fn]
    pub fn new() -> Self {
        ScopedStack { cs: Vec::new() }
    }

    // [spec:cg3:def:scoped-stack.cg3.scoped-stack.get-fn]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.get-fn]
    /// A cleared scratch `C` — a recycled spare when available (its capacity
    /// retained from a previous user, contents cleared), else a fresh default.
    pub fn get(&mut self) -> C
    where
        C: Default,
    {
        self.cs.pop().unwrap_or_default()
    }

    /// Return a scratch object (the C++ `~proxy()`): clear it and keep it as a
    /// spare. Dropping the value instead is allowed (capacity loss only).
    pub fn put(&mut self, mut c: C) {
        c.clear();
        self.cs.push(c);
    }
}

impl<C: Poolable + Default> Default for ScopedStack<C> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Poolable for Vec<u32> {
        fn clear(&mut self) {
            Vec::clear(self);
        }
    }

    // get() hands out cleared scratch objects; put() recycles capacity (the
    // C++ proxy's clear-on-release), and nested get()s coexist safely.
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.scoped-stack-fn/test]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.get-fn/test]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.proxy-fn/test]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.operator-fn/test]
    #[test]
    fn get_put_recycles_cleared() {
        let mut ss: ScopedStack<Vec<u32>> = ScopedStack::new();
        let mut a = ss.get();
        a.extend([1, 2, 3]);
        let cap = a.capacity();
        // Nested scratch while `a` is live (the C++ nested-proxy case).
        let mut b = ss.get();
        b.push(9);
        ss.put(b);
        ss.put(a);
        // Recycled spare comes back CLEARED with capacity retained (LIFO).
        let c = ss.get();
        assert!(c.is_empty(), "clear-on-release");
        assert_eq!(c.capacity(), cap, "capacity recycled");
    }
}
