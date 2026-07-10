//! Port of `src/scoped_stack.hpp`.
//!
//! A stack allocator that hands out reusable temporary `C` objects via an
//! RAII [`Proxy`]. `get()` reserves the next slot (index = current depth,
//! post-incrementing the depth) and grows a backing `Vec<C>` lazily; while the
//! returned proxy is alive it derefs to that slot's `C`; when the proxy drops
//! it `clear()`s the slot and decrements the depth. Slots are reused across
//! nested scopes, so a borrowed `C` may retain capacity from a previous user
//! (it is cleared on release, not on acquire).
//!
//! Faithful port. The C++ proxy holds a back-pointer `scoped_stack* ss`; this
//! is translated 1:1 as a raw `*mut ScopedStack<C>`, which is what makes the
//! `operator->` / `operator*` / `operator C&` access (all mapped here to
//! `Deref`/`DerefMut`) and the nested-scope reuse work as in C++. A safe
//! `&mut ScopedStack` alternative would compile but forbid nested proxies
//! (the whole point of a scoped *stack*), so the pointer is retained.
//!
//! # Safety
//! Exactly as in the C++ original, this assumes proxies are created and
//! destroyed in strict LIFO order and that the owning `ScopedStack` outlives
//! every live proxy. Violating that is undefined behaviour.
//!
//! The `C: clear()` requirement reuses [`crate::pool::Poolable`] so a concrete
//! type usable in both a `Pool` and a `ScopedStack` needs only one `clear`.

use crate::pool::Poolable;

// [spec:cg3:def:scoped-stack.cg3.scoped-stack]
pub struct ScopedStack<C> {
    z: usize,
    cs: Vec<C>,
}

// [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy]
pub struct Proxy<C: Poolable> {
    z: usize,
    ss: *mut ScopedStack<C>,
}

impl<C: Poolable> Proxy<C> {
    // [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy.proxy-fn]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.proxy-fn]
    fn new(ss: *mut ScopedStack<C>) -> Self
    where
        C: Default,
    {
        // SAFETY: `ss` refers to a live ScopedStack that outlives this proxy;
        // proxies are created/destroyed in strict LIFO order (module note).
        unsafe {
            let s = &mut *ss;
            let z = s.z;
            s.z += 1;
            if s.cs.len() < s.z {
                s.cs.resize_with(s.z, C::default);
            }
            Proxy { z, ss }
        }
    }
}

// [spec:cg3:def:scoped-stack.cg3.scoped-stack.proxy.operator-fn]
// [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.operator-fn]
// C++ `operator->` returns `&ss->cs[z]`; the sibling `operator*` and implicit
// `operator C&` return the reference `ss->cs[z]`. All three map to Deref /
// DerefMut here.
impl<C: Poolable> core::ops::Deref for Proxy<C> {
    type Target = C;

    fn deref(&self) -> &C {
        // SAFETY: see Proxy::new.
        unsafe {
            let s = &*self.ss;
            &s.cs[self.z]
        }
    }
}

impl<C: Poolable> core::ops::DerefMut for Proxy<C> {
    fn deref_mut(&mut self) -> &mut C {
        // SAFETY: see Proxy::new.
        unsafe {
            let s = &mut *self.ss;
            &mut s.cs[self.z]
        }
    }
}

impl<C: Poolable> Drop for Proxy<C> {
    fn drop(&mut self) {
        // C++ ~proxy(): ss->cs[z].clear(); --ss->z;
        // SAFETY: see Proxy::new.
        unsafe {
            let s = &mut *self.ss;
            s.cs[self.z].clear();
            s.z -= 1;
        }
    }
}

impl<C: Poolable> ScopedStack<C> {
    // [spec:cg3:def:scoped-stack.cg3.scoped-stack.scoped-stack-fn]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.scoped-stack-fn]
    pub fn new() -> Self {
        ScopedStack { z: 0, cs: Vec::new() }
    }

    // [spec:cg3:def:scoped-stack.cg3.scoped-stack.get-fn]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.get-fn]
    pub fn get(&mut self) -> Proxy<C>
    where
        C: Default,
    {
        Proxy::new(self as *mut Self)
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

    /// Minimal `Poolable + Default` slot type: a growable string whose `clear`
    /// empties it (so we can observe "cleared on release, not on acquire").
    #[derive(Default)]
    struct Slot {
        s: String,
    }

    impl Poolable for Slot {
        fn clear(&mut self) {
            self.s.clear();
        }
    }

    // `new()` builds an empty stack; `get()` hands out a `Proxy` (via
    // `Proxy::new`, which reserves the next slot and post-increments depth); the
    // proxy derefs (operator-> / operator* / operator C&, all Deref/DerefMut) to
    // that slot; and dropping the proxy clears the slot and pops the depth.
    // Nested `get()` reserves a deeper slot without disturbing the outer one.
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.scoped-stack-fn/test]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.get-fn/test]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.proxy-fn/test]
    // [spec:cg3:sem:scoped-stack.cg3.scoped-stack.proxy.operator-fn/test]
    #[test]
    fn nested_scopes_and_deref() {
        let mut ss: ScopedStack<Slot> = ScopedStack::new();
        {
            // Outer scope reserves slot 0.
            let mut a = ss.get();
            a.s.push_str("outer"); // DerefMut -> slot 0
            assert_eq!(a.s, "outer"); // Deref -> slot 0
            {
                // Nested scope reserves slot 1 while `a` (slot 0) stays live.
                let mut b = ss.get();
                assert_eq!(b.s, "", "a fresh deeper slot starts empty");
                b.s.push_str("inner");
                assert_eq!(b.s, "inner");
                // Outer proxy still refers to its own slot, unchanged.
                assert_eq!(a.s, "outer");
            } // `b` dropped: slot 1 cleared, depth back to 1.
            // Outer still valid.
            assert_eq!(a.s, "outer");
        } // `a` dropped: slot 0 cleared, depth back to 0.

        // Reuse: the next get() re-hands slot 0, which was cleared on release
        // (not on acquire), so it comes back empty even though we wrote to it.
        let c = ss.get();
        assert_eq!(c.s, "", "released slot was cleared before reuse");
    }
}
