# src/GrammarApplicator_reflow.cpp

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn]
> uint32_t GrammarApplicator::addTagToReading(Reading& reading, Tag* tag, bool rehash)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn]
> Adds a Tag to a Reading, updating all of the reading's derived tag indexes,
> the parent cohort's flags, and (optionally) the window's bag-of-tags; returns
> the (possibly substituted) tag's hash. Steps: if `tag->type & T_VARSTRING`,
> replace `tag = generateVarstringTag(tag)` first (expand the varstring into a
> concrete tag). Possible-sets: look up `grammar->sets_by_tag[tag->hash]`; if
> found, grow `reading.parent->possible_sets` to at least the bitset's size and
> OR it in (mark all sets that could contain this tag as candidates for the
> cohort). Insert `tag->hash` into `reading.tags` (sorted set), push it onto
> `reading.tags_list` (ordered vector), and insert into `reading.tags_bloom`.
> Ordered mode only (`ordered`): append a space (if `tags_string` non-empty)
> then `tag->tag` to `reading.tags_string`, and recompute `reading.tags_string_hash
> = hash_value(reading.tags_string)`. Parentheses: if the hash is in
> `grammar->parentheses`, set `reading.parent->is_pleft = tag->hash`; if in
> `grammar->parentheses_reverse`, set `reading.parent->is_pright = tag->hash`.
> Mapping: if `tag->type & T_MAPPING` OR `tag->tag[0] == grammar->mapping_prefix`,
> set the `T_MAPPING` bit on the tag, and if the reading already has a different
> `mapping`, print "cannot add a mapping tag to a reading which already is
> mapped!" and `CG3Quit(1)`; otherwise set `reading.mapping = tag`. Textual: if
> `tag->type & (T_TEXTUAL|T_WORDFORM|T_BASEFORM)`, insert into
> `reading.tags_textual` and its bloom. Numerical: if `T_NUMERICAL`, store
> `reading.tags_numerical[tag->hash] = tag` and clear `CT_NUM_CURRENT` on the
> parent cohort (invalidate cached min/max). Baseform: if `reading.baseform`
> is 0 and `tag` is `T_BASEFORM`, set `reading.baseform = tag->hash`.
> Dependency: if `parse_dep && (tag->type & T_DEPENDENCY) && !(parent->type &
> CT_DEP_DONE)`, set `parent->dep_self = tag->dep_self`, `parent->dep_parent =
> tag->dep_parent`, and if `dep_parent == dep_self` reset `parent->dep_parent =
> DEP_NO_PARENT`; set `has_dep = true`. Relations: if `grammar->has_relations &&
> (tag->type & T_RELATION)`: if `tag->dep_parent && tag->comparison_hash`,
> insert `tag->dep_parent` into `parent->relations_input[tag->comparison_hash]`;
> if `tag->dep_self`, set `gWindow->relation_map[tag->dep_self] =
> parent->global_number`; set `has_relations = true` and call
> `parent->setRelated()`. Plain: if NOT `T_SPECIAL`, insert into
> `reading.tags_plain` and its bloom. If `rehash` is true, call
> `reading.rehash()`. Bag-of-tags: if `grammar->has_bag_of_tags`, mirror a
> subset of the above into `reading.parent->parent->bag_of_tags` (`bot`): insert
> hash into `bot.tags`/`tags_list`/`tags_bloom`; if textual, into
> `bot.tags_textual`(+bloom); if numerical, `bot.tags_numerical[hash]=tag`; if
> `!reading.baseform && T_BASEFORM` set `bot.baseform=tag->hash` (QUIRK: this
> tests `reading.baseform`, which was just set to this tag's hash a few lines
> earlier when the reading had no prior baseform — so `bot.baseform` is set only
> in the case where the reading ALREADY had a baseform, likely a bug); if not
> special, into `bot.tags_plain`(+bloom); if `rehash`, `bot.rehash()`. Return
> `tag->hash`.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.attach-parent-child-fn]
> bool GrammarApplicator::attachParentChild(Cohort& parent, Cohort& child, bool allowloop, bool allowcrossing)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.attach-parent-child-fn]
> Attaches a dependency edge making `child` a dependent of `parent`, subject to
> loop/crossing guards; returns true on success, false if refused. First set
> `parent.dep_self = parent.global_number` and `child.dep_self =
> child.global_number` (normalize self-ids). Guard 1: if `!allowloop &&
> dep_block_loops && wouldParentChildLoop(&parent, &child)`, print a "would
> cause a loop. Will not attach them." warning (only when `verbosity_level > 0`)
> and return false. Guard 2: if `!allowcrossing && dep_block_crossing &&
> wouldParentChildCross(&parent, &child)`, print a "would cause crossing
> branches" warning (verbosity-gated) and return false. Detach child from its
> old parent: if `child.dep_parent == DEP_NO_PARENT`, set it to `child.dep_self`
> first; then find `child.dep_parent` in `gWindow->cohort_map` and, if present,
> call `remChild(child.dep_self)` on that old parent (removes child from its
> `dep_children`). Reattach: set `child.dep_parent = parent.global_number` and
> `parent.addChild(child.global_number)` (insert into parent's `dep_children`).
> Mark both cohorts `type |= CT_DEP_DONE`. Cross-window check: if
> `!dep_has_spanned && child.parent != parent.parent` (edge spans two windows),
> print an "Info: Dependency ... spans the window boundaries. Enumeration will
> be global from here on." message and set `dep_has_spanned = true`. Return
> true.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.del-tag-from-reading-fn]
> void GrammarApplicator::delTagFromReading(Reading& reading, uint32_t utag)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.del-tag-from-reading-fn]
> Removes a tag (by hash `utag`) from a reading and refreshes its state. Steps:
> `erase(reading.tags_list, utag)` (remove all occurrences from the ordered
> vector); `reading.tags.erase(utag)`; `reading.tags_textual.erase(utag)`;
> `reading.tags_numerical.erase(utag)`; `reading.tags_plain.erase(utag)`. If
> `reading.mapping` is set and `utag == reading.mapping->hash`, clear
> `reading.mapping = nullptr`. If `utag == reading.baseform`, clear
> `reading.baseform = 0`. Always call `reading.rehash()`. Clear `CT_NUM_CURRENT`
> on `reading.parent->type` (invalidate the cohort's cached numeric min/max).
> NOTE: the bloom filters (tags_bloom/tags_plain_bloom/tags_textual_bloom) are
> NOT updated (blooms are additive-only, so they may report false positives
> until rebuilt), and `tags_string`/`tags_string_hash` (ordered mode) and the
> window bag-of-tags are NOT touched. The overload taking a `Tag*` simply
> forwards `tag->hash` to this one.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.delimit-at-fn]
> Cohort* GrammarApplicator::delimitAt(SingleWindow& current, Cohort* cohort)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.delimit-at-fn]
> Splits SingleWindow `current` at `cohort`, moving everything AFTER `cohort`
> into a freshly-created following window, and returns the new last cohort of
> `current` (the one that received the end tag). Create the new window `nwin`:
> if `current` is the parent Window's `current`, use `allocPushSingleWindow()`;
> otherwise search `current.parent->next` for `&current` and, if found, insert
> `nwin = allocSingleWindow()` right after it (`next.insert(++iter, nwin)`);
> failing that search `current.parent->previous` and insert `nwin` at that
> position; then call `gWindow->rebuildSingleWindowLinks()`. `assert(nwin != 0)`.
> Move window-trailing state onto `nwin`: `std::swap(current.flush_after,
> nwin->flush_after)`, `std::swap(current.text_post, nwin->text_post)`, and
> `nwin->has_enclosures = current.has_enclosures`. Build a synthetic BEGIN
> cohort `cCohort` in `nwin`: `global_number = current.parent->cohort_counter++`,
> `wordform = tag_begin`; give it one reading `cReading` with `baseform =
> begintag`, seed its `possible_sets` with `grammar->sets_any`, and
> `addTagToReading(*cReading, begintag)`; append the reading and append
> `cCohort` to `nwin`. Now relocate the tail: `lc = cohort->local_number`; find
> `cohort` in `current.all_cohorts` starting at `begin()+lc`, advance one
> (`++nc`) to the first cohort after it, remember `from = nc`. For each cohort
> from `nc` to end of `current.all_cohorts`: set its `parent = nwin`; if it is
> `CT_ENCLOSED|CT_REMOVED|CT_IGNORED`, push it onto `nwin->all_cohorts` only,
> else `nwin->appendCohort(*nc)` (adds to both cohorts and all_cohorts). Then
> truncate `current`: erase `current.cohorts` from `begin()+lc+1` to end, and
> erase `current.all_cohorts` from `from` to end. Finally set `cohort =
> current.cohorts.back()` (the new last real cohort of `current`) and, for each
> of its readings, `addTagToReading(*reading, endtag)` (append the END tag).
> Call `gWindow->rebuildCohortLinks()`. Return `cohort`.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.generate-varstring-tag-fn]
> Tag* GrammarApplicator::generateVarstringTag(const Tag* tag)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.generate-varstring-tag-fn]
> Expands a VARSTRING tag template into a concrete tag by substituting unified
> sets, regex capture groups ($1..$9), and applying case markers (%u/%U/%l/%L),
> then returns the interned resulting Tag. Uses a thread-local UnicodeString
> `tmp` initialized to `tag->tag`; `did_something = false`. (1) Escape markers:
> replace the literal marker forms with control-code sentinels so combined
> markers don't clash — for each of the 13 pairs replace `%u`→`\x01u`,
> `%U`→`\x01U`, `%l`→`\x01l`, `%L`→`\x01L`, `$1`→`\x011` … `$9`→`\x019` via
> `findAndReplace` (order as listed). (2) Unified-set substitution: if
> `tag->vs_sets` is set, for each index i, gather the set's tags via
> `getTagList(*(*tag->vs_sets)[i], tags)`, build `rpl` by concatenating each
> tag's text with `_` inserted between multiple tags (composite tags), then
> `findAndReplace(tmp, (*tag->vs_names)[i], rpl)`; if that replaced anything,
> `did_something = true`. (3) Capture groups: for i in 0..min(regexgrp_ct, 9)
> (from `context_stack.back()`), replace sentinel `\x01(i+1)` with the captured
> text `USV((*context_stack.back().regexgrps)[i])`; if replaced,
> `did_something = true`. (4) Case markers: loop until none remain — each pass,
> find the RIGHTMOST occurrence among the four sentinels `\x01u`, `\x01U`,
> `\x01l`, `\x01L` (via successive `lastIndexOf`, taking the max position
> `mpos`); if found, read `mode = tmp[mpos+1]`, delete the 2-char marker at
> `mpos`, then apply to the text now at `mpos`: `u` = uppercase the single char
> at `mpos`; `U` = uppercase from `mpos` to end; `l` = lowercase the single char
> at `mpos`; `L` = lowercase from `mpos` to end; set `did_something = true`.
> Processing rightmost-first lets multiple/nested markers resolve correctly.
> (5) Re-append type suffixes: if `tag->type & T_CASE_INSENSITIVE` append `i`;
> if `tag->type & T_REGEXP` append `r` (so the regenerated string re-parses to
> the same flags). Get the terminated buffer `nt`; if `!did_something` AND the
> result equals the original `tag->tag`, print a warning that it was "Unable to
> generate from tag ... Possibly missing KEEPORDER and/or capturing regex ..."
> Return `addTag(nt, tag->type)` (intern/parse the new tag string preserving the
> original type bits). Uppercasing/lowercasing use ICU UnicodeString
> toUpper/toLower (locale-independent, Unicode full case mapping).

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.is-child-of-fn]
> bool GrammarApplicator::isChildOf(const Cohort* child, const Cohort* parent)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.is-child-of-fn]
> Returns whether `child` is a descendant of `parent` in the dependency tree
> (or the same node). `retval = false`. Fast cases: if `parent->global_number
> == child->global_number`, true (same node counts as child-of). Else if
> `parent->global_number == child->dep_parent`, true (direct parent). Else walk
> up the ancestor chain from `child`: loop `i` from 0 while `i < 1000`, with
> `inner` starting at `child`. Each iteration: if `inner->dep_parent == 0` or
> `== DEP_NO_PARENT`, set `retval = false` and break (reached the root). Look up
> `inner->dep_parent` in `gWindow->cohort_map`; if present, advance `inner` to
> that cohort, else break (dangling parent). After advancing, if `inner->dep_parent
> == parent->global_number`, set `retval = true` and break. (NOTE the check
> compares the NEW inner's dep_parent to parent, i.e. it effectively tests
> grandparent links as it climbs.) If the loop ran the full 1000 iterations
> (`i == 1000`), print a verbosity-gated warning about exceeding the counter
> ("indicating a loop higher up in the tree"). Return `retval`.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.make-base-from-word-fn]
> Tag* GrammarApplicator::makeBaseFromWord(Tag* tag)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.make-base-from-word-fn]
> Derives a baseform tag from a wordform tag by stripping the inner angle
> brackets, e.g. `"<foo>"` → `"foo"`. Let `len = tag->tag.size()`. If `len < 4`,
> return `tag` unchanged (too short to strip). Otherwise build a thread-local
> UString `n` of size `len-2`: set `n[0] = '"'` and `n[len-3] = '"'` (the outer
> quotes), and copy `len-4` code units from `tag->tag.data()+2` into `&n[1]`
> (i.e. keep everything except the character at index 1 and the character at
> index `len-2` — the `<` and `>` just inside the quotes). Then `nt = addTag(n)`
> (intern the new tag string) and return `nt`. The `uint32_t` overload just
> resolves the hash to a `Tag*` via `grammar->single_tags` and calls this one.
> No mutation of the input tag.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.merge-mappings-fn]
> void GrammarApplicator::mergeMappings(Cohort& cohort)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.merge-mappings-fn]
> Merges duplicate/mapping-equivalent readings within a cohort by delegating to
> `mergeReadings`. Always calls `mergeReadings(cohort.readings)`. Additionally,
> when `trace` is enabled, also calls `mergeReadings(cohort.deleted)` and
> `mergeReadings(cohort.delayed)` so the deleted/delayed lists get the same
> merging (needed to keep trace output consistent). No return value.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.merge-readings-fn]
> void GrammarApplicator::mergeReadings(ReadingList& readings)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.merge-readings-fn]
> Deduplicates a ReadingList in place, collapsing readings that are identical
> apart from their mapping tags into a single reading carrying all those mapping
> tags. Uses two thread-local maps cleared/reserved each call: `mapped`
> (uint32 → pair<uint32 count, Reading*>) and `mlist` (uint32 → ReadingList).
> First pass over each reading `r`: compute two hashes `hp` and `hplain`, both
> starting at `r->hash_plain` (or, in `ordered` mode, at `r->tags_string_hash`);
> `nm` = number of mapping tags in the reading chain (0 or more). If `trace`,
> fold each `r->hit_by` rule id into `hp` via `hash_value`. If `r->mapping`,
> `++nm`. Then walk the sub-reading chain (`r->next`, `->next`, …): for each
> sub, fold its hash into both `hp` and `hplain` (using `tags_string_hash` in
> ordered mode, else `hash_plain`); if `trace`, fold the sub's `hit_by` into
> `hp`; if the sub has a mapping, `++nm`. Dedup bookkeeping keyed by `hplain`
> (the mapping-independent identity): if `mapped` already has `hplain`, then if
> the stored count `!= 0` and the new `nm == 0`, mark the NEW reading
> `r->deleted = true`; else if the stored count `!= nm` and stored count `== 0`,
> mark the STORED reading's Reading `deleted = true`. Update `mapped[hplain] =
> (nm, r)`. Append `r` to `mlist[hp + nm]` (bucket keyed by the mapping-aware
> hash plus mapping count). After the pass: if `mlist.size() == readings.size()`
> (no two readings shared a bucket), return without changes. Otherwise rebuild:
> clear `readings` and a thread-local `order` vector. For each bucket `miter`
> (iterated in `mlist` key order): take the front reading of the bucket's list,
> `nr = alloc_reading(*front)` (a copy); if `nr->mapping`, erase its mapping
> hash from `nr->tags_list`. For every reading in the bucket: if it has a
> mapping whose hash is not already in `nr->tags_list`, append that mapping hash
> to `nr->tags_list`; then `free_reading` it (frees all originals). Push `nr`
> into `order`. Finally `std::sort(order, Reading::cmp_number)` and insert all
> of `order` at the beginning of `readings`. Net: identical readings differing
> only by mapping are fused into one reading holding the union of mapping tags,
> with some conflicting mapped/unmapped duplicates flagged `deleted`.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-dependency-window-fn]
> void GrammarApplicator::reflowDependencyWindow(uint32_t max)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-dependency-window-fn]
> Resolves the raw per-cohort dependency ids (`dep_self`/`dep_parent` values as
> read from input) into actual global cohort numbers and wires up parent/child
> links, processing `gWindow->dep_window` (a map of dep-id → Cohort*). `max`
> bounds how far to process. If `dep_delimit && !max && !input_eof` and there's
> a next window with >1 cohort, set `max = gWindow->next.back()->cohorts[1]->global_number`.
> Ensure a root entry at key 0 in `dep_window`: if empty or its first entry's
> `parent==0`, set `dep_window[0] = gWindow->current->cohorts[0]`; else if key 0
> absent, set it (in two steps to avoid a cross-compiler evaluation-order
> segfault) to the first entry's `parent->cohorts[0]`. Ensure `cohort_map[0]`
> similarly points at a window's cohort[0]. Then loop over `dep_window` in id
> order with iterator `begin`: skip leading entries already `CT_DEP_DONE` or
> with `!dep_self`. Clear `dep_map`. Build a batch [begin,end): advance `end`
> collecting cohorts, skipping ones that are `CT_DEP_DONE` or have `!dep_self`;
> stop if `max` set and `cohort->global_number >= max`, or if a `dep_self` value
> repeats (already in `dep_map`) — for each accepted cohort set
> `dep_map[cohort->dep_self] = cohort->global_number` and normalize
> `cohort->dep_self = cohort->global_number`. If `dep_map` ends up empty, break
> the outer loop. Set `dep_map[0] = 0`. Second inner pass from `begin` to `end`:
> for each cohort (stop if `max` and `global_number >= max`): skip if
> `dep_parent == DEP_NO_PARENT`; only process cohorts whose `dep_self ==
> global_number`. If not `CT_DEP_DONE` and the cohort's `dep_parent` is NOT a
> key in `dep_map`: print a verbosity-gated "Parent %u of dep %u ... does not
> exist - ignoring." warning and set `cohort->dep_parent = DEP_NO_PARENT`. Else:
> if not `CT_DEP_DONE`, translate `cohort->dep_parent = dep_map[cohort->dep_parent]`
> (raw id → real global number); set `cohort_map[0] = cohort->parent->cohorts[0]`;
> look up the (translated) `dep_parent` in `cohort_map` and, if found, call
> `addChild(cohort->dep_self)` on that parent cohort; mark `cohort |=
> CT_DEP_DONE`. After the outer loop finishes, `dep_map.clear()` and
> `dep_window.clear()`. No return value.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-reading-fn]
> void GrammarApplicator::reflowReading(Reading& reading)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-reading-fn]
> Rebuilds all of a reading's derived tag indexes from its `tags_list` (used
> after low-level tag surgery). Clears: `tags`, `tags_plain`, `tags_textual`,
> `tags_numerical`, `tags_bloom`, `tags_textual_bloom`, `tags_plain_bloom`; sets
> `mapping = nullptr`; clears `tags_string`. Seeds the parent cohort's
> `possible_sets` with `grammar->sets_any` via `insert_if_exists(...)` (OR in
> the always-matching sets). Then swaps the current `tags_list` into a local
> `tlist` (emptying `reading.tags_list`) and, for each tag hash `tter` in
> `tlist`, calls `addTagToReading(reading, tter, false)` (rehash=false), which
> re-populates all the cleared indexes (and re-appends to `tags_list`,
> `tags_string`, etc.). Finally calls `reading.rehash()` once. Note
> `reading.baseform` is NOT cleared here (nor by rehash); since
> `addTagToReading` only sets baseform when it is currently 0, the existing
> baseform value is preserved across the reflow (the tag set is unchanged, so
> this keeps the same baseform). No return value.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-relation-window-fn]
> void GrammarApplicator::reflowRelationWindow(uint32_t max)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-relation-window-fn]
> Resolves deferred named relations (`relations_input`, keyed by relation name
> hash → set of target cohort ids) into concrete relations (`relations`, keyed
> by name → set of resolved global cohort numbers) using
> `gWindow->relation_map` (which maps a relation-id to the global number of the
> cohort that declared it). `max` bounds processing. If `!max && !input_eof`
> and there's a next window with >1 cohort, set `max =
> gWindow->next.back()->cohorts[0]->global_number`. Walk to the leftmost cohort:
> start at `gWindow->current->cohorts[1]` and follow `->prev` until null. Then
> iterate cohorts forward via `->next`: stop if `max` set and
> `cohort->global_number >= max`. For each cohort, iterate its
> `relations_input` entries `rel` (name → id-set): create a pooled scratch set
> `newrel = ss_u32sv.get()`; for each `target` id in `rel->second`, look it up
> in `gWindow->relation_map` — if found, insert the mapped global number into
> `cohort->relations[rel->first]`; if not found, keep it in `newrel` (deferred
> for a later window). If `newrel` is empty (all resolved), erase this entry
> from `relations_input` (`rel = erase(rel)`); otherwise replace `rel->second =
> newrel` and advance `++rel`. No return value; unresolved targets remain in
> `relations_input` to be retried on a later reflow.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-cohort-fn]
> void GrammarApplicator::reflowTextuals_Cohort(Cohort& c)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-cohort-fn]
> Runs `reflowTextuals_Reading` over every reading in a cohort, across all four
> reading lists in order: `c.readings`, then `c.deleted`, then `c.ignored`,
> then `c.delayed`. Each `reflowTextuals_Reading` recursively re-derives the
> reading's `tags_textual` index. No return value.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-fn]
> void GrammarApplicator::reflowTextuals()

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-fn]
> Re-derives the `tags_textual` index for every reading of every cohort in all
> currently-loaded windows. Iterates `gWindow->previous` (calling
> `reflowTextuals_SingleWindow` on each), then the `gWindow->current` window,
> then each window in `gWindow->next`. Called when a newly-added regex or
> case-insensitive tag causes some tags to be reclassified as `T_TEXTUAL`, so
> existing readings must re-scan their tags. No return value.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-reading-fn]
> void GrammarApplicator::reflowTextuals_Reading(Reading& r)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-reading-fn]
> Recomputes a reading's `tags_textual` set (and its bloom) by scanning its
> `tags`. First, if the reading has a sub-reading (`r.next`), recurse into
> `reflowTextuals_Reading(*r.next)` (so the whole sub-reading chain is
> processed). Then for each tag hash `it` in `r.tags`, look up `tag =
> grammar->single_tags[it]`, and if `tag->type & T_TEXTUAL`, insert `it` into
> both `r.tags_textual` and `r.tags_textual_bloom`. This only ADDS entries (it
> does not clear `tags_textual` first), so it is meant to pick up tags newly
> reclassified as textual. No return value.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-single-window-fn]
> void GrammarApplicator::reflowTextuals_SingleWindow(SingleWindow& sw)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-single-window-fn]
> Runs `reflowTextuals_Cohort` on every cohort in `sw.all_cohorts` (the full
> cohort list including enclosed/removed/ignored), re-deriving textual tag
> indexes window-wide. No return value.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.split-all-mappings-fn]
> void GrammarApplicator::splitAllMappings(all_mappings_t& all_mappings, Cohort& cohort, bool mapped)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.split-all-mappings-fn]
> Applies pending mapping-tag splits to a cohort's readings. If `all_mappings`
> (a map Reading* → TagList of mapping tags) is empty, return immediately.
> Snapshot the current readings into a thread-local `readings` copy (so
> appending new readings during iteration is safe). For each `reading` in the
> snapshot: look it up in `all_mappings`; if absent, `continue`; otherwise call
> `splitMappings(iter->second, cohort, *reading, mapped)` (which distributes the
> mapping tags across new/duplicate readings). After processing, sort
> `cohort.readings` by `Reading::cmp_number`. If `grammar->reopen_mappings` is
> non-empty, for each reading whose `mapping` hash is in `reopen_mappings`, set
> `reading->mapped = false` (re-open those mappings so later mapping rules can
> apply again). Finally `all_mappings.clear()`. No return value. The `mapped`
> flag is passed through to control whether the produced readings are marked as
> already-mapped.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.split-mappings-fn]
> void GrammarApplicator::splitMappings(TagList& mappings, Cohort& cohort, Reading& reading, bool mapped)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.split-mappings-fn]
> Splits one reading into as many readings as there are mapping tags in
> `mappings`, so each resulting reading carries exactly one mapping. First pass
> over `mappings` (mutating iteration): for each entry `tag`, while `tag` is
> `T_VARSTRING`, replace it with `generateVarstringTag(tag)` (in place). Then if
> the (resolved) `tag` is NOT a mapping (not `T_MAPPING` and its first char is
> not `grammar->mapping_prefix`), it isn't really a mapping tag: add it to the
> reading via `addTagToReading(reading, tag)` and erase it from `mappings`;
> otherwise advance. After this pass `mappings` holds only true mapping tags.
> If the reading currently has a `mapping`, push that existing mapping onto
> `mappings` and remove it from the reading (`delTagFromReading(reading,
> reading.mapping->hash)`), so it participates in the split too. Take the LAST
> mapping off `mappings` (`tag = mappings.back(); pop_back()`) to reuse for the
> original reading. `i = mappings.size()`. For each remaining `ttag` in
> `mappings`: dedup — scan `cohort.readings` for an existing reading with the
> same `hash_plain` and a `mapping` whose hash equals `ttag->hash`; if found,
> `continue` (skip duplicating). Otherwise clone: `nr = alloc_reading(reading)`;
> set `nr->mapped = mapped`; set `nr->number = reading.number - i--` (so clones
> sort just before the original, decreasing); `mp = addTagToReading(*nr, ttag)`;
> set `nr->mapping` to `single_tags[mp]` if `addTagToReading` returned a
> different hash (varstring expansion) else to `ttag`; `cohort.appendReading(nr)`
> and `++numReadings`. Finally, for the original `reading`: set `reading.mapped
> = mapped`; `mp = addTagToReading(reading, tag)`; set `reading.mapping` to
> `single_tags[mp]` if `mp != tag->hash` else `tag`. No return value. Net: N
> mapping tags become N readings (the original plus N-1 clones), each with one
> mapping, skipping any that already exist in the cohort.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.unmap-reading-fn]
> bool GrammarApplicator::unmapReading(Reading& reading, const uint32_t rule)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.unmap-reading-fn]
> Removes a reading's mapping and mapped state, recording the responsible rule.
> `readings_changed = false`. If `reading.mapping` is set: set `reading.noprint
> = false`, call `delTagFromReading(reading, reading.mapping->hash)` (which also
> clears `reading.mapping`), and set `readings_changed = true`. If
> `reading.mapped` is true: set it false and `readings_changed = true`. If
> anything changed, push `rule` onto `reading.hit_by` (trace of which rule
> touched it). Return `readings_changed`.

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-cross-fn]
> bool GrammarApplicator::wouldParentChildCross(const Cohort* parent, const Cohort* child)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-cross-fn]
> Tests whether attaching `child` under `parent` would create crossing
> dependency branches. Compute `mn = min(parent->global_number,
> child->global_number)` and `mx = max(...)`. Loop `i` from `mn+1` to `mx-1`
> (i.e. over the cohorts strictly between them): each iteration, look up
> `parent->dep_parent` in `gWindow->cohort_map`; if found AND that cohort's
> `dep_parent != DEP_NO_PARENT` AND that grandparent id is `< mn` or `> mx`
> (i.e. points outside the [mn,mx] span), return true (a crossing). If the loop
> completes with no such case, return false. NOTE/QUIRK: the loop body does not
> depend on `i` — it recomputes the same lookup on `parent->dep_parent` every
> iteration — so effectively it performs the single check `mx - mn - 1` times
> (returns true if the one condition holds and there is at least one cohort
> between parent and child, else false). Faithfully reproduce this (do not
> "fix" it to iterate over the in-between cohorts).

> [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-loop-fn]
> bool GrammarApplicator::wouldParentChildLoop(const Cohort* parent, const Cohort* child)

> [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-loop-fn]
> Tests whether making `child` a dependent of `parent` would create a cycle.
> `retval = false`. Fast cases, in order: if `parent->global_number ==
> child->global_number`, return true (self-loop). Else if `parent->global_number
> == child->dep_parent`, false (child is already parent's child — re-attaching
> is fine). Else if `parent->global_number == parent->dep_parent`, false (parent
> is its own root). Else if `parent->dep_parent == child->global_number`, true
> (parent already descends directly from child). Else climb from `parent` up its
> ancestry: loop `i` in 0..<1000 with `inner` starting at `parent`. Each
> iteration: if `inner->dep_parent == 0` or `== DEP_NO_PARENT`, set `retval =
> false` and break (hit the root, no loop). Look up `inner->dep_parent` in
> `gWindow->cohort_map`; if present advance `inner` to it, else break. After
> advancing, if `inner->dep_parent == child->global_number`, set `retval = true`
> and break (child is an ancestor of parent, so attaching would loop). If the
> loop ran all 1000 iterations, print a verbosity-gated warning about exceeding
> the counter ("indicating a loop higher up in the tree"). Return `retval`.
> (Mirror image of isChildOf but climbing from `parent` and looking for
> `child`.)

