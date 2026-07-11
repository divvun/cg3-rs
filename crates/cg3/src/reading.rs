//! Port of `src/Reading.hpp` — the `Reading` type and its aliases.
//!
//! CORE TYPE-SKELETON pass: type definitions only, no ported method bodies.
//!
//! Pointer/id mapping notes (C++ mixes `Tag*` and raw tag `uint32` ids here —
//! each field follows the `.hpp` exactly):
//! * `mapping` is `Tag*` → [`Option<TagId>`].
//! * `parent` is `Cohort*` → [`Option<CohortId>`]; `next` is `Reading*`
//!   (sub-reading chain) → [`Option<ReadingId>`].
//! * `baseform`, `hash`, `hash_plain`, `number`, `tags_string_hash` are raw
//!   `uint32_t` (tag hashes / counters), NOT pointers — kept as `u32`.
//! * `tags`, `tags_plain`, `tags_textual`, `tags_list`, `hit_by` hold raw tag
//!   *hashes* (`uint32_t`), NOT `Tag*` — kept as `u32` containers.
//! * `tags_numerical` is `bc::flat_map<uint32_t, Tag*>`: `u32` key → `Tag*`
//!   value → [`BTreeMap<u32, TagId>`] (see substitution note below).
//!
//! Container substitutions:
//! * `bc::flat_map` (boost ordered, sorted-by-key flat map) → [`BTreeMap`],
//!   which preserves the same key ordering. A dedicated flat-map port is a
//!   later concern.

use std::collections::BTreeMap;

use crate::arena::{CohortId, ReadingId, TagId};
use crate::bloomish::Uint32Bloomish;
use crate::grammar::Grammar;
use crate::inlines::{hash_value, ui32};
use crate::sorted_vector::uint32SortedVector;
use crate::store::RuntimeStore;
use crate::types::{TagHash, UString, Uint32Vector};

// [spec:cg3:def:reading.cg3.reading-list]
/// C++ `typedef std::vector<Reading*> ReadingList` → `Vec<ReadingId>`.
pub type ReadingList = Vec<ReadingId>;

// [spec:cg3:def:reading.cg3.reading.tags-list-t]
/// C++ `typedef uint32Vector tags_list_t` (member typedef of `Reading`).
pub type tags_list_t = Uint32Vector;

// [spec:cg3:def:reading.cg3.reading.tags-numerical-t]
/// C++ `typedef bc::flat_map<uint32_t, Tag*> tags_numerical_t` (member typedef
/// of `Reading`). `Tag*` value → [`TagId`]; ordered flat map → [`BTreeMap`].
pub type tags_numerical_t = BTreeMap<u32, TagId>;

// [spec:cg3:def:reading.cg3.reading]
/// A single reading (analysis) of a cohort.
///
/// The seven C++ `uint8_t : 1` bitfields become individual `bool` fields
/// (`mapped`, `deleted`, `noprint`, `matched_target`, `matched_tests`,
/// `immutable`, `active`); `clear()` resets them all to false and `Default`
/// yields that same blank state.
#[derive(Default)]
pub struct Reading {
    // --- C++ `uint8_t : 1` bitfields (no in-class initializer) ---
    pub mapped: bool,
    pub deleted: bool,
    pub noprint: bool,
    pub matched_target: bool,
    pub matched_tests: bool,
    pub immutable: bool,
    pub active: bool,

    /// Wave 4: the tag HASH of this reading's baseform, `None` when unset
    /// (the C++ `uint32_t baseform = 0` zero-sentinel). A real baseform hash is
    /// never 0 (SuperFastHash never returns the reserved value), so `Some`/`None`
    /// partitions cleanly; the pipe/binary wire boundary still reads/writes the
    /// C++ `0` byte-for-byte.
    pub baseform: Option<TagHash>,
    pub hash: u32,
    pub hash_plain: u32,
    pub number: u32,
    pub tags_bloom: Uint32Bloomish,
    pub tags_plain_bloom: Uint32Bloomish,
    pub tags_textual_bloom: Uint32Bloomish,
    /// C++ `Tag* mapping = nullptr`.
    pub mapping: Option<TagId>,
    /// C++ `Cohort* parent = nullptr`.
    pub parent: Option<CohortId>,
    /// C++ `Reading* next = nullptr` — sub-reading chain.
    pub next: Option<ReadingId>,
    pub hit_by: Uint32Vector,
    pub tags_list: tags_list_t,
    pub tags: uint32SortedVector,
    pub tags_plain: uint32SortedVector,
    pub tags_textual: uint32SortedVector,
    pub tags_numerical: tags_numerical_t,

    // ToDo (C++): Remove for real ordered mode
    pub tags_string: UString,
    pub tags_string_hash: u32,
}

// ---------------------------------------------------------------------------
// Ported method/function bodies (Reading.cpp).
//
// ARENA-MODEL NOTES
// * C++ `Reading*` values live in the runtime `pool<Reading>` (`pool_readings`);
//   here they live in `RuntimeStore.readings` (an `Arena<Reading>`). The C++
//   pool's "get a pre-cleared object / else allocate new" split maps onto the
//   arena's "reuse a freed slot / else push a new slot" split. There is no way
//   to peek whether `alloc` will reuse before calling it, so the pooled-vs-new
//   divergence (below) is applied *after* the alloc, keyed on whether the
//   returned index falls inside the pre-existing capacity (== a reused/pooled
//   slot) or equals it (== a brand-new slot). This is exact: a freed index is
//   always strictly < the capacity at the time of freeing, and capacity never
//   shrinks.
// * Anything that touches a *second* `Reading` (the `next` sub-reading chain) or
//   a `Tag`/`Cohort` cannot be a plain `&self`/`&mut self` method under the
//   arena model, so several C++ member functions become store-taking free fns.
//   Only `cmp_number` (reads two readings' scalar fields) stays a pure
//   associated function. See the crate report for the full mapping.

/// Copy the copy-constructor member-initializer fields of `r` (shared by the
/// copy ctor and `alloc_reading_copy`). Reproduces the C++ `Reading(const
/// Reading&)` init list: `matched_target`/`matched_tests` forced false,
/// `immutable`/`active` COPIED, `number = r.number + 100`, `next` shallow-copied
/// (deep clone handled by the caller). NOT a manifest symbol — port infra.
fn copy_ctor_fields(r: &Reading) -> Reading {
    Reading {
        mapped: r.mapped,
        deleted: r.deleted,
        noprint: r.noprint,
        matched_target: false,
        matched_tests: false,
        immutable: r.immutable,
        active: r.active,
        baseform: r.baseform,
        hash: r.hash,
        hash_plain: r.hash_plain,
        number: r.number.wrapping_add(100),
        tags_bloom: r.tags_bloom,
        tags_plain_bloom: r.tags_plain_bloom,
        tags_textual_bloom: r.tags_textual_bloom,
        mapping: r.mapping,
        parent: r.parent,
        next: r.next,
        hit_by: r.hit_by.clone(),
        tags_list: r.tags_list.clone(),
        tags: r.tags.clone(),
        tags_plain: r.tags_plain.clone(),
        tags_textual: r.tags_textual.clone(),
        tags_numerical: r.tags_numerical.clone(),
        tags_string: r.tags_string.clone(),
        tags_string_hash: r.tags_string_hash,
    }
}

/// Verbatim field-for-field copy (like `operator=`). Used only to detach a
/// sub-reading source out of the arena before recursing, so the recursive call
/// can borrow the store mutably without aliasing. NOT the copy ctor (it does not
/// bump `number` or clear `matched_*`), and NOT a manifest symbol — port infra.
pub(crate) fn clone_verbatim(r: &Reading) -> Reading {
    Reading {
        mapped: r.mapped,
        deleted: r.deleted,
        noprint: r.noprint,
        matched_target: r.matched_target,
        matched_tests: r.matched_tests,
        immutable: r.immutable,
        active: r.active,
        baseform: r.baseform,
        hash: r.hash,
        hash_plain: r.hash_plain,
        number: r.number,
        tags_bloom: r.tags_bloom,
        tags_plain_bloom: r.tags_plain_bloom,
        tags_textual_bloom: r.tags_textual_bloom,
        mapping: r.mapping,
        parent: r.parent,
        next: r.next,
        hit_by: r.hit_by.clone(),
        tags_list: r.tags_list.clone(),
        tags: r.tags.clone(),
        tags_plain: r.tags_plain.clone(),
        tags_textual: r.tags_textual.clone(),
        tags_numerical: r.tags_numerical.clone(),
        tags_string: r.tags_string.clone(),
        tags_string_hash: r.tags_string_hash,
    }
}

/// C++ free overload `Reading* alloc_reading(Cohort* p = nullptr)`.
///
/// Unspecced as a standalone (its behavior is captured by the sem of
/// `reading.cg3.reading.allocate-reading-fn`): pops from `pool_readings`; if the
/// pool is empty returns `new Reading(p)`, otherwise reuses the popped
/// (already-cleared) object, setting `number = p ? (p->readings.size() * 1000 +
/// 1000) : 0` and `parent = p`. Both branches yield an identical blank Reading
/// parented to `p` with `number` staged from `p`'s current reading count, so
/// there is NO pooled-vs-new divergence here.
///
/// `p` is `Option<CohortId>` (not the task's bare `CohortId`) to preserve the
/// C++ `p ? … : 0` null branch faithfully.
pub fn alloc_reading(store: &mut RuntimeStore, p: Option<CohortId>) -> ReadingId {
    let number = match p {
        // UI32(p->readings.size() * 1000 + 1000)
        Some(cid) => ui32(
            store
                .cohorts
                .get(cid.0)
                .readings
                .len()
                .wrapping_mul(1000)
                .wrapping_add(1000),
        ),
        None => 0,
    };
    let r = Reading {
        number,
        parent: p,
        ..Default::default()
    };
    ReadingId(store.readings.alloc(r))
}

// [spec:cg3:def:reading.cg3.alloc-reading-fn]
// [spec:cg3:sem:reading.cg3.alloc-reading-fn]
/// C++ free overload `Reading* alloc_reading(const Reading& o)`.
///
/// Builds a copy of `o` in the reading arena and returns its id. Reproduces the
/// pooled-vs-new divergence: the fields are built with copy-constructor
/// semantics (`immutable`/`active` COPIED from `o`); then if the alloc reused a
/// freed ("pooled") slot, `immutable` and `active` are FORCED to false — exactly
/// the C++ pooled branch — while a brand-new slot keeps the copied values (the
/// `new Reading(o)` branch). Both branches set `number = o.number + 100`, clear
/// `matched_*`, and deep-clone the whole `next` sub-reading chain.
///
/// The parent slot is allocated (and its pooled/new fate decided) BEFORE the
/// `next` chain is deep-cloned, matching the C++ order (`pool.get()` for the
/// parent, then recursion for the children).
pub fn alloc_reading_copy(store: &mut RuntimeStore, o: &Reading) -> ReadingId {
    let pooled = store.readings.will_reuse();
    let r = copy_ctor_fields(o);
    let child_src = r.next;
    let idx = store.readings.alloc(r);
    // Pooled reuse (pool.get() returned a cleared object) forces both flags off.
    if pooled {
        let rr = store.readings.get_mut(idx);
        rr.immutable = false;
        rr.active = false;
    }
    // if (r->next) { r->next = alloc_reading(*r->next); }
    if let Some(child_id) = child_src {
        let src = clone_verbatim(store.readings.get(child_id.0));
        let new_child = alloc_reading_copy(store, &src);
        store.readings.get_mut(idx).next = Some(new_child);
    }
    ReadingId(idx)
}

// [spec:cg3:def:inlines.cg3.reverse-fn]
// [spec:cg3:sem:inlines.cg3.reverse-fn]
/// C++ `inlines.hpp` `template<typename T> T* reverse(T* head)` — in-place
/// singly-linked-list reversal. The only linked type in the port is the
/// `Reading::next` chain, so the raw-pointer generic becomes safe id-chain
/// reversal over the arena (wave 4; previously duplicated per applicator).
/// Returns the new head (the old tail), or `head` for a 1-element chain.
pub fn reverse(store: &mut RuntimeStore, head: ReadingId) -> ReadingId {
    let mut nr: Option<ReadingId> = None;
    let mut cur: Option<ReadingId> = Some(head);
    while let Some(h) = cur {
        let next = store.readings.get(h.0).next;
        store.readings.get_mut(h.0).next = nr;
        nr = Some(h);
        cur = next;
    }
    nr.unwrap_or(head)
}

// [spec:cg3:def:reading.cg3.free-reading-fn]
// [spec:cg3:sem:reading.cg3.free-reading-fn]
/// C++ `void free_reading(Reading*& r)`.
///
/// Returns the reading (and its whole `next` chain) to the pool. If `r` is
/// `None`, returns immediately. Otherwise it mirrors `pool_readings.put(r)`:
/// [`reading_clear`] resets the object and recursively frees its `next` chain
/// back to the pool, then the slot itself is returned to the arena free-list.
/// Children are freed before the parent (matching the C++ `clear()`-then-
/// `put()` order). (The C++ `Reading*&` caller-handle null-out is ownership by
/// value here — wave 4.)
pub fn free_reading(store: &mut RuntimeStore, r: Option<ReadingId>) {
    let Some(id) = r else { return };
    reading_clear(store, id);
    store.readings.free_slot(id.0);
}

// [spec:cg3:def:reading.cg3.reading.clear-fn]
// [spec:cg3:sem:reading.cg3.reading.clear-fn]
/// C++ `void Reading::clear()` — resets the reading at `id` to a blank, reusable
/// state.
///
/// STORE-TAKING FREE FN (not `&mut self`): `clear` calls `free_reading(next)`,
/// which recursively returns the `next` chain to the pool — that touches other
/// arena slots, so the store is required. Field-reset order matches the C++
/// exactly: scalars/blooms/`mapping`/`parent`, then `free_reading(next)` (+ the
/// redundant `next = nullptr`), then the containers and `tags_string_hash`.
pub fn reading_clear(store: &mut RuntimeStore, id: ReadingId) {
    {
        let r = store.readings.get_mut(id.0);
        r.mapped = false;
        r.deleted = false;
        r.noprint = false;
        r.matched_target = false;
        r.matched_tests = false;
        r.immutable = false;
        r.active = false;
        r.baseform = None;
        r.hash = 0;
        r.hash_plain = 0;
        r.number = 0;
        r.tags_bloom.clear();
        r.tags_plain_bloom.clear();
        r.tags_textual_bloom.clear();
        r.mapping = None;
        r.parent = None;
    }
    // free_reading(next): frees the chain and nulls the handle. Copied out to a
    // local so the store can be borrowed mutably by the recursion.
    let next = store.readings.get_mut(id.0).next;
    free_reading(store, next);
    {
        let r = store.readings.get_mut(id.0);
        r.next = None; // redundant `next = nullptr`, reproduced verbatim
        r.hit_by.clear();
        r.tags_list.clear();
        r.tags.clear();
        r.tags_plain.clear();
        r.tags_textual.clear();
        r.tags_numerical.clear();
        r.tags_string.clear();
        r.tags_string_hash = 0;
    }
}

// [spec:cg3:def:reading.cg3.reading.reading-fn]
// [spec:cg3:sem:reading.cg3.reading.reading-fn]
/// C++ copy constructor `Reading::Reading(const Reading& r)`.
///
/// STORE-TAKING FREE FN (the deep-clone of `next` allocates from the pool).
/// Produces a `Reading` value whose fields follow the copy-ctor init list
/// (`matched_*` cleared, `immutable`/`active` COPIED, `number = r.number + 100`,
/// hashes copied verbatim), and whose `next` — if any — is a fresh deep clone of
/// the source chain via `allocateReading(*next)`
/// (→ [`Reading::allocate_reading_copy`]).
pub fn reading_copy(store: &mut RuntimeStore, r: &Reading) -> Reading {
    let mut copy = copy_ctor_fields(r);
    if let Some(next_id) = copy.next {
        let src = clone_verbatim(store.readings.get(next_id.0));
        copy.next = Some(Reading::allocate_reading_copy(store, &src));
    }
    copy
}

// [spec:cg3:def:reading.cg3.reading.rehash-fn]
// [spec:cg3:sem:reading.cg3.reading.rehash-fn]
/// C++ `uint32_t Reading::rehash()` — recomputes and caches `hash`/`hash_plain`
/// for the reading at `id` and returns the new `hash`.
///
/// STORE + GRAMMAR TAKING FREE FN (not `&mut self`): the fold needs
/// `mapping->hash` (a `Tag` in the grammar arena) and the `next->rehash()`
/// recursion needs the reading arena.
///
/// NOTE: the Wave-2 task brief's description of this fn ("baseform + parent-hash
/// XOR + bloom rebuild") does not match `Reading::rehash`; the actual algorithm
/// (per `Reading.cpp` / the `rehash-fn` sem) is the tags-fold + mapping + next
/// chain below.
pub fn reading_rehash(store: &mut RuntimeStore, grammar: &Grammar, id: ReadingId) -> u32 {
    // mapping->hash, resolved once (None when there is no mapping tag).
    let mapping = store.readings.get(id.0).mapping;
    let mapping_hash = mapping.map(|tid| grammar.single_tags_list.get(tid.0).hash);

    // hash = 0; hash_plain = 0; then fold the sorted tags, skipping the mapping
    // tag's own hash (if !mapping || mapping->hash != iter).
    let mut hash: u32 = 0;
    {
        let r = store.readings.get(id.0);
        for &iter in r.tags.iter() {
            let fold = match mapping_hash {
                None => true,
                Some(mh) => mh.get() != iter,
            };
            if fold {
                hash = hash_value(iter, hash);
            }
        }
    }
    let hash_plain = hash;
    if let Some(mh) = mapping_hash {
        hash = hash_value(mh.get(), hash);
    }
    let next = store.readings.get(id.0).next;
    if let Some(next_id) = next {
        reading_rehash(store, grammar, next_id);
        let next_hash = store.readings.get(next_id.0).hash;
        hash = hash_value(next_hash, hash);
    }
    {
        let r = store.readings.get_mut(id.0);
        r.hash = hash;
        r.hash_plain = hash_plain;
    }
    hash
}

impl Reading {
    // [spec:cg3:def:reading.cg3.reading.allocate-reading-fn]
    // [spec:cg3:sem:reading.cg3.reading.allocate-reading-fn]
    /// C++ `Reading* Reading::allocateReading(Cohort* p)` — a thin wrapper that
    /// returns `alloc_reading(p)` (the `Cohort*` overload) and does NOT use
    /// `this`, so it is an associated fn taking the store rather than `&self`.
    pub fn allocate_reading(store: &mut RuntimeStore, p: Option<CohortId>) -> ReadingId {
        alloc_reading(store, p)
    }

    /// C++ sibling overload `Reading* Reading::allocateReading(const Reading& r)`
    /// — delegates to `alloc_reading(const Reading&)`. Also does not use `this`.
    pub fn allocate_reading_copy(store: &mut RuntimeStore, r: &Reading) -> ReadingId {
        alloc_reading_copy(store, r)
    }

    // [spec:cg3:def:reading.cg3.reading.cmp-number-fn]
    // [spec:cg3:sem:reading.cg3.reading.cmp-number-fn]
    /// C++ `static bool Reading::cmp_number(Reading* a, Reading* b)` — a
    /// strict-weak comparator: orders ascending by `number`, tie-broken
    /// ascending by `hash`. Reads only the two readings' scalar fields, so it
    /// stays a pure associated fn (no store).
    pub fn cmp_number(a: &Reading, b: &Reading) -> bool {
        if a.number == b.number {
            return a.hash < b.hash;
        }
        a.number < b.number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // reverse(): in-place reversal of the Reading::next id chain (the safe
    // arena form of the C++ raw-pointer inlines::reverse).
    // [spec:cg3:sem:inlines.cg3.reverse-fn/test]
    #[test]
    fn reverse_reading_chain() {
        let mut store = RuntimeStore::new();
        let a = alloc_reading(&mut store, None);
        let b = alloc_reading(&mut store, None);
        let c = alloc_reading(&mut store, None);
        store.readings.get_mut(a.0).next = Some(b);
        store.readings.get_mut(b.0).next = Some(c);

        let new_head = reverse(&mut store, a);
        assert_eq!(new_head, c);
        assert_eq!(store.readings.get(c.0).next, Some(b));
        assert_eq!(store.readings.get(b.0).next, Some(a));
        assert_eq!(store.readings.get(a.0).next, None);

        // Single-element chain: unchanged head.
        let solo = alloc_reading(&mut store, None);
        assert_eq!(reverse(&mut store, solo), solo);
    }
}
