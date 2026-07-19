//! Port of `src/Set.hpp` — the `Set` type, its set-flag constants, and the
//! set-container typedefs. Wave 2 TYPE-SKELETON pass: only the type definitions
//! are ported here; the methods (`setName`, `empty`, `rehash`, `reindex`,
//! `markUsed`, `getNonEmpty`, the destructor) and the free `trie_reindex`
//! helper land in a later pass.
//!
//! Pointer→arena mapping: C++ `Set*` → [`SetId`], `Tag*` → `TagId`.

use crate::arena::{SetId, TagId};
use crate::grammar::Grammar;
use crate::inlines::{hash_value, ui32};
use crate::sorted_vector::{Comparator, sorted_vector};
use crate::tag::{T_MAPPING, T_SPECIAL, TagSortedVector, compare_Tag};
use crate::tag_trie::{trie_delete, trie_markused, trie_rehash};
use crate::types::{SetNumber, UString, Uint32Vector};
use std::collections::HashMap;

// PLACEHOLDER `Comparator<TagId>` for `compare_Tag`, needed so the
// `TagSortedVector` (`Set::ff_tags`) accessor methods resolve — every
// `sorted_vector` accessor is gated behind `Comp: Comparator<T>`, and
// `compare_Tag`'s real (arena-aware) `operator()` is deferred in `tag.rs`.
// The faithful C++ `compare_Tag` orders by `Tag::hash` (see the free fn
// `crate::tag::compare_tag`), which a stateless `Comparator` cannot express;
// this placeholder orders by raw `TagId`. `Set`'s ported methods use `ff_tags`
// only in order-INDEPENDENT ways (emptiness test in `empty`, mark-used walk in
// `markUsed`), so this ordering choice does not affect their correctness. The
// only `sorted_vector<_, compare_Tag>` instance in the crate is `Set::ff_tags`,
// so the blast radius is limited; a hash-ordered container is a later-pass
// reconciliation. UNRESOLVED DEP: if the real impl later lands in `tag.rs`,
// this placeholder must be removed to avoid a duplicate-impl conflict.
impl Comparator<TagId> for compare_Tag {
    fn comp(&self, a: &TagId, b: &TagId) -> bool {
        a.0 < b.0
    }
}

// C++ anonymous `enum` of `Set::type` bit flags. No spec:def id; reproduced as
// `u16` constants to match the `uint16_t type` field they are OR'd into.
bitflags::bitflags! {
    /// C++ `enum` of `ST_*` set-type bit flags over `uint16_t` (wave 4: a
    /// typed `bitflags` set instead of a bare `u16`).
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
    pub struct SetType: u16 {
        const ANY = 1 << 0;
        const SPECIAL = 1 << 1;
        const TAG_UNIFY = 1 << 2;
        const SET_UNIFY = 1 << 3;
        const CHILD_UNIFY = 1 << 4;
        const MAPPING = 1 << 5;
        const USED = 1 << 6;
        const STATIC = 1 << 7;
        const ORDERED = 1 << 8;
    }
}

// The C++ constant names, kept so call sites read like the source.
pub const ST_ANY: SetType = SetType::ANY;
pub const ST_SPECIAL: SetType = SetType::SPECIAL;
pub const ST_TAG_UNIFY: SetType = SetType::TAG_UNIFY;
pub const ST_SET_UNIFY: SetType = SetType::SET_UNIFY;
pub const ST_CHILD_UNIFY: SetType = SetType::CHILD_UNIFY;
pub const ST_MAPPING: SetType = SetType::MAPPING;
pub const ST_USED: SetType = SetType::USED;
pub const ST_STATIC: SetType = SetType::STATIC;
pub const ST_ORDERED: SetType = SetType::ORDERED;

pub const MASK_ST_UNIFY: SetType = SetType::TAG_UNIFY
    .union(SetType::SET_UNIFY)
    .union(SetType::CHILD_UNIFY);

/// C++ `trie_t` (`bc::flat_map<Tag*, trie_node_t, compare_Tag>`) — the real
/// port lives in the `tag_trie` module.
pub use crate::tag_trie::trie_t;

// [spec:cg3:def:set.cg3.set]
/// C++ `class Set`. A named set of tags/child-sets: its type-flag mask, source
/// line, cached hash/number, name, the two tries (plain + special),
/// fail-fast tags, and the child-set/operator id vectors.
///
/// NOTE(lead): the C++ `static std::ostream* dump_hashes_out;` (a shared debug
/// sink used by `rehash`) is a class static, not a per-instance field, and is
/// not reproduced here; it is a method-pass / global-I/O concern.
#[derive(Default)]
pub struct Set {
    /// `uint16_t type = 0;` — stored as 32-bit in the binary format, so safe to
    /// bump when needed. (`type` is a Rust keyword → raw identifier.)
    pub r#type: SetType,
    /// `uint32_t line = 0;`
    pub line: u32,
    /// `uint32_t hash = 0;`
    pub hash: u32,
    /// `uint32_t number = 0;`
    pub number: SetNumber,
    /// `UString name;`
    pub name: UString,
    /// `trie_t trie;` — placeholder type; see [`trie_t`].
    pub trie: trie_t,
    /// `trie_t trie_special;` — placeholder type; see [`trie_t`].
    pub trie_special: trie_t,
    /// `TagSortedVector ff_tags;`
    pub ff_tags: TagSortedVector,
    /// `uint32Vector set_ops;`
    pub set_ops: Uint32Vector,
    /// `uint32Vector sets;`
    pub sets: Uint32Vector,
}

// [spec:cg3:def:set.cg3.set-set]
/// C++ `typedef sorted_vector<Set*> SetSet;`
pub type SetSet = sorted_vector<SetId>;

// [spec:cg3:def:set.cg3.set-vector]
/// C++ `typedef std::vector<Set*> SetVector;`
pub type SetVector = Vec<SetId>;

// [spec:cg3:def:set.cg3.setuint32-hash-map]
// NOTE(lead): C++ `std::unordered_map<uint32_t, Set*>` → std `HashMap<u32,
// SetId>` (an accepted faithful substitution for a hash-map keyed by id).
/// C++ `typedef std::unordered_map<uint32_t, Set*> Setuint32HashMap;`
pub type Setuint32HashMap = HashMap<u32, SetId>;

// ===========================================================================
// Method bodies (Wave 2 translate pass). Ported literally, bug-for-bug, from
// `src/Set.cpp` / `src/Set.hpp`; each fn carries its `[spec:cg3:def]` +
// `[spec:cg3:sem]` ids verbatim.
//
// ARENA-MODEL SIGNATURE CONVENTION: methods that only touch `self`
// (`empty`, `setName`, the destructor) are ordinary `impl Set` methods.
// Methods that resolve OTHER sets/tags out of the grammar (`rehash` reads tag
// hashes via `trie_rehash`; `reindex`/`markUsed` recurse into child sets via
// `grammar.sets_by_contents` and read/mutate tags) cannot take `&mut self` +
// `&mut Grammar` at once, because the `Set` lives INSIDE `grammar.sets_list`.
// They are ported as associated fns `Set::f(grammar: &mut Grammar, id: SetId)`
// that index `grammar.sets_list[id.0]` with short borrows, clone the trie out
// before calling the `tag_trie` helpers (which need a `&Grammar`/`&mut Grammar`
// borrow that would otherwise alias the set's own trie), and collect child
// `SetId`s / `TagId`s before recursing. `ContextualTest::mark_used` and
// `Grammar` call the `Set::mark_used(grammar, id)` form.
// ===========================================================================

impl Set {
    // [spec:cg3:def:set.cg3.set.empty-fn]
    // [spec:cg3:sem:set.cg3.set.empty-fn]
    /// C++ `bool Set::empty() const`. Emptiness is judged on the four
    /// containers `ff_tags`, `trie`, `trie_special`, `sets` ONLY — `set_ops`,
    /// `type` and `name` are not consulted.
    pub fn empty(&self) -> bool {
        self.ff_tags.empty()
            && self.trie.is_empty()
            && self.trie_special.is_empty()
            && self.sets.is_empty()
    }

    // [spec:cg3:def:set.cg3.set.set-name-fn]
    // [spec:cg3:sem:set.cg3.set.set-name-fn]
    /// C++ `void Set::setName(uint32_t to)` (the `uint32_t` overload; default
    /// arg `to = 0` is passed explicitly by callers — Rust has no default
    /// args). Builds the synthetic name `_G_<line>_<to>_`. When `to == 0`, a
    /// fresh `UI32(rand())` id is substituted.
    ///
    /// PORT NOTES: the C++ `sprintf` into the shared global scratch buffer
    /// `cbuffers[0]` (overwriting it as a side effect) is reproduced with a
    /// local `format!` — no global scratch buffer exists in the port, so that
    /// side effect is intentionally NOT reproduced. The C++ used the
    /// process-global libc `rand()` for the `to == 0` fallback; wave 4 threads
    /// the PRNG state in from the owning [`Grammar`](crate::grammar::Grammar)
    /// (`rand_state`, stepped by [`rand_step`]) — exact libc-`rand()` value
    /// parity is not achievable without seed parity and is not required (the
    /// id only needs to be unique-ish for a synthetic set name).
    pub fn set_name(&mut self, mut to: u32, rand_state: &mut u32) {
        if to == 0 {
            to = ui32(rand_step(rand_state));
        }
        // sprintf(&cbuffers[0][0], "_G_%u_%u_", line, to) -> n = chars written.
        let s = format!("_G_{}_{}_", self.line, to);
        let n = s.len();
        self.name.reserve(n); // name.reserve(n)
        self.name.clear();
        self.name.push_str(&s); // name.assign(&cbuffers[0][0], &cbuffers[0][0] + n)
    }

    // [spec:cg3:def:set.cg3.set.rehash-fn]
    // [spec:cg3:sem:set.cg3.set.rehash-fn]
    /// C++ `uint32_t Set::rehash()`. Recomputes and stores `hash`, returning it.
    /// Associated fn (`grammar`/`id` form) because Step 2 reads tag hashes via
    /// `trie_rehash`, which needs an immutable `&Grammar` borrow that would
    /// alias the set's own trie — the trie is cloned out first.
    ///
    /// QUIRK reproduced: the unify branch reads `name[0]` unconditionally
    /// (assuming `name` is non-empty). C++ `std::string::operator[](0)` on an
    /// empty string returns the NUL terminator, so the faithful port reads the
    /// first char or `'\0'` (never panics). The `Set::dump_hashes_out` debug
    /// stream is a class-static global-I/O concern and is NOT reproduced (same
    /// precedent as `Tag::rehash`).
    pub fn rehash(grammar: &mut Grammar, id: SetId) -> u32 {
        let mut retval: u32 = 0;

        let ty = grammar.sets_list[id.0].r#type;
        if ty.intersects(ST_TAG_UNIFY | ST_SET_UNIFY) {
            if ty.intersects(ST_TAG_UNIFY) {
                retval = hash_value(5153, retval);
            }
            if ty.intersects(ST_SET_UNIFY) {
                retval = hash_value(5171, retval);
            }

            // Parse and incorporate multi-use identifier, if any.
            let name = grammar.sets_list[id.0].name.clone();
            // name[0] read unconditionally (empty -> '\0', so no branch taken).
            let name0 = name.chars().next().unwrap_or('\0');
            // u_sscanf(name.data(), "&&%u:%*S", &u) == 1 && u != 0
            if name0 == '&' {
                if let Some(u) = scan_prefixed_uint(&name, '&')
                    && u != 0
                {
                    retval = hash_value(u, retval);
                }
            }
            // else if name[0] == '$' && u_sscanf(name.data(), "$$%u:%*S", &u) == 1 && u != 0
            else if name0 == '$'
                && let Some(u) = scan_prefixed_uint(&name, '$')
                && u != 0
            {
                retval = hash_value(u, retval);
            }
        }

        if grammar.sets_list[id.0].sets.is_empty() {
            retval = hash_value(3499, retval); // Combat hash-collisions
            if !grammar.sets_list[id.0].trie.is_empty() {
                let trie = grammar.sets_list[id.0].trie.clone();
                retval = hash_value(trie_rehash(&trie, grammar), retval);
            }
            if !grammar.sets_list[id.0].trie_special.is_empty() {
                let trie_special = grammar.sets_list[id.0].trie_special.clone();
                retval = hash_value(trie_rehash(&trie_special, grammar), retval);
            }
        } else {
            retval = hash_value(2683, retval); // Combat hash-collisions
            let sets = grammar.sets_list[id.0].sets.clone();
            for i in sets {
                retval = hash_value(i, retval);
            }
            let set_ops = grammar.sets_list[id.0].set_ops.clone();
            for i in set_ops {
                retval = hash_value(i, retval);
            }
        }
        grammar.sets_list[id.0].hash = retval;

        // The `dump_hashes_out` (class-static debug stream) branch is not ported.

        retval
    }

    // [spec:cg3:def:set.cg3.set.reindex-fn]
    // [spec:cg3:sem:set.cg3.set.reindex-fn]
    /// C++ `void Set::reindex(Grammar& grammar)`. Recomputes the derived
    /// special-type flags on `type`. Associated fn (`grammar`/`id` form)
    /// because it recurses into child sets via `grammar.sets_by_contents` and
    /// reads tag types via `trie_reindex`.
    ///
    /// QUIRK reproduced: the child lookup `grammar.sets_by_contents.find(s)->second`
    /// dereferences the iterator with NO presence check (C++ UB when `s` is
    /// absent); the port's `HashMap` index panics on the same missing key.
    pub fn reindex(grammar: &mut Grammar, id: SetId) {
        grammar.sets_list[id.0].r#type &= !ST_SPECIAL;
        grammar.sets_list[id.0].r#type &= !ST_CHILD_UNIFY;

        let trie = grammar.sets_list[id.0].trie.clone();
        let r_trie = trie_reindex(&trie, grammar);
        grammar.sets_list[id.0].r#type |= r_trie;
        let trie_special = grammar.sets_list[id.0].trie_special.clone();
        let r_special = trie_reindex(&trie_special, grammar);
        grammar.sets_list[id.0].r#type |= r_special;

        let sets = grammar.sets_list[id.0].sets.clone();
        for s in sets {
            // find(s)->second — no end-check (see QUIRK above).
            let set = grammar.sets_by_contents[&s];
            Set::reindex(grammar, set);
            let set_type = grammar.sets_list[set.0].r#type;
            if set_type.intersects(ST_SPECIAL) {
                grammar.sets_list[id.0].r#type |= ST_SPECIAL;
            }
            if set_type.intersects(ST_TAG_UNIFY | ST_SET_UNIFY | ST_CHILD_UNIFY) {
                grammar.sets_list[id.0].r#type |= ST_CHILD_UNIFY;
            }
            if set_type.intersects(ST_MAPPING) {
                grammar.sets_list[id.0].r#type |= ST_MAPPING;
            }
        }

        if grammar.sets_list[id.0]
            .r#type
            .intersects(ST_TAG_UNIFY | ST_SET_UNIFY | ST_CHILD_UNIFY)
        {
            grammar.sets_list[id.0].r#type |= ST_SPECIAL;
            grammar.sets_list[id.0].r#type |= ST_CHILD_UNIFY;
        }
    }

    // [spec:cg3:def:set.cg3.set.mark-used-fn]
    // [spec:cg3:sem:set.cg3.set.mark-used-fn]
    /// C++ `void Set::markUsed(Grammar& grammar)`. Marks this set and everything
    /// it references as used. Associated fn (`grammar`/`id` form) because it
    /// mutates tags (`trie_markused`, `ff_tags`) and recurses into child sets.
    ///
    /// There is NO visited-guard — the full set graph is walked (relies on that
    /// graph being acyclic). The child lookup dereferences
    /// `grammar.sets_by_contents.find(s)->second` with no presence check (same
    /// QUIRK as `reindex`). The tries are cloned out before `trie_markused` so
    /// the mutable `&Grammar` borrow does not alias the set's own trie
    /// (`trie_markused` reads only structure + `TagId`s, mutating the tags, so
    /// the clone yields identical marking).
    pub fn mark_used(grammar: &mut Grammar, id: SetId) {
        grammar.sets_list[id.0].r#type |= ST_USED;

        let trie = grammar.sets_list[id.0].trie.clone();
        trie_markused(&trie, grammar);
        let trie_special = grammar.sets_list[id.0].trie_special.clone();
        trie_markused(&trie_special, grammar);

        let ff_tags: Vec<TagId> = grammar.sets_list[id.0].ff_tags.iter().copied().collect();
        for tag in ff_tags {
            grammar.single_tags_list[tag.0].mark_used();
        }

        let sets = grammar.sets_list[id.0].sets.clone();
        for s in sets {
            // find(s)->second — no end-check.
            let set = grammar.sets_by_contents[&s];
            Set::mark_used(grammar, set);
        }
    }
}

// [spec:cg3:def:set.cg3.set.set-fn]
// [spec:cg3:sem:set.cg3.set.set-fn]
/// C++ `~Set()`. The destructor calls `trie_delete(trie)` and
/// `trie_delete(trie_special)` (recursively `reset()`ing every child sub-trie
/// `unique_ptr`), then the remaining members are torn down by their own
/// destructors — reproduced here as the Rust drop glue running after this
/// `Drop::drop` body. (In Rust the nested `Box`es free automatically, so the
/// explicit `trie_delete` calls are a bug-for-bug faithful no-op.) The trivial
/// `Set() = default;` ctor is covered by the struct's `#[derive(Default)]`.
impl Drop for Set {
    fn drop(&mut self) {
        trie_delete(&mut self.trie);
        trie_delete(&mut self.trie_special);
    }
}

// [spec:cg3:def:set.cg3.trie-reindex-fn]
// [spec:cg3:sem:set.cg3.trie-reindex-fn]
/// C++ free helper `inline uint8_t trie_reindex(const trie_t& trie)`. Walks a
/// `trie_t`, accumulating special flags starting from 0: `T_SPECIAL` tag ->
/// `ST_SPECIAL`, `T_MAPPING` tag -> `ST_MAPPING`, recursing into child
/// sub-tries. Only `ST_SPECIAL` (2) and `ST_MAPPING` (32) can be set (both <
/// 256), so the `u8` return is lossless. Order-independent (OR accumulation),
/// so the `BTreeMap` is iterated directly; `grammar` resolves each `TagId`'s
/// `Tag::type`.
pub fn trie_reindex(trie: &trie_t, grammar: &Grammar) -> SetType {
    let mut type_ = SetType::empty();
    for (k, node) in trie.iter() {
        let tag_type = grammar.single_tags_list[k.0].r#type;
        if tag_type.intersects(T_SPECIAL) {
            type_ |= ST_SPECIAL;
        }
        if tag_type.intersects(T_MAPPING) {
            type_ |= ST_MAPPING;
        }
        if let Some(sub) = &node.trie {
            type_ |= trie_reindex(sub, grammar);
        }
    }
    type_
}

// ---------------------------------------------------------------------------
// Local stand-ins for the ICU `u_sscanf` unify-name parse and libc `rand()`
// used by `rehash` / `setName`. Deliberately un-annotated (they stand in for
// helpers owned by other, not-yet-wired modules); reimplemented here so this
// file compiles standalone.
// ---------------------------------------------------------------------------

/// Reproduces `u_sscanf(name.data(), "pp%u:%*S", &u) == 1` for a doubled prefix
/// char `p` (`'&'` -> `"&&"`, `'$'` -> `"$$"`). Because `%*S` is
/// assignment-suppressed, the return count can only be 0 or 1, so `== 1`
/// reduces to "the literal `pp` matched and `%u` read at least one digit". The
/// trailing `:` and `%*S` never affect the count, so they are not required
/// here. `%u` skips leading whitespace and accepts an optional sign, then reads
/// decimal digits (base 10 only). `Some(value)` == count 1; `None` == count 0.
fn scan_prefixed_uint(name: &str, p: char) -> Option<u32> {
    let chars: Vec<char> = name.chars().collect();
    // literal `pp`
    if chars.len() < 2 || chars[0] != p || chars[1] != p {
        return None;
    }
    let mut i = 2usize;
    // %u skips leading whitespace.
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    // %u accepts an optional sign (converted via strtoul, i.e. unsigned wrap).
    let mut neg = false;
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        neg = chars[i] == '-';
        i += 1;
    }
    let mut any = false;
    let mut val: u64 = 0;
    while i < chars.len() && chars[i].is_ascii_digit() {
        val = val
            .wrapping_mul(10)
            .wrapping_add((chars[i] as u32 - '0' as u32) as u64);
        i += 1;
        any = true;
    }
    if !any {
        return None; // %u not assigned -> count 0 (!= 1)
    }
    let mut out = val as u32;
    if neg {
        out = out.wrapping_neg();
    }
    Some(out)
}

/// Stand-in for libc `rand()`: returns a value in `[0, RAND_MAX]`
/// (`RAND_MAX == 2^31 - 1`, matching glibc). A xorshift32 generator drives it,
/// stepping the caller-owned state (the C++ used the process-global libc PRNG;
/// wave 4 makes the state `Grammar`-owned — see `Grammar::rand_state`). The
/// state must be non-zero; exact glibc-`rand()` value parity is NOT reproduced
/// (it would require additive-feedback + seed parity, irrelevant to the only
/// caller `setName`, which just needs a unique-ish synthetic id).
pub(crate) fn rand_step(state: &mut u32) -> i32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x & 0x7fff_ffff) as i32
}
