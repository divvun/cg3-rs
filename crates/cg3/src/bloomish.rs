//! Port of `src/bloomish.hpp`.
//!
//! A tiny bloom-filter-like membership bitset over an integer type `Cont`.
//! Four buckets are selected by the low three bits of a value in strict
//! priority order; each `insert` OR-accumulates the whole value into the
//! chosen bucket and `matches` tests whether every set bit of a value is
//! present in its bucket. It yields false positives but never false negatives.
//!
//! Faithful port of the `template<typename Cont>` class. To express the
//! generic integer arithmetic (`v & 4`, `|=`, `== v`, `static_cast<Cont>(0)`)
//! without external crates, the element type is bounded on the relevant
//! `core::ops` traits plus `From<u8>` (used for the literals `0`, `1`, `2`,
//! `4`). The only C++ instantiation is `bloomish<uint32_t>`.

use core::ops::{BitAnd, BitOrAssign};

// [spec:cg3:def:bloomish.cg3.bloomish]
#[derive(Clone, Copy)]
pub struct Bloomish<Cont> {
    value: [Cont; 4],
}

impl<Cont> Bloomish<Cont>
where
    Cont: Copy + PartialEq + From<u8> + BitAnd<Output = Cont> + BitOrAssign,
{
    // [spec:cg3:def:bloomish.cg3.bloomish.bloomish-fn]
    // [spec:cg3:sem:bloomish.cg3.bloomish.bloomish-fn]
    pub fn new() -> Self {
        let mut b = Bloomish { value: [Cont::from(0u8); 4] };
        b.clear();
        b
    }

    // [spec:cg3:def:bloomish.cg3.bloomish.clear-fn]
    // [spec:cg3:sem:bloomish.cg3.bloomish.clear-fn]
    pub fn clear(&mut self) {
        self.value = [Cont::from(0u8); 4];
    }

    // [spec:cg3:def:bloomish.cg3.bloomish.insert-fn]
    // [spec:cg3:sem:bloomish.cg3.bloomish.insert-fn]
    pub fn insert(&mut self, v: Cont) {
        if v & Cont::from(4u8) != Cont::from(0u8) {
            self.value[3] |= v;
        }
        else if v & Cont::from(2u8) != Cont::from(0u8) {
            self.value[2] |= v;
        }
        else if v & Cont::from(1u8) != Cont::from(0u8) {
            self.value[1] |= v;
        }
        else {
            self.value[0] |= v;
        }
    }

    // [spec:cg3:def:bloomish.cg3.bloomish.matches-fn]
    // [spec:cg3:sem:bloomish.cg3.bloomish.matches-fn]
    pub fn matches(&self, v: Cont) -> bool {
        if v & Cont::from(4u8) != Cont::from(0u8) {
            return self.value[3] & v == v;
        }
        else if v & Cont::from(2u8) != Cont::from(0u8) {
            return self.value[2] & v == v;
        }
        else if v & Cont::from(1u8) != Cont::from(0u8) {
            return self.value[1] & v == v;
        }
        self.value[0] & v == v
    }
}

impl<Cont> Default for Bloomish<Cont>
where
    Cont: Copy + PartialEq + From<u8> + BitAnd<Output = Cont> + BitOrAssign,
{
    fn default() -> Self {
        Self::new()
    }
}

// [spec:cg3:def:bloomish.cg3.uint32-bloomish]
pub type Uint32Bloomish = Bloomish<u32>;
