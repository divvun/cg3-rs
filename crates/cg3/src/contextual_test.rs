//! Types from `src/ContextualTest.hpp` (spec `docs/spec/port/src/ContextualTest.md`).
//!
//! C++ raw pointers become typed arena indices ([`CtxId`]); `uint32_t`
//! set/relation references stay `u32` (they are already numeric ids in CG-3,
//! resolved via `grammar.getSet(..)`).
//!
//! Method-body pass: `operator==` (ported as [`ContextualTest::equals`]),
//! `rehash`, and the free [`copy_cntx`] are implemented here. `markUsed` remains
//! deferred — it recurses into `Grammar::getSet(..)->markUsed(..)`, and neither
//! `Grammar::get_set` nor `Set::mark_used` has been ported yet.

use crate::arena::{Arena, CtxId};
use crate::inlines::{CG3_HASH_SEED, hash_value, super_fast_hash};
use std::collections::VecDeque;

// [spec:cg3:def:contextual-test.cg3.context-vector]
/// C++ `typedef std::vector<ContextualTest*> ContextVector`.
pub type ContextVector = Vec<CtxId>;

// [spec:cg3:def:contextual-test.cg3.context-list]
/// C++ `typedef std::list<ContextualTest*> ContextList`.
///
/// NOTE (substitution): the C++ container is `std::list` (doubly-linked). Ported
/// as `VecDeque<CtxId>` to preserve the operations actually used —
/// `addContextualTest` front-inserts (`push_front`) and `reverseContextualTests`
/// reverses in place — while avoiding `std::collections::LinkedList` (which has
/// no in-place reverse). Kept distinct from [`ContextVector`] (a `std::vector`).
pub type ContextList = VecDeque<CtxId>;

// Anonymous `enum : uint64_t { POS_* }` from ContextualTest.hpp. These bit flags
// accumulate into `ContextualTest::pos` (a `u64`). The header enum is unnamed and
// carries no `[spec:cg3:def]` id, so the constants are reproduced verbatim here
// without an annotation, mirroring the un-annotated source.
pub const POS_CAREFUL: u64 = 1 << 0; // C
pub const POS_NEGATE: u64 = 1 << 1; // Prefix NEGATE
pub const POS_NOT: u64 = 1 << 2; // Prefix NOT
pub const POS_SCANFIRST: u64 = 1 << 3; // *
pub const POS_SCANALL: u64 = 1 << 4; // **
pub const POS_ABSOLUTE: u64 = 1 << 5; // @
pub const POS_SPAN_RIGHT: u64 = 1 << 6; // >
pub const POS_SPAN_LEFT: u64 = 1 << 7; // <
pub const POS_SPAN_BOTH: u64 = 1 << 8; // W
pub const POS_DEP_PARENT: u64 = 1 << 9; // p
pub const POS_DEP_SIBLING: u64 = 1 << 10; // s
pub const POS_DEP_CHILD: u64 = 1 << 11; // c
pub const POS_PASS_ORIGIN: u64 = 1 << 12; // o
pub const POS_NO_PASS_ORIGIN: u64 = 1 << 13; // O
pub const POS_LEFT_PAR: u64 = 1 << 14; // L
pub const POS_RIGHT_PAR: u64 = 1 << 15; // R
pub const POS_SELF: u64 = 1 << 16; // S
pub const POS_NONE: u64 = 1 << 17; // Prefix NONE
pub const POS_ALL: u64 = 1 << 18; // Prefix ALL
pub const POS_DEP_DEEP: u64 = 1 << 19; // * or **
pub const POS_MARK_SET: u64 = 1 << 20; // X
pub const POS_JUMP: u64 = 1 << 21; // x, jM, jA, jT, jCn
pub const POS_LOOK_DELETED: u64 = 1 << 22; // D
pub const POS_LOOK_DELAYED: u64 = 1 << 23; // d
pub const POS_TMPL_OVERRIDE: u64 = 1 << 24;
pub const POS_UNKNOWN: u64 = 1 << 25; // ?
pub const POS_RELATION: u64 = 1 << 26; // r:
pub const POS_ATTACH_TO: u64 = 1 << 27; // A
pub const POS_NUMERIC_BRANCH: u64 = 1 << 28; // f
pub const POS_BAG_OF_TAGS: u64 = 1 << 29; // B
pub const POS_DEP_GLOB: u64 = 1 << 30; // pp or cc
pub const POS_64BIT: u64 = 1 << 31;
pub const POS_LEFT: u64 = 1 << 32; // l
pub const POS_RIGHT: u64 = 1 << 33; // r
pub const POS_LEFTMOST: u64 = 1 << 34; // ll
pub const POS_RIGHTMOST: u64 = 1 << 35; // rr
pub const POS_NO_BARRIER: u64 = 1 << 36; // N
pub const POS_WITH: u64 = 1 << 37; // w
pub const POS_LOOK_IGNORED: u64 = 1 << 38; // I
pub const POS_INACTIVE: u64 = 1 << 39; // t
pub const POS_ACTIVE: u64 = 1 << 40; // T

pub const MASK_POS_DEP: u64 = POS_DEP_PARENT | POS_DEP_SIBLING | POS_DEP_CHILD | POS_DEP_GLOB;
pub const MASK_POS_DEPREL: u64 = MASK_POS_DEP | POS_RELATION;
pub const MASK_POS_CDEPREL: u64 = MASK_POS_DEPREL | POS_CAREFUL;
pub const MASK_POS_LORR: u64 = POS_LEFT | POS_RIGHT | POS_LEFTMOST | POS_RIGHTMOST;
pub const MASK_POS_SCAN: u64 = POS_SCANFIRST | POS_SCANALL | POS_DEP_DEEP | POS_DEP_GLOB;
pub const MASK_SELF_NB: u64 = POS_SELF | POS_NO_BARRIER;

// [spec:cg3:def:contextual-test.cg3.pos-jump-pos]
/// C++ `enum POS_JUMP_POS : int8_t`. The named jump targets for
/// `ContextualTest::jump_pos`; all positive values (not enumerated) address
/// WITH's cohorts. The `jump_pos` field itself stays an `i8` (see
/// [`ContextualTest`]) because it holds values outside this enum.
#[repr(i8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum POS_JUMP_POS {
    JUMP_TARGET = -2,
    JUMP_ATTACH = -1,
    JUMP_MARK = 0,
    // All positive numbers are WITH's cohorts
}

// [spec:cg3:def:contextual-test.cg3.gsr-specials]
/// C++ `enum GSR_SPECIALS { GSR_ANY = 32767 }`.
#[repr(i32)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GSR_SPECIALS {
    GSR_ANY = 32767,
}

// [spec:cg3:def:contextual-test.cg3.contextual-test]
/// C++ `class ContextualTest`.
///
/// `tmpl`/`linked` were `ContextualTest*` → `Option<CtxId>`. The `target`,
/// `relation`, `barrier`, and `cbarrier` fields are `uint32_t` set/relation
/// numbers in CG-3 (not pointers), so they stay `u32`. `jump_pos` defaults to `0`,
/// which is [`POS_JUMP_POS::JUMP_MARK`]. Derived `Default` reproduces every C++
/// in-class initializer; `operator==` is intentionally NOT derived because the
/// C++ overload has special (hash-based) handling of `linked` — ported as
/// [`ContextualTest::equals`], which threads the contextual-test arena so it can
/// read `linked`'s cached hash.
// [spec:cg3:def:contextual-test.cg3.contextual-test.contextual-test-fn]
// [spec:cg3:sem:contextual-test.cg3.contextual-test.contextual-test-fn]
// C++ `ContextualTest() = default`: the `#[derive(Default)]` below reproduces
// every in-class initializer verbatim (`jump_pos` defaults to `0`, i.e.
// `JUMP_MARK`; `tmpl`/`linked` to `None`; `ors` empty; all scalars to `0`/false).
#[derive(Clone, Debug, Default)]
pub struct ContextualTest {
    pub is_used: bool,
    pub offset: i32,
    pub offset_sub: i32,
    pub line: u32,
    pub hash: u32,
    pub seed: u32,
    pub pos: u64,
    pub target: u32,
    pub relation: u32,
    pub barrier: u32,
    pub cbarrier: u32,
    pub jump_pos: i8,
    pub tmpl: Option<CtxId>,
    pub linked: Option<CtxId>,
    pub ors: ContextVector,
}

impl ContextualTest {
    // [spec:cg3:def:contextual-test.cg3.contextual-test.rehash-fn]
    // [spec:cg3:sem:contextual-test.cg3.contextual-test.rehash-fn]
    //
    // SIGNATURE: C++ `uint32_t rehash()` is a `&mut self` method. Under the arena
    // model `self` lives inside `contexts`, and the `linked`/`ors` recursion needs
    // `&mut Arena<ContextualTest>` — which cannot coexist with a `&mut self`
    // borrowed from that same arena. It is therefore ported as an associated fn
    // resolving `self` by `id`.
    //
    // TMPL-POINTER-HASH SUBSTITUTION: the C++ folds
    // `UI32(reinterpret_cast<uintptr_t>(tmpl))` — the low 32 bits of the `tmpl`
    // POINTER ADDRESS, which is already non-deterministic across runs. Under the
    // arena there is no address to fold, so we fold the `CtxId` (`t.0`) instead.
    // PARITY: this replaces one run-varying value with a stable, deterministic one
    // while keeping the fold structure identical; hashes therefore differ from the
    // C++ whenever `tmpl` is set (but the C++ value was itself non-reproducible).
    pub fn rehash(contexts: &mut Arena<ContextualTest>, id: CtxId) -> u32 {
        // Memoized: `if (hash) { return hash; }`
        {
            let h = contexts[id.0].hash;
            if h != 0 {
                return h;
            }
        }

        // Snapshot scalars + child ids so the arena is free to be re-borrowed by
        // the recursive `rehash` calls below.
        let (pos, jump_pos, target, barrier, cbarrier, relation, offset, offset_sub, seed, linked, tmpl, ors) = {
            let ct = &contexts[id.0];
            (
                ct.pos, ct.jump_pos, ct.target, ct.barrier, ct.cbarrier, ct.relation, ct.offset,
                ct.offset_sub, ct.seed, ct.linked, ct.tmpl, ct.ors.clone(),
            )
        };

        // `hash = hash_value(pos)` — the `uint64_t` overload: SuperFastHash over
        // the 8 raw (little-endian) bytes of `pos`, seeded with CG3_HASH_SEED, with
        // no prior hash mixed in.
        let mut hash = super_fast_hash(&pos.to_le_bytes(), CG3_HASH_SEED);
        // ARG-ORDER QUIRK: every subsequent fold is `hash_value(hash, <field>)` —
        // the CURRENT hash is the value (`c`) arg and `<field>` is the seed (`h`)
        // arg (the REVERSE of `Set::rehash`).
        hash = hash_value(hash, jump_pos as u32); // int8_t sign-extends to u32
        hash = hash_value(hash, target);
        hash = hash_value(hash, barrier);
        hash = hash_value(hash, cbarrier);
        hash = hash_value(hash, relation);
        hash = hash_value(hash, offset.unsigned_abs()); // abs(offset); |i32::MIN| is UB in C++
        if offset < 0 {
            hash = hash_value(hash, 5000);
        }
        hash = hash_value(hash, offset_sub.unsigned_abs()); // abs(offset_sub)
        if offset_sub < 0 {
            hash = hash_value(hash, 5000);
        }
        if let Some(l) = linked {
            // Mirror the C++ incremental member-write so a (pathological)
            // self-referential cycle terminates on the partial hash like the C++
            // does, rather than recursing forever.
            contexts[id.0].hash = hash;
            let lh = Self::rehash(contexts, l);
            hash = hash_value(hash, lh);
        }
        if let Some(t) = tmpl {
            hash = hash_value(hash, t.0); // id substituted for the pointer address
        }
        for or in ors {
            contexts[id.0].hash = hash;
            let oh = Self::rehash(contexts, or);
            hash = hash_value(hash, oh);
        }

        hash = hash.wrapping_add(seed); // `hash += seed;` (u32 wraparound)

        contexts[id.0].hash = hash;
        hash
    }

    // [spec:cg3:def:contextual-test.cg3.contextual-test.operator-fn]
    // [spec:cg3:sem:contextual-test.cg3.contextual-test.operator-fn]
    //
    // C++ `bool operator==(const ContextualTest& other) const`. Rust's `PartialEq`
    // cannot carry the extra arena argument the `linked` hash-comparison needs, so
    // the overload is ported as this named method. LINKED-COMPARE CHOICE: when the
    // two `linked` ids differ, C++ still counts them equal iff both are non-null
    // AND `linked->hash == other.linked->hash`; that requires reading the linked
    // tests' cached `hash`, so `contexts: &Arena<ContextualTest>` is threaded in
    // (shared borrow — `self`/`other` may alias into it harmlessly).
    pub fn equals(&self, other: &ContextualTest, contexts: &Arena<ContextualTest>) -> bool {
        if self.hash != other.hash {
            return false;
        }
        if self.pos != other.pos {
            return false;
        }
        if self.jump_pos != other.jump_pos {
            return false;
        }
        if self.target != other.target {
            return false;
        }
        if self.barrier != other.barrier {
            return false;
        }
        if self.cbarrier != other.cbarrier {
            return false;
        }
        if self.relation != other.relation {
            return false;
        }
        if self.offset != other.offset {
            return false;
        }
        if self.offset_sub != other.offset_sub {
            return false;
        }
        if self.linked != other.linked {
            // Equal only if both are set AND their cached hashes match.
            let linked_ok = match (self.linked, other.linked) {
                (Some(a), Some(b)) => contexts[a.0].hash == contexts[b.0].hash,
                _ => false,
            };
            if !linked_ok {
                return false;
            }
        }
        if self.tmpl != other.tmpl {
            // `tmpl` is compared by identity (id equality here).
            return false;
        }
        if self.ors != other.ors {
            // `std::vector::operator==`: same length + element-id equality, in order.
            return false;
        }
        true
    }

    // Header inline `bool operator!=(const ContextualTest& o) const { return !(*this == o); }`
    // (un-annotated in the spec, mirrored here for callers).
    pub fn not_equals(&self, other: &ContextualTest, contexts: &Arena<ContextualTest>) -> bool {
        !self.equals(other, contexts)
    }
}

// [spec:cg3:def:contextual-test.cg3.copy-cntx-fn]
// [spec:cg3:sem:contextual-test.cg3.copy-cntx-fn]
//
// Free helper shallow-copying most fields from `src` into `trg`. `tmpl`/`linked`
// are copied as ids (the C++ raw-pointer copy). FLAGGED NON-COPY: `is_used` and
// `ors` are intentionally left untouched — `trg` keeps whatever it already had.
pub fn copy_cntx(src: &ContextualTest, trg: &mut ContextualTest) {
    trg.offset = src.offset;
    trg.offset_sub = src.offset_sub;
    trg.line = src.line;
    trg.hash = src.hash;
    trg.seed = src.seed;
    trg.pos = src.pos;
    trg.target = src.target;
    trg.relation = src.relation;
    trg.barrier = src.barrier;
    trg.cbarrier = src.cbarrier;
    trg.jump_pos = src.jump_pos;
    trg.tmpl = src.tmpl;
    trg.linked = src.linked;
}
