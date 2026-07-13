//! Port of `src/TagTrie.hpp` — the tag-sequence trie used inside every `Set`
//! (`Set::trie` / `Set::trie_special`).
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). The `trie_getTags` sort-then-
//! pop corruption is reproduced faithfully (see [`trie_get_tags`]).
//!
//! ## Container representation (`compare_Tag` hash-ordering)
//! C++ `trie_t = bc::flat_map<Tag*, trie_node_t, compare_Tag>` — a sorted-vector
//! map keyed by `Tag*` and ordered ASCENDING by `Tag->hash` (via `compare_Tag`).
//! The port keys the map by [`TagId`] instead:
//!
//! ```text
//! trie_t = std::collections::BTreeMap<TagId, trie_node_t>
//! ```
//!
//! A `BTreeMap<TagId, …>` iterates in ascending-`TagId` order, NOT ascending
//! `Tag->hash` order. Wherever the C++ SEMANTICS depend on iteration order
//! (serialize byte layout, structural rehash, and the two `getTags` / the
//! `getTagList` collectors) the port re-derives the C++ order by collecting the
//! entries and STABLE-sorting them by `grammar.single_tags_list[id].hash`
//! (helper [`ordered_entries`]). A stable sort keeps the `TagId` order among
//! equal-hash entries. Those functions therefore take `grammar: &Grammar`.
//! Order-INSENSITIVE functions (`insert`, `copy`, `delete`, `singular`,
//! `markused`, `has_type`) iterate the `BTreeMap` directly and — except where a
//! `Tag` field must be read/written — need no grammar.
//!
//! ### DIVERGENCE the lead must weigh (equal-hash tag collision)
//! Because C++ `compare_Tag` orders by `hash` ALONE, two DISTINCT `Tag*` with an
//! equal `hash` collide as ONE flat_map key (spec EDGE on `trie-insert-fn`).
//! Keyed by `TagId`, two distinct `TagId`s with an equal hash stay as SEPARATE
//! keys here. For the normal case (CG-3 tag hashes are effectively unique) the
//! behaviour is identical; only the pathological hash-collision EDGE diverges.
//!
//! ## Function-name mapping (C++ has overloads; Rust has none)
//! | C++                                   | Rust                             |
//! |---------------------------------------|----------------------------------|
//! | `trie_insert(t, tv[, w])`             | [`trie_insert`] (`w` explicit)   |
//! | `trie_getTagList(t) -> TagVector`     | [`trie_get_tag_list`]            |
//! | `trie_getTagList(t, tags)` (void)     | [`trie_get_tag_list_append`]     |
//! | `trie_getTagList(t, tags, node)`      | [`trie_get_tag_list_find`]       |
//! | `trie_getTags(t) -> set`              | [`trie_get_tags`]                |
//! | `trie_getTags(t, rv, tv)` (void)      | [`trie_get_tags_into`]           |
//! | `trie_getTagsOrdered(t) -> set`       | [`trie_get_tags_ordered`]        |
//! | `trie_getTagsOrdered(t, rv, tv)`      | [`trie_get_tags_ordered_into`]   |
//! | `_trie_copy_helper(t)`                | [`trie_copy_helper`]             |
//!
//! ## Out of scope (NOT in TagTrie.hpp — do not port here)
//! `trie_unserialize` lives in `src/Grammar.hpp`
//! (`[spec:cg3:def:grammar.cg3.trie-unserialize-fn]`) and `trie_reindex` lives
//! in `src/Set.hpp` — both belong to their respective port modules.

use std::collections::BTreeMap;
use std::io::Write;

use crate::arena::TagId;
use crate::grammar::Grammar;
use crate::inlines::{hash_value, write_be};
use crate::tag::{T_USED, TagList, TagVector, TagVectorSet};

// [spec:cg3:def:tag-trie.cg3.trie-node-t]
/// C++ `struct trie_node_t { bool terminal = false; std::unique_ptr<trie_t> trie; }`.
///
/// `std::unique_ptr<trie_t>` (a nullable owning child) → `Option<Box<trie_t>>`.
#[derive(Default, Clone, Debug)]
pub struct trie_node_t {
    /// `bool terminal = false;`
    pub terminal: bool,
    /// `std::unique_ptr<trie_t> trie;` — the child level, null when absent.
    pub trie: Option<Box<trie_t>>,
}

// [spec:cg3:def:tag-trie.cg3.trie-t]
/// C++ `typedef bc::flat_map<Tag*, trie_node_t, compare_Tag> trie_t`.
///
/// Ported as a `BTreeMap` keyed by [`TagId`]; C++ ordering-by-`Tag->hash` is
/// re-derived where it matters (see module docs). Clean public alias so the
/// lead can repoint `Set::trie` / `Set::trie_special` here (replacing the
/// `crate::set::TrieTodo` placeholder).
pub type trie_t = BTreeMap<TagId, trie_node_t>;

/// Collects the trie's entries in the C++ `compare_Tag` order — ascending
/// `Tag->hash`. Not a manifest symbol: port infrastructure standing in for the
/// flat_map's intrinsic hash ordering. STABLE sort → equal-hash entries keep
/// their `TagId` order (they would have collided into one key in C++).
fn ordered_entries<'a>(trie: &'a trie_t, grammar: &Grammar) -> Vec<(TagId, &'a trie_node_t)> {
    let mut v: Vec<(TagId, &trie_node_t)> = trie.iter().map(|(k, n)| (*k, n)).collect();
    v.sort_by(|a, b| {
        let ha = grammar.single_tags_list[a.0.0].hash;
        let hb = grammar.single_tags_list[b.0.0].hash;
        ha.cmp(&hb)
    });
    v
}

/// `std::sort(tv.begin(), tv.end(), compare_Tag())` — sort a tag vector ascending
/// by `Tag->hash`. (Stable here vs C++ `std::sort`'s unstable; only differs on
/// equal-hash ties, which do not occur with unique tag hashes.)
fn sort_tv_by_hash(tv: &mut TagVector, grammar: &Grammar) {
    tv.sort_by(|a, b| {
        let ha = grammar.single_tags_list[a.0].hash;
        let hb = grammar.single_tags_list[b.0].hash;
        ha.cmp(&hb)
    });
}

// [spec:cg3:def:tag-trie.cg3.trie-insert-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-insert-fn]
/// C++ `trie_insert(trie_t&, const TagVector&, size_t w = 0)`. No Rust default
/// args: callers pass `w = 0`. Keyed BTreeMap access mirrors flat_map
/// `operator[]` (default-inserts `{terminal=false, trie=None}` for a missing
/// key), so no grammar/hash ordering is needed. EDGE: an empty `tv` makes
/// `tv.len() - 1` underflow — in C++ this reaches `tv[0]` OOB (UB); here it
/// panics. Callers must pass a non-empty vector.
pub fn trie_insert(trie: &mut trie_t, tv: &TagVector, w: usize) -> bool {
    let node = trie.entry(tv[w]).or_default();
    if node.terminal {
        return false;
    }
    if w < tv.len() - 1 {
        if node.trie.is_none() {
            node.trie = Some(Box::new(trie_t::new()));
        }
        return trie_insert(node.trie.as_deref_mut().unwrap(), tv, w + 1);
    }
    node.terminal = true;
    node.trie = None; // node.trie.reset()
    true
}

// [spec:cg3:def:tag-trie.cg3.trie-copy-helper-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-copy-helper-fn]
/// C++ `_trie_copy_helper` → `Box<trie_t>` (was `std::unique_ptr<trie_t>`). The
/// `Tag` keys are shared (`TagId`s copied, tags not cloned); only node structure
/// and terminal flags are duplicated. Order-independent (a keyed rebuild), so no
/// grammar needed.
pub fn trie_copy_helper(trie: &trie_t) -> Box<trie_t> {
    let mut nt = Box::new(trie_t::new());
    for (k, node) in trie.iter() {
        let n = nt.entry(*k).or_default();
        n.terminal = node.terminal;
        if let Some(sub) = &node.trie {
            n.trie = Some(trie_copy_helper(sub));
        }
    }
    nt
}

// [spec:cg3:def:tag-trie.cg3.trie-copy-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-copy-fn]
/// C++ `trie_copy` — deep-copies a whole trie, returning a new `trie_t` by value.
/// Order-independent, so no grammar needed.
pub fn trie_copy(trie: &trie_t) -> trie_t {
    let mut nt = trie_t::new();
    for (k, node) in trie.iter() {
        let n = nt.entry(*k).or_default();
        n.terminal = node.terminal;
        if let Some(sub) = &node.trie {
            n.trie = Some(trie_copy_helper(sub));
        }
    }
    nt
}

// [spec:cg3:def:tag-trie.cg3.trie-delete-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-delete-fn]
/// C++ `trie_delete` — depth-first frees every descendant sub-trie, leaving the
/// passed-in map's own top-level keys and terminal flags intact (only child
/// `.trie` pointers are freed/nulled). Order-independent, so no grammar needed.
pub fn trie_delete(trie: &mut trie_t) {
    for node in trie.values_mut() {
        if node.trie.is_some() {
            trie_delete(node.trie.as_deref_mut().unwrap());
            node.trie = None; // p.second.trie.reset()
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-singular-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-singular-fn]
/// C++ `trie_singular` — true iff the trie is a single non-branching chain that
/// ends in a terminal. Only inspects the sole entry, so order-independent.
pub fn trie_singular(trie: &trie_t) -> bool {
    if trie.len() != 1 {
        return false;
    }
    // trie.begin()->second — the sole entry's node.
    let node = trie.values().next().unwrap();
    if node.terminal {
        return true;
    }
    if let Some(sub) = &node.trie {
        return trie_singular(sub);
    }
    false
}

// [spec:cg3:def:tag-trie.cg3.trie-rehash-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-rehash-fn]
/// C++ `trie_rehash` — folds each tag's precomputed `hash` (and, recursively, the
/// sub-trie's rehash) into a running value with `hash_value`. ORDER-SENSITIVE
/// (`hash_value` is non-commutative), so entries are visited in ascending-hash
/// order via [`ordered_entries`] and `grammar` is required. Terminal flags are
/// NOT hashed (parity note).
pub fn trie_rehash(trie: &trie_t, grammar: &Grammar) -> u32 {
    let mut retval: u32 = 0;
    for (k, node) in ordered_entries(trie, grammar) {
        let h = grammar.single_tags_list[k.0].hash;
        retval = hash_value(h.get(), retval);
        if let Some(sub) = &node.trie {
            retval = hash_value(trie_rehash(sub, grammar), retval);
        }
    }
    retval
}

// [spec:cg3:def:tag-trie.cg3.trie-markused-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-markused-fn]
/// C++ `trie_markused` — calls `kv.first->markUsed()` on every tag (and recurses).
/// `Tag::markUsed()` is `type |= T_USED`; the `Tag` methods are not ported yet,
/// so the mask is applied inline here. Marking is order-independent, but it
/// MUTATES the tags, so this takes `grammar: &mut Grammar`. (Lead: at the call
/// site the trie lives inside a `Set` owned by the same `Grammar`; the borrow of
/// `grammar.single_tags_list` and the immutable borrow of the set's trie must be
/// split — restructure or clone as needed.)
pub fn trie_markused(trie: &trie_t, grammar: &mut Grammar) {
    for (k, node) in trie.iter() {
        grammar.single_tags_list[k.0].r#type |= T_USED;
        if let Some(sub) = &node.trie {
            trie_markused(sub, grammar);
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-has-type-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-has-type-fn]
/// C++ `trie_hasType` — true iff any tag anywhere has any bit of `type_` set in
/// its own `type` mask. Order-independent for the boolean result, but reads
/// `Tag::type`, so `grammar` is required. (C++ takes `trie_t&`; the port takes
/// `&trie_t` since it never mutates.)
pub fn trie_has_type(trie: &trie_t, type_: crate::tag::TagType, grammar: &Grammar) -> bool {
    for (k, node) in trie.iter() {
        if grammar.single_tags_list[k.0].r#type.intersects(type_) {
            return true;
        }
        if let Some(sub) = &node.trie
            && trie_has_type(sub, type_, grammar) {
                return true;
            }
    }
    false
}

// Unspecced C++ overload `trie_getTagList(const trie_t&, TagList&)` (void): appends
// every tag of every path onto `the_tags` (no node search). Output order matches
// the C++ flat_map hash order, so `grammar` is required.
/// See [`trie_get_tag_list_find`] for the spec'd sibling overload.
pub fn trie_get_tag_list_append(trie: &trie_t, the_tags: &mut TagList, grammar: &Grammar) {
    for (k, node) in ordered_entries(trie, grammar) {
        the_tags.push(k);
        if let Some(sub) = &node.trie {
            trie_get_tag_list_append(sub, the_tags, grammar);
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-get-tag-list-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-get-tag-list-fn]
/// C++ `trie_getTagList(const trie_t&, TagList&, const void* node)` — DFS that
/// reconstructs the tag path leading to a specific node, matched by POINTER
/// IDENTITY. On success `the_tags` holds the full root-to-node path; on failure
/// it is restored to its entry state. Output order is the flat_map hash order,
/// so `grammar` is required.
///
/// IDENTITY NOTE: C++ compares `node == &kv`, the address of the flat_map
/// (key,value) PAIR. The port compares against the address of the node VALUE
/// (`&kv.second`) cast to `*const c_void`. The matcher port (a later wave; C++
/// stores `&kv` in `unif_tags`) must store the SAME node-value address so the
/// identity token is consistent.
pub fn trie_get_tag_list_find(
    trie: &trie_t,
    the_tags: &mut TagList,
    node: *const core::ffi::c_void,
    grammar: &Grammar,
) -> bool {
    for (k, n) in ordered_entries(trie, grammar) {
        the_tags.push(k);
        if node == (n as *const trie_node_t as *const core::ffi::c_void) {
            return true;
        }
        if let Some(sub) = &n.trie
            && trie_get_tag_list_find(sub, the_tags, node, grammar) {
                return true;
            }
        the_tags.pop(); // theTags.pop_back()
    }
    false
}

// Unspecced C++ overload `trie_getTagList(const trie_t&) -> TagVector`: returns the
// full tag list (delegates sub-tries to [`trie_get_tag_list_append`]). Output
// order is the flat_map hash order, so `grammar` is required.
pub fn trie_get_tag_list(trie: &trie_t, grammar: &Grammar) -> TagVector {
    let mut the_tags = TagVector::new();
    for (k, node) in ordered_entries(trie, grammar) {
        the_tags.push(k);
        if let Some(sub) = &node.trie {
            trie_get_tag_list_append(sub, &mut the_tags, grammar);
        }
    }
    the_tags
}

// Unspecced C++ shared-`tv` helper `trie_getTags(const trie_t&, TagVectorSet&,
// TagVector&)`. Extends `tv` one level deeper; on a terminal it reproduces the
// SORT-THEN-POP BUG (see [`trie_get_tags`]).
pub fn trie_get_tags_into(
    trie: &trie_t,
    rv: &mut TagVectorSet,
    tv: &mut TagVector,
    grammar: &Grammar,
) {
    for (k, node) in ordered_entries(trie, grammar) {
        tv.push(k);
        if node.terminal {
            // BUG (bug-for-bug): sort `tv` in place by hash, insert, then pop the
            // LAST (highest-hash) element — NOT necessarily the tag just pushed —
            // corrupting the shared prefix for later siblings at this level.
            sort_tv_by_hash(tv, grammar);
            rv.insert(tv.clone());
            tv.pop();
            continue;
        }
        if let Some(sub) = &node.trie {
            trie_get_tags_into(sub, rv, tv, grammar);
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-get-tags-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-get-tags-fn]
/// C++ `trie_getTags(const trie_t&) -> TagVectorSet`. Collects each root-to-
/// terminal path, individually SORTED by `compare_Tag`, into a `TagVectorSet`
/// (equivalent sequences merge). A FRESH `tv` per top-level entry; deeper levels
/// delegate to [`trie_get_tags_into`], which carries the sort-then-pop BUG. See
/// that helper for the reproduced quirk. `grammar` is required for both the hash
/// ordering and the per-sequence sort.
pub fn trie_get_tags(trie: &trie_t, grammar: &Grammar) -> TagVectorSet {
    let mut rv = TagVectorSet::new();
    for (k, node) in ordered_entries(trie, grammar) {
        let mut tv = TagVector::new();
        tv.push(k);
        if node.terminal {
            sort_tv_by_hash(&mut tv, grammar);
            rv.insert(tv.clone());
            tv.pop();
            continue;
        }
        if let Some(sub) = &node.trie {
            trie_get_tags_into(sub, &mut rv, &mut tv, grammar);
        }
    }
    rv
}

// Unspecced C++ shared-`tv` helper `trie_getTagsOrdered(const trie_t&,
// TagVectorSet&, TagVector&)`. Like [`trie_get_tags_into`] but WITHOUT sorting,
// so backtracking (`pop`) correctly removes the just-pushed tag.
pub fn trie_get_tags_ordered_into(
    trie: &trie_t,
    rv: &mut TagVectorSet,
    tv: &mut TagVector,
    grammar: &Grammar,
) {
    for (k, node) in ordered_entries(trie, grammar) {
        tv.push(k);
        if node.terminal {
            rv.insert(tv.clone());
            tv.pop();
            continue;
        }
        if let Some(sub) = &node.trie {
            trie_get_tags_ordered_into(sub, rv, tv, grammar);
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-get-tags-ordered-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-get-tags-ordered-fn]
/// C++ `trie_getTagsOrdered(const trie_t&) -> TagVectorSet`. Like
/// [`trie_get_tags`] but WITHOUT any per-sequence sorting: paths preserve their
/// in-trie (ascending-hash) order, so `pop` always removes the just-pushed tag
/// (no corruption). `grammar` is required for the hash ordering.
pub fn trie_get_tags_ordered(trie: &trie_t, grammar: &Grammar) -> TagVectorSet {
    let mut rv = TagVectorSet::new();
    for (k, node) in ordered_entries(trie, grammar) {
        let mut tv = TagVector::new();
        tv.push(k);
        if node.terminal {
            rv.insert(tv.clone());
            tv.pop();
            continue;
        }
        if let Some(sub) = &node.trie {
            trie_get_tags_ordered_into(sub, &mut rv, &mut tv, grammar);
        }
    }
    rv
}

// [spec:cg3:def:tag-trie.cg3.trie-serialize-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-serialize-fn]
/// C++ `trie_serialize(const trie_t&, std::ostream&)`. Emits a big-endian byte
/// stream; the top-level entry count is written by the CALLER, not here. Per
/// node: `[number: u32 BE][terminal: u8][childCount: u32 BE][children…]`.
/// BYTE-PARITY: the emitted identifier is `Tag->number` while the iteration/order
/// key is `Tag->hash` (they need not correlate) — hence `grammar` supplies both
/// the ordering AND `number`, and entries are visited in ascending-hash order.
pub fn trie_serialize<W: Write>(trie: &trie_t, out: &mut W, grammar: &Grammar) {
    for (k, node) in ordered_entries(trie, grammar) {
        let number = grammar.single_tags_list[k.0].number;
        write_be(out, number); // writeBE<uint32_t>(out, kv.first->number)
        write_be(out, node.terminal as u8); // writeBE<uint8_t>(out, kv.second.terminal)
        if let Some(sub) = &node.trie {
            write_be(out, sub.len() as u32); // writeBE<uint32_t>(out, UI32(sub->size()))
            trie_serialize(sub, out, grammar);
        } else {
            write_be(out, 0u32); // writeBE<uint32_t>(out, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Grammar;
    use crate::tag::{T_MAPPING, Tag};

    /// Intern a fresh `Tag` into the grammar arena with an explicit `hash`,
    /// `number`, and `type`, returning its `TagId`. Building tags directly (rather
    /// than via the parser) keeps the trie tests self-contained while still using
    /// the real `Grammar` arena the trie functions read through.
    fn mk_tag(g: &mut Grammar, hash: u32, number: u32, type_: crate::tag::TagType) -> TagId {
        let t = Tag {
            hash: crate::types::TagHash(hash),
            number,
            r#type: type_,
            ..Default::default()
        };
        TagId(g.single_tags_list.alloc(t))
    }

    // trie_insert builds length-N paths (creating child levels), trie_singular
    // reports a non-branching terminal chain, trie_has_type finds a type bit
    // anywhere in the trie, and trie_get_tag_list flattens every path. A second
    // insert of the SAME path returns false (already terminal); a divergent path
    // adds a branch so trie_singular becomes false.
    // [spec:cg3:sem:tag-trie.cg3.trie-insert-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-singular-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-has-type-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-get-tag-list-fn/test]
    #[test]
    fn insert_singular_has_type_and_tag_list() {
        let mut g = Grammar::default();
        // Distinct ascending hashes so ordering is unambiguous.
        let a = mk_tag(&mut g, 10, 0, crate::tag::TagType::empty());
        let b = mk_tag(&mut g, 20, 1, T_MAPPING);
        let c = mk_tag(&mut g, 30, 2, crate::tag::TagType::empty());

        let mut trie = trie_t::new();
        // Insert the 2-tag path [a, b].
        assert!(trie_insert(&mut trie, &vec![a, b], 0));
        // A single non-branching chain ending in a terminal -> singular.
        assert!(trie_singular(&trie));
        // Re-inserting the identical path: the a-node is not terminal (it has a
        // child), so recursion reaches the terminal b-node -> already present.
        assert!(!trie_insert(&mut trie, &vec![a, b], 0));

        // `b` carries T_MAPPING; has_type sees it through the sub-trie.
        assert!(trie_has_type(&trie, T_MAPPING, &g));
        assert!(!trie_has_type(&trie, crate::tag::T_FAILFAST, &g));

        // trie_get_tag_list flattens the whole trie (every key at every depth),
        // in ascending-hash order: a (10) then its child b (20).
        let list = trie_get_tag_list(&trie, &g);
        assert_eq!(list, vec![a, b]);

        // Add a divergent path [a, c] -> the a-node now branches (b and c),
        // so the trie is no longer a single chain.
        assert!(trie_insert(&mut trie, &vec![a, c], 0));
        assert!(!trie_singular(&trie));
        // Now flattening yields a, then its children b (20) and c (30) ordered.
        let list = trie_get_tag_list(&trie, &g);
        assert_eq!(list, vec![a, b, c]);
    }

    // trie_get_tags reproduces the documented SORT-THEN-POP corruption: on a
    // terminal it sorts the shared `tv` prefix by hash and pops the HIGHEST-hash
    // element (not the just-pushed one), corrupting the prefix for later siblings.
    // trie_get_tags_ordered does NOT sort, so its `pop` is correct.
    // [spec:cg3:sem:tag-trie.cg3.trie-get-tags-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-get-tags-ordered-fn/test]
    #[test]
    fn get_tags_sort_pop_corruption() {
        let mut g = Grammar::default();
        // Root tag `p` has a HIGHER hash than the two leaves so that sorting the
        // shared prefix reorders it to the end and the pop removes the wrong tag.
        let leaf_lo = mk_tag(&mut g, 5, 0, crate::tag::TagType::empty()); // low hash leaf
        let leaf_hi = mk_tag(&mut g, 7, 1, crate::tag::TagType::empty()); // higher-hash leaf
        let p = mk_tag(&mut g, 100, 2, crate::tag::TagType::empty()); // high-hash shared prefix

        // Trie shape: p -> { leaf_lo (terminal), leaf_hi (terminal) }.
        let mut trie = trie_t::new();
        assert!(trie_insert(&mut trie, &vec![p, leaf_lo], 0));
        assert!(trie_insert(&mut trie, &vec![p, leaf_hi], 0));

        // ORDERED variant: no sorting, faithful backtracking. Both full paths
        // survive: [p, leaf_lo] and [p, leaf_hi].
        let ordered = trie_get_tags_ordered(&trie, &g);
        let mut ordered_v: Vec<TagVector> = ordered.into_iter().collect();
        ordered_v.sort();
        assert_eq!(ordered_v, vec![vec![p, leaf_lo], vec![p, leaf_hi]]);

        // BUGGY variant: for the first terminal (leaf_lo), tv = [p, leaf_lo] is
        // sorted by hash -> [leaf_lo(5), p(100)], inserted, then the LAST element
        // (p, highest hash) is popped, leaving tv = [leaf_lo]. The second sibling
        // then pushes leaf_hi onto the corrupted prefix -> [leaf_lo, leaf_hi],
        // which is sorted -> [leaf_lo(5), leaf_hi(7)] and inserted. So the second
        // path lost `p` entirely.
        let buggy = trie_get_tags(&trie, &g);
        let mut buggy_v: Vec<TagVector> = buggy.into_iter().collect();
        buggy_v.sort();
        // Sorted by Vec<TagId> Ord: [leaf_lo, leaf_hi] (ids [0,1]) then
        // [leaf_lo, p] (ids [0,2]). The second path lost `p` -> corruption.
        assert_eq!(
            buggy_v,
            vec![vec![leaf_lo, leaf_hi], vec![leaf_lo, p]],
            "sort-then-pop corrupts the shared prefix for the later sibling"
        );
        // The corrupted result differs from the faithful ordered result.
        assert_ne!(buggy_v, ordered_v);
    }

    // trie_rehash folds hashes order-sensitively; trie_markused sets T_USED on
    // every tag; trie_serialize emits the big-endian byte layout in ascending-hash
    // order. All three read/write through the grammar arena.
    // [spec:cg3:sem:tag-trie.cg3.trie-rehash-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-markused-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-serialize-fn/test]
    #[test]
    fn rehash_markused_serialize() {
        let mut g = Grammar::default();
        let a = mk_tag(&mut g, 0x11, 7, crate::tag::TagType::empty()); // number 7
        let b = mk_tag(&mut g, 0x22, 9, crate::tag::TagType::empty()); // number 9

        let mut trie = trie_t::new();
        // Two single-tag terminal paths: a and b (both top-level terminals).
        assert!(trie_insert(&mut trie, &vec![a], 0));
        assert!(trie_insert(&mut trie, &vec![b], 0));

        // rehash is deterministic and non-zero for a non-empty trie.
        let h1 = trie_rehash(&trie, &g);
        let h2 = trie_rehash(&trie, &g);
        assert_eq!(h1, h2);
        assert_ne!(h1, 0);

        // markused sets T_USED on every tag reachable from the trie.
        assert!(!g.single_tags_list[a.0].r#type.intersects(T_USED));
        trie_markused(&trie, &mut g);
        assert!(g.single_tags_list[a.0].r#type.intersects(T_USED));
        assert!(g.single_tags_list[b.0].r#type.intersects(T_USED));

        // serialize: two top-level terminal, childless nodes visited in
        // ascending-hash order (a:0x11 then b:0x22). Per node the bytes are
        // [number: u32 BE][terminal: u8 = 1][childCount: u32 BE = 0].
        let mut buf: Vec<u8> = Vec::new();
        trie_serialize(&trie, &mut buf, &g);
        #[rustfmt::skip]
        let expected: Vec<u8> = vec![
            0, 0, 0, 7,   1,   0, 0, 0, 0, // node a: number 7, terminal, 0 children
            0, 0, 0, 9,   1,   0, 0, 0, 0, // node b: number 9, terminal, 0 children
        ];
        assert_eq!(buf, expected);
    }

    // trie_copy / trie_copy_helper deep-copy node structure + terminal flags
    // (sharing tag ids), and trie_delete frees only descendant sub-tries while
    // keeping top-level keys/terminal flags. Copy of a nested trie exercises the
    // recursive helper; delete then flattens the copy to its top level.
    // [spec:cg3:sem:tag-trie.cg3.trie-copy-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-copy-helper-fn/test]
    // [spec:cg3:sem:tag-trie.cg3.trie-delete-fn/test]
    #[test]
    fn copy_and_delete() {
        let mut g = Grammar::default();
        let a = mk_tag(&mut g, 1, 0, crate::tag::TagType::empty());
        let b = mk_tag(&mut g, 2, 1, crate::tag::TagType::empty());
        let c = mk_tag(&mut g, 3, 2, crate::tag::TagType::empty());

        // Two paths sharing the `a` prefix: [a, b] and [a, c] -> a has a sub-trie
        // with two children (drives trie_copy_helper recursion).
        let mut trie = trie_t::new();
        trie_insert(&mut trie, &vec![a, b], 0);
        trie_insert(&mut trie, &vec![a, c], 0);

        // Deep copy: independent structure, same tag ids, same paths.
        let mut copy = trie_copy(&trie);
        assert_eq!(trie_get_tag_list(&copy, &g), vec![a, b, c]);
        // The a-node in the copy owns its own (non-shared) sub-trie.
        assert!(copy.get(&a).unwrap().trie.is_some());

        // Mutating the copy's structure must not affect the original.
        let d = mk_tag(&mut g, 4, 3, crate::tag::TagType::empty());
        trie_insert(&mut copy, &vec![a, d], 0);
        assert_eq!(trie_get_tag_list(&copy, &g), vec![a, b, c, d]);
        assert_eq!(trie_get_tag_list(&trie, &g), vec![a, b, c]); // original intact

        // trie_delete frees descendant sub-tries but keeps top-level keys.
        trie_delete(&mut copy);
        // The single top-level key `a` remains, but its child level is gone.
        assert_eq!(copy.len(), 1);
        assert!(copy.contains_key(&a));
        assert!(copy.get(&a).unwrap().trie.is_none());
    }
}
