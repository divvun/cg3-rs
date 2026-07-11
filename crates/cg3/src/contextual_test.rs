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
bitflags::bitflags! {
    /// C++ `POS_*` contextual-test position bits over `uint64_t` (wave 4: a
    /// typed `bitflags` set instead of a bare `u64`).
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
    pub struct PosFlags: u64 {
        const CAREFUL = 1 << 0; // C
        const NEGATE = 1 << 1; // Prefix NEGATE
        const NOT = 1 << 2; // Prefix NOT
        const SCANFIRST = 1 << 3; // *
        const SCANALL = 1 << 4; // **
        const ABSOLUTE = 1 << 5; // @
        const SPAN_RIGHT = 1 << 6; // >
        const SPAN_LEFT = 1 << 7; // <
        const SPAN_BOTH = 1 << 8; // W
        const DEP_PARENT = 1 << 9; // p
        const DEP_SIBLING = 1 << 10; // s
        const DEP_CHILD = 1 << 11; // c
        const PASS_ORIGIN = 1 << 12; // o
        const NO_PASS_ORIGIN = 1 << 13; // O
        const LEFT_PAR = 1 << 14; // L
        const RIGHT_PAR = 1 << 15; // R
        const SELF = 1 << 16; // S
        const NONE = 1 << 17; // Prefix NONE
        const ALL = 1 << 18; // Prefix ALL
        const DEP_DEEP = 1 << 19; // * or **
        const MARK_SET = 1 << 20; // X
        const JUMP = 1 << 21; // x, jM, jA, jT, jCn
        const LOOK_DELETED = 1 << 22; // D
        const LOOK_DELAYED = 1 << 23; // d
        const TMPL_OVERRIDE = 1 << 24;
        const UNKNOWN = 1 << 25; // ?
        const RELATION = 1 << 26; // r:
        const ATTACH_TO = 1 << 27; // A
        const NUMERIC_BRANCH = 1 << 28; // f
        const BAG_OF_TAGS = 1 << 29; // B
        const DEP_GLOB = 1 << 30; // pp or cc
        const BIT64 = 1 << 31;
        const LEFT = 1 << 32; // l
        const RIGHT = 1 << 33; // r
        const LEFTMOST = 1 << 34; // ll
        const RIGHTMOST = 1 << 35; // rr
        const NO_BARRIER = 1 << 36; // N
        const WITH = 1 << 37; // w
        const LOOK_IGNORED = 1 << 38; // I
        const INACTIVE = 1 << 39; // t
        const ACTIVE = 1 << 40; // T
    }
}

// The C++ constant names, kept so call sites read like the source.
pub const POS_CAREFUL: PosFlags = PosFlags::CAREFUL;
pub const POS_NEGATE: PosFlags = PosFlags::NEGATE;
pub const POS_NOT: PosFlags = PosFlags::NOT;
pub const POS_SCANFIRST: PosFlags = PosFlags::SCANFIRST;
pub const POS_SCANALL: PosFlags = PosFlags::SCANALL;
pub const POS_ABSOLUTE: PosFlags = PosFlags::ABSOLUTE;
pub const POS_SPAN_RIGHT: PosFlags = PosFlags::SPAN_RIGHT;
pub const POS_SPAN_LEFT: PosFlags = PosFlags::SPAN_LEFT;
pub const POS_SPAN_BOTH: PosFlags = PosFlags::SPAN_BOTH;
pub const POS_DEP_PARENT: PosFlags = PosFlags::DEP_PARENT;
pub const POS_DEP_SIBLING: PosFlags = PosFlags::DEP_SIBLING;
pub const POS_DEP_CHILD: PosFlags = PosFlags::DEP_CHILD;
pub const POS_PASS_ORIGIN: PosFlags = PosFlags::PASS_ORIGIN;
pub const POS_NO_PASS_ORIGIN: PosFlags = PosFlags::NO_PASS_ORIGIN;
pub const POS_LEFT_PAR: PosFlags = PosFlags::LEFT_PAR;
pub const POS_RIGHT_PAR: PosFlags = PosFlags::RIGHT_PAR;
pub const POS_SELF: PosFlags = PosFlags::SELF;
pub const POS_NONE: PosFlags = PosFlags::NONE;
pub const POS_ALL: PosFlags = PosFlags::ALL;
pub const POS_DEP_DEEP: PosFlags = PosFlags::DEP_DEEP;
pub const POS_MARK_SET: PosFlags = PosFlags::MARK_SET;
pub const POS_JUMP: PosFlags = PosFlags::JUMP;
pub const POS_LOOK_DELETED: PosFlags = PosFlags::LOOK_DELETED;
pub const POS_LOOK_DELAYED: PosFlags = PosFlags::LOOK_DELAYED;
pub const POS_TMPL_OVERRIDE: PosFlags = PosFlags::TMPL_OVERRIDE;
pub const POS_UNKNOWN: PosFlags = PosFlags::UNKNOWN;
pub const POS_RELATION: PosFlags = PosFlags::RELATION;
pub const POS_ATTACH_TO: PosFlags = PosFlags::ATTACH_TO;
pub const POS_NUMERIC_BRANCH: PosFlags = PosFlags::NUMERIC_BRANCH;
pub const POS_BAG_OF_TAGS: PosFlags = PosFlags::BAG_OF_TAGS;
pub const POS_DEP_GLOB: PosFlags = PosFlags::DEP_GLOB;
pub const POS_64BIT: PosFlags = PosFlags::BIT64;
pub const POS_LEFT: PosFlags = PosFlags::LEFT;
pub const POS_RIGHT: PosFlags = PosFlags::RIGHT;
pub const POS_LEFTMOST: PosFlags = PosFlags::LEFTMOST;
pub const POS_RIGHTMOST: PosFlags = PosFlags::RIGHTMOST;
pub const POS_NO_BARRIER: PosFlags = PosFlags::NO_BARRIER;
pub const POS_WITH: PosFlags = PosFlags::WITH;
pub const POS_LOOK_IGNORED: PosFlags = PosFlags::LOOK_IGNORED;
pub const POS_INACTIVE: PosFlags = PosFlags::INACTIVE;
pub const POS_ACTIVE: PosFlags = PosFlags::ACTIVE;

pub const MASK_POS_DEP: PosFlags = POS_DEP_PARENT
    .union(POS_DEP_SIBLING)
    .union(POS_DEP_CHILD)
    .union(POS_DEP_GLOB);
pub const MASK_POS_DEPREL: PosFlags = MASK_POS_DEP.union(POS_RELATION);
pub const MASK_POS_CDEPREL: PosFlags = MASK_POS_DEPREL.union(POS_CAREFUL);
pub const MASK_POS_LORR: PosFlags = POS_LEFT
    .union(POS_RIGHT)
    .union(POS_LEFTMOST)
    .union(POS_RIGHTMOST);
pub const MASK_POS_SCAN: PosFlags = POS_SCANFIRST
    .union(POS_SCANALL)
    .union(POS_DEP_DEEP)
    .union(POS_DEP_GLOB);
pub const MASK_SELF_NB: PosFlags = POS_SELF.union(POS_NO_BARRIER);

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
    pub pos: PosFlags,
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
        let (
            pos,
            jump_pos,
            target,
            barrier,
            cbarrier,
            relation,
            offset,
            offset_sub,
            seed,
            linked,
            tmpl,
            ors,
        ) = {
            let ct = &contexts[id.0];
            (
                ct.pos,
                ct.jump_pos,
                ct.target,
                ct.barrier,
                ct.cbarrier,
                ct.relation,
                ct.offset,
                ct.offset_sub,
                ct.seed,
                ct.linked,
                ct.tmpl,
                ct.ors.clone(),
            )
        };

        // `hash = hash_value(pos)` — the `uint64_t` overload: SuperFastHash over
        // the 8 raw (little-endian) bytes of `pos`, seeded with CG3_HASH_SEED, with
        // no prior hash mixed in.
        let mut hash = super_fast_hash(&pos.bits().to_le_bytes(), CG3_HASH_SEED);
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
