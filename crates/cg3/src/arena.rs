//! Arena storage + typed indices — the port's faithful replacement for C++ raw
//! pointers.
//!
//! CG-3 allocates `Tag`/`Set`/`Rule`/`ContextualTest` from the `Grammar` and
//! pools `Cohort`/`Reading`/`SingleWindow` at runtime, referring to all of them
//! by raw pointer. This port replaces `T*` with a typed index into an
//! [`Arena<T>`]:
//!
//! * **Grammar-owned** arenas (static, built at parse time, stable during
//!   application): tags, sets, rules, contextual tests.
//! * **Runtime-owned** arenas (dynamic, created/destroyed while applying rules,
//!   pooled): single-windows, cohorts, readings.
//!
//! Freed slots are reused (mirroring the C++ object pools). Indices are plain
//! `u32` with **no generation counter**, faithfully mirroring CG-3's pointer
//! reuse (a stale id can resolve to a recycled object — bug-for-bug with a
//! dangling/recycled `T*`). Nullable pointer fields (`T* x = nullptr`) become
//! `Option<XId>`; a raw sentinel of `0`/`u32::MAX` is used only where CG-3 used
//! a numeric sentinel rather than a null pointer.

use std::ops::{Index, IndexMut};

/// Index-addressable slab with free-list slot reuse.
///
/// Not a manifest symbol — port infrastructure standing in for the C++ object
/// pools + the implicit `T*` address space.
pub struct Arena<T> {
    slots: Vec<Option<T>>,
    free: Vec<u32>,
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Arena { slots: Vec::new(), free: Vec::new() }
    }

    /// Allocate `value`, reusing a freed slot if one exists (LIFO, like the
    /// pools). Returns its index.
    pub fn alloc(&mut self, value: T) -> u32 {
        if let Some(i) = self.free.pop() {
            self.slots[i as usize] = Some(value);
            i
        } else {
            let i = self.slots.len() as u32;
            self.slots.push(Some(value));
            i
        }
    }

    /// Free the slot, returning the value. The index goes on the free-list for
    /// reuse (matching pool `put`).
    pub fn free_slot(&mut self, i: u32) -> Option<T> {
        let v = self.slots[i as usize].take();
        if v.is_some() {
            self.free.push(i);
        }
        v
    }

    pub fn get(&self, i: u32) -> &T {
        self.slots[i as usize].as_ref().expect("arena slot freed/empty")
    }

    pub fn get_mut(&mut self, i: u32) -> &mut T {
        self.slots[i as usize].as_mut().expect("arena slot freed/empty")
    }

    pub fn try_get(&self, i: u32) -> Option<&T> {
        self.slots.get(i as usize).and_then(|s| s.as_ref())
    }

    /// Highest allocated index + 1 (slots ever created, including freed).
    pub fn capacity(&self) -> u32 {
        self.slots.len() as u32
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena::new()
    }
}

impl<T> Index<u32> for Arena<T> {
    type Output = T;
    fn index(&self, i: u32) -> &T {
        self.get(i)
    }
}

impl<T> IndexMut<u32> for Arena<T> {
    fn index_mut(&mut self, i: u32) -> &mut T {
        self.get_mut(i)
    }
}

/// Declare a typed arena index newtype over `u32`.
///
/// Type-distinct ids (a `TagId` cannot be used where a `CohortId` is expected)
/// give back some of the safety C++ `T*` lacked, for free.
macro_rules! typed_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
        pub struct $name(pub u32);

        impl $name {
            #[inline]
            pub fn raw(self) -> u32 { self.0 }
        }
    };
}

// --- Grammar-owned ids (static grammar objects) ---
typed_id!(/// Index into `Grammar`'s tag arena (C++ `Tag*`).
    TagId);
typed_id!(/// Index into `Grammar`'s set arena (C++ `Set*`).
    SetId);
typed_id!(/// Index into `Grammar`'s rule arena (C++ `Rule*`).
    RuleId);
typed_id!(/// Index into `Grammar`'s contextual-test arena (C++ `ContextualTest*`).
    CtxId);

// --- Runtime-owned ids (pooled during application) ---
typed_id!(/// Index into the runtime cohort arena (C++ `Cohort*`).
    CohortId);
typed_id!(/// Index into the runtime reading arena (C++ `Reading*`).
    ReadingId);
typed_id!(/// Index into the runtime single-window arena (C++ `SingleWindow*`).
    SwId);
