# src/Relabeller.cpp, src/Relabeller.hpp

> [spec:cg3:def:relabeller.cg3.freq-sorter]
> struct freq_sorter {
>   const bc::flat_map<Tag*, size_t>& tag_freq;
> }

> [spec:cg3:def:relabeller.cg3.freq-sorter.freq-sorter-fn]
> freq_sorter(const bc::flat_map<Tag*, size_t>& tag_freq)

> [spec:cg3:sem:relabeller.cg3.freq-sorter.freq-sorter-fn]
> Constructor for the `freq_sorter` comparator functor. Stores the passed
> `const bc::flat_map<Tag*, size_t>& tag_freq` by reference in the
> functor's `tag_freq` member (no copy). The caller must keep the referred
> map alive for the functor's lifetime.

> [spec:cg3:def:relabeller.cg3.freq-sorter.operator-fn]
> bool operator()(Tag* a, Tag* b) const

> [spec:cg3:sem:relabeller.cg3.freq-sorter.operator-fn]
> Comparator that sorts tags by descending frequency. Returns
> `tag_freq.find(a)->second > tag_freq.find(b)->second`, i.e. `a` orders
> before `b` when `a`'s recorded frequency is strictly greater than `b`'s.
> It calls `.find()` on the stored `tag_freq` map and dereferences the
> result WITHOUT checking against `end()`, so it assumes both `a` and `b`
> are present as keys; a missing key would be undefined behaviour (in
> practice every tag passed here was counted into `tag_freq` first). `const`
> and side effect free. Ties (equal frequency) compare as not-less, giving
> an unspecified but stable-enough relative order for the caller's cheap
> trie-compression heuristic.

> [spec:cg3:def:relabeller.cg3.relabeller]
> class Relabeller {
>   std::ostream* ux_stderr = nullptr;
>   Grammar* grammar = nullptr;
>   const Grammar* relabels = nullptr;
>   std::unique_ptr<const UStringSetMap> relabel_as_list;
>   std::unique_ptr<const UStringSetMap> relabel_as_set;
> }

> [spec:cg3:def:relabeller.cg3.relabeller.add-set-to-grammar-fn]
> void Relabeller::addSetToGrammar(Set* s)

> [spec:cg3:sem:relabeller.cg3.relabeller.add-set-to-grammar-fn]
> Registers an already-allocated set `s` into the target grammar's
> `sets_list` and finalizes it. Steps: (1) `s->setName(UI32(grammar->
> sets_list.size() + 100))` — gives it a generated name of the form
> `_G_<s.line>_<sets_list.size()+100>_` (the +100 keeps these numbers clear
> of low reserved set numbers; if the argument were 0 setName would use a
> random number, but size+100 is never 0). (2) `grammar->sets_list.
> push_back(s)`. (3) `s->number = UI32(grammar->sets_list.size() - 1)` —
> the set's own index in `sets_list` after the push. (4) `reindexSet(*s)`
> to recompute its type flags (and recursively those of its child sets).
> No return value.

> [spec:cg3:def:relabeller.cg3.relabeller.add-taglists-to-set-fn]
> void Relabeller::addTaglistsToSet(const TagVectorSet& tvs, Set* s)

> [spec:cg3:sem:relabeller.cg3.relabeller.add-taglists-to-set-fn]
> Populates set `s`'s tries from a collection of tag lists `tvs` (a
> `TagVectorSet` = `std::set<TagVector, compare_TagVector>`), mirroring
> logic extracted from TextualParser::parseTagList. If `tvs` is empty,
> returns immediately (no-op).
> First pass — normalize and count frequencies: create an empty
> `bc::flat_map<Tag*, size_t> tag_freq` and an empty `TagVectorSet
> tvs_sort_uniq`. For every tag list `tvc` in `tvs`: `const_cast` it to a
> mutable `TagVector& tags`, sort it in place with `compare_Tag` (by
> Tag::hash ascending), then `erase(unique(...))` to drop adjacent
> duplicate tag pointers. Insert `tags` into `tvs_sort_uniq`; only if the
> insert was newly added (`.second == true`), increment `++tag_freq[t]`
> for each tag `t` in `tags` (so a tag list that duplicates an
> already-inserted normalized list does not double-count). Note this
> mutates the tag vectors held inside the caller's `tvs` set in place.
> Second pass — build tries: construct `freq_sorter fs(tag_freq)`. For
> each normalized list `tvc` in `tvs_sort_uniq`: if empty, skip; if it has
> exactly one tag, call `grammar->addTagToSet(tvc[0], s)` (which routes the
> single tag into `s->trie` or `s->trie_special`/`ff_tags`/ST_ANY per the
> tag's type flags) and continue. Otherwise `const_cast` to mutable
> `TagVector& tv` and sort it by `fs` (highest frequency first — a cheap
> imperfect trie-prefix compression). Then scan `tv`: set `special = true`
> if any tag has the `T_SPECIAL` type bit. If `special`, `trie_insert(s->
> trie_special, tv)`; else `trie_insert(s->trie, tv)`. (`trie_insert`
> stores the whole `tv` as one root-to-terminal path.) No return value.

> [spec:cg3:def:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
> uint32_t Relabeller::copyRelabelSetToGrammar(const Set* s_r)

> [spec:cg3:sem:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn]
> Deep-copies a set `s_r` from the relabels grammar into the target
> grammar and returns the new set's number. Steps: (1) `s_g = grammar->
> allocateSet()` (allocates a bare Set tracked in the grammar's sets_all,
> not yet in sets_list). (2) Copy child-set references, recursing first:
> `nsets = s_r->sets.size()`; resize `s_g->sets` to `nsets`; for each i,
> take `child_num_r = s_r->sets[i]`, recursively `copyRelabelSetToGrammar(
> relabels->sets_list[child_num_r])` to obtain `child_num_g`, and set
> `s_g->sets[i] = child_num_g`. This ensures every referenced sub-set
> exists in the target grammar before this set is finalized. (3) Copy set
> operators verbatim: resize `s_g->set_ops` to `s_r->set_ops.size()` and
> copy each element (the S_* op enum values are the same across grammars).
> (4) Copy the tries WITH tag transfer: `s_g->trie = trie_copy(s_r->trie,
> *grammar)` and `s_g->trie_special = trie_copy(s_r->trie_special,
> *grammar)` — the two-argument Relabeller::trie_copy that re-interns each
> tag into the target grammar via addTag. (5) `s_g->ff_tags = s_r->ff_tags`
> — copies the fail-fast TagSortedVector by value, i.e. copies the raw
> Tag* pointers straight from the relabels grammar WITHOUT transferring
> them into the target grammar (the source even carries a `// TODO: does
> this get copied correctly?` comment; see report). (6) `addSetToGrammar(
> s_g)` to append it to sets_list, name it, assign its number, and reindex.
> Returns `s_g->number`.

> [spec:cg3:def:relabeller.cg3.relabeller.reindex-set-fn]
> void Relabeller::reindexSet(Set& s)

> [spec:cg3:sem:relabeller.cg3.relabeller.reindex-set-fn]
> Recomputes a set's derived type flags, recursing into child sets. Steps:
> (1) Clear the `ST_SPECIAL` and `ST_CHILD_UNIFY` bits of `s.type`
> (`s.type &= ~ST_SPECIAL; s.type &= ~ST_CHILD_UNIFY;`). (2) OR in the
> result of `trie_reindex(s.trie)` and `trie_reindex(s.trie_special)` —
> `trie_reindex` returns ST_SPECIAL if any tag in the trie is T_SPECIAL and
> ST_MAPPING if any is T_MAPPING (recursively). (3) For each child set
> number `i` in `s.sets`: fetch `set = grammar->sets_list[i]`, recursively
> `reindexSet(*set)`, then propagate upward — if the child now has
> ST_SPECIAL, set `s`'s ST_SPECIAL; if the child has any unify bit
> (ST_TAG_UNIFY | ST_SET_UNIFY | ST_CHILD_UNIFY), set `s`'s ST_CHILD_UNIFY;
> if the child has ST_MAPPING, set `s`'s ST_MAPPING. (4) Finally, if `s`
> itself now has any unify bit set (ST_TAG_UNIFY | ST_SET_UNIFY |
> ST_CHILD_UNIFY), additionally set both ST_SPECIAL and ST_CHILD_UNIFY on
> `s`. Mutates only the `type` fields; no return value. Note it does NOT
> clear ST_TAG_UNIFY/ST_SET_UNIFY/ST_MAPPING first, so those pre-existing
> bits are preserved.

> [spec:cg3:def:relabeller.cg3.relabeller.relabel-as-list-fn]
> void Relabeller::relabelAsList(Set* set_g, const Set* set_r, const Tag* fromTag)

> [spec:cg3:sem:relabeller.cg3.relabeller.relabel-as-list-fn]
> Rewrites grammar set `set_g` in place, replacing every tag list that
> contains `fromTag` with the cartesian expansion of (that list minus
> `fromTag`) times each tag list of the relabel set `set_r`. Steps:
> (1) `old_tvs = trie_getTagsOrdered(set_g->trie)` — snapshot every
> root-to-terminal tag path currently in the set's main trie (each as a
> TagVector, in trie order). (2) `trie_delete(set_g->trie)` then
> `set_g->trie.clear()` — wipe the main trie completely. (3) Build a new
> `TagVectorSet taglists`. For each path `old_tags` in `old_tvs`: split it
> into `tags_except_from` (every tag whose `hash != fromTag->hash`) and a
> boolean `seen` (true if any tag's `hash == fromTag->hash`) — matching is
> by hash, not pointer identity. Determine the `suffixes` to append: if
> `seen`, `suffixes = trie_getTagsOrdered(set_r->trie)` (all of the relabel
> target's tag paths); otherwise `suffixes` is a set containing a single
> empty TagVector (so the list passes through unchanged). For each `suf` in
> `suffixes`: form `tags = tags_except_from` followed by all of `suf`, run
> it through `transferTags(tags)` (re-interning each tag into the target
> grammar), and `taglists.insert(tags)` (dedup via compare_TagVector).
> (4) `addTaglistsToSet(taglists, set_g)` to rebuild the set's tries from
> the expanded lists. Net effect: lists lacking `fromTag` are kept
> (re-interned) unchanged; each list containing `fromTag` is replaced by
> one new list per relabel-target list, with `fromTag` removed and the
> target's tags appended. `set_g->trie_special`, `set_g->ff_tags` and
> `set_g->sets` are untouched. No return value.

> [spec:cg3:def:relabeller.cg3.relabeller.relabel-as-set-fn]
> void Relabeller::relabelAsSet(Set* set_g, const Set* set_r, const Tag* fromTag)

> [spec:cg3:sem:relabeller.cg3.relabeller.relabel-as-set-fn]
> Rewrites grammar set `set_g` so that occurrences of `fromTag` are
> replaced by a reference to a copy of the whole relabel set `set_r`, while
> preserving lists that did not contain `fromTag`. Steps:
> (1) If `set_g->trie` is empty, return immediately — the set is only a
> +/OR/- composition of other sets, and those other sets get relabelled via
> their own entries. (2) If `set_g->sets` is non-empty, emit a warning to
> `ux_stderr`: `"Warning: SET %d has both trie and sets, this was
> unexpected."` with `set_g->number` (execution continues). (3)
> `old_tvs = trie_getTagsOrdered(set_g->trie)`, then `trie_delete(set_g->
> trie)` and `set_g->trie.clear()`.
> (4) Partition the paths. For each `old_tags` in `old_tvs`, build
> `tags_except_from` (tags with `hash != fromTag->hash`) and `seen`
> (any tag `hash == fromTag->hash`). If `tags_except_from` is empty, skip
> this path (a bare `fromTag` list contributes nothing here). Else if
> `seen`, `tvs_with_from.insert(transferTags(tags_except_from))`; else
> `tvs_no_from.insert(transferTags(tags_except_from))` — both re-interned
> into the target grammar.
> (5) Build s_gN (the "no fromTag" set): `s_gN = grammar->allocateSet()`;
> `addTaglistsToSet(tvs_no_from, s_gN)`; `s_gN->trie_special =
> trie_copy(set_g->trie_special)` (the ONE-argument TagTrie::trie_copy — no
> tag re-interning, keeping set_g's own already-target-grammar tags);
> `s_gN->ff_tags = set_g->ff_tags`; `s_gN->sets = set_g->sets` (expected
> empty); `s_gN->set_ops = set_g->set_ops`; `addSetToGrammar(s_gN)`.
> (6) Copy the relabel set: `s_gR_num = copyRelabelSetToGrammar(set_r)`.
> (7) Determine s_gI (the "had fromTag, intersected with relabel set"): if
> `tvs_with_from` is empty, `s_gI_num = s_gR_num` (avoid intersecting with
> the empty set, which would never match). Otherwise: `s_gW = grammar->
> allocateSet()`; `addTaglistsToSet(tvs_with_from, s_gW)`;
> `addSetToGrammar(s_gW)`; if `s_gW->getNonEmpty().empty()` warn `"Warning:
> unexpected empty tries when relabelling set %d!\n"` with `set_g->number`;
> then `s_gI = grammar->allocateSet()` with `s_gI->sets = { s_gR_num,
> s_gW->number }` and `s_gI->set_ops = { S_PLUS }` (a set-intersection of
> the copied relabel set and the fromTag-bearing lists); `addSetToGrammar(
> s_gI)`; `s_gI_num = s_gI->number`.
> (8) Reshape `set_g` itself into an OR: `set_g->sets = { s_gN->number,
> s_gI_num }`, `set_g->set_ops = { S_OR }`, then `reindexSet(*set_g)` (set_g
> was already in sets_list, so it is only reindexed, not re-added). The
> set_g main trie is now empty and its meaning is `s_gN OR s_gI`. No return
> value.

> [spec:cg3:def:relabeller.cg3.relabeller.relabel-fn]
> void Relabeller::relabel()

> [spec:cg3:sem:relabeller.cg3.relabeller.relabel-fn]
> Top-level driver that applies all collected relabel rules to the target
> grammar. Steps:
> (1) Build `tag_by_str`, an `unordered_map<UString, Tag*, hash_ustring>`,
> by iterating `grammar->single_tags_list` and setting
> `tag_by_str[tag_g->tag] = tag_g` (last-wins per tag string).
> (2) Build `sets_by_tag`, an `unordered_map<UString, std::set<Set*>,
> hash_ustring>`: for every set `it` in `grammar->sets_list`, get
> `trie_getTagList(it->trie)` (all tags in that set's MAIN trie — not
> trie_special) and, for each such tag, insert `it` into
> `sets_by_tag[tag->tag]`. So this maps a tag's string to every grammar set
> whose main trie mentions it.
> (3) RELABEL AS LIST: for each entry `it` in `*relabel_as_list` (a
> `UStringSetMap`, key = fromTag string, value = relabel target Set* from
> the relabels grammar): resolve `set_r = relabels->sets_list[it.second->
> number]` and `fromTag = tag_by_str[it.first]` (note operator[] would
> default-insert a null Tag* if the string is absent, but see below). Look
> up `sets_by_tag.find(it.first)`; if found, for each `set_g` in that set,
> call `relabelAsList(set_g, set_r, fromTag)`. If the tag string is in no
> grammar set, `sets_by_tag.find` misses and nothing is relabelled (so the
> possibly-null `fromTag` is never dereferenced).
> (4) RELABEL AS SET: identical loop over `*relabel_as_set`, calling
> `relabelAsSet(set_g, set_r, fromTag)` for each matching grammar set.
> (5) Finalize: `grammar->sets_by_tag.clear()` (the grammar's own tag->set
> index must be rebuilt because sets_list has grown), then
> `grammar->reindex()`, then `grammar->num_tags = grammar->
> single_tags_list.size()`. No return value; the grammar is mutated in
> place.

> [spec:cg3:def:relabeller.cg3.relabeller.relabeller-fn]
> Relabeller::Relabeller(Grammar& res, const Grammar& relabels, std::ostream& ux_err)

> [spec:cg3:sem:relabeller.cg3.relabeller.relabeller-fn]
> Constructor. Stores `ux_stderr = &ux_err`, `grammar = &res` (the target
> grammar to be relabelled), and `relabels = &relabels` (the grammar of
> relabel rules). Then partitions the relabel rules into two maps: builds
> local `std::unique_ptr<UStringSetMap>` `as_list` and `as_set` (each maps
> a fromTag string to a relabel target Set*).
> For each `rule` in `relabels.rule_by_number`: compute `fromTags =
> trie_getTagList(rule->maplist->trie)` (the rule's map-list tags),
> `target = relabels.sets_list[rule->target]`, and `toTags =
> trie_getTagList(target->trie)` (the target set's main-trie tags). Then
> apply these guard checks, each emitting a warning to `ux_stderr` and
> `continue`-ing (skipping the rule) when it fails:
> - If NOT (`rule->maplist->trie_special.empty()` AND
>   `target->trie_special.empty()`): warn `"Warning: Relabel rule '%S' on
>   line %d has %d special tags, skipping!\n"`. NOTE: this format string
>   has three conversions (%S, %d, %d) but is passed only two arguments
>   (`rule->name.data()`, `rule->line`); the third `%d` has no matching
>   argument (see report — apparent bug producing garbage for the count).
> - If `!rule->tests.empty()`: warn `"... had context tests, skipping!\n"`
>   (args: name, line).
> - If `rule->wordform` is non-null: warn `"... had a wordform,
>   skipping!\n"` (args: name, line).
> - If `rule->type != K_MAP`: warn `"... has unexpected keyword (expected
>   MAP), skipping!\n"` (args: name, line).
> - If `fromTags.size() != 1`: warn `"... has %d tags in the maplist
>   (expected 1), skipping!\n"` (args: name, line, fromTags.size()).
> If all guards pass, take `fromTag = fromTags[0]`. For each `toit` in
> `toTags`, if `toit->type & T_SPECIAL`, warn `"Warning: Special tags (%S)
> not supported yet.\n"` with the tag text (a warning only — the rule is
> still recorded). Finally: if `toTags` is non-empty (the target set has
> literal tags in its main trie), `as_list->emplace(fromTag->tag.data(),
> target)`; otherwise `as_set->emplace(fromTag->tag.data(), target)`. Since
> `emplace` on an unordered_map does not overwrite, a duplicate fromTag
> string keeps its first target. After the loop, moves `as_list` into
> `relabel_as_list` and `as_set` into `relabel_as_set`.

> [spec:cg3:def:relabeller.cg3.relabeller.tag-vector]
> typedef std::vector<Tag*> TagVector

> [spec:cg3:def:relabeller.cg3.relabeller.transfer-tags-fn]
> TagVector Relabeller::transferTags(const TagVector& tv_r)

> [spec:cg3:sem:relabeller.cg3.relabeller.transfer-tags-fn]
> Re-interns a list of tags into the target grammar and returns the
> canonical pointers. Given `tv_r` (tags belonging to the relabels
> grammar), builds a new `TagVector tv_g`: for each `tag_r` in `tv_r`,
> allocates `Tag* tag_g = new Tag(*tag_r)` (a deep copy), then
> `tag_g = grammar->addTag(tag_g)` — addTag either inserts the new tag
> (assigning it a number in the target grammar's single_tags_list) or, if
> an identical tag already exists there, deletes the just-`new`ed copy and
> returns the existing canonical Tag*. Pushes the resulting canonical
> `tag_g` onto `tv_g`. Returns `tv_g`, a same-length vector of tags now
> owned/interned by the target grammar, preserving input order.

> [spec:cg3:def:relabeller.cg3.relabeller.u-string-map]
> typedef std::unordered_map<UString, UString, hash_ustring> UStringMap

> [spec:cg3:def:relabeller.cg3.relabeller.u-string-set-map]
> typedef std::unordered_map<UString, Set*, hash_ustring> UStringSetMap

> [spec:cg3:def:relabeller.cg3.trie-copy-fn]
> inline trie_t trie_copy(const trie_t& trie, Grammar& grammar)

> [spec:cg3:sem:relabeller.cg3.trie-copy-fn]
> The two-argument, tag-re-interning copy of a tag trie (a different
> overload from the one-argument `trie_copy` in TagTrie.hpp). Returns a new
> `trie_t nt` by value. Iterates each entry `p` of the source `trie`
> (`p.first` is a `Tag*`, `p.second` a `trie_node_t`): allocates `Tag* t =
> new Tag(*p.first)` (deep copy), then `t = grammar.addTag(t)` to re-intern
> it into `grammar` (deletes the copy and returns the existing tag if an
> identical one is already present). Sets `nt[t].terminal =
> p.second.terminal`. If the source node has a child trie
> (`p.second.trie`), sets `nt[t].trie = _trie_copy_helper(*p.second.trie)`.
> IMPORTANT QUIRK: that recursive call passes a single argument, so by
> overload resolution it binds to the ONE-argument
> `CG3::_trie_copy_helper(const trie_t&)` from TagTrie.hpp — which copies
> child levels by the ORIGINAL tag pointers WITHOUT re-interning them into
> `grammar`. Consequently only the top level of the returned trie has its
> tags transferred to the target grammar; any nested (multi-tag-list)
> levels retain pointers to the source (relabels) grammar's Tag objects
> (see report). Returns `nt`.

> [spec:cg3:def:relabeller.cg3.trie-copy-helper-fn]
> inline trie_t* _trie_copy_helper(const trie_t& trie, Grammar& grammar)

> [spec:cg3:sem:relabeller.cg3.trie-copy-helper-fn]
> The two-argument, tag-re-interning recursive helper (a different overload
> from the one-argument `_trie_copy_helper` in TagTrie.hpp). Allocates a
> heap `trie_t* nt = new trie_t`. For each entry `p` of the source `trie`:
> `Tag* t = new Tag(*p.first)`, `t = grammar.addTag(t)` (re-intern into
> `grammar`, deleting the copy if a duplicate exists), `(*nt)[t].terminal =
> p.second.terminal`, and if `p.second.trie` exists, `(*nt)[t].trie =
> _trie_copy_helper(*p.second.trie)`. As with `trie_copy`, that recursive
> call is single-argument and therefore resolves to the ONE-argument
> TagTrie.hpp helper, so nested levels are copied by original tag pointer
> without re-interning. Returns the raw `nt` pointer. QUIRK: this
> two-argument overload is never actually called anywhere in the Relabeller
> code — `trie_copy` (two-arg) and this helper both recurse through the
> single-argument form — so it is effectively dead code; the intended
> deep re-interning of nested trie levels does not occur (see report).

