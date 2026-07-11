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
        let mut b = Bloomish {
            value: [Cont::from(0u8); 4],
        };
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
        } else if v & Cont::from(2u8) != Cont::from(0u8) {
            self.value[2] |= v;
        } else if v & Cont::from(1u8) != Cont::from(0u8) {
            self.value[1] |= v;
        } else {
            self.value[0] |= v;
        }
    }

    // [spec:cg3:def:bloomish.cg3.bloomish.matches-fn]
    // [spec:cg3:sem:bloomish.cg3.bloomish.matches-fn]
    pub fn matches(&self, v: Cont) -> bool {
        if v & Cont::from(4u8) != Cont::from(0u8) {
            return self.value[3] & v == v;
        } else if v & Cont::from(2u8) != Cont::from(0u8) {
            return self.value[2] & v == v;
        } else if v & Cont::from(1u8) != Cont::from(0u8) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // `new()` builds a zeroed filter (via `clear`), `insert` OR-accumulates a
    // value into the bucket chosen by its low three bits in strict priority
    // (bit-4 > bit-2 > bit-1 > bucket 0), and `matches` reports true iff every
    // set bit of the queried value is present in that same bucket. This one
    // test drives the constructor, insert bucket selection, and matches subset
    // logic together.
    // [spec:cg3:sem:bloomish.cg3.bloomish.bloomish-fn/test]
    // [spec:cg3:sem:bloomish.cg3.bloomish.insert-fn/test]
    // [spec:cg3:sem:bloomish.cg3.bloomish.matches-fn/test]
    #[test]
    fn insert_selects_bucket_and_matches_subset() {
        let mut b: Uint32Bloomish = Bloomish::new();

        // Fresh filter: nothing (with any bit) matches, and the empty value 0
        // trivially matches (0 & bucket == 0).
        assert!(!b.matches(1));
        assert!(!b.matches(2));
        assert!(!b.matches(4));
        assert!(b.matches(0));

        // 5 = 0b101 has bit-4 set -> routed to bucket 3; it now matches itself.
        b.insert(5);
        assert!(b.matches(5));
        // A different bit-4 value with a superset-in-bucket pattern: 4 (0b100)
        // is a subset of the stored 5, so it matches (false positive is fine).
        assert!(b.matches(4));
        // 6 = 0b110 is bit-4 too, but bit-2 (value 2) was never OR'd into
        // bucket 3, so it does NOT match: no false negatives.
        assert!(!b.matches(6));

        // Priority: 3 = 0b011 has bit-1 AND bit-2 set; bit-2 wins, so it lands
        // in bucket 2 (NOT bucket 1). Querying it before insert => no match.
        assert!(!b.matches(3));
        b.insert(3);
        assert!(b.matches(3));
        // 2 (bit-2) is a subset of 3 in the same bucket 2 => matches.
        assert!(b.matches(2));
        // 1 (bit-1) alone routes to bucket 1, which is still empty => no match,
        // proving 3 did not land in bucket 1.
        assert!(!b.matches(1));
    }

    // `clear` resets every bucket to zero, so previously-inserted values stop
    // matching. Driven directly after populating the filter.
    // [spec:cg3:sem:bloomish.cg3.bloomish.clear-fn/test]
    #[test]
    fn clear_resets_all_buckets() {
        let mut b: Uint32Bloomish = Bloomish::new();
        b.insert(5); // bucket 3
        b.insert(2); // bucket 2
        b.insert(1); // bucket 1
        assert!(b.matches(5) && b.matches(2) && b.matches(1));

        b.clear();
        assert!(!b.matches(5));
        assert!(!b.matches(2));
        assert!(!b.matches(1));
        // Default is defined as new() (also clear()ed): equally empty.
        let d: Uint32Bloomish = Bloomish::default();
        assert!(!d.matches(5));
    }
}
