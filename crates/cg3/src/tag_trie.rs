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

use crate::arena::{Arena, TagId};
use crate::grammar::{GrammarCore, Phase};
use crate::inlines::{hash_value, write_be};
use crate::tag::{T_USED, Tag, TagList, TagVector, TagVectorSet};

// [spec:cg3:def:tag-trie.cg3.trie-node-t]
/// C++ `struct trie_node_t { bool terminal = false; std::unique_ptr<trie_t> trie; }`.
///
/// `std::unique_ptr<trie_t>` (a nullable owning child) → `Option<Box<TagTrie>>`.
///
/// `Clone` is written out, beside [`trie_copy_helper`], rather than derived.
#[derive(Default, Debug)]
pub struct TrieNode {
    /// `bool terminal = false;`
    pub terminal: bool,
    /// `std::unique_ptr<trie_t> trie;` — the child level, null when absent.
    pub trie: Option<Box<TagTrie>>,
}

/// Frees the levels below a node one at a time. The derived drop would
/// recurse once per level, and a trie read from a `.cg3b` is as deep as the
/// file makes it.
impl Drop for TrieNode {
    fn drop(&mut self) {
        let mut pending: Vec<Box<TagTrie>> = self.trie.take().into_iter().collect();
        while let Some(mut level) = pending.pop() {
            let nodes = std::mem::take(&mut *level).into_values();
            pending.extend(nodes.filter_map(|mut node| node.trie.take()));
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-t]
/// C++ `typedef bc::flat_map<Tag*, trie_node_t, compare_Tag> trie_t`.
///
/// Ported as a `BTreeMap` keyed by [`TagId`]; C++ ordering-by-`Tag->hash` is
/// re-derived where it matters (see module docs). Clean public alias so the
/// lead can repoint `Set::trie` / `Set::trie_special` here (replacing the
/// `crate::set::TrieTodo` placeholder).
pub type TagTrie = BTreeMap<TagId, TrieNode>;

/// Collects the trie's entries in the C++ `compare_Tag` order — ascending
/// `Tag->hash`. Not a manifest symbol: port infrastructure standing in for the
/// flat_map's intrinsic hash ordering. STABLE sort → equal-hash entries keep
/// their `TagId` order (they would have collided into one key in C++).
fn ordered_entries<'a>(trie: &'a TagTrie, tags: &Arena<Tag>) -> Vec<(TagId, &'a TrieNode)> {
    let mut v: Vec<(TagId, &TrieNode)> = trie.iter().map(|(k, n)| (*k, n)).collect();
    v.sort_by(|a, b| {
        let ha = tags[a.0.0].hash;
        let hb = tags[b.0.0].hash;
        ha.cmp(&hb)
    });
    v
}

/// One level of a [`TrieWalk`]: the entries of one trie map still to visit.
enum Level<'a> {
    /// In `BTreeMap` (`TagId`) order, for walks whose result is order-free.
    Keys(std::collections::btree_map::Iter<'a, TagId, TrieNode>),
    /// In the C++ `compare_Tag` order, from [`ordered_entries`].
    Ordered(std::vec::IntoIter<(TagId, &'a TrieNode)>),
}

impl<'a> Iterator for Level<'a> {
    type Item = (TagId, &'a TrieNode);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Level::Keys(it) => it.next().map(|(k, n)| (*k, n)),
            Level::Ordered(it) => it.next(),
        }
    }
}

// [spec:cg3:req:robustness.depth-bounded]
/// A pre-order walk over a trie that keeps its path on the heap. A trie is as
/// deep as the longest composite tag in its set, and a `.cg3b` can make that
/// any depth at all, so the C++'s recursive walks would let the input choose
/// how much stack they take.
///
/// [`next_entry`](Self::next_entry) yields each entry of the level being
/// walked, with its depth (0 for the trie itself), and picks the level above
/// back up where it left off once a level runs out — where a recursive walk
/// returns to its caller. It enters a sub-trie only when told to: the caller
/// calls [`descend`](Self::descend) exactly where the recursion it replaces
/// recurses, so each walk visits in the order of its C++ original.
pub struct TrieWalk<'a> {
    levels: Vec<Level<'a>>,
    /// Orders each level by hash, as [`ordered_entries`] does, when set.
    tags: Option<&'a Arena<Tag>>,
}

impl<'a> TrieWalk<'a> {
    /// A walk visiting each level in `TagId` order.
    pub fn new(trie: &'a TagTrie) -> Self {
        TrieWalk {
            levels: vec![Level::Keys(trie.iter())],
            tags: None,
        }
    }

    /// A walk visiting each level in ascending `Tag::hash` order.
    pub fn ordered<P: Phase>(trie: &'a TagTrie, grammar: &'a GrammarCore<P>) -> Self {
        let tags = &grammar.single_tags_list;
        TrieWalk {
            levels: vec![Level::Ordered(ordered_entries(trie, tags).into_iter())],
            tags: Some(tags),
        }
    }

    /// The next entry and its depth, or `None` once the whole trie is walked.
    pub fn next_entry(&mut self) -> Option<(TagId, &'a TrieNode, usize)> {
        loop {
            let depth = self.levels.len().checked_sub(1)?;
            match self.levels[depth].next() {
                Some((k, node)) => return Some((k, node, depth)),
                None => {
                    self.levels.pop();
                }
            }
        }
    }

    /// Walk `sub`, the sub-trie of the entry just yielded, before the rest of
    /// that entry's level.
    pub fn descend(&mut self, sub: &'a TagTrie) {
        let level = match self.tags {
            Some(tags) => Level::Ordered(ordered_entries(sub, tags).into_iter()),
            None => Level::Keys(sub.iter()),
        };
        self.levels.push(level);
    }

    /// Visit every entry, entering every sub-trie: the walk of the functions
    /// that only need to see each tag once.
    pub fn each(mut self, mut visit: impl FnMut(TagId, &'a TrieNode)) {
        while let Some((k, node, _)) = self.next_entry() {
            visit(k, node);
            if let Some(sub) = &node.trie {
                self.descend(sub);
            }
        }
    }
}

/// `std::sort(tv.begin(), tv.end(), compare_Tag())` — sort a tag vector ascending
/// by `Tag->hash`. (Stable here vs C++ `std::sort`'s unstable; only differs on
/// equal-hash ties, which do not occur with unique tag hashes.)
fn sort_tv_by_hash<P: Phase>(tv: &mut TagVector, grammar: &GrammarCore<P>) {
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
///
/// The C++ recurses once per tag of `tv`; this descends in a loop, so a long
/// composite tag costs no stack.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_insert(trie: &mut TagTrie, tv: &TagVector, w: usize) -> bool {
    let mut level = trie;
    let mut w = w;
    loop {
        let node = level.entry(tv[w]).or_default();
        if node.terminal {
            return false;
        }
        if w + 1 >= tv.len() {
            node.terminal = true;
            node.trie = None; // node.trie.reset()
            return true;
        }
        w += 1;
        level = node.trie.get_or_insert_with(Box::default);
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-copy-helper-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-copy-helper-fn]
/// C++ `_trie_copy_helper` → `Box<trie_t>` (was `std::unique_ptr<trie_t>`). The
/// `Tag` keys are shared (`TagId`s copied, tags not cloned); only node structure
/// and terminal flags are duplicated. Order-independent (a keyed rebuild), so no
/// grammar needed.
///
/// The C++ recurses per level; this copies each level on a heap stack and
/// hangs the copy under its parent's entry once the level is complete, so the
/// copy costs no stack however deep the trie is.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_copy_helper(trie: &TagTrie) -> Box<TagTrie> {
    let mut levels = vec![CopyLevel::of(trie)];
    let mut copy = None;
    while copy.is_none() {
        copy = copy_step(&mut levels);
    }
    copy.unwrap_or_default()
}

/// One step of [`trie_copy_helper`]: copy the next entry of the innermost
/// level being copied, or, once that level is done, hang its copy under the
/// entry it was copied for. Returns the whole copy when the outermost level
/// is done.
fn copy_step(levels: &mut Vec<CopyLevel<'_>>) -> Option<Box<TagTrie>> {
    let Some(top) = levels.last_mut() else {
        return Some(Box::default());
    };
    if let Some((k, node)) = top.src.next() {
        match &node.trie {
            Some(sub) => {
                top.open = Some((*k, node.terminal));
                levels.push(CopyLevel::of(sub));
            }
            None => {
                let leaf = TrieNode {
                    terminal: node.terminal,
                    trie: None,
                };
                top.out.insert(*k, leaf);
            }
        }
        return None;
    }
    let done = levels.pop()?;
    let Some(parent) = levels.last_mut() else {
        return Some(Box::new(done.out));
    };
    if let Some((k, terminal)) = parent.open.take() {
        let branch = TrieNode {
            terminal,
            trie: Some(Box::new(done.out)),
        };
        parent.out.insert(k, branch);
    }
    None
}

/// A level [`trie_copy_helper`] is copying: its source entries still to copy,
/// the copy so far, and the entry whose sub-trie is being copied beneath it.
struct CopyLevel<'a> {
    src: std::collections::btree_map::Iter<'a, TagId, TrieNode>,
    out: TagTrie,
    open: Option<(TagId, bool)>,
}

impl<'a> CopyLevel<'a> {
    fn of(src: &'a TagTrie) -> Self {
        CopyLevel {
            src: src.iter(),
            out: TagTrie::new(),
            open: None,
        }
    }
}

// [spec:cg3:req:robustness.depth-bounded]
/// A deep copy through [`trie_copy_helper`]. The derived `Clone` recursed once
/// per level through `BTreeMap::clone`, and sets clone their tries freely.
impl Clone for TrieNode {
    fn clone(&self) -> Self {
        TrieNode {
            terminal: self.terminal,
            trie: self.trie.as_deref().map(trie_copy_helper),
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-copy-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-copy-fn]
/// C++ `trie_copy` — deep-copies a whole trie, returning a new `trie_t` by value.
/// Order-independent, so no grammar needed.
pub fn trie_copy(trie: &TagTrie) -> TagTrie {
    let mut nt = TagTrie::new();
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
/// Walks with an explicit stack: a trie read from a `.cg3b` is as deep as the
/// file makes it.
pub fn trie_delete(trie: &mut TagTrie) {
    // p.second.trie.reset() on every node, top level first.
    let mut pending: Vec<Box<TagTrie>> = trie.values_mut().filter_map(|n| n.trie.take()).collect();
    while let Some(mut level) = pending.pop() {
        pending.extend(level.values_mut().filter_map(|n| n.trie.take()));
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-singular-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-singular-fn]
/// C++ `trie_singular` — true iff the trie is a single non-branching chain that
/// ends in a terminal. Only inspects the sole entry, so order-independent.
/// Follows the chain in a loop where the C++ recurses.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_singular(trie: &TagTrie) -> bool {
    let mut level = trie;
    while let Some(node) = sole_entry(level) {
        if node.terminal {
            return true;
        }
        let Some(sub) = &node.trie else {
            return false;
        };
        level = sub;
    }
    false
}

/// `trie.begin()->second` when the trie has exactly one entry.
fn sole_entry(trie: &TagTrie) -> Option<&TrieNode> {
    if trie.len() != 1 {
        return None;
    }
    trie.values().next()
}

// [spec:cg3:def:tag-trie.cg3.trie-rehash-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-rehash-fn]
/// C++ `trie_rehash` — folds each tag's precomputed `hash` (and, recursively, the
/// sub-trie's rehash) into a running value with `hash_value`. ORDER-SENSITIVE
/// (`hash_value` is non-commutative), so entries are visited in ascending-hash
/// order via [`ordered_entries`] and `grammar` is required. Terminal flags are
/// NOT hashed (parity note).
///
/// Each level's running value is kept on a heap stack beside the walk, and a
/// finished sub-trie's value is folded into its parent's before the parent's
/// next entry, where the C++ recursion returns it.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_rehash<P: Phase>(trie: &TagTrie, grammar: &GrammarCore<P>) -> u32 {
    let mut walk = TrieWalk::ordered(trie, grammar);
    let mut retvals: Vec<u32> = vec![0];
    while let Some((k, node, depth)) = walk.next_entry() {
        fold_finished_levels(&mut retvals, depth + 1);
        let h = grammar.single_tags_list[k.0].hash;
        retvals[depth] = hash_value(h.get(), retvals[depth]);
        if let Some(sub) = &node.trie {
            walk.descend(sub);
            retvals.push(0);
        }
    }
    fold_finished_levels(&mut retvals, 1);
    retvals[0]
}

/// Fold the values of the levels [`trie_rehash`] has finished, innermost
/// first, each into the level above it, until `live` levels are left.
fn fold_finished_levels(retvals: &mut Vec<u32>, live: usize) {
    while retvals.len() > live {
        let Some(sub) = retvals.pop() else { return };
        if let Some(parent) = retvals.last_mut() {
            *parent = hash_value(sub, *parent);
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-markused-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-markused-fn]
/// C++ `trie_markused` — calls `kv.first->markUsed()` on every tag (and recurses).
/// `Tag::markUsed()` is `type |= T_USED`; the `Tag` methods are not ported yet,
/// so the mask is applied inline here. Marking is order-independent, but it
/// MUTATES the tags, so this takes `grammar: &mut Grammar`. (Lead: at the call
/// site the trie lives inside a `Set` owned by the same `Grammar`; the borrow of
/// `grammar.single_tags_list` and the immutable borrow of the set's trie must be
/// split — restructure or clone as needed.) Walks with a [`TrieWalk`].
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_markused<P: Phase>(trie: &TagTrie, grammar: &mut GrammarCore<P>) {
    TrieWalk::new(trie).each(|k, _| grammar.single_tags_list.get_mut(k.0).r#type |= T_USED);
}

// [spec:cg3:def:tag-trie.cg3.trie-has-type-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-has-type-fn]
/// C++ `trie_hasType` — true iff any tag anywhere has any bit of `type_` set in
/// its own `type` mask. Order-independent for the boolean result, but reads
/// `Tag::type`, so `grammar` is required. (C++ takes `trie_t&`; the port takes
/// `&trie_t` since it never mutates.) Walks with a [`TrieWalk`].
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_has_type<P: Phase>(
    trie: &TagTrie,
    type_: crate::tag::TagType,
    grammar: &GrammarCore<P>,
) -> bool {
    let mut walk = TrieWalk::new(trie);
    while let Some((k, node, _)) = walk.next_entry() {
        if grammar.single_tags_list[k.0].r#type.intersects(type_) {
            return true;
        }
        if let Some(sub) = &node.trie {
            walk.descend(sub);
        }
    }
    false
}

// Unspecced C++ overload `trie_getTagList(const trie_t&, TagList&)` (void): appends
// every tag of every path onto `the_tags` (no node search). Output order matches
// the C++ flat_map hash order, so `grammar` is required.
/// See [`trie_get_tag_list_find`] for the spec'd sibling overload. Walks with a
/// [`TrieWalk`].
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_get_tag_list_append<P: Phase>(
    trie: &TagTrie,
    the_tags: &mut TagList,
    grammar: &GrammarCore<P>,
) {
    TrieWalk::ordered(trie, grammar).each(|k, _| the_tags.push(k));
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
/// (`&kv.second`) cast to `*const c_void`. This faithful-overload form is kept
/// for the spec'd C++ signature; the live unification path no longer routes
/// through it — the matcher/`getTagList` port replaced the `&kv` address token
/// with the address-free `UnifKey` (`(special, root-to-node TagId path)`), which
/// `getTagList` resolves by appending `path` directly (same output order as this
/// walk's successful branch).
///
/// Walks with a [`TrieWalk`]: cutting `the_tags` back to the depth of each
/// entry before pushing it is the C++'s `pop_back` after each subtree.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_get_tag_list_find<P: Phase>(
    trie: &TagTrie,
    the_tags: &mut TagList,
    node: *const core::ffi::c_void,
    grammar: &GrammarCore<P>,
) -> bool {
    let base = the_tags.len();
    let mut walk = TrieWalk::ordered(trie, grammar);
    while let Some((k, n, depth)) = walk.next_entry() {
        the_tags.truncate(base + depth);
        the_tags.push(k);
        if node == (n as *const TrieNode as *const core::ffi::c_void) {
            return true;
        }
        if let Some(sub) = &n.trie {
            walk.descend(sub);
        }
    }
    the_tags.truncate(base);
    false
}

// Unspecced C++ overload `trie_getTagList(const trie_t&) -> TagVector`: returns the
// full tag list (delegates sub-tries to [`trie_get_tag_list_append`]). Output
// order is the flat_map hash order, so `grammar` is required.
pub fn trie_get_tag_list<P: Phase>(trie: &TagTrie, grammar: &GrammarCore<P>) -> TagVector {
    let mut the_tags = TagVector::new();
    for (k, node) in ordered_entries(trie, &grammar.single_tags_list) {
        the_tags.push(k);
        if let Some(sub) = &node.trie {
            trie_get_tag_list_append(sub, &mut the_tags, grammar);
        }
    }
    the_tags
}

// Unspecced C++ shared-`tv` helper `trie_getTags(const trie_t&, TagVectorSet&,
// TagVector&)`. Extends `tv` one level deeper; on a terminal it reproduces the
// SORT-THEN-POP BUG (see [`trie_get_tags`]). Walks with a [`TrieWalk`]; the
// C++ does nothing to `tv` on returning from a sub-trie, so neither does this.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_get_tags_into<P: Phase>(
    trie: &TagTrie,
    rv: &mut TagVectorSet,
    tv: &mut TagVector,
    grammar: &GrammarCore<P>,
) {
    let mut walk = TrieWalk::ordered(trie, grammar);
    while let Some((k, node, _)) = walk.next_entry() {
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
            walk.descend(sub);
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
pub fn trie_get_tags<P: Phase>(trie: &TagTrie, grammar: &GrammarCore<P>) -> TagVectorSet {
    let mut rv = TagVectorSet::new();
    for (k, node) in ordered_entries(trie, &grammar.single_tags_list) {
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
// so backtracking (`pop`) correctly removes the just-pushed tag. Walks with a
// [`TrieWalk`], as [`trie_get_tags_into`] does.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_get_tags_ordered_into<P: Phase>(
    trie: &TagTrie,
    rv: &mut TagVectorSet,
    tv: &mut TagVector,
    grammar: &GrammarCore<P>,
) {
    let mut walk = TrieWalk::ordered(trie, grammar);
    while let Some((k, node, _)) = walk.next_entry() {
        tv.push(k);
        if node.terminal {
            rv.insert(tv.clone());
            tv.pop();
            continue;
        }
        if let Some(sub) = &node.trie {
            walk.descend(sub);
        }
    }
}

// [spec:cg3:def:tag-trie.cg3.trie-get-tags-ordered-fn]
// [spec:cg3:sem:tag-trie.cg3.trie-get-tags-ordered-fn]
/// C++ `trie_getTagsOrdered(const trie_t&) -> TagVectorSet`. Like
/// [`trie_get_tags`] but WITHOUT any per-sequence sorting: paths preserve their
/// in-trie (ascending-hash) order, so `pop` always removes the just-pushed tag
/// (no corruption). `grammar` is required for the hash ordering.
pub fn trie_get_tags_ordered<P: Phase>(trie: &TagTrie, grammar: &GrammarCore<P>) -> TagVectorSet {
    let mut rv = TagVectorSet::new();
    for (k, node) in ordered_entries(trie, &grammar.single_tags_list) {
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
///
/// Walks with a [`TrieWalk`], so the bytes come out in the C++'s order.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_serialize<W: Write, P: Phase>(trie: &TagTrie, out: &mut W, grammar: &GrammarCore<P>) {
    let mut walk = TrieWalk::ordered(trie, grammar);
    while let Some((k, node, _)) = walk.next_entry() {
        let number = grammar.single_tags_list[k.0].number;
        write_be(out, number); // writeBE<uint32_t>(out, kv.first->number)
        write_be(out, node.terminal as u8); // writeBE<uint8_t>(out, kv.second.terminal)
        if let Some(sub) = &node.trie {
            write_be(out, sub.len() as u32); // writeBE<uint32_t>(out, UI32(sub->size()))
            walk.descend(sub);
        } else {
            write_be(out, 0u32); // writeBE<uint32_t>(out, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::GrammarDraft;
    use crate::tag::{T_MAPPING, Tag};

    /// Intern a fresh `Tag` into the grammar arena with an explicit `hash`,
    /// `number`, and `type`, returning its `TagId`. Building tags directly (rather
    /// than via the parser) keeps the trie tests self-contained while still using
    /// the real `Grammar` arena the trie functions read through.
    fn mk_tag<P: Phase>(
        g: &mut GrammarCore<P>,
        hash: u32,
        number: u32,
        type_: crate::tag::TagType,
    ) -> TagId {
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
        let mut g = GrammarDraft::default();
        // Distinct ascending hashes so ordering is unambiguous.
        let a = mk_tag(&mut g, 10, 0, crate::tag::TagType::empty());
        let b = mk_tag(&mut g, 20, 1, T_MAPPING);
        let c = mk_tag(&mut g, 30, 2, crate::tag::TagType::empty());

        let mut trie = TagTrie::new();
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
        let mut g = GrammarDraft::default();
        // Root tag `p` has a HIGHER hash than the two leaves so that sorting the
        // shared prefix reorders it to the end and the pop removes the wrong tag.
        let leaf_lo = mk_tag(&mut g, 5, 0, crate::tag::TagType::empty()); // low hash leaf
        let leaf_hi = mk_tag(&mut g, 7, 1, crate::tag::TagType::empty()); // higher-hash leaf
        let p = mk_tag(&mut g, 100, 2, crate::tag::TagType::empty()); // high-hash shared prefix

        // Trie shape: p -> { leaf_lo (terminal), leaf_hi (terminal) }.
        let mut trie = TagTrie::new();
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
        let mut g = GrammarDraft::default();
        let a = mk_tag(&mut g, 0x11, 7, crate::tag::TagType::empty()); // number 7
        let b = mk_tag(&mut g, 0x22, 9, crate::tag::TagType::empty()); // number 9

        let mut trie = TagTrie::new();
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
        let mut g = GrammarDraft::default();
        let a = mk_tag(&mut g, 1, 0, crate::tag::TagType::empty());
        let b = mk_tag(&mut g, 2, 1, crate::tag::TagType::empty());
        let c = mk_tag(&mut g, 3, 2, crate::tag::TagType::empty());

        // Two paths sharing the `a` prefix: [a, b] and [a, c] -> a has a sub-trie
        // with two children (drives trie_copy_helper recursion).
        let mut trie = TagTrie::new();
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
