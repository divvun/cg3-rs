# src/GrammarApplicator_matchSet.cpp

> [spec:cg3:def:grammar-applicator-match-set.cg3.capture-regex-fn]
> inline void captureRegex(int32_t gc, uint8_t& regexgrp_ct, RXGS* regexgrps, Tag& tag)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.capture-regex-fn]
> Template helper that harvests the capture groups of a Tag's ICU regexp
> (`tag.regexp`) immediately after a successful `uregex_setText`+`uregex_find`
> on some input, appending them to the current rule context's capture store.
> Parameters: `gc` = number of capture groups (from `uregex_groupCount`);
> `regexgrp_ct` = a running count (by reference) of how many groups have
> already been stored in this rule context; `regexgrps` = pointer to the
> vector<UnicodeString> store; `tag` = the tag whose regexp was just matched.
> Uses a fixed 1024-UChar (`BUFSIZE`) stack buffer `tmp` plus a heap fallback
> UString `_stmp`. Loop i from 1 to gc INCLUSIVE (1-based; group 0, the whole
> match, is deliberately NOT captured): set `tmp[0]=0`, call
> `uregex_group(tag.regexp, i, tmp, BUFSIZE, &status)` which copies group i's
> matched text into `tmp` and returns its true length `len`. If `len >= BUFSIZE`
> the group overflowed the stack buffer: reset status, `_stmp.resize(len+1)`,
> repoint `tmp` at `&_stmp[0]`, and re-extract with capacity `len+1`. Then grow
> the store to at least `regexgrp_ct+1` slots via
> `regexgrps->resize(max(regexgrp_ct+1, regexgrps->size()))` (never shrinks;
> preserves existing entries). Take the slot at index `regexgrp_ct`, clear it
> (`ucstr.remove()`), and `append(tmp, len)` the group text. Increment
> `regexgrp_ct`. Net effect: the gc groups are stored sequentially starting at
> the current `regexgrp_ct`, and `regexgrp_ct` advances by gc so that successive
> regexp tags matched within the same rule accumulate their groups end-to-end
> (these are what `$1..$9` later resolve to). No return value; mutates
> `regexgrp_ct` and `*regexgrps`. ICU status codes from `uregex_group` are not
> checked here. PORT/regex parity: with the `regex` crate these are the
> 1-based capture group texts from the last match's Captures; a group that did
> not participate yields an empty string (ICU returns len 0 for it).

> [spec:cg3:def:grammar-applicator-match-set.cg3.check-options-fn]
> inline bool _check_options(std::vector<Reading*>& rv, uint32_t options, size_t nr)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.check-options-fn]
> Small predicate deciding, from an accumulated list of matched readings,
> whether a cohort test succeeded. Parameters: `rv` = the readings that
> matched, `options` = the contextual test's option bitmask, `nr` = the total
> number of eligible readings. Logic in order: (1) if `POS_CAREFUL` (the `C`
> flag) is set AND `rv.size() != nr`, return false — careful mode requires
> every eligible reading to have matched; (2) else if any dependency/relation
> position flag is set (`options & MASK_POS_DEPREL`, i.e. p/s/c/pp/cc or r:),
> return true unconditionally; (3) otherwise return `!rv.empty()` (true iff at
> least one reading matched). NOTE (dead code): this free function is defined
> in the translation unit but is not called anywhere in the codebase — the
> live careful/normal cohort logic lives in doesSetMatchCohortCareful /
> doesSetMatchCohortNormal. Reproduce for completeness; it is not on any
> executed path.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-line-fn]
> uint32_t GrammarApplicator::doesRegexpMatchLine(const Reading& reading, const Tag& tag, bool bypass_index)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-line-fn]
> Ordered-mode helper (marked "ToDo: Remove for real ordered mode") that tests
> a regexp tag against a reading's whole concatenated tag string rather than
> against its individual tags. Steps: compute `gc = uregex_groupCount(tag.regexp)`.
> Build a 64-bit index key `ih = (UI64(reading.tags_string_hash) << 32) | tag.hash`.
> Memoization: if `!bypass_index` and `index_regexp_no` contains `ih`, return 0
> (cached non-match). Else if `!bypass_index` and `gc == 0` and
> `index_regexp_yes` contains `ih`, return `reading.tags_string_hash` (cached
> match; caching of a positive result only happens when there are zero capture
> groups). Otherwise do the real match: `uregex_setText(tag.regexp,
> reading.tags_string.data(), size, &status)` — on ICU error print an
> "uregex_setText(MatchLine)" diagnostic (tag text + numLines) to ux_stderr and
> `CG3Quit(1)`. Reset status; call `uregex_find(tag.regexp, -1, &status)`. The
> start index `-1` means an UNANCHORED search over the entire input string; any
> anchoring comes from the compiled pattern itself. If it returns true, set
> `match = reading.tags_string_hash`. On ICU error from find, print
> "uregex_find(MatchLine)" diagnostic and `CG3Quit(1)`. If matched: when `gc > 0`
> AND `context_stack` is non-empty AND `context_stack.back().regexgrps != 0`,
> call `captureRegex(gc, context_stack.back().regexgrp_ct,
> context_stack.back().regexgrps, tag)` to harvest groups and do NOT cache;
> otherwise insert `ih` into `index_regexp_yes`. If not matched, insert `ih`
> into `index_regexp_no`. Return `match` (`reading.tags_string_hash` or 0).
> Regex parity: `tag.regexp` is the tag's compiled `URegularExpression`,
> compiled either from the inner text of a `/.../` tag (no added anchors, so it
> can match anywhere in `tags_string`) or as `^`+tagtext+`$` (fully anchored),
> with the `UREGEX_CASE_INSENSITIVE` flag iff the tag also carries
> T_CASE_INSENSITIVE.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-reading-fn]
> uint32_t GrammarApplicator::doesRegexpMatchReading(const Reading& reading, const Tag& tag, bool bypass_index)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-reading-fn]
> Tests whether any textual tag of a reading matches a regexp tag. First: if
> the tag is `T_REGEXP_LINE` (ordered-mode whole-line regex), delegate to
> `doesRegexpMatchLine(reading, tag, bypass_index)` and return its result.
> Otherwise iterate `reading.tags_textual` (the set of tag hashes that
> Grammar::reindex() pre-marked as potentially regex/textual-matchable) in its
> stored order; for each hash `mter` call `doesTagMatchRegexp(mter, tag,
> bypass_index)`, and keep the first non-zero result, breaking out of the loop.
> Return that result (the matched tag's hash) or 0 if none matched. Only reads
> `reading.tags_textual`; from `tag` it uses only type/hash/regexp.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
> bool GrammarApplicator::doesSetMatchCohortCareful(Cohort& cohort, const uint32_t set, dSMC_Context* context)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
> Careful ("C"-flag) variant of cohort matching: the set must match EVERY
> eligible reading of the cohort. `retval` starts false. Early-out guard: if a
> context is present AND none of POS_LOOK_DELETED/POS_LOOK_DELAYED/
> POS_LOOK_IGNORED/POS_NOT is set (i.e. `!(!context || (options & those))` is
> true) AND (`set >= cohort.possible_sets.size()` OR
> `!cohort.possible_sets.test(set)`), return false immediately (the possible_sets
> bitset says this set can't be in this cohort). Fetch `theset =
> grammar->sets_list[set]`. Build a 4-slot `ReadingList*` array `lists`: slot 0
> = `&cohort.readings`; slots 1..3 are null unless the corresponding option is
> set, in which case slot 1 = `&cohort.deleted` (POS_LOOK_DELETED), slot 2 =
> `&cohort.delayed` (POS_LOOK_DELAYED), slot 3 = `&cohort.ignored`
> (POS_LOOK_IGNORED). Iterate `lists` in order, skipping null slots. For each
> reading in a list: if `context && context->test`, replace `reading` with
> `get_sub_reading(reading, context->test->offset_sub)` and `continue` if that
> is null; if reading is inactive and POS_ACTIVE is set, `continue`; if reading
> is active and POS_INACTIVE is set, `continue`. Then set `retval =
> doesSetMatchCohort_helper(cohort, *reading, *theset, context)`; if `retval`
> is false, `break` the inner loop. After each list, if `!retval` `break` the
> outer loop too. Because `retval` is overwritten per reading and any failure
> breaks, the result is true only if the LAST processed reading matched and no
> earlier one failed — i.e. all readings across all included lists matched.
> EDGE: if `cohort.readings` is empty (and it's the first list), the inner loop
> never runs, `retval` stays false, and the function returns false. Finally, if
> `context && !context->matched_target && (options & POS_NOT)`, set `retval =
> doesSetMatchCohort_testLinked(cohort, *theset, context)` (run the linked
> test even though nothing matched, for negation). Return `retval`.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
> inline bool GrammarApplicator::doesSetMatchCohort_helper(Cohort& cohort, Reading& reading, const Set& theset, dSMC_Context* context)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
> Core per-reading matcher used by the Careful/Normal cohort tests; handles
> child-unification snapshot/rollback, negation, the linked contextual test,
> and attach-to bookkeeping. `retval` starts false. Grab scratch containers
> `utags = ss_utags.get()` and `usets = ss_usets.get()` (pooled unif_tags_t /
> unif_sets_t) and remember `orz = (context_stack.empty() ? 0 :
> context_stack.back().regexgrp_ct)` (the capture-group count to roll back to on
> failure). If `context` is set AND the current rule does NOT have
> FL_CAPTURE_UNIF AND the set is `ST_CHILD_UNIFY` AND context_stack non-empty:
> take a copy of the current unification state into the scratch
> (`*utags = *context_stack.back().unif_tags; *usets = *context_stack.back().unif_sets`)
> so it can be restored if the match fails. Call `doesSetMatchReading(reading,
> theset.number, (theset.type & (ST_CHILD_UNIFY | ST_SPECIAL)) != 0)` — i.e.
> bypass_index is forced true when the set is child-unify or special. If it
> matches: set `retval = true`, and if `context`: when `context->options &
> POS_ATTACH_TO` set `reading.matched_target = true`; always set
> `context->matched_target = true`. Then if `retval && context && (options &
> POS_NOT)`, invert: `retval = !retval` (NOT negation is applied here,
> per-reading). Then if `retval && context && !context->in_barrier`: set
> `retval = doesSetMatchCohort_testLinked(cohort, theset, context)` (chain the
> linked test); and if `options & POS_ATTACH_TO`: set
> `reading.matched_tests = retval`, and if still true and context_stack
> non-empty, record the attach-to target in `context_stack.back().attach_to`
> (`.cohort = &cohort`, `.reading = nullptr` [filled in later by
> doesSetMatchCohortNormal], `.subreading = &reading`). Rollback on failure:
> if `!retval && context && !FL_CAPTURE_UNIF && ST_CHILD_UNIFY && context_stack
> non-empty` AND the live unif_tags differs from the saved `utags` (different
> size or unequal contents), `swap` the saved `utags` back into
> `context_stack.back().unif_tags`; similarly if the live unif_sets size
> differs from saved `usets`, `swap` `usets` back. Finally, if `!retval` and
> context_stack non-empty, restore `context_stack.back().regexgrp_ct = orz`
> (discard capture groups produced by the failed attempt). Return `retval`.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
> bool GrammarApplicator::doesSetMatchCohortNormal(Cohort& cohort, const uint32_t set, dSMC_Context* context)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
> Normal (non-careful) cohort matching: the set matches the cohort if ANY
> eligible reading matches. `retval` starts false. Same early-out guard as the
> careful variant: if context present AND none of POS_LOOK_DELETED/DELAYED/
> IGNORED/POS_NOT set AND (`set >= cohort.possible_sets.size()` OR
> `!cohort.possible_sets.test(set)`), return false. Fetch `theset =
> grammar->sets_list[set]`. First, if `cohort.wread` (the merged
> "whole-cohort" reading) is present AND (no context OR not in a barrier),
> try `retval = doesSetMatchCohort_helper(cohort, *cohort.wread, *theset,
> context)`. If `retval` and (no context OR `context->did_test`), return early.
> Build the same 4-slot `lists` array as the careful variant (readings; plus
> deleted/delayed/ignored gated by the POS_LOOK_* options). Iterate lists in
> order, skipping nulls. For each reading: keep `reading_head = reading`; if
> `context && context->test`, replace `reading` with `get_sub_reading(reading,
> context->test->offset_sub)` and `continue` if null; skip via `continue` if
> reading inactive & POS_ACTIVE, or reading active & POS_INACTIVE. Call
> `doesSetMatchCohort_helper(cohort, *reading, *theset, context)`; if it
> returns true, set `retval = true` and — because the helper only knew the
> subreading — if the recorded attach_to points at this cohort and subreading,
> back-fill `context_stack.back().attach_to.reading = reading_head`. After each
> reading, if `retval` AND (no context OR the test has no linked test OR
> `context->did_test`), return `retval` immediately (first match wins in the
> common case; when there IS a linked test that hasn't been run, keep scanning).
> After all lists: if `context && !context->matched_target && (options &
> POS_NOT)`, set `retval = doesSetMatchCohort_testLinked(cohort, *theset,
> context)`. Then, possible_sets pruning: if `context && !context->matched_target
> && !(options & (POS_ACTIVE|POS_INACTIVE))`, and this set is not one of
> `grammar->sets_any` (checked via `!grammar->sets_any || set >= size ||
> !test(set)`): compute `was_sub = (context->test && context->test->offset_sub
> != 0)`; if `!was_sub && set < cohort.possible_sets.size()`, clear the bit
> `cohort.possible_sets.reset(set)` so future tests of this set on this cohort
> short-circuit. Return `retval`.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
> inline bool GrammarApplicator::doesSetMatchCohort_testLinked(Cohort& cohort, const Set& theset, dSMC_Context* context)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
> Runs the "linked" contextual test that follows the current one (the LINK
> chain), if any, and returns whether it matched. Locals: `retval = true`,
> `reset = false`, `linked = nullptr`, `min = max = nullptr`. Determine the
> linked test: if `context->test && context->test->linked`, use
> `context->test->linked`. Else if `tmpl_cntx.linked` is non-empty (template
> context has deferred links), save `min = tmpl_cntx.min`, `max = tmpl_cntx.max`,
> take `linked = tmpl_cntx.linked.back()`, `pop_back()` it, and set
> `reset = true`. If a `linked` test was found: if `!context->did_test`, run it
> once — if `linked->pos & POS_NO_PASS_ORIGIN`, call `runContextualTest(cohort.parent,
> cohort.local_number, linked, context->deep, &cohort)` (origin = this cohort),
> else `runContextualTest(cohort.parent, cohort.local_number, linked,
> context->deep, context->origin)` (origin = the inherited origin); store
> `context->matched_tests = (result != 0)`. Unless the set is `ST_CHILD_UNIFY`,
> set `context->did_test = true` (so it isn't re-run). Set `retval =
> context->matched_tests`. If `reset`, push `linked` back onto
> `tmpl_cntx.linked` (restore the template stack). If `!retval`, restore
> `tmpl_cntx.min = min; tmpl_cntx.max = max`. Return `retval`. When there is no
> linked test at all, `retval` remains its initial true.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn]
> bool GrammarApplicator::doesSetMatchReading(const Reading& reading, const uint32_t set, bool bypass_index, bool unif_mode)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn]
> Tests whether a reading matches a set (LIST or SET), evaluating set operators
> recursively, with a yes/no memo cache. Index cache: if `!bypass_index &&
> !unif_mode`, then if `index_readingSet_no[set]` contains `reading.hash`
> return false, and if `index_readingSet_yes[set]` contains `reading.hash`
> return true. `retval = false`. Fetch `theset = *grammar->sets_list[set]`.
> Dispatch on set type: (a) `ST_ANY` (the `(*)` set): `retval = true`. (b) else
> if `theset.sets` is empty it's a LIST set: `retval = doesSetMatchReading_tags(
> reading, theset, ((theset.type & ST_TAG_UNIFY) != 0) | unif_mode)` (the
> unif_mode argument is OR of the incoming flag and whether this set is
> tag-unify). (c) else if `theset.type & ST_SET_UNIFY` (`&&`-unified set):
> reference `usets = (*context_stack.back().unif_sets)[theset.number]`. If
> `usets` is empty (first evaluation): the single child `uset =
> *grammar->sets_list[theset.sets[0]]`; for each of its sub-sets `tset` (i in
> 0..uset.sets.size()), if `doesSetMatchReading(reading, tset.number,
> bypass_index, tagUnify|unif_mode)` insert `tset.number` into `usets`; set
> `retval = !usets.empty()`. If `usets` already populated (subsequent
> evaluations): get pooled `sets = ss_u32sv.get()`; for each `usi` in `usets`,
> if `doesSetMatchReading(reading, usi, bypass_index, unif_mode)` insert into
> `sets`; `retval = !sets->empty()`. (d) else it's a SET set: loop `i` over
> `theset.sets`; `match = doesSetMatchReading(reading, theset.sets[i],
> bypass_index, tagUnify|unif_mode)`, `failfast = false`. Then while `i <
> size-1` and `set_ops[i] != S_OR`, apply the operator (this lets non-OR
> operators bind tighter than OR): `S_PLUS` (`+`) — if `match`, replace `match`
> with the result of the next sub-set; `S_FAILFAST` (`^`) — if the next sub-set
> matches, set `match=false` and `failfast=true`; `S_MINUS` (`-`) — if `match`
> and the next sub-set matches, set `match=false`; any other op throws
> std::runtime_error("Set operator not implemented!"). Increment `i` inside the
> while. After the while: if `match`, `++match_sub`, `retval=true`, break; if
> `failfast`, `++match_sub`, `retval=false`, break. After the SET loop,
> propagate a unified tag: if (`unif_mode` OR `theset.type & ST_TAG_UNIFY`) and
> context_stack non-empty, look through `theset.sets` for the first entry
> present in `context_stack.back().unif_tags`; if found (`tag`), write that
> same tag pointer for every `theset.sets[i]` into `unif_tags`. Finally update
> the cache: if `retval`, insert `reading.hash` into `index_readingSet_yes[set]`;
> else, if `!(theset.type & ST_TAG_UNIFY) && !unif_mode`, insert into
> `index_readingSet_no[set]` (negative results from unify/tag-unify contexts
> are NOT cached). Return `retval`. NOTE: these indexes are periodically
> cleared elsewhere (every `((num_windows+4)*2+1)` windows) to bound memory.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-tags-fn]
> bool GrammarApplicator::doesSetMatchReading_tags(const Reading& reading, const Set& theset, bool unif_mode)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-tags-fn]
> Tests whether a reading matches a LIST set, i.e. whether the reading contains
> any full tag-combination (trie path) stored in the set. `retval = false`.
> Fail-fast pre-check: if `theset.ff_tags` (fail-fast `^tag` entries) is
> non-empty, for each `tag` in it call `doesTagMatchReading(reading, *tag,
> unif_mode)`; if any matches, return false immediately (a fail-fast tag being
> present disqualifies the reading). Main fast path (used ~80% of the time):
> if `theset.trie` is non-empty AND `reading.tags_plain` is non-empty, do a
> merge-style intersection of the two sorted stores. Initialize `iiter =
> theset.trie.lower_bound(single_tags[reading.tags_plain.front()])` and `oiter =
> reading.tags_plain.lower_bound(theset.trie.begin()->first->hash)`. While both
> iterators are in range: if `*oiter == iiter->first->hash` (a shared first
> tag): if that trie node is `terminal`, then in `unif_mode` require
> `check_unif_tags(theset.number, &*iiter)` (skip this node via `++iiter;
> continue;` if the unification tag conflicts), otherwise set `retval = true`
> and break; else if the node has a child trie and `doesSetMatchReading_trie(
> reading, theset, *child, unif_mode)` succeeds, set `retval = true` and break;
> then `++iiter`. Then advance whichever iterator lags: while `*oiter <
> iiter->first->hash` advance `oiter`; while `iiter->first->hash < *oiter`
> advance `iiter`. Second path: if still `!retval` and `theset.trie_special`
> (tags needing special/computed matching, e.g. regex, numeric, meta) is
> non-empty, set `retval = doesSetMatchReading_trie(reading, theset,
> theset.trie_special, unif_mode)`. Return `retval`.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-trie-fn]
> bool GrammarApplicator::doesSetMatchReading_trie(const Reading& reading, const Set& theset, const trie_t& trie, bool unif_mode)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-trie-fn]
> Recursive trie walk that tests whether a reading contains a complete tag
> path stored in `trie` (used for special/computed tags and as the recursion
> for tag combinations). For each key/value `kv` in `trie` (each `kv.first` is
> a Tag*, `kv.second` a trie_node_t): compute `match = (doesTagMatchReading(
> reading, *kv.first, unif_mode) != 0)`. If it matched: if the tag is
> `T_FAILFAST`, `continue` (a fail-fast tag matching does not by itself satisfy
> a path here — it just isn't a hit); if `kv.second.terminal` (end of a stored
> combination): in `unif_mode`, require `check_unif_tags(theset.number, &kv)` —
> if it returns false (unification conflict) `continue`; otherwise return true.
> If not terminal but the node has a child trie and
> `doesSetMatchReading_trie(reading, theset, *kv.second.trie, unif_mode)`
> returns true, return true. If nothing returns true after the whole loop,
> return false. `theset` is threaded through only for the `theset.number` used
> by `check_unif_tags`.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-icase-fn]
> uint32_t GrammarApplicator::doesTagMatchIcase(uint32_t test, const Tag& tag, bool bypass_index)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-icase-fn]
> Case-insensitively tests whether an input tag (`test`, a tag hash) equals a
> case-insensitive pattern tag (`tag`), with a yes/no memo cache. `match = 0`.
> Build 64-bit key `ih = (UI64(tag.hash) << 32) | test`. If `!bypass_index` and
> `index_icase_no` contains `ih`, return 0. Else if `!bypass_index` and
> `index_icase_yes` contains `ih`, return `test`. Otherwise look up the input
> tag `itag = *grammar->single_tags.find(test)->second` and compare with
> `ux_strCaseCompare(tag.tag, itag.tag)` — a full-string case-fold comparison
> using ICU `u_strCaseCompare` with `U_FOLD_CASE_DEFAULT` (returns true iff the
> two strings are equal ignoring case; on ICU error it throws
> std::runtime_error). If equal, `match = itag.hash`. Then cache: if `match`
> insert `ih` into `index_icase_yes`, else into `index_icase_no`. Return
> `match` (`itag.hash` or 0). PORT parity: use Unicode default case folding
> (full, not simple), matching ICU's `U_FOLD_CASE_DEFAULT`, comparing the
> entire strings for equality (not a substring/regex match).

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn]
> uint32_t GrammarApplicator::doesTagMatchReading(const Reading& reading, const Tag& tag, bool unif_mode, bool bypass_index)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn]
> The central "does this single tag match this reading" dispatcher. `retval =
> 0`, `match = 0`. A cascade of mutually-exclusive branches selected by
> `tag.type` (first matching branch wins):
> (1) `!(tag.type & T_SPECIAL) || (tag.type & T_FAILFAST)` — plain/raw tag (and
> fail-fast tags): let `ite = reading.tags_plain.end()`. Compute `raw_in =
> reading.tags_plain_bloom.matches(tag.hash)`. If `T_FAILFAST`: look up
> `reading.tags_plain.find(tag.plain_hash)` (fail-fast tags key off the
> underlying tag's plain_hash) and set `raw_in` = found. Else if `raw_in` (bloom
> filter says maybe): confirm with `reading.tags_plain.find(tag.hash)`, set
> `raw_in` = found. If `raw_in`, `match = tag.hash`. (When bloom says no and not
> fail-fast, no lookup happens and match stays 0.)
> (2) `T_SET` — inline set reference: `sh = hash_value(tag.tag)`; `sh =
> grammar->sets_by_name.find(sh)->second`; `match = doesSetMatchReading(reading,
> sh, bypass_index, unif_mode)`.
> (3) `T_VARSTRING` — `nt = generateVarstringTag(&tag)`; `match =
> doesTagMatchReading(reading, *nt, unif_mode, bypass_index)` (recurse on the
> generated concrete tag).
> (4) `T_META` — if `tag.regexp` and `reading.parent->text` (the cohort's raw
> text) is non-empty: `uregex_setText(tag.regexp, reading.parent->text ...)`
> (ICU error → "uregex_setText(MatchSet)" diagnostic + CG3Quit(1)); reset
> status; `uregex_find(tag.regexp, -1)` (unanchored over the whole text) → on
> true `match = tag.hash`; ICU error on find → diagnostic + CG3Quit(1); if
> matched and `gc = uregex_groupCount > 0` and context has regexgrps, call
> `captureRegex(...)`.
> (5) `tag.regexp` (regular regexp tag) — `match = doesRegexpMatchReading(
> reading, tag, bypass_index)`.
> (6) `T_CASE_INSENSITIVE` — loop `reading.tags_textual`, `match =
> doesTagMatchIcase(mter, tag, bypass_index)`, break on first non-zero.
> (7) `T_REGEXP_ANY` (the `<.*>`/`".*"`/`"<.*>"` any-forms): if `T_BASEFORM`,
> `match = reading.baseform`, and in unif_mode enforce against
> `unif_last_baseform` (if already set and differs, `match = 0`; else record
> it). Else if `T_WORDFORM`, `match = reading.parent->wordform->hash`, unif via
> `unif_last_wordform`. Else loop `reading.tags_textual`: for each `mter`,
> `itag = single_tags[mter]`; if `itag` is not baseform/wordform, `match =
> itag.hash` and unif via `unif_last_textual`; break on first `match`.
> (8) `T_NUMERICAL` — loop `reading.tags_numerical` (map of hash→Tag*); for each
> `itag`, `rv = test_tag_numerical(reading, tag, *itag)`; if `rv` non-zero set
> `match = rv` (no break; the LAST matching numerical tag wins).
> (9) `T_VARIABLE | T_LOCAL_VARIABLE` — `match = 0`. Choose `vars` = `variables`
> normally, but `reading.parent->parent->variables_set` (the window's local
> vars) when the reading's window is not the current window AND the tag is
> `T_LOCAL_VARIABLE`. Find key tag `key = single_tags[tag.comparison_hash]` (the
> variable name). If `key` is `T_REGEXP`, `find_if` over `vars` matching via
> `doesTagMatchRegexp(kv.first, *key)`; if `T_CASE_INSENSITIVE`, via
> `doesTagMatchIcase`; else `vars.find(tag.comparison_hash)`. If a var was
> found: if `tag.variable_hash == 0` (existence test), `match = tag.hash`;
> otherwise compare the variable's stored value `it->second` to `comp =
> single_tags[tag.variable_hash]` — if `comp` is T_REGEXP use
> `doesTagMatchRegexp`, if T_CASE_INSENSITIVE use `doesTagMatchIcase`, else
> require `comp->hash == it->second`; on success `match = tag.hash`.
> (10) `T_PAR_LEFT` — if `par_left_tag` set AND `reading.parent->local_number ==
> par_left_pos` AND `reading.tags` contains `par_left_tag`, `match =
> grammar->tag_any`. (11) `T_PAR_RIGHT` — symmetric with par_right.
> (12) `T_ENCL` — find the cohort right after `reading.parent` in
> `sw->all_cohorts` (starting the search at `begin + local_number`, then `++c`);
> if it exists and `(*c)->enclosed`, `match = true` (1).
> (13) `T_TARGET` — if `rule_target && reading.parent == rule_target`, `match =
> tag_any`. (14) `T_MARK` — if `reading.parent == get_mark()`, `match = tag_any`.
> (15) `T_ATTACHTO` — if `reading.parent == get_attach_to().cohort`, `match =
> tag_any`. (16) `T_SAME_BASIC` — if `reading.hash_plain == same_basic`,
> `match = tag_any`. (17) `T_CONTEXT` — if `context_stack.size() > 1`, take
> `list = context_stack[size-2].context`; if `tag.context_ref_pos <=
> list.size()` and `reading.parent == list[tag.context_ref_pos-1]` (1-based
> position), `match = tag_any`.
> Finally: if `match` is non-zero, `++match_single` and `retval = match`.
> Return `retval`. Regex parity note: T_META regex runs against the cohort's
> secondary/parenthetical text (`reading.parent->text`), unanchored via
> `uregex_find(-1)`; the compiled pattern's own anchors (`/.../` = none,
> otherwise `^..$`) and its `UREGEX_CASE_INSENSITIVE` flag govern matching.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-regexp-fn]
> uint32_t GrammarApplicator::doesTagMatchRegexp(uint32_t test, const Tag& tag, bool bypass_index)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-regexp-fn]
> Tests whether one input tag (`test`, a tag hash) matches a regexp pattern
> tag (`tag`), with a yes/no memo cache and optional capture harvesting.
> Compute `gc = uregex_groupCount(tag.regexp)`. `match = 0`. Build key `ih =
> (UI64(tag.hash) << 32) | test`. If `!bypass_index` and `index_regexp_no`
> contains `ih`, return 0. Else if `!bypass_index` and `gc == 0` and
> `index_regexp_yes` contains `ih`, return `test` (positive results are cached
> only when the pattern has no capture groups). Otherwise: look up `itag =
> *grammar->single_tags.find(test)->second`; set the regex subject to the input
> tag's text via `uregex_setText(tag.regexp, itag.tag.data(), itag.tag.size(),
> &status)` — on ICU error print "uregex_setText(MatchTag)" diagnostic (tag
> text + numLines) and `CG3Quit(1)`. Reset status; call `uregex_find(tag.regexp,
> -1, &status)` — start index `-1` = UNANCHORED search over the whole input tag
> text; on true set `match = itag.hash`; ICU error → "uregex_find(MatchTag)"
> diagnostic + `CG3Quit(1)`. If matched: when `gc > 0` AND context_stack
> non-empty AND `context_stack.back().regexgrps != 0`, call `captureRegex(gc,
> context_stack.back().regexgrp_ct, context_stack.back().regexgrps, tag)` and do
> NOT cache; otherwise insert `ih` into `index_regexp_yes`. If not matched,
> insert `ih` into `index_regexp_no`. Return `match` (`itag.hash` or 0).
> Regex parity (make-or-break): `tag.regexp` is the tag's compiled
> `URegularExpression`. Its pattern source is: for a `/.../` tag, the inner text
> between the slashes with NO added anchors (so it matches anywhere in the
> subject); for any other regexp tag, the tag text wrapped as `^`+text+`$`
> (fully anchored to the whole subject). It is compiled with
> `UREGEX_CASE_INSENSITIVE` iff the tag carries T_CASE_INSENSITIVE, else no
> flags. `uregex_find(-1)` performs an unanchored ICU search; a Rust port must
> replicate ICU semantics (Unicode-aware; `.` excludes line terminators; `^`/`$`
> at input boundaries unless the pattern opts into multiline) and preserve the
> anchoring/flag decisions above so identical patterns match identically.

> [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.get-tags-matching-fn]
> void GrammarApplicator::getTagsMatching(const Reading& reading, TagList& theTags, TagList& rvTags)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.get-tags-matching-fn]
> Collects, into `rvTags`, the reading's own tags that are matched by any of the
> pattern tags in `theTags`. Double loop: for each pattern tag `tag` in
> `theTags`, for each tag hash `tt` in `reading.tags_list` (all of the reading's
> tags, in order), let `itag = single_tags[tt]` and compute `match = 0` by the
> first applicable rule: if `tag.regexp`, `match = doesTagMatchRegexp(tt, tag)`
> (default bypass_index=false); else if `tag` is `T_CASE_INSENSITIVE`, `match =
> doesTagMatchIcase(tt, tag)`; else if `tag` is `T_REGEXP_ANY` AND `itag` is
> `T_TEXTUAL`: if `tag` is `T_BASEFORM` and `itag` is `T_BASEFORM` set `match =
> reading.baseform`; else if `tag` is `T_WORDFORM` and `itag` is `T_WORDFORM`
> set `match = reading.parent->wordform->hash`; else if `itag` is neither
> baseform nor wordform, match only when both first characters agree in kind —
> `(tag.tag[0]=='"' && itag->tag[0]=='"') || (tag.tag[0]=='<' && itag->tag[0]==
> '<')` — then `match = itag->hash`; else if `tag` is `T_NUMERICAL` AND `itag`
> is `T_NUMERICAL`, `match = test_tag_numerical(reading, tag, *itag)`; else
> (plain) if `tag.hash == itag->hash`, `match = itag->hash`. If `match` is
> non-zero, push `itag` (the reading's own Tag*) onto `rvTags`. No return value;
> `rvTags` accumulates (is appended to, not cleared). Duplicates are possible if
> multiple pattern tags match the same reading tag.

> [spec:cg3:def:grammar-applicator-match-set.cg3.tag-set-subset-of-t-set-fn]
> inline bool TagSet_SubsetOf_TSet(const TagSortedVector& a, const T& b)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.tag-set-subset-of-t-set-fn]
> Template predicate: returns true iff every tag in the sorted set `a`
> (`TagSortedVector`, tags of a set) is present, by hash, in `b` (a sorted
> container of tag hashes, e.g. a reading's tags). Both `a` (as `ai->hash`) and
> `b` (as raw hashes) are sorted ascending, enabling a merge scan. Initialize
> `bi = b.lower_bound((*a.begin())->hash)` (first element of `b` not less than
> the first tag's hash). For each `ai` in `a`: advance `bi` while `bi != b.end()`
> and `*bi < ai->hash`; then if `bi == b.end()` or `*bi != ai->hash`, return
> false (that tag of `a` is missing from `b`). If the loop completes, return
> true. (The commented-out size short-circuit is disabled and not part of
> behavior.) EDGE: assumes `a` is non-empty — it dereferences `a.begin()`
> unconditionally; callers only invoke it with non-empty `a`.

> [spec:cg3:def:grammar-applicator-match-set.cg3.test-tag-numerical-fn]
> uint32_t test_tag_numerical(const Reading& reading, const Tag& tag, const Tag& itag)

> [spec:cg3:sem:grammar-applicator-match-set.cg3.test-tag-numerical-fn]
> Compares a query numeric tag (`tag`) against a reading's numeric tag (`itag`)
> and returns `itag.hash` on a match, else 0. `match = 0`. Proceed only if
> `tag.comparison_hash == itag.comparison_hash` (same numeric key/name); else
> return 0. Establish `compval` (the query's left-hand value): start `compval =
> tag.comparison_val`; then if `(tag.type & T_NUMERIC_MATH) && tag.comparison_offset`,
> evaluate a math expression — construct `MathParser mp(reading.parent->getMin(
> tag.comparison_hash), reading.parent->getMax(tag.comparison_hash))` (min/max
> of that key across the cohort's readings), take `tag.tag`, drop its first
> `comparison_offset` code units and its last 1 code unit, and set `compval =
> mp.eval(expr)`; else if `compval <= NUMERIC_MIN` (the "MIN" sentinel, =
> -(2^48)), set `compval = reading.parent->getMin(tag.comparison_hash)`; else if
> `compval >= NUMERIC_MAX` (the "MAX" sentinel, = 2^48-1), set `compval =
> reading.parent->getMax(tag.comparison_hash)`. Let A = `tag.comparison_op`, B =
> `itag.comparison_op`, V = `itag.comparison_val`. Then an ordered if/else-if
> cascade — the FIRST branch whose condition holds sets `match = itag.hash`
> (evaluate exactly in this order; ops are OP_EQUALS/NOTEQUALS/LESSTHAN/
> LESSEQUALS/GREATERTHAN/GREATEREQUALS):
> 1. A=EQ,B=EQ, compval==V; 2. A=NEQ,B=EQ, compval!=V; 3. A=EQ,B=NEQ, compval!=V;
> 4. A=NEQ,B=NEQ, compval==V; 5. A=EQ,B=LT, compval<V; 6. A=EQ,B=LE, compval<=V;
> 7. A=EQ,B=GT, compval>V; 8. A=EQ,B=GE, compval>=V; 9. A=NEQ,B=LT (uncond);
> 10. A=NEQ,B=LE (uncond); 11. A=NEQ,B=GT (uncond); 12. A=NEQ,B=GE (uncond);
> 13. A=LT,B=NEQ (uncond); 14. A=LE,B=NEQ (uncond); 15. A=GT,B=NEQ (uncond);
> 16. A=GE,B=NEQ (uncond); 17. A=LT,B=EQ, compval>V; 18. A=LE,B=EQ, compval>=V;
> 19. A=LT,B=LT (uncond); 20. A=LE,B=LE (uncond); 21. A=LE,B=LT (uncond);
> 22. A=LT,B=LE (uncond); 23. A=LT,B=GT, compval>V; 24. A=LT,B=GE, compval>V;
> 25. A=LE,B=GT, compval>V; 26. A=LE,B=GE, compval>=V; 27. A=GT,B=EQ, compval<V;
> 28. A=GE,B=EQ, compval<=V; 29. A=GT,B=GT (uncond); 30. A=GE,B=GE (uncond);
> 31. A=GE,B=GT (uncond); 32. A=GT,B=GE (uncond); 33. A=GT,B=LT, compval<V;
> 34. A=GT,B=LE, compval<V; 35. A=GE,B=LT, compval<V; 36. A=GE,B=LE, compval<=V.
> Any op combination not listed (or whose value condition fails) leaves `match`
> at 0. Return `match`. NOTE the roles: `compval` derives from the query tag
> (`tag`), the threshold `V` and the operator `B` from the reading's tag
> (`itag`); getMin/getMax force a recompute of the cohort's per-key min/max
> (they clear/repopulate `num_min`/`num_max` unless CT_NUM_CURRENT is set).

