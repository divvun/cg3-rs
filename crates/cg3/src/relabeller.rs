//! Port of `src/Relabeller.hpp` / `src/Relabeller.cpp` — renames tags/sets in a
//! target grammar according to a second "relabels" grammar of MAP rules.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). Uses TWO grammars: the target
//! `grammar` (mutated) and the read-only `relabels` grammar (the rules). C++ raw
//! `Tag*`/`Set*` become [`TagId`]/[`SetId`] resolved through the owning grammar's
//! arenas.
//!
//! ## Arena adaptations (documented once)
//! * C++ `grammar->allocateSet()` returns a `new Set` tracked only in `sets_all`,
//!   NOT yet in `sets_list`; `addSetToGrammar` then `push_back`s it and sets
//!   `number = sets_list.size()-1`. The port's [`GrammarCore::allocate_set`] places
//!   the set in the `sets_list` ARENA at slot `SetId.0`, but not in the numbered
//!   `sets_list_order` vector; `addSetToGrammar` pushes it there and assigns the
//!   dense `number = sets_list_order.len()-1`, deriving the `setName` argument
//!   from `sets_list_order.len()` (the analog of `sets_list.size()`) — exactly
//!   like C++.
//! * `relabels.sets_list[n]` / `grammar->sets_list[i]` — `n`/`i` are DENSE set
//!   NUMBERS, resolved through the owning grammar's `sets_list_order`
//!   ([`GrammarCore::set_id_by_number`]).
//! * `reindexSet` recurses over `s.sets` treating each entry as a set NUMBER
//!   (`grammar->sets_list[i]`), NOT a content hash — so it is a DISTINCT function
//!   from [`crate::set::Set::reindex`] (which resolves children by content hash).
//!   It is ported here as a private method rather than reusing `Set::reindex`.
//! * `grammar->addTag(new Tag(*tag_r))` (deep-copy then intern) → clone the tag
//!   value out of the source arena and hand it to [`TagSpace::add_tag`] (by value),
//!   which interns/dedups and returns the canonical [`TagId`].
//! * The relabel rules are keyed by tag STRING; the two maps use
//!   `HashMap<String, SetId>` (the `Set*` value is the relabel target's SetId in
//!   the RELABELS grammar).
//!
//! ## Flagged bugs reproduced
//! * The `%d special tags` warning format string has THREE conversions (%S, %d,
//!   %d) but only TWO arguments — the third `%d` reads garbage. The port emits no
//!   diagnostics (I/O deferred), but the truncated arg list is preserved as a
//!   comment at the site so the count-arg mismatch is documented.
//! * [`trie_copy`] (two-arg, re-interning): only the TOP level of the returned
//!   trie has its tags transferred into the target grammar. The nested recursion
//!   goes through the ONE-arg [`crate::tag_trie::trie_copy_helper`], which copies
//!   child levels by the ORIGINAL `TagId` WITHOUT re-interning — so nested
//!   (multi-tag-list) levels keep pointers to the SOURCE (relabels) grammar's
//!   tags. NOTE: in the arena port a `TagId` is only meaningful relative to a
//!   grammar; a source-grammar `TagId` reused as a target-grammar key resolves to
//!   whatever occupies that arena slot in the target — the faithful analog of the
//!   C++ dangling cross-grammar `Tag*`.
//! * [`trie_copy_helper`] (two-arg, re-interning) is DEAD CODE — never called (both
//!   `trie_copy` and this helper recurse through the one-arg form). Ported for
//!   completeness and marked dead.
//! * `copyRelabelSetToGrammar` copies `ff_tags` by value (`s_g->ff_tags =
//!   s_r->ff_tags`) — the raw source-grammar `TagId`s are copied WITHOUT
//!   re-interning into the target grammar (the C++ carried a `// TODO: does this
//!   get copied correctly?` comment).

use std::collections::{HashMap, HashSet};

use crate::arena::{SetId, TagId};
use crate::grammar::{GrammarCore, TagSpace};
use crate::set::Set;
use crate::strings::Keywords;
use crate::tag::{T_SPECIAL, TagVector, TagVectorSet};
use crate::tag_trie::{
    TagTrie, TrieNode, trie_copy_helper, trie_delete, trie_get_tag_list, trie_get_tags_ordered,
    trie_insert,
};
use crate::types::SetNumber;

// C++ Strings.hpp `enum : uint32_t { ... S_OR = 3, S_PLUS, S_MINUS, ... }`. Only
// the two operators the relabeller emits are reproduced here (same precedent as
// `grammar.rs`, which reproduces `S_OR`/`S_MINUS` locally). No spec:def id.
const S_OR: u32 = 3;
const S_PLUS: u32 = 4;

// [spec:cg3:def:relabeller.cg3.relabeller.tag-vector]
/// C++ `typedef std::vector<Tag*> Relabeller::TagVector`. Identical to the
/// crate-wide [`crate::tag::TagVector`] (`Vec<TagId>`); re-aliased here to match
/// the source's private typedef.
pub type RelabellerTagVector = Vec<TagId>;

// [spec:cg3:def:relabeller.cg3.relabeller.u-string-map]
/// C++ header typedef: an unordered map from string to string.
/// Declared in the header but unused by any ported method; reproduced for
/// fidelity.
pub type StringMap = HashMap<String, String>;

// [spec:cg3:def:relabeller.cg3.relabeller.u-string-set-map]
/// C++ header typedef: an unordered map from string to `Set*`.
/// The `Set*` value is a relabel-target set in the RELABELS grammar → [`SetId`].
pub type StringSetMap = HashMap<String, SetId>;

// [spec:cg3:def:relabeller.cg3.freq-sorter]
/// C++ `struct freq_sorter` — a comparator that sorts tags by DESCENDING
/// frequency. In the port it carries a borrow of the `tag_freq` map (keyed by
/// [`TagId`], the analog of the C++ `bc::flat_map<Tag*, size_t>`).
///
/// PORT NOTE: C++ used `freq_sorter` as an `std::sort` comparator (`operator()`).
/// Rust's `sort_by` wants a total `Ordering`, so [`Self::cmp`] wraps the boolean
/// `operator()` — `Ordering::Less` when `a`'s frequency is strictly greater than
/// `b`'s (highest frequency first), else `Greater`/`Equal` derived from the
/// symmetric compare. This reproduces the "highest frequency first" intent; the
/// stability difference from C++ `std::sort` only shows on equal-frequency ties,
/// which do not affect the cheap trie-compression heuristic.
struct FreqSorter<'a> {
    // [spec:cg3:def:relabeller.cg3.freq-sorter.freq-sorter-fn]
    // [spec:cg3:sem:relabeller.cg3.freq-sorter.freq-sorter-fn]
    // Constructor: store `tag_freq` by reference (no copy). The referred map must
    // outlive the functor (guaranteed by the `'a` borrow).
    tag_freq: &'a HashMap<TagId, usize>,
}

impl<'a> FreqSorter<'a> {
    fn new(tag_freq: &'a HashMap<TagId, usize>) -> Self {
        FreqSorter { tag_freq }
    }

    // [spec:cg3:def:relabeller.cg3.freq-sorter.operator-fn]
    // [spec:cg3:sem:relabeller.cg3.freq-sorter.operator-fn]
    /// C++ `bool operator()(Tag* a, Tag* b) const` — returns
    /// `tag_freq.find(a)->second > tag_freq.find(b)->second` WITHOUT an `end()`
    /// check (assumes both keys present; a missing key is UB in C++). The port's
    /// `HashMap` index `[a]` panics on a missing key — the same "must be present"
    /// contract. `a` orders before `b` iff `a`'s frequency is strictly greater.
    fn operator(&self, a: TagId, b: TagId) -> bool {
        self.tag_freq[&a] > self.tag_freq[&b]
    }

    /// Adapter turning the boolean `operator()` into a total `Ordering` for
    /// `sort_by` (see the struct-level PORT NOTE).
    fn cmp(&self, a: TagId, b: TagId) -> std::cmp::Ordering {
        if self.operator(a, b) {
            std::cmp::Ordering::Less
        } else if self.operator(b, a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }
}

/// A set [`Relabeller::copy_relabel_set_to_grammar`] is copying: the
/// relabel grammar's set, its copy, the numbers of the source's child sets,
/// and the numbers of the child sets copied so far.
struct RelabelCopy {
    s_r: SetId,
    s_g: SetId,
    children_r: Vec<u32>,
    sets_g: Vec<u32>,
}

// [spec:cg3:def:relabeller.cg3.trie-copy-fn]
// [spec:cg3:sem:relabeller.cg3.trie-copy-fn]
/// The TWO-argument, tag-re-interning copy of a tag trie (a different overload
/// from the one-argument [`crate::tag_trie::trie_copy`]). Iterates each entry of
/// `trie`: deep-copies the tag (clone the `Tag` value out of `grammar`'s arena)
/// and re-interns it via [`TagSpace::add_tag`], keying the new node by the
/// canonical target-grammar [`TagId`]; copies `terminal`; and for a child trie
/// recurses.
///
/// QUIRK (reproduced): the recursive call is the ONE-argument
/// [`crate::tag_trie::trie_copy_helper`], so nested child levels are copied by the
/// ORIGINAL `TagId` WITHOUT re-interning into `grammar`. Only the top level has
/// its tags transferred; deeper levels keep source-grammar tag ids.
pub fn trie_copy(trie: &TagTrie, grammar: &mut GrammarCore) -> TagTrie {
    let mut nt = TagTrie::new();
    // Collect the source keys/nodes first so the `&mut grammar` re-intern borrow
    // does not alias an immutable borrow of `trie` (which lives inside a Set in
    // the same grammar at the call sites).
    let entries: Vec<(TagId, TrieNode)> = trie.iter().map(|(k, n)| (*k, n.clone())).collect();
    for (k, node) in entries {
        // Tag* t = new Tag(*p.first); t = grammar.addTag(t);
        let tagcopy = grammar.single_tags_list[k.0].clone();
        let t = grammar.add_tag(tagcopy);
        let n = nt.entry(t).or_default();
        n.terminal = node.terminal;
        if let Some(sub) = &node.trie {
            // QUIRK: single-argument → the one-arg helper (no re-interning).
            n.trie = Some(trie_copy_helper(sub));
        }
    }
    nt
}

// [spec:cg3:def:relabeller.cg3.trie-copy-helper-fn]
// [spec:cg3:sem:relabeller.cg3.trie-copy-helper-fn]
/// The TWO-argument, tag-re-interning recursive helper (a different overload from
/// the one-argument [`crate::tag_trie::trie_copy_helper`]). Allocates a heap
/// `Box<trie_t>`, re-interns each top-level tag into `grammar`, and — as in
/// [`trie_copy`] — recurses through the ONE-argument helper (no nested
/// re-interning).
///
/// QUIRK (reproduced): this two-argument overload is NEVER called anywhere in the
/// Relabeller (both [`trie_copy`] and this helper recurse through the one-arg
/// form), so it is effectively DEAD CODE in the C++. Ported for completeness (it
/// is public API, exercised only by the spec test); the intended deep
/// re-interning of nested trie levels does not occur.
pub fn trie_copy_helper_reintern(trie: &TagTrie, grammar: &mut GrammarCore) -> Box<TagTrie> {
    let mut nt = Box::new(TagTrie::new());
    let entries: Vec<(TagId, TrieNode)> = trie.iter().map(|(k, n)| (*k, n.clone())).collect();
    for (k, node) in entries {
        let tagcopy = grammar.single_tags_list[k.0].clone();
        let t = grammar.add_tag(tagcopy);
        let n = nt.entry(t).or_default();
        n.terminal = node.terminal;
        if let Some(sub) = &node.trie {
            // single-argument → the one-arg helper (no re-interning).
            n.trie = Some(trie_copy_helper(sub));
        }
    }
    nt
}

/// A relabel rule the relabeller has no way to apply.
#[derive(Debug, thiserror::Error)]
#[error(
    "Error: Relabel rule on line {line} is a {keyword} rule, which has no tag list to relabel from; only MAP rules relabel"
)]
pub struct RelabelRuleError {
    /// The line of the relabel file the rule is on.
    pub line: u32,
    /// The rule's keyword.
    pub keyword: &'static str,
}

impl RelabelRuleError {
    fn for_rule(rule: &crate::rule::Rule) -> Self {
        RelabelRuleError {
            line: rule.line,
            keyword: crate::strings::KEYWORDS_STR
                .get(rule.r#type as usize)
                .copied()
                .unwrap_or_default(),
        }
    }
}

/// The C++ constructor's guards: whether `rule` is one the relabeller skips
/// (with a warning, deferred I/O) rather than records.
fn is_skipped(
    relabels: &GrammarCore,
    rule: &crate::rule::Rule,
    maplist: SetId,
    target: SetId,
    from_tags: usize,
) -> bool {
    // if (!(maplist->trie_special.empty() && target->trie_special.empty()))
    let maplist_special_empty = relabels.sets_list[maplist.0].trie_special.is_empty();
    let target_special_empty = relabels.sets_list[target.0].trie_special.is_empty();
    if !(maplist_special_empty && target_special_empty) {
        // "Warning: Relabel rule '%S' on line %d has %d special tags,
        // skipping!\n" — BUG: three conversions (%S, %d, %d), only two
        // args (rule->name, rule->line); the third %d reads garbage.
        // Diagnostic deferred; the arg-count mismatch is preserved here.
        return true;
    }
    // In order, each skipping with a warning: "... had context tests",
    // "... had a wordform", "... has unexpected keyword (expected MAP)",
    // "... has %d tags in the maplist (expected 1)".
    !rule.tests.is_empty()
        || rule.wordform.is_some()
        || rule.r#type != Keywords::KMap
        || from_tags != 1
}

// [spec:cg3:def:relabeller.cg3.relabeller]
/// C++ `class Relabeller`. Owns pointers to the target `grammar` (mutated) and the
/// read-only `relabels` grammar, plus the two partitioned relabel-rule maps.
///
/// The C++ error-stream pointer has no field analogue:
/// diagnostics are tracing events (wave 4). The two grammars are held as
/// `&mut`/`&` borrows for the lifetime of the relabeller (the C++ raw pointers).
pub struct Relabeller<'g, 'r> {
    /// C++ `Grammar* grammar` — the target grammar (mutated).
    grammar: &'g mut GrammarCore,
    /// C++ `const Grammar* relabels` — the relabel-rules grammar (read-only).
    relabels: &'r GrammarCore,
    /// C++ `std::unique_ptr<const StringSetMap> relabel_as_list`.
    relabel_as_list: StringSetMap,
    /// C++ `std::unique_ptr<const StringSetMap> relabel_as_set`.
    relabel_as_set: StringSetMap,
}

impl<'g, 'r> Relabeller<'g, 'r> {
    // [spec:cg3:def:relabeller.cg3.relabeller.relabeller-fn+1]
    // [spec:cg3:sem:relabeller.cg3.relabeller.relabeller-fn+1]
    // [spec:cg3:req:robustness.cli-arguments]
    /// Constructor. Stores the target/relabels grammars and partitions the relabel
    /// rules into `relabel_as_list` (target set has literal tags in its main trie)
    /// and `relabel_as_set` (target set has none). Each rule is guarded (special
    /// tags, context tests, wordform, non-MAP keyword, or maplist ≠ 1 tag → the
    /// rule is skipped); guard diagnostics are deferred I/O. `emplace` on an
    /// unordered_map does not overwrite → a duplicate fromTag string keeps its
    /// FIRST target.
    ///
    /// DIVERGENCE: the C++ reads `rule->maplist->trie` before any guard, so a
    /// rule with no maplist — `SELECT`, `REMOVE`, any keyword that takes no tag
    /// list — dereferences null. It is refused here instead.
    pub fn new(
        res: &'g mut GrammarCore,
        relabels: &'r GrammarCore,
        _ux_err: (),
    ) -> Result<Self, RelabelRuleError> {
        let mut as_list: StringSetMap = StringSetMap::new();
        let mut as_set: StringSetMap = StringSetMap::new();

        // for (auto rule : relabels.rule_by_number)
        let rule_ids: Vec<u32> = (0..relabels.rule_by_number.capacity())
            .filter(|&i| relabels.rule_by_number.try_get(i).is_some())
            .collect();
        for rid in rule_ids {
            let rule = &relabels.rule_by_number[rid];
            // fromTags = trie_getTagList(rule->maplist->trie)
            let Some(maplist) = rule.maplist else {
                return Err(RelabelRuleError::for_rule(rule));
            };
            let from_trie = relabels.sets_list[maplist.0].trie.clone();
            let from_tags = trie_get_tag_list(&from_trie, relabels);
            // target = relabels.sets_list[rule->target] — rule->target is a NUMBER.
            let target = relabels.set_id_by_number(rule.target);
            let to_trie = relabels.sets_list[target.0].trie.clone();
            let to_tags = trie_get_tag_list(&to_trie, relabels);

            if is_skipped(relabels, rule, maplist, target, from_tags.len()) {
                continue;
            }

            let from_tag = from_tags[0];
            for toit in &to_tags {
                if relabels.single_tags_list[toit.0]
                    .r#type
                    .intersects(T_SPECIAL)
                {
                    // "Warning: Special tags (%S) not supported yet.\n" — warning
                    // only; the rule is still recorded. Diagnostic deferred.
                }
            }

            let from_tag_str = relabels.single_tags_list[from_tag.0].tag.to_string();
            if !to_tags.is_empty() {
                // as_list->emplace(fromTag->tag.data(), target) — first wins.
                as_list.entry(from_tag_str).or_insert(target);
            } else {
                as_set.entry(from_tag_str).or_insert(target);
            }
        }

        Ok(Relabeller {
            grammar: res,
            relabels,
            relabel_as_list: as_list,
            relabel_as_set: as_set,
        })
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.transfer-tags-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.transfer-tags-fn]
    /// Re-interns a list of tags into the target grammar and returns the
    /// canonical target [`TagId`]s in input order. For each tag: hand its
    /// deep-copied value to [`TagSpace::add_tag`] (which inserts it or returns the
    /// existing canonical tag), and push the result.
    ///
    /// ARENA NOTE: the C++ takes `TagVector` (`Tag*`s) whose elements may belong
    /// to EITHER grammar (`relabelAsList` mixes set_g tags from the target with
    /// suffix tags from the relabels grammar) — a raw pointer carries its own
    /// data, an arena `TagId` does not. So the port takes the tags BY VALUE
    /// (`Vec<Tag>`), each already cloned out of its owning arena by the caller —
    /// the exact analog of C++ `new Tag(*tag_r)`.
    fn transfer_tags(&mut self, tv_r: Vec<crate::tag::Tag>) -> RelabellerTagVector {
        let mut tv_g: RelabellerTagVector = RelabellerTagVector::new();
        for tagcopy in tv_r {
            // Tag* tag_g = new Tag(*tag_r); tv_g.push_back(grammar->addTag(tag_g));
            let tag_g = self.grammar.add_tag(tagcopy);
            tv_g.push(tag_g);
        }
        tv_g
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.add-taglists-to-set-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.add-taglists-to-set-fn]
    /// Populates set `s`'s tries from a collection of tag lists `tvs`, mirroring
    /// `TextualParser::parseTagList`. First pass: normalize each list (sort by
    /// `compare_Tag` = ascending `Tag::hash`, drop adjacent duplicates) and count
    /// tag frequencies, but only for newly-inserted normalized lists (no
    /// double-count). Second pass: single-tag lists route via
    /// [`Grammar::add_tag_to_set`]; longer lists are sorted highest-frequency-first
    /// (cheap trie-prefix compression) and inserted whole into `trie_special` (if
    /// any tag is `T_SPECIAL`) else `trie`.
    ///
    /// PORT NOTE: `tvs`/`tvs_sort_uniq` are `TagVectorSet` (`BTreeSet<TagVector>`);
    /// the C++ `const_cast`ed and mutated the vectors held INSIDE the set in place.
    /// A `BTreeSet` cannot be mutated in place (its ordering invariant), so the
    /// port collects, normalizes, and re-inserts — observationally identical for
    /// the values processed. The C++ `tvs` in-place mutation of the CALLER's set
    /// is not reproduced (the callers discard `tvs` immediately after).
    fn add_taglists_to_set(&mut self, tvs: &TagVectorSet, s: SetId) {
        if tvs.is_empty() {
            return;
        }

        let mut tag_freq: HashMap<TagId, usize> = HashMap::new();
        let mut tvs_sort_uniq: TagVectorSet = TagVectorSet::new();

        for tvc in tvs {
            // TagVector& tags = const_cast(tvc); sort by compare_Tag (hash asc),
            // erase(unique(...)) adjacent duplicates.
            let mut tags: TagVector = tvc.clone();
            tags.sort_by(|&a, &b| {
                let ha = self.grammar.single_tags_list[a.0].hash;
                let hb = self.grammar.single_tags_list[b.0].hash;
                ha.cmp(&hb)
            });
            tags.dedup();
            // if (tvs_sort_uniq.insert(tags).second) { ++tag_freq[t] for t in tags }
            if tvs_sort_uniq.insert(tags.clone()) {
                for &t in &tags {
                    *tag_freq.entry(t).or_insert(0) += 1;
                }
            }
        }

        let fs = FreqSorter::new(&tag_freq);
        // Iterate the normalized/unique lists (BTreeSet order == std::set order
        // via compare_TagVector-equivalent hash ordering is NOT guaranteed here;
        // see the tag.rs TagVectorSet reconciliation note — order affects only
        // the cheap trie-compression heuristic, not correctness).
        let normalized: Vec<TagVector> = tvs_sort_uniq.iter().cloned().collect();
        for tvc in normalized {
            if tvc.is_empty() {
                continue;
            }
            if tvc.len() == 1 {
                // grammar->addTagToSet(tvc[0], s)
                self.grammar.add_tag_to_set(tvc[0], s);
                continue;
            }
            // TagVector& tv = const_cast(tvc); sort by frequency, high-to-low.
            let mut tv: TagVector = tvc;
            tv.sort_by(|&a, &b| fs.cmp(a, b));
            let mut special = false;
            for &tag in &tv {
                if self.grammar.single_tags_list[tag.0]
                    .r#type
                    .intersects(T_SPECIAL)
                {
                    special = true;
                    break;
                }
            }
            if special {
                let dst = &mut self.grammar.sets_list.get_mut(s.0).trie_special;
                trie_insert(dst, &tv, 0);
            } else {
                let dst = &mut self.grammar.sets_list.get_mut(s.0).trie;
                trie_insert(dst, &tv, 0);
            }
        }
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.relabel-as-list-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.relabel-as-list-fn]
    /// Rewrites `set_g` in place: every tag list containing `fromTag` (matched by
    /// HASH, not pointer) is replaced by the cartesian expansion of (that list
    /// minus `fromTag`) × each tag list of the relabel set `set_r`; lists lacking
    /// `fromTag` pass through (re-interned) unchanged. Then rebuilds the set's
    /// tries via [`Self::add_taglists_to_set`]. `set_g->trie_special`, `ff_tags`,
    /// and `sets` are untouched.
    fn relabel_as_list(&mut self, set_g: SetId, set_r: SetId, from_tag: TagId) {
        // old_tvs = trie_getTagsOrdered(set_g->trie)
        let set_g_trie = self.grammar.sets_list[set_g.0].trie.clone();
        let old_tvs = trie_get_tags_ordered(&set_g_trie, self.grammar);
        // trie_delete(set_g->trie); set_g->trie.clear();
        {
            let t = &mut self.grammar.sets_list.get_mut(set_g.0).trie;
            trie_delete(t);
            t.clear();
        }

        let from_hash = self.grammar.single_tags_list[from_tag.0].hash;

        let mut taglists: TagVectorSet = TagVectorSet::new();
        for old_tags in &old_tvs {
            let mut tags_except_from: TagVector = TagVector::new();
            let mut seen = false;
            for &old_tag in old_tags {
                if self.grammar.single_tags_list[old_tag.0].hash == from_hash {
                    seen = true;
                } else {
                    tags_except_from.push(old_tag);
                }
            }
            // suffixes: relabel target's ordered paths if seen, else one empty vec.
            let suffixes: Vec<TagVector> = if seen {
                // trie_getTagsOrdered(set_r->trie) — set_r is in the RELABELS grammar.
                let set_r_trie = self.relabels.sets_list[set_r.0].trie.clone();
                trie_get_tags_ordered(&set_r_trie, self.relabels)
                    .into_iter()
                    .collect()
            } else {
                vec![TagVector::new()]
            };
            for suf in &suffixes {
                // tags = tags_except_from ++ suf; transferTags; insert.
                // Clone each tag VALUE out of its owning arena (tags_except_from
                // came from set_g → target grammar; suf came from set_r →
                // RELABELS grammar) — see the transfer_tags ARENA NOTE.
                let mut tags: Vec<crate::tag::Tag> = tags_except_from
                    .iter()
                    .map(|&t| self.grammar.single_tags_list[t.0].clone())
                    .collect();
                tags.extend(
                    suf.iter()
                        .map(|&t| self.relabels.single_tags_list[t.0].clone()),
                );
                let tags = self.transfer_tags(tags);
                taglists.insert(tags);
            }
        }
        self.add_taglists_to_set(&taglists, set_g);
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.reindex-set-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.reindex-set-fn]
    /// Recomputes `s`'s derived type flags, recursing into child sets. Clears
    /// `ST_SPECIAL`/`ST_CHILD_UNIFY`, ORs in `trie_reindex` of both tries, then for
    /// each child NUMBER `i` in `s.sets` fetches `grammar->sets_list[i]`
    /// (== `SetId(i)`), recurses, and propagates SPECIAL/unify/MAPPING upward.
    /// Finally, if `s` itself has any unify bit, sets both `ST_SPECIAL` and
    /// `ST_CHILD_UNIFY`. Does NOT clear the tag/set-unify/mapping bits first, so
    /// pre-existing bits are preserved.
    ///
    /// ARENA NOTE: distinct from [`crate::set::Set::reindex`] — children are resolved by set
    /// NUMBER (`sets_list[i]`), not by content hash (`sets_by_contents`).
    ///
    /// The C++ recurses into the child sets; this is the same walk as
    /// [`crate::set::Set::reindex`], kept on a heap stack.
    // [spec:cg3:req:robustness.depth-bounded]
    fn reindex_set(&mut self, s: SetId) {
        // Set* set = grammar->sets_list[i]; — i is a set NUMBER.
        Set::reindex_members(self.grammar, s, |grammar, i| {
            grammar.set_id_by_number(SetNumber(i))
        });
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.add-set-to-grammar-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.add-set-to-grammar-fn]
    /// Registers the already-allocated set `s` into the target grammar and
    /// finalizes it. C++: `setName(sets_list.size()+100)` (size BEFORE the push),
    /// `push_back`, `number = sets_list.size()-1` (== the pre-push size),
    /// `reindexSet`. The port pushes onto `sets_list_order` (the numbered vector;
    /// the arena slot was taken at `allocate_set` time).
    fn add_set_to_grammar(&mut self, s: SetId) {
        // s->setName(UI32(grammar->sets_list.size() + 100))
        let name_arg = (self.grammar.sets_list_order.len() as u32).wrapping_add(100);
        let grammar = &mut self.grammar;
        grammar
            .sets_list
            .get_mut(s.0)
            .set_name(name_arg, &mut grammar.rand_state);
        // grammar->sets_list.push_back(s); s->number = UI32(sets_list.size()-1);
        self.grammar.sets_list_order.push(s);
        let num = (self.grammar.sets_list_order.len() - 1) as u32;
        self.grammar.sets_list.get_mut(s.0).number = SetNumber(num);
        self.reindex_set(s);
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    /// Deep-copies the RELABELS-grammar set `s_r` into the target grammar and
    /// returns the new set's number. Recurses into child sets FIRST (so every
    /// referenced sub-set exists before finalizing), copies set operators verbatim,
    /// copies the tries WITH tag transfer via the two-arg [`trie_copy`], copies
    /// `ff_tags` by value (raw source ids — no re-intern, flagged quirk), then
    /// `addSetToGrammar`.
    ///
    /// The C++ recurses into the child sets; this keeps the sets still being
    /// copied on a heap stack. Each copy is allocated when it is begun and
    /// added to the grammar once its children are, the order the recursion
    /// allocates and adds them in.
    // [spec:cg3:req:robustness.depth-bounded]
    fn copy_relabel_set_to_grammar(&mut self, s_r: SetId) -> u32 {
        let mut open = vec![self.copy_relabel_begin(s_r)];
        let mut number = None;
        while number.is_none() {
            number = self.copy_relabel_step(&mut open);
        }
        number.unwrap_or_default()
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    /// One step of [`Self::copy_relabel_set_to_grammar`]: begin copying the
    /// next child of the innermost set being copied, or, once it has none
    /// left, finish that set and hand its number to its parent. Returns the
    /// number of the outermost set once it is finished.
    fn copy_relabel_step(&mut self, open: &mut Vec<RelabelCopy>) -> Option<u32> {
        let top = open.last_mut()?;
        if let Some(&child_num_r) = top.children_r.get(top.sets_g.len()) {
            // relabels->sets_list[child_num_r] — child_num_r is a set NUMBER.
            let child_r = self.relabels.set_id_by_number(SetNumber(child_num_r));
            open.push(self.copy_relabel_begin(child_r));
            return None;
        }
        let done = open.pop()?;
        let number = self.copy_relabel_finish(done);
        match open.last_mut() {
            Some(parent) => {
                parent.sets_g.push(number);
                None
            }
            None => Some(number),
        }
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    /// Begin copying `s_r`: allocate its copy in the target grammar.
    fn copy_relabel_begin(&mut self, s_r: SetId) -> RelabelCopy {
        // s_g = grammar->allocateSet()
        let s_g = self.grammar.allocate_set();
        RelabelCopy {
            s_r,
            s_g,
            children_r: self.relabels.sets_list[s_r.0].sets.clone(),
            sets_g: Vec::new(),
        }
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
    /// Finish a copy whose children are copied: fill in everything else and
    /// add it to the grammar, returning its number.
    fn copy_relabel_finish(&mut self, copy: RelabelCopy) -> u32 {
        let RelabelCopy {
            s_r, s_g, sets_g, ..
        } = copy;
        self.grammar.sets_list.get_mut(s_g.0).sets = sets_g;

        // Copy set operators verbatim (same enum values across grammars).
        let set_ops = self.relabels.sets_list[s_r.0].set_ops.clone();
        self.grammar.sets_list.get_mut(s_g.0).set_ops = set_ops;

        // Copy the tries WITH tag transfer (two-arg trie_copy).
        let src_trie = self.relabels.sets_list[s_r.0].trie.clone();
        let new_trie = trie_copy(&src_trie, self.grammar);
        let src_trie_sp = self.relabels.sets_list[s_r.0].trie_special.clone();
        let new_trie_sp = trie_copy(&src_trie_sp, self.grammar);
        {
            let node = self.grammar.sets_list.get_mut(s_g.0);
            node.trie = new_trie;
            node.trie_special = new_trie_sp;
        }

        // s_g->ff_tags = s_r->ff_tags — raw source TagIds copied WITHOUT
        // re-interning into the target grammar (flagged quirk; C++ had a
        // "// TODO: does this get copied correctly?" comment).
        let ff = self.relabels.sets_list[s_r.0].ff_tags.clone();
        self.grammar.sets_list.get_mut(s_g.0).ff_tags = ff;

        self.add_set_to_grammar(s_g);
        self.grammar.sets_list[s_g.0].number.get()
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.relabel-as-set-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.relabel-as-set-fn]
    /// Rewrites `set_g` so occurrences of `fromTag` become a reference to a copy of
    /// the whole relabel set `set_r`, preserving lists that lacked `fromTag`.
    /// Reshapes `set_g` into `s_gN OR s_gI` where `s_gN` holds the no-fromTag
    /// lists (plus set_g's own trie_special/ff_tags/sets) and `s_gI` is the copied
    /// relabel set intersected (`S_PLUS`) with the fromTag-bearing lists. See the
    /// spec sem for the full 8-step sequence.
    fn relabel_as_set(&mut self, set_g: SetId, set_r: SetId, from_tag: TagId) {
        // (1) If set_g->trie empty, return (only a composition of other sets).
        if self.grammar.sets_list[set_g.0].trie.is_empty() {
            return;
        }
        // (2) Warn if set_g also has child sets (execution continues).
        if !self.grammar.sets_list[set_g.0].sets.is_empty() {
            // "Warning: SET %d has both trie and sets, this was unexpected."
            // with set_g->number — diagnostic deferred.
        }

        // (3) Snapshot + wipe the main trie.
        let set_g_trie = self.grammar.sets_list[set_g.0].trie.clone();
        let old_tvs = trie_get_tags_ordered(&set_g_trie, self.grammar);
        {
            let t = &mut self.grammar.sets_list.get_mut(set_g.0).trie;
            trie_delete(t);
            t.clear();
        }

        let from_hash = self.grammar.single_tags_list[from_tag.0].hash;

        // (4) Partition the paths (matched by hash), re-interning the survivors.
        let mut tvs_with_from: TagVectorSet = TagVectorSet::new();
        let mut tvs_no_from: TagVectorSet = TagVectorSet::new();
        for old_tags in &old_tvs {
            let mut tags_except_from: TagVector = TagVector::new();
            let mut seen = false;
            for &old_tag in old_tags {
                if self.grammar.single_tags_list[old_tag.0].hash == from_hash {
                    seen = true;
                } else {
                    tags_except_from.push(old_tag);
                }
            }
            if tags_except_from.is_empty() {
                continue;
            }
            // set_g's tags live in the TARGET grammar — clone values from there
            // (see the transfer_tags ARENA NOTE).
            let tags_values: Vec<crate::tag::Tag> = tags_except_from
                .iter()
                .map(|&t| self.grammar.single_tags_list[t.0].clone())
                .collect();
            let transferred = self.transfer_tags(tags_values);
            if seen {
                tvs_with_from.insert(transferred);
            } else {
                tvs_no_from.insert(transferred);
            }
        }

        // (5) Build s_gN (the "no fromTag" set).
        let s_gn = self.grammar.allocate_set();
        self.add_taglists_to_set(&tvs_no_from, s_gn);
        // s_gN->trie_special = trie_copy(set_g->trie_special) — ONE-arg TagTrie
        // copy (no tag re-interning; keeps set_g's own target-grammar tags).
        let set_g_trie_sp = self.grammar.sets_list[set_g.0].trie_special.clone();
        let copied_sp = crate::tag_trie::trie_copy(&set_g_trie_sp);
        // s_gN->ff_tags = set_g->ff_tags; sets = set_g->sets; set_ops = set_g->set_ops.
        let (ff, sets, set_ops) = {
            let sg = &self.grammar.sets_list[set_g.0];
            (sg.ff_tags.clone(), sg.sets.clone(), sg.set_ops.clone())
        };
        {
            let node = self.grammar.sets_list.get_mut(s_gn.0);
            node.trie_special = copied_sp;
            node.ff_tags = ff;
            node.sets = sets;
            node.set_ops = set_ops;
        }
        self.add_set_to_grammar(s_gn);

        // (6) Copy the relabel set.
        let s_gr_num = self.copy_relabel_set_to_grammar(set_r);

        // (7) Determine s_gI.
        let s_gi_num: u32;
        if tvs_with_from.is_empty() {
            // avoid intersecting with ∅ (never matches).
            s_gi_num = s_gr_num;
        } else {
            let s_gw = self.grammar.allocate_set();
            self.add_taglists_to_set(&tvs_with_from, s_gw);
            self.add_set_to_grammar(s_gw);
            // if (s_gW->getNonEmpty().empty()) warn — getNonEmpty == trie if
            // non-empty else trie_special; ".empty()" here means BOTH empty.
            let s_gw_non_empty_empty = {
                let sw = &self.grammar.sets_list[s_gw.0];
                if !sw.trie.is_empty() {
                    sw.trie.is_empty()
                } else {
                    sw.trie_special.is_empty()
                }
            };
            if s_gw_non_empty_empty {
                // "Warning: unexpected empty tries when relabelling set %d!\n"
                // with set_g->number — diagnostic deferred.
            }

            // s_gI->sets = { s_gR_num, s_gW->number }; set_ops = { S_PLUS }.
            let s_gi = self.grammar.allocate_set();
            let s_gw_number = self.grammar.sets_list[s_gw.0].number.get();
            {
                let node = self.grammar.sets_list.get_mut(s_gi.0);
                node.sets = vec![s_gr_num, s_gw_number];
                node.set_ops = vec![S_PLUS];
            }
            self.add_set_to_grammar(s_gi);
            s_gi_num = self.grammar.sets_list[s_gi.0].number.get();
        }

        // (8) Reshape set_g into an OR: { s_gN->number, s_gI_num }, ops { S_OR }.
        let s_gn_number = self.grammar.sets_list[s_gn.0].number.get();
        {
            let node = self.grammar.sets_list.get_mut(set_g.0);
            node.sets = vec![s_gn_number, s_gi_num];
            node.set_ops = vec![S_OR];
        }
        // set_g was already in sets_list → only reindexed, not re-added.
        self.reindex_set(set_g);
    }

    // [spec:cg3:def:relabeller.cg3.relabeller.relabel-fn]
    // [spec:cg3:sem:relabeller.cg3.relabeller.relabel-fn]
    /// Top-level driver. Builds `tag_by_str` (tag string → target-grammar TagId,
    /// last-wins) and `sets_by_tag` (tag string → set of target sets whose MAIN
    /// trie mentions it), applies RELABEL AS LIST then RELABEL AS SET for every
    /// matching set, then finalizes: clears the grammar's own `sets_by_tag` index,
    /// `reindex()`es, and sets `num_tags = single_tags_list.size()`.
    ///
    /// The finalizing reindex is the one step here that can fail, and it used to
    /// re-raise as a process exit from inside the relabeller. It travels back to
    /// the caller instead — `[dec:cg3:results-not-unwinding]`.
    pub fn relabel(&mut self) -> Result<(), crate::error::Cg3Error> {
        // (1) tag_by_str: iterate single_tags_list (arena, insertion order),
        // last-wins per tag string.
        let mut tag_by_str: HashMap<String, TagId> = HashMap::new();
        let tag_ids: Vec<TagId> = (0..self.grammar.single_tags_list.capacity())
            .filter(|&i| self.grammar.single_tags_list.try_get(i).is_some())
            .map(TagId)
            .collect();
        for tid in &tag_ids {
            let s = self.grammar.single_tags_list[tid.0].tag.to_string();
            tag_by_str.insert(s, *tid);
        }

        // (2) sets_by_tag: for every set in sets_list, index its MAIN-trie tags.
        // Iterate the C++ sets_list vector (the numbered order).
        let mut sets_by_tag: HashMap<String, HashSet<SetId>> = HashMap::new();
        let set_ids: Vec<SetId> = self.grammar.sets_list_order.clone();
        for sid in &set_ids {
            let trie = self.grammar.sets_list[sid.0].trie.clone();
            let to_tags = trie_get_tag_list(&trie, self.grammar);
            for toit in to_tags {
                let ts = self.grammar.single_tags_list[toit.0].tag.to_string();
                sets_by_tag.entry(ts).or_default().insert(*sid);
            }
        }

        // (3) RELABEL AS LIST.
        let as_list: Vec<(String, SetId)> = self
            .relabel_as_list
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect();
        for (from_str, target) in as_list {
            // set_r = relabels->sets_list[it.second->number] — target's number in
            // the RELABELS grammar, resolved through its sets_list_order.
            let target_number = self.relabels.sets_list[target.0].number;
            let set_r = self.relabels.set_id_by_number(target_number);
            // fromTag = tag_by_str[it.first] — operator[] default-inserts a null
            // Tag* if absent, but the sets_by_tag.find miss below skips use.
            let from_tag = tag_by_str.get(&from_str).copied();
            if let Some(sets_g) = sets_by_tag.get(&from_str) {
                let sets_g: Vec<SetId> = sets_g.iter().copied().collect();
                for set_g in sets_g {
                    #[expect(
                        clippy::unwrap_used,
                        reason = "sets_by_tag is keyed by the text of tags in the grammar's sets, and tag_by_str, built above, holds the text of every tag in the grammar"
                    )]
                    self.relabel_as_list(set_g, set_r, from_tag.unwrap());
                }
            }
        }

        // (4) RELABEL AS SET.
        let as_set: Vec<(String, SetId)> = self
            .relabel_as_set
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect();
        for (from_str, target) in as_set {
            let target_number = self.relabels.sets_list[target.0].number;
            let set_r = self.relabels.set_id_by_number(target_number);
            let from_tag = tag_by_str.get(&from_str).copied();
            if let Some(sets_g) = sets_by_tag.get(&from_str) {
                let sets_g: Vec<SetId> = sets_g.iter().copied().collect();
                for set_g in sets_g {
                    #[expect(
                        clippy::unwrap_used,
                        reason = "sets_by_tag is keyed by the text of tags in the grammar's sets, and tag_by_str, built above, holds the text of every tag in the grammar"
                    )]
                    self.relabel_as_set(set_g, set_r, from_tag.unwrap());
                }
            }
        }

        // (5) Finalize. `single_tags_list.size()` == the count of live arena
        // slots; tags are never freed during relabelling, so `capacity()` (the
        // grammar's own size analog, see its reindex) equals that count.
        self.grammar.sets_by_tag.clear();
        let _ = self.grammar.reindex(false, false)?;
        self.grammar.num_tags = self.grammar.single_tags_list.capacity() as usize;
        Ok(())
    }
}
