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
//! Freed slots are reused (mirroring the C++ object pools). Grammar-owned
//! indices are plain `u32`; the RUNTIME ids (`CohortId`/`ReadingId`/`SwId`)
//! resolve through [`GenArena`], which packs an 8-bit GENERATION into the id
//! (wave 4): a stale id — the analog of a dangling/recycled `T*` — is
//! DETECTED (panic on `get`/`get_mut`, `None` from `try_get`) instead of
//! silently aliasing the recycled object. Nullable pointer fields
//! (`T* x = nullptr`) become `Option<XId>`; a raw sentinel of `0`/`u32::MAX`
//! is used only where CG-3 used a numeric sentinel rather than a null pointer.

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


/// Generational arena: like [`Arena`], but each id packs `(generation << 24) |
/// slot_index`, and every resolution checks that the slot's current generation
/// matches the id's. Freeing a slot bumps its generation (wrapping u8; the
/// 256-reuse ABA window is documented and acceptable for the runtime pools),
/// so a stale id — the C++ dangling/recycled pointer — panics on `get`/
/// `get_mut` and yields `None` from `try_get`, instead of silently resolving
/// to the recycled object. Freeing with a stale id is a no-op (matching the
/// pool's silently-ignored duplicate `put`).
///
/// Not a manifest symbol — port infrastructure (wave 4, `w4-arena-generations`).
pub struct GenArena<T> {
    slots: Vec<Option<T>>,
    gens: Vec<u8>,
    free: Vec<u32>,
}

const GEN_SHIFT: u32 = 24;
const INDEX_MASK: u32 = (1 << GEN_SHIFT) - 1;

impl<T> GenArena<T> {
    pub fn new() -> Self {
        GenArena { slots: Vec::new(), gens: Vec::new(), free: Vec::new() }
    }

    /// The slot index carried by a packed id.
    #[inline]
    pub fn index_of(id: u32) -> u32 {
        id & INDEX_MASK
    }

    #[inline]
    fn gen_of(id: u32) -> u8 {
        (id >> GEN_SHIFT) as u8
    }

    #[inline]
    fn pack(idx: u32, generation: u8) -> u32 {
        ((generation as u32) << GEN_SHIFT) | idx
    }

    /// Allocate `value`, reusing a freed slot if one exists (LIFO, like the
    /// pools). Returns the packed id (index + current generation).
    pub fn alloc(&mut self, value: T) -> u32 {
        if let Some(i) = self.free.pop() {
            self.slots[i as usize] = Some(value);
            Self::pack(i, self.gens[i as usize])
        } else {
            let i = self.slots.len() as u32;
            assert!(i <= INDEX_MASK, "GenArena: slot index space exhausted");
            self.slots.push(Some(value));
            self.gens.push(0);
            Self::pack(i, 0)
        }
    }

    /// Whether allocating would REUSE a pooled slot (the C++ `pool.get()`
    /// returned-a-cleared-object signal `alloc_reading_copy` needs).
    pub fn will_reuse(&self) -> bool {
        !self.free.is_empty()
    }

    /// Free the slot, returning the value and BUMPING the slot's generation
    /// (invalidating every outstanding id for it). A stale or already-freed id
    /// is a no-op (the pool's silently-ignored duplicate insert).
    pub fn free_slot(&mut self, id: u32) -> Option<T> {
        let i = Self::index_of(id) as usize;
        if i >= self.slots.len() || self.gens[i] != Self::gen_of(id) {
            return None;
        }
        let v = self.slots[i].take();
        if v.is_some() {
            self.gens[i] = self.gens[i].wrapping_add(1);
            self.free.push(i as u32);
        }
        v
    }

    #[inline]
    fn check(&self, id: u32) -> usize {
        let i = Self::index_of(id) as usize;
        assert!(
            i < self.slots.len() && self.gens[i] == Self::gen_of(id),
            "GenArena: stale id {id:#x} (slot {i}, id gen {}, slot gen {}) — \
             the C++ equivalent is a dangling pointer to a recycled object",
            Self::gen_of(id),
            self.gens.get(i).copied().unwrap_or(0),
        );
        i
    }

    pub fn get(&self, id: u32) -> &T {
        let i = self.check(id);
        self.slots[i].as_ref().expect("GenArena slot freed/empty")
    }

    pub fn get_mut(&mut self, id: u32) -> &mut T {
        let i = self.check(id);
        self.slots[i].as_mut().expect("GenArena slot freed/empty")
    }

    pub fn try_get(&self, id: u32) -> Option<&T> {
        let i = Self::index_of(id) as usize;
        if i >= self.slots.len() || self.gens[i] != Self::gen_of(id) {
            return None;
        }
        self.slots[i].as_ref()
    }

    /// Highest allocated slot index + 1 (slots ever created, including freed).
    pub fn capacity(&self) -> u32 {
        self.slots.len() as u32
    }
}

impl<T> Default for GenArena<T> {
    fn default() -> Self {
        GenArena::new()
    }
}

impl<T> Index<u32> for GenArena<T> {
    type Output = T;
    fn index(&self, i: u32) -> &T {
        self.get(i)
    }
}

impl<T> IndexMut<u32> for GenArena<T> {
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
