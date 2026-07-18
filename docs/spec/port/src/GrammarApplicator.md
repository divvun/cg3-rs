# src/GrammarApplicator.cpp, src/GrammarApplicator.hpp

> [spec:cg3:def:grammar-applicator.cg3.d-smc-context]
> struct dSMC_Context {
>   const ContextualTest* test = nullptr;
>   Cohort** deep = nullptr;
>   Cohort* origin = nullptr;
>   uint64_t options = 0;
>   bool did_test = false;
>   bool matched_target = false;
>   bool matched_tests = false;
>   bool in_barrier = false;
> }

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator+2]
> class GrammarApplicator {
>   bool always_span = false;
>   bool apply_mappings = true;
>   bool apply_corrections = true;
>   bool no_before_sections = false;
>   bool no_sections = false;
>   bool no_after_sections = false;
>   bool trace = false;
>   bool trace_name_only = false;
>   bool trace_no_removed = false;
>   bool trace_encl = false;
>   bool allow_magic_readings = true;
>   bool no_pass_origin = false;
>   bool unsafe = false;
>   bool ordered = false;
>   bool show_end_tags = false;
>   bool unicode_tags = false;
>   bool unique_tags = false;
>   bool dry_run = false;
>   bool input_eof = false;
>   bool seen_barrier = false;
>   bool is_conv = false;
>   bool split_mappings = false;
>   bool pipe_deleted = false;
>   bool add_spacing = true;
>   bool print_ids = false;
>   cg3_sformat fmt_input = CG3SF_CG;
>   cg3_sformat fmt_output = CG3SF_CG;
>   bool dep_has_spanned = false;
>   uint32_t dep_delimit = 0;
>   bool dep_absolute = false;
>   bool dep_original = false;
>   bool dep_block_loops = true;
>   bool dep_block_crossing = false;
>   uint32_t num_windows = 2;
>   uint32_t soft_limit = 300;
>   uint32_t hard_limit = 500;
>   uint32Vector sections;
>   uint32IntervalVector valid_rules;
>   uint32IntervalVector trace_rules;
>   uint32IntervalVector debug_rules;
>   uint32FlatHashMap variables;
>   uint32_t verbosity_level = 0;
>   uint32_t debug_level = 0;
>   uint32_t section_max_count = 0;
>   bool has_dep = false;
>   bool parse_dep = false;
>   uint32_t dep_highest_seen = 0;
>   std::unique_ptr<Window> gWindow;
>   bool has_relations = false;
>   Grammar* grammar = nullptr;
>   Profiler* profiler = nullptr;
>   std::istream* ux_stdin = nullptr;
>   std::ostream* ux_stdout = nullptr;
>   std::ostream* ux_stderr = nullptr;
>   UString span_pattern_latin;
>   UString span_pattern_utf;
>   UChar ws[4]{ ' ', '\t', 0, 0 };
>   uint32_t numLines = 0;
>   uint32_t numWindows = 0;
>   uint32_t numCohorts = 0;
>   uint32_t numReadings = 0;
>   bool did_index = false;
>   sorted_vector<std::pair<uint32_t, uint32_t>> dep_deep_seen;
>   uint32_t numsections = 0;
>   RSType runsections;
>   externals_t externals;
>   uint32Vector ci_depths;
>   std::map<uint32_t, CohortIterator> cohortIterators;
>   std::map<uint32_t, TopologyLeftIter> topologyLeftIters;
>   std::map<uint32_t, TopologyRightIter> topologyRightIters;
>   std::map<uint32_t, DepParentIter> depParentIters;
>   std::map<uint32_t, DepDescendentIter> depDescendentIters;
>   std::map<uint32_t, DepAncestorIter> depAncestorIters;
>   uint32_t match_single = 0, match_comp = 0, match_sub = 0;
>   uint32_t begintag = 0, endtag = 0, substtag = 0;
>   Tag *tag_begin = nullptr;
>   uint32_t par_left_tag = 0, par_right_tag = 0;
>   uint32_t par_left_pos = 0, par_right_pos = 0;
>   bool did_final_enclosure = false;
>   uint32_t mprefix_key = 0, mprefix_value = 0;
>   tmpl_context_t tmpl_cntx;
>   std::vector<regexgrps_t> regexgrps_store;
>   bc::flat_map<uint32_t, uint8_t> regexgrps_z;
>   bc::flat_map<uint32_t, regexgrps_t*> regexgrps_c;
>   uint32_t same_basic = 0;
>   Cohort* rule_target = nullptr;
>   Cohort* merge_with = nullptr;
>   Rule* current_rule = nullptr;
>   std::vector<Rule_Context> context_stack;
>   std::vector<CohortSet*> cohortsets;
>   std::vector<size_t*> rocits;
>   readings_plain_t readings_plain;
>   std::vector<URegularExpression*> text_delimiters;
>   bc::flat_map<uint32_t, unif_tags_t*> unif_tags_rs;
>   std::vector<unif_tags_t> unif_tags_store;
>   bc::flat_map<uint32_t, unif_sets_t*> unif_sets_rs;
>   std::vector<unif_sets_t> unif_sets_store;
>   uint32_t unif_last_wordform = 0;
>   uint32_t unif_last_baseform = 0;
>   uint32_t unif_last_textual = 0;
>   bc::flat_map<uint32_t, uint32_t> rule_hits;
>   scoped_stack<unif_tags_t> ss_utags;
>   scoped_stack<unif_sets_t> ss_usets;
>   scoped_stack<uint32SortedVector> ss_u32sv;
>   uint64FlatHashSet index_regexp_yes;
>   uint64FlatHashSet index_regexp_no;
>   uint64FlatHashSet index_icase_yes;
>   uint64FlatHashSet index_icase_no;
>   std::vector<uint32FlatHashSet> index_readingSet_yes;
>   std::vector<uint32FlatHashSet> index_readingSet_no;
>   uint32FlatHashSet index_ruleCohort_no;
>   bool reset_cohorts_for_loop = false;
>   bool finish_reading_loop = true;
>   bool finish_cohort_loop = true;
>   bool in_nested = false;
>   size_t used_regex = 0;
>   enum ST_RETVALS { TRV_BREAK = (1 << 0), TRV_BARRIER = (1 << 1), TRV_BREAK_DEFAULT = (1 << 2), };
>   std::deque<Reading> subs_any;
> }
>
> Deliberate de-warting (v1): the `owns_grammar`, `filebase`, `tag_end`,
> `tag_subst`, `context_target`, and `ss_taglist` members are dropped from the
> port. `owns_grammar` is vacuous under Rust value ownership (the grammar is
> owned by value and dropped in the destructor); `filebase` is never read (the
> `filebase()` accessor is a constant `""`); `tag_end`/`tag_subst` are redundant
> `TagId` caches (the run path routes through the `endtag`/`substtag` hash
> forms, and `tag_begin` is retained because it *is* read); `context_target` is
> write-only in the C++ original too (five writers, zero readers) — its would-be
> attach-diagnostics reader is unported; `ss_taglist` is an unexercised pool
> (no method in the ported call graph touches it). Each is regenerated if a
> future parity path needs it.
>
> Stage-B re-homing (v2): the options-derived, setup-written, run-read-only
> "cfg" members are extracted, unchanged in semantics, into a new `EngineConfig`
> value held as the first member of `GrammarApplicator` (`self.cfg`). This is a
> pure field re-homing — no C++ analog as a type, no signature or logic change;
> the members map 1:1 (same names, types, defaults, and per-field C++ reference
> comments). The re-homed members are: `always_span`, `apply_mappings`,
> `apply_corrections`, `no_before_sections`, `no_sections`, `no_after_sections`,
> `trace`, `trace_name_only`, `trace_no_removed`, `trace_encl`,
> `allow_magic_readings`, `no_pass_origin`, `unsafe`, `ordered`, `show_end_tags`,
> `unicode_tags`, `unique_tags`, `dry_run`, `is_conv`, `split_mappings`,
> `pipe_deleted`, `add_spacing`, `print_ids`, `fmt_input`, `fmt_output`,
> `dep_delimit`, `dep_absolute`, `dep_original`, `dep_block_loops`,
> `dep_block_crossing`, `num_windows`, `soft_limit`, `hard_limit`, `sections`,
> `valid_rules`, `trace_rules`, `debug_rules`, `verbosity_level`, `debug_level`,
> `section_max_count`, `parse_dep`, `span_pattern_latin`, `span_pattern_utf`,
> `ws`, `did_index`, `numsections`, `runsections`, `begintag`, `endtag`,
> `substtag`, `tag_begin`, `mprefix_key`, `mprefix_value`, and `text_delimiters`
> (54 members). The run-mutable, document-lifetime, scratch, and diagnostics
> members (e.g. `input_eof`, `variables`, `has_dep`, `numLines`, `match_single`,
> `context_stack`) remain flat on `GrammarApplicator`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-profiling-example-fn]
> void addProfilingExample(T& item)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-profiling-example-fn+1]
> Template helper (only invoked when a profiler exists). Renders a text
> snapshot of the entire in-flight window set and records its string-pool
> offset on `item`. Steps: take a reference to `profiler->buf` (a
> std::stringstream). The rendering must run with `trace` force-disabled: the
> C++ constructs a scoped `swapper<bool>(true, trace, ttrace=false)` that swaps
> the shared `trace` member with a false local for the duration and restores it
> on return. The port removes that mutation — `trace` is immutable config — by
> threading the effective trace flag `false` as an explicit argument down the
> `printSingleWindow` → `printCohort` → `printReading` chain; the shared `trace`
> field is never written. Behavior (the emitted bytes) is identical; only the
> mechanism changed. Reset buf (`buf.str("")`,
> `buf.clear()`). Write the literal line `# PREVIOUS WINDOWS\n`, then for
> every SingleWindow `s` in `gWindow->previous` call
> `printSingleWindow(s, buf, true)` (profiling=true). Write
> `# CURRENT WINDOW\n` and `printSingleWindow(gWindow->current, buf, true)`.
> Write `# NEXT WINDOWS\n` and `printSingleWindow(s, buf, true)` for each `s`
> in `gWindow->next`. Finally `sz = profiler->addString(buf.str())` interns
> the rendered snapshot and `item.example_window = sz` stores the returned
> offset. Returns nothing.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-tag-fn]
> Tag* GrammarApplicator::addTag(const UChar* txt, uint32_t type)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-fn]
> Interns a tag from a UChar* string and returns the canonical Tag*. First a
> fast path: compute `thash = hash_value(txt)`, look it up in
> `grammar->single_tags`; if found and that Tag's `tag` is non-empty and equals
> `txt` exactly, return it immediately. Otherwise build a Tag: if `type` has the
> `T_VARSTRING` bit, `tag = CG3::parseTag(txt, 0, *this, unescape)` where
> `unescape = ((type & T_PRESERVE_ESC) == 0)`; else allocate a fresh `Tag`, call
> `tag->parseTagRaw(txt, grammar)`, then `tag = addTag(tag)` (the Tag* overload,
> which rehashes with a seed loop up to 10000 to resolve hash collisions,
> deleting the temporary and reusing the stored Tag when an equal tag already
> exists, otherwise inserting into `grammar->single_tags`). Then run a one-time
> "textual" marking pass with a local `reflow=false`: (a) if the new tag is
> `T_REGEXP` and `!is_textual(tag->tag)`, insert `tag->regexp` into
> `grammar->regex_tags`; only if that insert was new, iterate every entry of
> `grammar->single_tags`, skip those already `T_TEXTUAL`, and for each remaining
> tag test it against every regex in `grammar->regex_tags` by
> `uregex_setText(regex, titer.tag.data(), size, &status)` then
> `uregex_find(regex, -1, &status)` (unanchored, matches anywhere); on any match
> set that tag's `type |= T_TEXTUAL` and `reflow=true`. (b) if the new tag is
> `T_CASE_INSENSITIVE` and `!is_textual`, insert it into `grammar->icase_tags`;
> if new, iterate all non-textual single_tags and for each icase tag compare via
> `ux_strCaseCompare(titer.tag, iter.tag)` (full-string case-insensitive equal);
> on match set `T_TEXTUAL` and `reflow=true`. If `reflow` is true, call
> `reflowTextuals()`. Return `tag`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn]
> uint32_t addTagToReading(Reading& reading, uint32_t tag, bool rehash = true)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn]
> Adds a tag to a reading and updates all derived structures; returns the (final)
> tag hash. The uint32 overload resolves the hash to a Tag* and forwards. Steps
> (Tag* overload): if `tag` is T_VARSTRING, replace it with
> `generateVarstringTag(tag)`. Look up `tag->hash` in `grammar->sets_by_tag`; if
> present, grow `reading.parent->possible_sets` to at least that bitset's size and
> OR it in. Insert `tag->hash` into `reading.tags`, push onto `reading.tags_list`,
> insert into `reading.tags_bloom`. If `ordered`, append the tag string to
> `reading.tags_string` (space-separated) and recompute
> `reading.tags_string_hash = hash_value(tags_string)`. If the hash is a
> parenthesis-open tag, set `parent->is_pleft`; if a parenthesis-close tag, set
> `parent->is_pright`. If the tag is T_MAPPING or its first char equals
> `grammar->mapping_prefix`: mark it T_MAPPING, and if `reading.mapping` is already
> a different tag, print an error and `CG3Quit(1)`; set `reading.mapping = tag`.
> If tag has any of T_TEXTUAL|T_WORDFORM|T_BASEFORM, insert into `tags_textual`
> and `tags_textual_bloom`. If T_NUMERICAL, store into `tags_numerical[hash]` and
> clear `CT_NUM_CURRENT` on the parent. If no baseform yet and tag is T_BASEFORM,
> set `reading.baseform = hash`. If `parse_dep && T_DEPENDENCY && parent not
> CT_DEP_DONE`: set `parent.dep_self = tag->dep_self`, `parent.dep_parent =
> tag->dep_parent`, and if they are equal set `dep_parent = DEP_NO_PARENT`; set
> `has_dep = true`. If `grammar->has_relations && T_RELATION`: if `tag->dep_parent
> && tag->comparison_hash` add `parent.relations_input[comparison_hash].insert(
> dep_parent)`; if `tag->dep_self` set `gWindow->relation_map[dep_self] =
> parent.global_number`; set `has_relations = true` and `parent.setRelated()`. If
> the tag is not T_SPECIAL, insert into `tags_plain` and `tags_plain_bloom`. If
> `rehash`, call `reading.rehash()`. If `grammar->has_bag_of_tags`, mirror the
> same tag/textual/numerical/baseform/plain insertions (and optional rehash) into
> the window-level `parent->parent->bag_of_tags` reading. Return `tag->hash`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.all-mappings-t]
> typedef std::map<Reading*, TagList> all_mappings_t

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.attach-parent-child-fn]
> bool attachParentChild(Cohort& parent, Cohort& child, bool allowloop = false, bool allowcrossing = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.attach-parent-child-fn]
> Attaches `child` under `parent` in the dependency tree, with loop/crossing
> guards. Set `parent.dep_self = parent.global_number` and `child.dep_self =
> child.global_number`. If `!allowloop && dep_block_loops &&
> wouldParentChildLoop(&parent, &child)`, warn (if verbose) and return false. If
> `!allowcrossing && dep_block_crossing && wouldParentChildCross(&parent, &child)`,
> warn (if verbose) and return false. If `child.dep_parent == DEP_NO_PARENT`, set
> it to `child.dep_self`. Look up the child's current `dep_parent` in
> `gWindow->cohort_map`; if found, call `remChild(child.dep_self)` on it (detach
> from old parent). Set `child.dep_parent = parent.global_number` and
> `parent.addChild(child.global_number)`. OR `CT_DEP_DONE` into both cohorts'
> `type`. If `!dep_has_spanned && child.parent != parent.parent` (dependency
> crosses a window boundary), print an Info message and set `dep_has_spanned =
> true` (enumeration becomes global from here on). Return true.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.check-unif-tags-fn]
> bool check_unif_tags(uint32_t set, const void* val)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.check-unif-tags-fn]
> Enforces $$-unification consistency for a set within the current context. If
> `context_stack` is empty, return false. Take `unif_tags =
> *context_stack.back().unif_tags`. Look up `set` in it: if present, return
> whether the stored value pointer equals `val` (i.e. same tag/trie matched as
> before). If absent, record `unif_tags[set] = val` and return true (first
> observation always accepted). `val` is compared by pointer identity only.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.del-tag-from-reading-fn]
> void delTagFromReading(Reading& reading, uint32_t tag)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.del-tag-from-reading-fn]
> Removes a tag (by hash) from a reading. The Tag* overload forwards to the
> uint32 overload. Steps: `erase(reading.tags_list, utag)` (remove from the
> ordered list), then `.erase(utag)` from `reading.tags`, `tags_textual`,
> `tags_numerical`, and `tags_plain`. If `reading.mapping` is set and its hash
> equals `utag`, set `reading.mapping = nullptr`. If `utag == reading.baseform`,
> set `reading.baseform = 0`. Call `reading.rehash()` and clear `CT_NUM_CURRENT`
> on the parent cohort. No return. (Note: it does not touch the bloom filters or
> the bag_of_tags; those stay stale until a full reflow.)

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.delimit-at-fn]
> Cohort* delimitAt(SingleWindow& current, Cohort* cohort)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.delimit-at-fn]
> Splits `current` window into two at `cohort`, moving everything after `cohort`
> into a new following window. Allocate the new window `nwin`: if `current` is the
> parent's current window, `allocPushSingleWindow`; otherwise find `current`
> within the parent's `next` (insert `nwin` after it) or `previous` (insert
> before), then `rebuildSingleWindowLinks`. Assert nwin != null. Swap
> `flush_after` and `text_post` from `current` into `nwin`, and copy
> `has_enclosures`. Create a fresh initial `>>>` cohort in `nwin` (global_number
> from `cohort_counter++`, wordform `tag_begin`, one reading with baseform
> `begintag`, sets_any marked, begintag added), and append it. Then, starting just
> after `cohort` (using its `local_number`), move each following cohort in
> `current.all_cohorts` to `nwin`: reparent it, and either push to
> `all_cohorts` only (if CT_ENCLOSED|CT_REMOVED|CT_IGNORED) or `appendCohort`.
> Erase the moved cohorts from `current.cohorts` (from `lc+1`) and
> `current.all_cohorts` (from the found position). Set `cohort =
> current.cohorts.back()` and add the `endtag` to each of its readings. Call
> `gWindow->rebuildCohortLinks()`. Return the new last cohort of `current`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-regexp-match-line-fn]
> uint32_t doesRegexpMatchLine(const Reading& reading, const Tag& tag, bool bypass_index = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-regexp-match-line-fn]
> Matches a regex tag against a reading's whole tag-line string (`ordered`-mode
> `<...>r` line regexes). `gc = uregex_groupCount(tag.regexp)`. Index key here is
> keyed differently: `ih = (uint64(reading.tags_string_hash) << 32) | tag.hash`
> (reading hash high, regex tag hash low). Unless `bypass_index`: if
> `index_regexp_no` contains `ih`, match=0; else if `gc==0` and
> `index_regexp_yes` contains `ih`, match=`reading.tags_string_hash`. Otherwise
> `uregex_setText(tag.regexp, reading.tags_string.data(), size, &status)`
> (error->quit), `uregex_find(tag.regexp, -1, &status)` (unanchored, whole
> string; error->quit); on match set match=`reading.tags_string_hash`. If matched
> and `gc>0` and context has `regexgrps`, `captureRegex(...)`; else if matched
> insert into `index_regexp_yes`; if not matched insert into `index_regexp_no`.
> Return match. Same unanchored/gc-quirk parity notes as doesTagMatchRegexp.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-regexp-match-reading-fn]
> uint32_t doesRegexpMatchReading(const Reading& reading, const Tag& tag, bool bypass_index = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-regexp-match-reading-fn]
> Matches a regex tag against a reading by trying each of the reading's textual
> tags. If `tag.type & T_REGEXP_LINE`, delegate to `doesRegexpMatchLine(reading,
> tag, bypass_index)` and return that. Otherwise iterate `reading.tags_textual`
> (the set of tags pre-marked T_TEXTUAL by Grammar::reindex/addTag) and for each
> call `doesTagMatchRegexp(mter, tag, bypass_index)`; return the first non-zero
> result (breaks on first match). Returns 0 if none match.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
> bool doesSetMatchCohortCareful(Cohort& cohort, const uint32_t set, dSMC_Context* context = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
> Tests whether ALL readings of a cohort match a set (the C/careful "every
> reading" semantics). Same `possible_sets` early-out and list selection as the
> Normal matcher (no `wread` special-casing here). For each selected reading list,
> for each reading: descend to the sub-reading when a test is present (skip null);
> honor POS_ACTIVE/POS_INACTIVE. Set `retval = doesSetMatchCohort_helper(...)`;
> the moment any reading fails (retval false), break out of both loops. After the
> loops: if context and not matched_target and POS_NOT, retval =
> `doesSetMatchCohort_testLinked`. Return retval (true only if every considered
> reading matched).

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
> inline bool doesSetMatchCohort_helper(Cohort& cohort, Reading& reading, const Set& theset, dSMC_Context* context = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
> Core per-reading match+link logic shared by the Normal/Careful cohort matchers.
> Snapshot the current `regexgrp_ct` (as `orz`) and, for ST_CHILD_UNIFY sets (when
> not FL_CAPTURE_UNIF), snapshot the context's `unif_tags`/`unif_sets` into scoped
> temporaries. Call `doesSetMatchReading(reading, theset.number, (theset.type &
> (ST_CHILD_UNIFY|ST_SPECIAL))!=0)` (bypass_index when child-unify or special). If
> it matches: retval=true; if there is a context, when POS_ATTACH_TO set
> `reading.matched_target=true`, and always set `context->matched_target=true`.
> If matched and context has POS_NOT, invert retval. If retval and context and not
> `context->in_barrier`: retval = `doesSetMatchCohort_testLinked(cohort, theset,
> context)`; and for POS_ATTACH_TO record `reading.matched_tests=retval` and, on
> success, set the context's `attach_to` to this cohort/subreading (reading filled
> later by the Normal matcher). On a NON-match with a child-unify context, restore
> (swap back) the snapshotted `unif_tags` and/or `unif_sets` if they changed, and
> restore `regexgrp_ct = orz`. Return retval.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
> bool doesSetMatchCohortNormal(Cohort& cohort, const uint32_t set, dSMC_Context* context = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
> Tests whether ANY reading of a cohort matches a set (the default "some reading"
> semantics). Early-out via the `possible_sets` bitset: unless the context has
> LOOK_DELETED/DELAYED/IGNORED/NOT, if `set` is beyond `possible_sets` size or its
> bit is clear, return false. Resolve `theset`. If the cohort has a `wread`
> (wordform reading) and we are not in a barrier, first try
> `doesSetMatchCohort_helper` on it. If that matched and (no context or
> `context->did_test`), return true. Build the list array `lists[4] =
> {&readings, deleted?, delayed?, ignored?}` gated by the POS_LOOK_* options.
> For each list and each reading: if the context has a `test`, descend to the
> sub-reading via `get_sub_reading(reading, test->offset_sub)` (skip if null);
> skip inactive/active readings per POS_ACTIVE/POS_INACTIVE. Call
> `doesSetMatchCohort_helper`; on success set retval=true and, for a matched
> sub-reading attach_to, backfill the parent reading pointer. Return true early
> when matched and there is no linked test still to run (or did_test). After the
> loops: if context didn't match the target and has POS_NOT, retval =
> `doesSetMatchCohort_testLinked`. Finally, negative-caching: if context and not
> matched_target and not ACTIVE/INACTIVE, and the set isn't in `grammar->sets_any`,
> and the test wasn't a sub-reading test, clear the set's bit in
> `cohort.possible_sets` (so this cohort is skipped for this set next time). Return
> retval.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
> inline bool doesSetMatchCohort_testLinked(Cohort& cohort, const Set& theset, dSMC_Context* context = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
> Runs the LINKed contextual test that follows a set match, memoizing its result
> in the dSMC_Context. `retval=true` by default. Determine the linked test: if
> `context->test` has a `linked`, use it; else if `tmpl_cntx.linked` is non-empty,
> save `tmpl_cntx.min/max`, take (and pop) the last entry as `linked` and mark
> `reset=true`. If a `linked` test exists: if not already tested
> (`!context->did_test`), run it via `runContextualTest(cohort.parent,
> cohort.local_number, linked, context->deep, origin)` where origin is `&cohort`
> when the link has POS_NO_PASS_ORIGIN else `context->origin`; store the boolean in
> `context->matched_tests`; set `context->did_test = true` unless the set is
> ST_CHILD_UNIFY (child-unify re-tests each time). `retval =
> context->matched_tests`. If `reset`, push the popped `linked` back onto
> `tmpl_cntx.linked`. If `retval` is false, restore `tmpl_cntx.min/max` to the
> saved values. Return `retval`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn]
> bool doesSetMatchReading(const Reading& reading, const uint32_t set, bool bypass_index = false, bool unif_mode = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn]
> Tests whether a reading matches an arbitrary set (LIST or SET, incl. set
> operators and unification). Cache: unless `bypass_index` or `unif_mode`, consult
> per-set `index_readingSet_no[set]`/`index_readingSet_yes[set]` keyed by
> `reading.hash`; a hit returns false/true immediately. Resolve `theset =
> *grammar->sets_list[set]`. Cases: if ST_ANY, retval=true. Else if the set has no
> sub-sets (LIST set), retval=`doesSetMatchReading_tags(reading, theset,
> ((theset.type & ST_TAG_UNIFY)!=0) | unif_mode)`. Else if ST_SET_UNIFY (&&sets):
> on the first evaluation, compute which of the child sets of `sets[0]` match the
> reading and store their numbers in the context's `unif_sets[theset.number]`,
> retval = that set is non-empty; on later evaluations, retval = any previously
> stored unified set still matches. Else (a SET set with operators): loop the
> sub-sets applying `set_ops`: OR is implicit (skipped, giving other ops
> precedence); within a run of non-OR ops handle S_PLUS (both must match),
> S_FAILFAST (`^`: if the right matches, force no-match and set failfast),
> S_MINUS (`-`: right match removes the left match); default op throws. A matching
> sub-expression sets retval=true and breaks (++match_sub); a failfast sets
> retval=false and breaks. Afterwards, if unifying, propagate any found unified
> tag pointer across all this set's sub-sets in the context `unif_tags`. Finally
> store the result into `index_readingSet_yes[set]` (on true) or
> `index_readingSet_no[set]` (on false, but only when not tag-unify and not
> unif_mode) keyed by `reading.hash`. Return retval.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-tags-fn]
> bool doesSetMatchReading_tags(const Reading& reading, const Set& theset, bool unif_mode = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-tags-fn]
> Tests whether a reading matches a LIST set. First, failfast guard: if
> `theset.ff_tags` is non-empty, for each ff tag call `doesTagMatchReading`; if any
> matches, return false (a failfast tag present in the reading vetoes the whole
> set). Fast path (the common case): if `theset.trie` and
> `reading.tags_plain` are both non-empty, do a merge-join between the reading's
> plain tags and the trie's top-level tag hashes — advancing both sorted
> iterators; when hashes are equal, if that trie node is terminal (respecting
> `check_unif_tags` in unif_mode) set retval=true and stop, else recurse via
> `doesSetMatchReading_trie` on the child trie. If still no match and
> `theset.trie_special` is non-empty, retval=`doesSetMatchReading_trie(reading,
> theset, theset.trie_special, unif_mode)` (handles the special/regex/failfast
> tags that can't be found by plain hash intersection). Return retval.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-trie-fn]
> bool doesSetMatchReading_trie(const Reading& reading, const Set& theset, const trie_t& trie, bool unif_mode = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-trie-fn]
> Recursively tests a tag trie against a reading (a trie node is a set of
> composite-tag prefixes). For each `kv` (tag -> trie node) in `trie`: compute
> `match = doesTagMatchReading(reading, *kv.first, unif_mode) != 0`. If it
> matched: if the tag is T_FAILFAST, `continue` (skip — failfast tags don't
> satisfy). If the node is `terminal`: in `unif_mode`, require
> `check_unif_tags(theset.number, &kv)` (unification consistency) — if that fails
> `continue`; otherwise return true. If the node has a child `trie` and a
> recursive `doesSetMatchReading_trie` on it returns true, return true. If nothing
> matched, return false.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-icase-fn]
> uint32_t doesTagMatchIcase(uint32_t test, const Tag& tag, bool bypass_index = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-icase-fn]
> Tests whether input tag `test` equals `tag` case-insensitively (NOT a regex —
> a full-string case-fold comparison). Index key `ih = (uint64(tag.hash) << 32) |
> test`. Unless `bypass_index`: if `index_icase_no` contains `ih`, match=0; else
> if `index_icase_yes` contains `ih`, match=`test`. Otherwise fetch `itag =
> grammar->single_tags[test]` and compute `ux_strCaseCompare(tag.tag, itag.tag)`
> (returns true when the two whole strings are equal ignoring case); on true set
> match=`itag.hash`. Insert `ih` into `index_icase_yes` on match else
> `index_icase_no`. Returns the matched hash or 0. PARITY: this is whole-string
> case-insensitive equality, not a substring/regex match.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn]
> uint32_t doesTagMatchReading(const Reading& reading, const Tag& tag, bool unif_mode = false, bool bypass_index = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn]
> Central single-tag matcher: returns the matched hash (nonzero) or 0, and on a
> match increments `match_single`. Dispatch is a strict if/else-if chain on
> `tag.type` (ORDER MATTERS): (1) if the tag is NOT T_SPECIAL, OR it is
> T_FAILFAST: treat it as a raw tag — check `reading.tags_plain` membership: for
> T_FAILFAST look up `tag.plain_hash`; otherwise first consult the plain bloom
> filter then `tags_plain.find(tag.hash)`; if present, match=`tag.hash`. (2) else
> if T_SET: resolve the set number from `tag.tag`'s hash via
> `grammar->sets_by_name` and match=`doesSetMatchReading(reading, sh, bypass_index,
> unif_mode)`. (3) else if T_VARSTRING: regenerate the tag
> (`generateVarstringTag`) and recurse. (4) else if T_META: if the tag has a
> regexp and the cohort text is non-empty, `uregex_setText` on
> `reading.parent->text`, `uregex_find(...,-1,...)` (unanchored; error->quit), on
> match set match=`tag.hash` and, if `gc>0` and context has regexgrps, capture
> groups. (5) else if `tag.regexp`: match=`doesRegexpMatchReading(...)`. (6) else
> if T_CASE_INSENSITIVE: iterate `reading.tags_textual`, first
> `doesTagMatchIcase` hit wins. (7) else if T_REGEXP_ANY (the `<.*>`/`".*"`
> wildcards): if T_BASEFORM match=`reading.baseform`; if T_WORDFORM
> match=wordform hash; else pick the first textual tag that is neither baseform
> nor wordform — each with `unif_mode` bookkeeping via `unif_last_baseform` /
> `unif_last_wordform` / `unif_last_textual` (first seen is stored; a later
> different value zeroes the match, enforcing unification). (8) else if
> T_NUMERICAL: for each numeric tag on the reading call `test_tag_numerical`;
> keep the last nonzero result. (9) else if T_VARIABLE|T_LOCAL_VARIABLE: choose
> the variable map (`variables`, or the window's `variables_set` for local vars in
> a non-current window); resolve the key tag (`comparison_hash`), matching the key
> by regexp/icase/exact; if found and `tag.variable_hash==0` match=`tag.hash`;
> else compare the variable's value against `variable_hash` by regexp/icase/exact.
> (10) T_PAR_LEFT / T_PAR_RIGHT: match `grammar->tag_any` if the reading is at
> `par_left_pos`/`par_right_pos` and carries the paren tag. (11) T_ENCL: match if
> the next cohort in `all_cohorts` is `enclosed`. (12) T_TARGET: match tag_any if
> `reading.parent == rule_target`. (13) T_MARK: match tag_any if
> `reading.parent == get_mark()`. (14) T_ATTACHTO: match tag_any if
> `reading.parent == get_attach_to().cohort`. (15) T_SAME_BASIC: match tag_any if
> `reading.hash_plain == same_basic`. (16) T_CONTEXT: if `context_stack.size() >
> 1`, match tag_any if the referenced position in the parent context list equals
> `reading.parent`. Finally if `match` nonzero, `++match_single` and return match,
> else return 0.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-regexp-fn]
> uint32_t doesTagMatchRegexp(uint32_t test, const Tag& tag, bool bypass_index = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-regexp-fn]
> Tests whether the input tag `test` (a tag hash) matches the compiled regular
> expression stored on `tag` (uses only `tag.hash` and `tag.regexp`). Returns the
> matched input tag's hash on success, else 0. Steps: query
> `gc = uregex_groupCount(tag.regexp)` (number of capture groups, not counting
> group 0). Build the index key `ih = (uint64(tag.hash) << 32) | test` (regex tag
> hash in the high 32 bits, input tag hash truncated to 32 bits in the low).
> Cache lookup (unless `bypass_index`): if `index_regexp_no` contains `ih`,
> match=0. Else if `gc == 0` AND `index_regexp_yes` contains `ih`, match=`test`
> (regexes WITH capture groups deliberately never read the yes-cache, so they
> re-run to re-capture). Otherwise do the real match: fetch the input tag `itag`
> from `grammar->single_tags[test]`, `uregex_setText(tag.regexp, itag.tag.data(),
> size, &status)` (error->quit), then `uregex_find(tag.regexp, -1, &status)`
> where startIndex `-1` means search the ENTIRE input string — i.e. an UNANCHORED
> match (matches anywhere, not just at the start; error->quit). If it matched, set
> match=`itag.hash`. Post-match: if matched and `gc > 0` and `context_stack` is
> non-empty and its top `regexgrps != 0`, call
> `captureRegex(gc, regexgrp_ct, regexgrps, tag)` to extract groups 1..gc into the
> context's capture buffer (appending, advancing `regexgrp_ct`); else if matched,
> insert `ih` into `index_regexp_yes`. If not matched, insert `ih` into
> `index_regexp_no`. Return `match`. REGEX PARITY: the ICU regex here is compiled
> elsewhere; matching is unanchored substring search; case-insensitivity comes
> from the tag's own compiled flags; the Rust `regex` crate's `find`/`is_match`
> are likewise unanchored and Unicode-aware, so the port must NOT anchor and must
> preserve the gc==0-only yes-cache quirk.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-wordforms-match-fn]
> bool doesWordformsMatch(const Tag* cword, const Tag* rword)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-wordforms-match-fn]
> Quick pre-filter: does a cohort's wordform `cword` satisfy a rule's wordform
> restriction `rword`? If `rword` is null or identical to `cword`, return true
> (no restriction / trivially equal). Otherwise: if `rword` is T_REGEXP, return
> whether `doesTagMatchRegexp(cword->hash, *rword)` matched; if T_CASE_INSENSITIVE,
> return whether `doesTagMatchIcase(cword->hash, *rword)` matched; otherwise (a
> plain differing wordform) return false. Returns bool.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.error-fn]
> void GrammarApplicator::error(const char* str, const UChar* p)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.error-fn]
> Formats a runtime error/warning to `ux_stderr`. The pointer `p` argument is
> ignored (`(void)p`). If `current_rule` is set and `current_rule->line != 0`,
> use the UChar label `"RT RULE"` and print `u_fprintf(ux_stderr, str, buf,
> current_rule->line, buf)` — i.e. the caller's format string `str` is filled with
> (label, rule line, label). Otherwise use label `"RT INPUT"` and print
> `u_fprintf(ux_stderr, str, buf, numLines, buf)` (label, input line, label). No
> return. (The sibling overloads `error(str, s, p)`, `error(str, s, p)` with
> UChar* s, and `error(str, s, S, p)` follow the identical branch structure but
> splice one or two extra string arguments `s`/`S` into the format between the
> first label and the line number.)

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.externals-t]
> typedef std::map<uint32_t, Process> externals_t

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.generate-varstring-tag-fn]
> Tag* generateVarstringTag(const Tag* tag)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.generate-varstring-tag-fn]
> Expands a T_VARSTRING template tag into a concrete tag by substituting unified
> sets, regex capture groups, and case markers. Work in a thread_local
> UnicodeString `tmp` initialized to `tag->tag`; track `did_something=false`.
> Step 1: convert the human `%[UuLl]` and `$1..$9` markers into private control
> codes (each `STR_VS*_raw` -> corresponding `STR_VS*` control string) via
> findAndReplace, to prevent later accidental matching. Step 2: if `tag->vs_sets`
> is set, for each unified set `i`, gather its tags via `getTagList` into a scoped
> list, join them into `rpl` separating multiple tags with `_`, and
> findAndReplace occurrences of `(*tag->vs_names)[i]` with `rpl` (set
> did_something if replaced). Step 3: replace `$1..$9` capture markers: for `i` in
> `0 .. min(context_stack.back().regexgrp_ct, 9)`, findAndReplace the `STR_VS(i+1)`
> control string with the captured group text `(*context_stack.back().regexgrps)[i]`
> (set did_something). Step 4: handle `%U %u %L %l` case operations by repeatedly
> finding the right-most of the four markers (via lastIndexOf), removing the 2-char
> marker, and applying: `u` upper-cases the single following char; `U` upper-cases
> the rest of the string from that point; `l`/`L` do the lower-case equivalents;
> loop until none remain. Step 5: if `tag->type` has T_CASE_INSENSITIVE append
> `i`; if T_REGEXP append `r` (regex/flag suffixes). If nothing was substituted
> and the result equals the original `tag->tag`, print a warning about being unable
> to generate (possibly missing KEEPORDER/capturing regex). Return `addTag(nt,
> tag->type)` where `nt` is the terminated buffer.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-apply-to-fn]
> ReadingSpec get_apply_to()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-apply-to-fn]
> Returns the ReadingSpec the current rule should act on. If `context_stack` is
> empty, returns a default (all-null) ReadingSpec. Else if
> `context_stack.back().attach_to.cohort != nullptr`, returns that `attach_to`
> spec (a prior test picked an attach target); otherwise returns
> `context_stack.back().target` (the rule's own matched target).

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-attach-to-fn]
> ReadingSpec get_attach_to()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-attach-to-fn]
> Returns the current rule context's `attach_to` ReadingSpec. If `context_stack`
> is empty, returns a default-constructed (all-null) ReadingSpec; otherwise
> returns `context_stack.back().attach_to` by value.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-grammar-fn]
> Grammar* get_grammar()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-grammar-fn]
> Trivial accessor: returns the `grammar` member pointer (the currently attached
> Grammar*, or nullptr if none set). No side effects.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-mark-fn]
> Cohort* get_mark()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-mark-fn]
> Returns the current rule context's `mark` cohort. If `context_stack` is empty,
> returns nullptr; otherwise returns `context_stack.back().mark`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-sub-reading-fn]
> Reading* get_sub_reading(Reading* tr, int sub_reading)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-sub-reading-fn]
> Selects a sub-reading from a reading's `next`-chain by index. If `sub_reading ==
> 0`, return `tr` unchanged. If `sub_reading == GSR_ANY` (32767): if there are no
> sub-readings (`tr->next == nullptr`) return `tr`; otherwise build an amalgamated
> reading in the `subs_any` deque — copy `tr`, then walk the chain and merge each
> sub-reading's tags into it: append a `0` separator then all `tags_list` entries,
> and union `tags`/`tags_plain`/`tags_textual` (with blooms) and `tags_numerical`,
> and OR in `mapped`/`mapping`/`matched_target`/`matched_tests`; rehash and return
> that combined reading. If `sub_reading > 0`, step forward `sub_reading` times
> down `next` (may become null). If `sub_reading < 0`, count the chain length,
> then (treating -1 as the innermost) if there is no `next` set `tr=nullptr`, else
> step forward from the negative index to reach the requested-from-end sub-reading.
> Returns the located reading or nullptr.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
> TagList getTagList(const Set& theSet, bool unif_mode = false) const

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
> Flattens a Set into an ordered `TagList` of concrete tags. The value-returning
> overload just allocates a list and calls the out-param overload. The out-param
> overload recurses by set type: ST_SET_UNIFY -> for each child of `sets[0]` that
> is in the context's stored `unif_sets[theSet.number]`, recurse into it;
> ST_TAG_UNIFY -> recurse into every `sets` member with `unif_mode=true`; a set
> with sub-sets -> recurse into each `sets` member (propagating `unif_mode`);
> otherwise (a LIST set) in `unif_mode`, only if the context's `unif_tags` has an
> entry for this set number, collect from `trie`/`trie_special` filtered by that
> unified value; in non-unif mode collect the full `trie`/`trie_special` via
> `trie_getTagList`. After collecting, remove CONSECUTIVE duplicate tags (adjacent
> equal entries only — not all duplicates, since AddCohort/Append can legitimately
> repeat tags across readings). No return (fills `theTags`). const method.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-tags-matching-fn]
> void getTagsMatching(const Reading& reading, TagList& theTags, TagList& rvTags)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-tags-matching-fn]
> Collects, into `rvTags`, the actual reading tags that match each pattern tag in
> `theTags`. For each pattern `tag` in `theTags`, iterate every tag `tt` in
> `reading.tags_list` (resolve `itag`): compute a match — if `tag.regexp`,
> `doesTagMatchRegexp(tt, tag)`; else if T_CASE_INSENSITIVE,
> `doesTagMatchIcase(tt, tag)`; else if `tag` is T_REGEXP_ANY and `itag` is
> T_TEXTUAL: for T_BASEFORM match `reading.baseform` when `itag` is a baseform;
> for T_WORDFORM match the wordform hash when `itag` is a wordform; else (neither)
> match `itag->hash` only if the leading char class agrees (both `"` or both `<`);
> else if both T_NUMERICAL, `test_tag_numerical(reading, tag, *itag)`; else exact
> `tag.hash == itag->hash`. If matched, push `itag` onto `rvTags`. No return; note
> a single reading tag can be appended once per matching pattern.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.grammar-applicator-fn]
> GrammarApplicator::~GrammarApplicator()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.grammar-applicator-fn+1]
> Destructor `~GrammarApplicator()`. In C++, if `owns_grammar` is true,
> `delete grammar`, then set `grammar = nullptr` and `ux_stderr = nullptr`. Then
> for every `URegularExpression*` in `text_delimiters`, call `uregex_close(rx)`
> to release the ICU regex objects. (Note: the vector itself is not cleared, but
> the object is being destroyed.) No return. In the port the `owns_grammar`
> branch is vacuous — the de-warting drops that member — because the grammar is
> owned by value and dropped with the applicator; the regex objects likewise
> release on drop, so the port destructor body is empty.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.index-fn]
> void GrammarApplicator::index()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.index-fn]
> Prepares the per-section rule schedule and misc runtime state; runs at most
> once. If `!add_spacing`, set `ws[2] = '\n'` (adds newline to the whitespace
> set). If `did_index` already true, return. Copy grammar flags:
> `if (grammar->ordered) ordered = true`; `if (grammar->has_dep || dep_delimit)
> parse_dep = true`. Build the `runsections` map (keyed by int32 section id): if
> `grammar->before_sections` non-empty, insert each rule's `number` into
> `runsections[-1]`; `after_sections` -> `runsections[-2]`; `null_section` ->
> `runsections[-3]`. For the main sections: if `sections` (the cmdline subset) is
> empty, for each `i` in `0..grammar->sections.size()`, and each rule `r` in
> `grammar->rules`, skip if `r->section < 0 || r->section > i`, else insert
> `r->number` into `runsections[i]` (so each section accumulates all rules from
> earlier sections too). If `sections` is non-empty, set
> `numsections = sections.size()` and for `n` in `0..numsections`, for `e` in
> `0..=n`, for each rule with `r->section == sections[e]-1`, insert into
> `runsections[n]`. If `valid_rules` (a set of rule *lines* from cmdline) is
> non-empty, translate it to rule *numbers*: iterate `grammar->rule_by_number`,
> and for each whose `line` is contained in `valid_rules`, push its `number` into
> a new vector, then replace `valid_rules` with it. Finally build the dependency
> span print patterns: seed `span_pattern_utf` and `span_pattern_latin` from
> constant UChar templates, compute width `w = floor(log10(hard_limit)) + 1`, and
> patch the width placeholder chars (`'0'+w`) into both patterns at their fixed
> offsets. Set `did_index = true`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.index-single-window-fn]
> void indexSingleWindow(SingleWindow& current)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.index-single-window-fn]
> Builds the window's rule→cohort candidate index from scratch. Clear
> `current.valid_rules`; resize `current.rule_to_cohorts` to
> `grammar->rule_by_number.size()` and clear each CohortSet. For each cohort in
> `current.cohorts`, for each bit set in its `possible_sets` bitset (a set number
> `psit`), look up `psit` in `grammar->rules_by_set`; for each rule number `rsit`
> that references that set, call `updateRuleToCohorts(*c, rsit)`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.init-empty-cohort-fn]
> Reading* initEmptyCohort(Cohort& cohort)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.init-empty-cohort-fn]
> Gives a cohort that has no readings a synthetic "magic" reading. Allocate a
> reading in `cCohort`. Its baseform is `makeBaseFromWord(cCohort.wordform)->hash`
> when `allow_magic_readings`, else just `cCohort.wordform->hash`. Mark the (*) set
> possible, add the wordform tag via `addTagToReading`, set `noprint = true`,
> append the reading, and `++numReadings`. Returns the new reading.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.init-empty-single-window-fn]
> void initEmptySingleWindow(SingleWindow* cSWindow)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.init-empty-single-window-fn]
> Seeds a new window with its initial `>>>` boundary cohort. Allocate a cohort in
> `cSWindow`, give it `global_number = gWindow->cohort_counter++` and wordform
> `tag_begin`. Allocate one reading with `baseform = begintag`, mark the (*) set
> possible (`insert_if_exists(possible_sets, grammar->sets_any)`), and add the
> begintag via `addTagToReading`. Append the reading to the cohort, then append
> the cohort to `cSWindow`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.is-child-of-fn]
> bool isChildOf(const Cohort* child, const Cohort* parent)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.is-child-of-fn]
> Tests whether `child` is a dependency descendant of `parent`. Returns true if
> `parent->global_number == child->global_number` (a cohort is its own child) or
> `parent->global_number == child->dep_parent` (direct parent). Otherwise walk up
> from `inner = child` for up to 1000 iterations: if `inner->dep_parent` is 0 or
> DEP_NO_PARENT, return false (reached a root); look up `inner->dep_parent` in
> `gWindow->cohort_map`; if found, move `inner` there, else break; then if
> `inner->dep_parent == parent->global_number`, return true. If the loop reaches
> 1000 iterations, and `verbosity_level > 0`, print a "counter exceeded 1000
> indicating a loop" warning; the accumulated `retval` (false unless set) is
> returned. Returns the bool.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.make-base-from-word-fn]
> Tag* makeBaseFromWord(uint32_t tag)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.make-base-from-word-fn]
> Derives a baseform tag from a wordform tag. The uint32 overload looks the tag up
> in `grammar->single_tags` and forwards to the Tag* overload. The Tag* overload:
> let `len = tag->tag.size()`. If `len < 4`, return `tag` unchanged (too short to
> strip). Otherwise build a new string `n` of length `len-2`: set `n[0]` and
> `n[len-3]` to `"` (double quote), and copy `len-4` chars from
> `tag->tag.data()+2` into `n[1..]`. In effect it converts a wordform written as
> `"<word>"` into a baseform `"word"` by dropping the outer `<`/`>` angle
> brackets while keeping the surrounding quotes. Returns `addTag(n)` (the interned
> new tag). Uses a thread_local scratch string.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.merge-mappings-fn]
> void mergeMappings(Cohort& cohort)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.merge-mappings-fn]
> Merges duplicate readings within a cohort. Calls
> `mergeReadings(cohort.readings)`. If `trace` is set, also calls
> `mergeReadings(cohort.deleted)` and `mergeReadings(cohort.delayed)`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.merge-readings-fn]
> void mergeReadings(ReadingList& readings)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.merge-readings-fn]
> Deduplicates/merges readings that share the same plain signature, folding their
> mapping tags together. Uses two thread_local flat_maps: `mapped` (hplain ->
> {mapping-count, Reading*}) and `mlist` (composite-hash -> list of readings).
> For each reading `r`: compute `hp` and `hplain` from `r->hash_plain` (or
> `tags_string_hash` when `ordered`); if `trace`, fold each `hit_by` value into
> `hp`; count mappings `nm` (this reading plus each sub-reading via `next`),
> folding each sub-reading's plain hash (and hit_by when tracing) into both `hp`
> and `hplain`. Dedup-by-mapping logic: if `hplain` already in `mapped`: if the
> stored count != 0 and `nm==0`, mark `r` deleted; else if stored count != nm and
> stored count == 0, mark the stored reading deleted. Record `mapped[hplain] =
> {nm, r}` and append `r` to `mlist[hp + nm]`. If `mlist.size() ==
> readings.size()` (nothing to merge), return. Otherwise clear `readings`; for
> each `mlist` bucket, clone the front reading, drop its mapping tag from
> `tags_list`, then for each reading in the bucket, if it has a mapping not
> already present, push that mapping hash onto the clone's `tags_list`, and
> `free_reading` each original. Collect clones, sort by `Reading::cmp_number`, and
> insert them back at the front of `readings`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-cohort-fn]
> void GrammarApplicator::pipeInCohort(Cohort* cohort, Process& input)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-cohort-fn]
> Deserializes one cohort packet from `Process& input` into `cohort`. Reads a
> packet length (unused beyond debug). Reads `cs` = expected cohort global number;
> if `cs != cohort->global_number`, print an error and `CG3Quit(1)`. Reads
> `flags`. If bit1 (`1<<1`) set, `readRaw(input, cohort->dep_parent)`. Reads a
> UTF8 wordform string; if it differs from the current `cohort->wordform->tag`,
> `addTag` it, set `cohort->wordform`, and set `force_readings = true`. Reads
> `cs` = number of readings; for each `i` call `pipeInReading(cohort->readings[i],
> input, force_readings)`. If bit0 (`1<<0`) set, read `cohort->text` as a UTF8
> string. No return. (Reads directly from `input`, not from a sub-buffer.)

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-reading-fn]
> void GrammarApplicator::pipeInReading(Reading* reading, Process& input, bool force)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-reading-fn]
> Deserializes one reading packet from an external `Process& input` back into
> `reading`. `readRaw(input, cs)` reads the packet length, reads `cs` bytes into a
> buffer, wraps it in an istringstream `ss`. `readRaw(ss, flags)`. If `!force`
> and bit0 (`1<<0`, "modified") is not set, return without changes. Set
> `reading->noprint = (flags & (1<<1)) != 0` and `reading->deleted =
> (flags & (1<<2)) != 0`. If bit3 (`1<<3`) set: read a UTF8 string; if it differs
> from the current baseform tag string, `addTag` it and set `reading->baseform`
> to the new hash; else leave. If bit3 not set, `reading->baseform = 0`. Rebuild
> `tags_list`: clear it, push the parent wordform hash, then the baseform (if
> any). `readRaw(ss, cs)` reads the tag count; for each, read a UTF8 string,
> `addTag` it, and push its hash. Finally `reflowReading(*reading)` to rebuild the
> derived tag structures. (debug_level>1 emits DEBUG traces throughout.) No
> return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-in-single-window-fn]
> void GrammarApplicator::pipeInSingleWindow(SingleWindow& window, Process& input)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-single-window-fn]
> Deserializes a window packet from `Process& input` into `window`. Reads a
> packet length `cs`; if `cs == 0`, return (empty/unchanged window). Reads `cs` =
> expected window number; if `cs != window.number`, print an error and
> `CG3Quit(1)`. Reads `cs` = number of cohorts; for each `i` in `0..cs` call
> `pipeInCohort(window.cohorts[i + 1], input)` (index +1 skips the initial `>>>`
> cohort). No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-out-cohort-fn]
> void GrammarApplicator::pipeOutCohort(const Cohort* cohort, std::ostream& output)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-out-cohort-fn]
> Serializes one cohort to a binary length-prefixed packet. Into a temp
> `ss`: `writeRaw(ss, cohort->global_number)`. Compute flags: bit0 (`1<<0`) if
> `cohort->text` non-empty; bit1 (`1<<1`) if `has_dep && cohort->dep_parent !=
> DEP_NO_PARENT`. `writeRaw(ss, flags)`. If bit1, `writeRaw(ss,
> cohort->dep_parent)`. `writeUTF8_Raw` the wordform tag string. Write
> `cs = cohort->readings.size()` then `pipeOutReading` each reading into `ss`. If
> cohort has text, `writeUTF8_Raw` it. Finally length-prefix the whole `ss` buffer
> to `output` (raw uint32 length then raw bytes). No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-out-reading-fn]
> void GrammarApplicator::pipeOutReading(const Reading* reading, std::ostream& output)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-out-reading-fn]
> Serializes one reading to a binary length-prefixed packet (for external
> processes). Uses a temp `std::ostringstream ss`. Compute flags: bit1 (`1<<1`)
> if `reading->noprint`, bit2 (`1<<2`) if `reading->deleted`, bit3 (`1<<3`) if
> `reading->baseform`. `writeRaw(ss, flags)`. If baseform, `writeUTF8_Raw` the
> baseform tag string. Count `cs` = number of tags_list entries excluding the
> baseform and the parent wordform hash and (when `has_dep`) excluding
> T_DEPENDENCY tags; `writeRaw(ss, cs)`. Then iterate tags_list again with the
> same skip rules and `writeUTF8_Raw` each surviving tag's string. Finally take
> `str = ss.str()`, write its length as a raw uint32 to `output`, then write the
> raw bytes. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pipe-out-single-window-fn]
> void GrammarApplicator::pipeOutSingleWindow(const SingleWindow& window, Process& output)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-out-single-window-fn]
> Serializes a whole window to the external `Process& output`. Into temp `ss`:
> `writeRaw(ss, window.number)`; compute `cs = window.cohorts.size() - 1` (the
> real cohorts, excluding the initial `>>>` at index 0) and `writeRaw(ss, cs)`.
> For `c` in `1..=cs`, `pipeOutCohort(window.cohorts[c], ss)`. Then length-prefix
> the buffer to `output` (raw uint32 length then bytes) and call
> `output.flush()`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.pos-output-helper-fn]
> bool posOutputHelper(const SingleWindow* sWindow, size_t position, const ContextualTest* test, const Cohort* cohort, const Cohort* cdeep)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pos-output-helper-fn]
> Validates whether a template-test result cohort satisfies the outer test's
> offset/span requirements. Builds a 4-element array of cohorts `cs = {cohort,
> cdeep, cohort, cdeep}`, overriding `cs[2]=tmpl_cntx.min` and `cs[3]=tmpl_cntx.max`
> when set, then `std::sort`s the 4 by cohort order (`compare_Cohort`). `good=false`.
> If the test overrides with `*`/`@`/absolute (POS_SCANFIRST|SCANALL|ABSOLUTE),
> good=true. Else for a positive `offset`, good iff `cs[0]->local_number - position
> == offset` (leftmost matches); for a negative offset, good iff
> `cs[3]->local_number - position == offset` (rightmost matches). Then: if the
> test has no span flag and `cdeep->parent != sWindow`, force good=false. Then if
> not POS_PASS_ORIGIN, force good=false when a negative offset's rightmost is still
> right of `position`, or a positive offset's leftmost is still left of `position`
> (didn't actually move past origin). Return good.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-cohort-fn]
> void GrammarApplicator::printCohort(Cohort* cohort, std::ostream& output, bool profiling)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-cohort-fn]
> Prints one cohort (wordform header, readings, and text) in CG format. If
> `cohort->local_number == 0` (the initial `>>>` cohort), jump straight to the
> `removed:` label (only print trailing text). If `profiling` and this cohort is
> `rule_target`, print `# RULE TARGET BEGIN\n`. If `cohort->wblank` is non-empty,
> print it via printPlainTextLine and add a newline unless it already ends in one
> (`ISNL`). If cohort is CT_REMOVED: if `!trace || trace_no_removed` jump to
> `removed:`; else emit `; ` prefix. Print the wordform tag string. If
> `cohort->wread` exists, print each of its tags (except the wordform hash) as
> ` <tag>`. Emit newline. If not profiling: call `cohort->unignoreAll()`, and if
> `!split_mappings` call `mergeMappings(*cohort)`. Sort `cohort->readings` by
> `Reading::cmp_number` and printReading each. If `trace && !trace_no_removed`,
> also sort and print `cohort->delayed` then `cohort->deleted`. At `removed:`: if
> `cohort->text` is non-empty and contains a non-whitespace char
> (`find_first_not_of(ws)`), print it and add a newline unless it ends in one.
> If `profiling` and cohort is `rule_target`, print `# RULE TARGET END\n`.
> Virtual; no return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-debug-rule-fn]
> void printDebugRule(const Rule& rule, bool target = true, bool cntx = true)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-debug-rule-fn+1]
> Dumps a full profiling-style snapshot of all windows to `ux_stderr`, bracketed
> by BEGIN/END markers, used when a rule's line is in `debug_rules`. Uses a
> thread_local static stringstream `buf`. The snapshot must render with `trace`
> force-disabled: the C++ constructs a scoped `swapper<bool>(true, trace,
> ttrace=false)` that temporarily forces `trace=false` for the duration and
> restores it on return. The port removes that mutation — `trace` is immutable
> config — by threading the effective trace flag `false` as an explicit argument
> down the `printSingleWindow` → `printCohort` → `printReading` chain; the shared
> `trace` field is never written. Behavior (the emitted bytes) is identical; only
> the mechanism changed. Resets buf. Writes
> `# ===== BEGIN RULE <rule.line> <" TARGET-MATCH" if target else " TARGET-FAIL">`
> `<" CONTEXT-MATCH" if cntx else " CONTEXT-FAIL"> =====\n`. Then writes
> `# PREVIOUS WINDOWS\n` and calls `printSingleWindow(s, buf, true)` for each `s`
> in `gWindow->previous`; `# CURRENT WINDOW\n` and
> `printSingleWindow(gWindow->current, buf, true)`; `# NEXT WINDOWS\n` and the
> same for each `s` in `gWindow->next`. Writes
> `# ===== END RULE <rule.line> =====\n`. Finally emits the whole buffer with
> `u_fprintf(ux_stderr, "%s", buf.str().c_str())`. `target` and `cntx` default to
> true. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-plain-text-line-fn]
> void GrammarApplicator::printPlainTextLine(UStringView line, std::ostream& output)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-plain-text-line-fn]
> Writes a raw text line verbatim: `u_fprintf(output, "%S", line.data())` — emits
> the UTF-16 string `line` with no added newline. Virtual; no return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
> void GrammarApplicator::printReading(const Reading* reading, std::ostream& output, size_t sub)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
> Recursively prints one reading (and its sub-readings) in CG text format. If
> `reading->noprint`, return immediately. If `reading->deleted`: if `!trace`
> return; else emit `;`. Emit `sub` tab characters (indent = sub-reading depth,
> default 1). If `reading->baseform`, print ` `-free the baseform tag string
> (`grammar->single_tags[baseform]->tag`). Then iterate `reading->tags_list` in
> order, building a `unique` sorted set and a deferred `mappings` list, skipping:
> the end tag (unless `show_end_tags`), the begin tag, the baseform, the parent
> wordform hash; if `unique_tags`, skip tags already seen; skip T_DEPENDENCY tags
> when `has_dep && !dep_original`; skip T_RELATION tags when `has_relations`;
> defer T_MAPPING tags to `mappings`. Each surviving tag is printed as ` <tag>`.
> After the loop, print each deferred mapping tag as ` <tag>` (mappings go last).
> Dependency block: if `has_dep` and the parent cohort is not CT_REMOVED, ensure
> `parent->dep_self` is set (default to `global_number`), resolve the print-parent
> `pr` (self's parent cohort: if `dep_parent==DEP_NO_PARENT` keep self; if
> `dep_parent==0` use `parent->parent->cohorts[0]`; else look up
> `gWindow->cohort_map[dep_parent]`), then print the dependency using a
> latin or unicode arrow pattern (`unicode_tags` selects `→` vs `->`): with
> `dep_absolute` print global numbers; else if `!dep_has_spanned` print local
> numbers; else print span pattern with window numbers and local numbers (special
> case when `dep_parent==DEP_NO_PARENT`, points to itself). ID/relations: if
> `print_ids` or parent is CT_RELATED, print ` ID:<global_number>` and, for each
> relation, ` R:<relation-name-tag>:<target-id>`. Trace: if `trace`, for each
> `hit_by` entry print a space then `printTrace(output, hit_by)`. Emit newline.
> Finally if `reading->next` exists, set `next->deleted = reading->deleted` and
> recurse `printReading(next, output, sub+1)`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-single-window-fn]
> void GrammarApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-single-window-fn]
> Prints one SingleWindow. First emit variable commands: for each `var` in
> `window->variables_output`, look up the key tag; find `var` in
> `window->variables_set`: if found and its value is not `grammar->tag_any`, build
> `<STR_CMD_SETVAR><key>=<value>>`; if found and value is tag_any, build
> `<STR_CMD_SETVAR><key>>`; if not found, build `<STR_CMD_REMVAR><key>>`; emit
> each via printStreamCommand. If `window->text` is non-empty and has a
> non-whitespace char, printPlainTextLine it. Then printCohort each cohort in
> `window->all_cohorts` (passing through `profiling`). If `window->text_post` is
> non-empty and non-whitespace, printPlainTextLine it. If `window->flush_after`,
> emit `STR_CMD_FLUSH` via printStreamCommand. Finally `u_fflush(output)`.
> Virtual; no return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-stream-command-fn]
> void GrammarApplicator::printStreamCommand(UStringView cmd, std::ostream& output)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-stream-command-fn]
> Writes a stream command line: `u_fprintf(output, "%S\n", cmd.data())` — emits
> the UTF-16 string `cmd` followed by a newline. Virtual; no return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-trace-fn]
> void GrammarApplicator::printTrace(std::ostream& output, uint32_t hit_by)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-trace-fn]
> Prints a single trace token identifying what applied a change. If
> `hit_by < grammar->rule_by_number.size()` it is a real rule: `r =
> grammar->rule_by_number[hit_by]`; print `keywords[r->type]` (the rule keyword).
> If `r->type` is one of K_ADDRELATION/K_SETRELATION/K_REMRELATION (or the plural
> …RELATIONS), print `(<first maplist tag>` and, for the plural forms,
> `,<first sublist tag>`, then `)` — the tag strings come from
> `r->maplist->getNonEmpty().begin()->first->tag` and `r->sublist->…`. Then,
> unless (`trace_name_only` is set and `r->name` is non-empty), print `:<r->line>`.
> If `r->name` is non-empty, print `:` then the name. Otherwise (hit_by is out of
> range) it encodes an enclosure pass: compute
> `pass = UINT32_MAX - hit_by` and print `ENCL:<pass>`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.profile-rule-context-fn]
> void profileRuleContext(bool test_good, const Rule* rule, const ContextualTest* test)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.profile-rule-context-fn]
> Records profiling counters for a contextual test's outcome. No-op unless
> `profiler` is set. Builds key `k = { ET_CONTEXT, test->hash }` and looks it up
> in `profiler->entries`; if absent, does nothing. If present (reference `t`),
> compute the effective match accounting for negation: the condition
> `(test_good && !(test->pos & POS_NEGATE)) || (!test_good && (test->pos & POS_NEGATE))`
> means "counts as a match". When it holds: `++t.num_match`; if
> `t.example_window` is still 0, call `addProfilingExample(t)` to capture a
> window snapshot; then `++profiler->rule_contexts[{ rule->number + 1, test->hash }]`.
> Otherwise `++t.num_fail`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.readings-plain-t]
> typedef bc::flat_map<uint32_t, Reading*> readings_plain_t

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-dependency-window-fn]
> void reflowDependencyWindow(uint32_t max = 0)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-dependency-window-fn]
> Resolves the raw dep_self/dep_parent numbers collected during input into actual
> parent/child links across `gWindow->dep_window`. If `dep_delimit && !max &&
> !input_eof` and the last of `gWindow->next` has more than 1 cohort, set `max` to
> that window's first real cohort's global_number (a cutoff). Ensure a synthetic
> root entry `[0]` exists in both `gWindow->dep_window` and `gWindow->cohort_map`,
> pointing at the appropriate `cohorts[0]` (with a careful 2-step assignment to
> avoid evaluation-order segfaults noted in the source comment). Then iterate over
> `dep_window` in outer passes: skip cohorts already `CT_DEP_DONE` or with no
> `dep_self`; clear `dep_map`; scan forward building `dep_map[cohort->dep_self] =
> cohort->global_number` and normalizing `dep_self = global_number`, stopping at a
> cohort >= `max`, or at a duplicate dep_self. If dep_map ended empty, break. Set
> `dep_map[0]=0`. Second inner pass over the same range: for each cohort with a
> real dep_parent whose `dep_self == global_number`: if its `dep_parent` is not in
> `dep_map` (parent doesn't exist), warn (if verbose) and set `dep_parent =
> DEP_NO_PARENT`; else translate `dep_parent` through `dep_map` to the real
> global number, register the child on the resolved parent via `addChild`, and
> mark `CT_DEP_DONE`. After all passes, clear `dep_map` and `dep_window`. No
> return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-reading-fn]
> void reflowReading(Reading& reading)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-reading-fn]
> Rebuilds all derived tag structures of a reading from its `tags_list`. Clears
> `tags`, `tags_plain`, `tags_textual`, `tags_numerical`, `tags_bloom`,
> `tags_textual_bloom`, `tags_plain_bloom`, sets `mapping = nullptr`, clears
> `tags_string`. `insert_if_exists(reading.parent->possible_sets,
> grammar->sets_any)` (mark the (*)-set as possible). Move the current `tags_list`
> out into a temp `tlist` (swap, leaving tags_list empty), then for each tag hash
> in `tlist` call `addTagToReading(reading, tter, false)` (rehash=false, which
> re-appends into tags_list and rebuilds all the derived structures/flags). Finally
> `reading.rehash()`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-relation-window-fn]
> void reflowRelationWindow(uint32_t max = 0)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-relation-window-fn]
> Resolves named relation targets (numeric ids collected during input) into actual
> cohort links. If `!max && !input_eof` and the last of `gWindow->next` has >1
> cohort, set `max` to that window's `cohorts[0]->global_number`. Find the
> leftmost cohort by walking `gWindow->current->cohorts[1]` back through `prev`.
> Then walk forward via `next`: stop when a cohort's `global_number >= max` (if
> max set). For each cohort, iterate its `relations_input` map: get a fresh scoped
> `uint32SortedVector newrel` (`ss_u32sv.get()`); for each numeric target in the
> entry, look it up in `gWindow->relation_map` — if found, insert the mapped
> cohort id into `cohort->relations[name]`; else keep the raw target in `newrel`
> (deferred, its window not yet seen). If `newrel` ended empty, erase this
> `relations_input` entry; otherwise replace the entry's target set with `newrel`
> and advance. No return. (Note: unlike reflowDependencyWindow, `max` here is taken
> from `cohorts[0]` rather than `cohorts[1]`.)

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-cohort-fn]
> void reflowTextuals_Cohort(Cohort& c)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-cohort-fn]
> Calls `reflowTextuals_Reading` on every reading of a cohort across all four
> reading lists: `readings`, `deleted`, `ignored`, and `delayed`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-fn]
> void reflowTextuals()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-fn]
> Reflows textual tags across the entire window set: calls
> `reflowTextuals_SingleWindow` on each window in `gWindow->previous`, then on
> `gWindow->current`, then on each in `gWindow->next`. No return. (Invoked from
> addTag when a newly-added regex/icase tag causes existing tags to become
> textual.)

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-reading-fn]
> void reflowTextuals_Reading(Reading& r)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-reading-fn]
> Rebuilds the `tags_textual` sets after tags have newly been marked T_TEXTUAL.
> If `r.next` exists, recurse into it first. Then for each tag hash in `r.tags`,
> look the tag up; if its type has T_TEXTUAL, insert the hash into `r.tags_textual`
> and `r.tags_textual_bloom`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-single-window-fn]
> void reflowTextuals_SingleWindow(SingleWindow& sw)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-single-window-fn]
> Calls `reflowTextuals_Cohort` on every cohort in `sw.all_cohorts`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reset-indexes-fn]
> void GrammarApplicator::resetIndexes()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reset-indexes-fn]
> Clears the per-run match caches to bound memory growth. For each set in
> `index_readingSet_yes` call `sv.clear()`; for each set in
> `index_readingSet_no` call `sv.clear()` (these are per-set-number hash sets, so
> the outer vectors keep their size but each inner set is emptied). Then
> `.clear()` on `index_regexp_yes`, `index_regexp_no`, `index_icase_yes`, and
> `index_icase_no`. No return. (Called periodically from runGrammarOnText every
> `resetAfter = (num_windows+4)*2+1` windows.)

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.rs-type]
> typedef std::map<int32_t, uint32IntervalVector> RSType

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn]
> Cohort* runContextualTest(SingleWindow* sWindow, size_t position, const ContextualTest* test, Cohort** deep = nullptr, Cohort* origin = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn]
> The main contextual-test dispatcher: locates the anchoring cohort, applies the
> test, and returns the matched cohort (or nullptr, encoded per the negation
> rules). If POS_UNKNOWN (`?` with no override), print error and CG3Quit(1).
> `retval=true`. JUMP handling (POS_JUMP): resolve a jump cohort `j` from
> `jump_pos` — JUMP_MARK->`get_mark()`, JUMP_ATTACH->`get_attach_to().cohort`,
> JUMP_TARGET->the nearest `is_with` target on the context stack, else a numbered
> context slot from `context_stack[size-2]`; if found, set `sWindow=j->parent` and
> `position=j->local_number`, else `retval=false`. Compute `pos = position +
> offset`. If jump failed, skip to the tail. Selection of the candidate cohort:
> if `test->tmpl`, run `runContextualTest_tmpl` on it; else if `test->ors` is
> non-empty, try each OR-branch via `runContextualTest_tmpl` (clearing
> `dep_deep_seen` each time) until one returns a cohort; else
> `getCohortInWindow(sWindow, position, test, pos)` computes the cohort at
> `position+offset` honoring absolute/span/window-crossing. If no cohort,
> retval=false. Otherwise (non-tmpl/non-ors path): apply POS_PASS_ORIGIN (origin =
> window's initial cohort), set `*deep`, and if inside a template update
> `tmpl_cntx.min/max` from the cohort and deep. Then dispatch by position class:
> DEP_PARENT/DEP_GLOB combinations pick a CohortIterator (depAncestorIters/
> depParentIters/depDescendentIters); DEP_CHILD|DEP_SIBLING -> `runDependencyTest`;
> LEFT_PAR|RIGHT_PAR -> `runParenthesisTest`; RELATION -> `runRelationTest`;
> BAG_OF_TAGS -> `doesSetMatchReading` on the window bag (optionally spanning),
> then follow `test->linked`; offset==0 scanning (SCANFIRST|SCANALL) -> a
> bidirectional left/right scan calling `runSingleTest` per position, spanning
> windows when SPAN flags or `always_span`, breaking on TRV_BREAK; otherwise pick a
> topologyLeft/topologyRight/plain CohortIterator by offset sign. When an iterator
> `it` is used: reset it; optionally test POS_SELF first; then iterate cohorts
> calling `runSingleTest`, honoring POS_LEFT/RIGHT ordering guards, POS_ALL (all
> must pass) and POS_NONE (none may pass) and TRV_BREAK; POS_NONE with nothing
> seen flips to a match on the anchor. Tail (`label_gotACohort`): if no cohort,
> retval=false; `(POS_NOT && !linked && !cohort)` inverts retval; POS_NEGATE
> inverts retval. Finally: if `!retval` return nullptr; else if no cohort return
> `sWindow->cohorts[0]` (the window's initial cohort as a truthy sentinel); else
> return the cohort.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
> Cohort* runContextualTest_tmpl(SingleWindow* sWindow, size_t position, const ContextualTest* test, ContextualTest* tmpl, Cohort*& cdeep, Cohort* origin)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
> Runs one template (or OR-branch) sub-test `tmpl` on behalf of the outer `test`.
> Save `tmpl_cntx.min/max/in_template`; set `in_template=true`; if `test->linked`,
> push it onto `tmpl_cntx.linked`. Save `tmpl`'s original pos/offset/cbarrier/
> barrier. If the outer test has POS_TMPL_OVERRIDE, overwrite `tmpl`'s pos with the
> outer pos minus NEGATE/NOT/JUMP, copy the offset, force POS_SCANALL when the
> offset is nonzero and not already scanning/absolute, and copy any cbarrier/
> barrier. Run `cohort = runContextualTest(sWindow, position, tmpl, &cdeep,
> origin)`. If POS_TMPL_OVERRIDE, restore `tmpl`'s saved fields, and if a cohort
> was found but it fails `posOutputHelper(sWindow, position, test, cohort, cdeep)`
> (nonzero offset), null the cohort. If `test->linked`, pop it back off
> `tmpl_cntx.linked`. If no cohort was found, restore the saved
> `tmpl_cntx.min/max/in_template`. Return cohort.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
> Cohort* runDependencyTest(SingleWindow* sWindow, Cohort* current, const ContextualTest* test, Cohort** deep = nullptr, Cohort* origin = nullptr, const Cohort...

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn]
> Runs a dependency-relative contextual test (children/siblings, possibly deep).
> `self` tracks the recursion origin: if passed and equal to `current`, return 0;
> if not passed, `self = current`. For POS_DEP_DEEP, guard against revisiting via
> `dep_deep_seen` keyed by `(test->hash, current->global_number)` (return 0 on
> repeat, else insert). If POS_SELF and not left/right-restricted, run the test on
> `current` itself first; return the result cohort on success, or 0 if a barrier
> was hit. Choose the candidate set `deps`: POS_DEP_CHILD -> `current->dep_children`;
> else the siblings = the children of `current`'s parent (root's children when
> dep_parent==0; else look up the parent in cohort_map — warn+return 0 if it has
> no children). For left/right/rightmost restricted tests, rebuild `deps` from the
> window's `cohort_map` filtering by `less_Cohort` ordering (optionally add self,
> optionally reverse for RIGHTMOST). Then iterate `deps`: skip self unless
> POS_SELF; skip ids absent from `cohort_map` (warn); skip CT_REMOVED cohorts;
> compute a `good` flag that disallows crossing window boundaries unless the
> matching SPAN flag is present; if good, `runSingleTest` the candidate. For
> POS_ALL, every candidate must pass (else null and stop, keeping the last on
> success); otherwise the first passing candidate wins and breaks; a barrier
> `continue`s; POS_DEP_DEEP recurses into `runDependencyTest(cohort->parent,
> cohort, test, deep, origin, self)` and returns the first deep hit. Returns the
> matched cohort or nullptr.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-grammar-on-single-window-fn]
> uint32_t runGrammarOnSingleWindow(SingleWindow& current)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-single-window-fn]
> Runs the grammar's sections over one window, returning an RV_* mask when it must
> unwind (RV_DELIMITED/RV_TRACERULE) else 0. First, unless `no_before_sections`,
> run `runRulesOnSingleWindow(current, runsections[-1])` (BEFORE-SECTIONS); return
> early if it yields RV_DELIMITED|RV_TRACERULE. Then, unless `no_sections`, iterate
> the main `runsections` map (keys >= 0) with a fixpoint: a per-section `counter`
> caps re-runs at `section_max_count` when set; run each section via
> `runRulesOnSingleWindow`; return early on RV_DELIMITED|RV_TRACERULE; if a section
> produced RV_SOMETHING, restart the pass counter from that section (re-run
> cumulative sections), otherwise advance to the next section and reset `pass=0`;
> if `pass` reaches 1000, print an endless-loop warning (dumping the window's
> wordforms) and break. Finally, unless `no_after_sections`, run
> `runsections[-2]` (AFTER-SECTIONS), returning early on
> RV_DELIMITED|RV_TRACERULE. Return 0 if it completed normally. (Note: earlier
> sections' rules are preprocessed into later sections, so cumulative recursion is
> already baked into `runsections`.)

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-grammar-on-text-fn]
> virtual void runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-text-fn]
> The top-level CG-text stream driver: reads input line by line, builds windows of
> cohorts/readings, runs the grammar, and prints results. Virtual. Setup: store
> `ux_stdin`/`ux_stdout`; error+quit if input is null/eof/output null/no grammar;
> warn if the grammar has no delimiters. Call `index()`. Compute `resetAfter =
> (num_windows+4)*2+1`. Strip a leading BOM. If `fmt_output==CG3SF_BINARY`, pre-open
> an empty window. Main loop over lines (`get_line_clean` into `line`/`cleaned`,
> trimming trailing whitespace): (a) if `ignoreinput`, treat as text. (b) a cohort
> header `"<...>"`: validate the `"<` … `>"` shape (else warn and treat as text);
> finalize the previous cohort (magic reading if empty); enforce the soft limit
> (`soft_limit`, looking back for a soft delimiter and calling `delimitAt`, or
> breaking at the current cohort if it is a soft delimiter) and the hard limit
> (`hard_limit`, or a hard `delimiters` match unless `dep_delimit`), appending
> `endtag`, splitting mappings, closing the window; allocate a new window+cohort
> when needed (running `runGrammarOnWindow` and periodic `resetIndexes` once
> `gWindow->next` exceeds `num_windows+1`); build the new cohort's wordform via
> `addTag` and parse any trailing static-reading tags into `cohort->wread`. (c) a
> reading line ` "..."` (or `; "..."` deleted line when `pipe_deleted`): parse
> indent to attach as a sub-reading (chaining via `indents`), extract baseform and
> each space-separated tag via `addTag` — mapping tags go into `all_mappings`,
> others via `addTagToReading`; warn on missing baseform; handle `--dep-delimit`
> window splitting via `reflowDependencyWindow`. (d) otherwise it is
> text/commands: recognize FLUSH (flush all pending windows, print them, optionally
> emit the flush command), IGNORE/RESUME (toggle passthrough), EXIT (emit and jump
> to exit), SETVAR/REMVAR (parse identifier[=value] lists into
> `variables_set`/`variables_rem`/`variables_output` and immediate `variables`),
> and TEXT-DELIMITER regex matches (via `testStringAgainst(line, text_delimiters)`
> which uses unanchored `uregex_find`) that split the window; leftover text is
> attached to the current cohort/window text or printed. After EOF: finalize the
> last cohort/window, flush all remaining windows through `runGrammarOnWindow` and
> print+free them, emit any trailing variable commands, and (if verbose) print
> totals. Increments `numLines` per line. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-grammar-on-window-fn]
> void runGrammarOnWindow()

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-window-fn]
> Prepares and runs the grammar on `gWindow->current`, including parenthesis
> enclosure wrapping/unwrapping and dependency/relation reflow. `current =
> gWindow->current`; `did_final_enclosure=false`. Apply the window's stored
> variables into the global `variables` map (set/rem), plus the mapping-prefix
> variable. If `has_dep`, `reflowDependencyWindow()` and (unless at EOF) seed the
> next window's cohorts into `dep_window`. If `has_relations`,
> `reflowRelationWindow()`. Parenthesis wrapping: if the grammar defines
> parentheses, scan cohorts right-to-left for a left-paren whose matching
> right-paren exists, splice the enclosed cohorts out of `cohorts` (marking them
> CT_ENCLOSED and bumping their `enclosed` depth), set `has_enclosures`, and repeat.
> Reset the par_* trackers; `pass=0`. Main loop (`label_runGrammarOnWindow_begin`):
> print+free any previous windows beyond `num_windows`; clear `rule_hits` and
> `index_ruleCohort_no`; `indexSingleWindow(*current)`; clear `hit_external`;
> rebuild cohort links; `++pass` and bail with an endless-loop warning if
> `pass>1000`; if `trace_encl`, tag every reading with an ENCL pass marker. Run
> `rv = runGrammarOnSingleWindow(*current)`; if RV_DELIMITED, restart the main
> loop (the window was split). Enclosure unwrapping (`label_unpackEnclosures`): if
> the window has enclosures, find the next enclosure to unwrap, splice those
> cohorts back into `cohorts` at their original position, set the par_left/right
> tag+pos trackers, and re-run the window (or re-unpack if RV_TRACERULE); when no
> enclosure with depth 1 remains and `!did_final_enclosure`, clear par_* trackers,
> set `did_final_enclosure=true`, and re-run for the final pass. Finally, reinsert
> any CT_IGNORED cohorts back into the visible `cohorts` (clearing CT_IGNORED and
> re-registering in `cohort_map`), and if any were reinserted renumber local_numbers
> and `reflowDependencyWindow()`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-parenthesis-test-fn]
> Cohort* runParenthesisTest(SingleWindow* sWindow, const Cohort* current, const ContextualTest* test, Cohort** deep = nullptr, Cohort* origin = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-parenthesis-test-fn]
> Runs a test against the enclosing parenthesis boundary. If `current->local_number`
> is outside `[par_left_pos, par_right_pos]`, return 0. Pick the boundary cohort:
> `sWindow->cohorts[par_left_pos]` for POS_LEFT_PAR, else
> `sWindow->cohorts[par_right_pos]`. Run `runSingleTest` on it; if it matched,
> return that cohort, else return nullptr.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-relation-test-fn]
> Cohort* runRelationTest(SingleWindow* sWindow, Cohort* current, const ContextualTest* test, Cohort** deep = nullptr, Cohort* origin = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-relation-test-fn]
> Runs a test against cohorts named-related to `current`. If `current` is not
> CT_RELATED or has no relations, return 0. Snapshot `regexgrp_ct`. Resolve the
> relation-name tag `rtag` (varstring-expand it). Collect related cohorts into a
> CohortSet `rels`: if `rtag` is tag_any, take all relation targets; if T_REGEXP,
> for each relation name test `doesTagMatchRegexp(name, *rtag, caps!=0)` and, on
> match, clamp `regexgrp_ct` to `min(regexgrp_ct, regexgrpz+caps)` (see note); else
> take exactly the `rtag.hash` relation's targets — each resolved through
> `cohort_map`. Apply position filters: POS_LEFT keeps rels left of current,
> POS_RIGHT keeps rels right of current, POS_SELF inserts current, POS_LEFTMOST
> keeps only the first, POS_RIGHTMOST only the last. Then iterate `rels` running
> `runSingleTest` on each: POS_ALL requires all to pass (else null), otherwise the
> first passing wins and breaks. If nothing matched, restore `regexgrp_ct` to the
> snapshot. Return the matched cohort or nullptr. NOTE (apparent quirk, do not
> fix): the regex branch uses `std::min(regexgrp_ct, UI8(regexgrpz + caps))`, which
> reduces rather than advances the capture-group count.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn]
> uint32_t runRulesOnSingleWindow(SingleWindow& current, const uint32IntervalVector& rules)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn]
> Applies a set of `rules` (one section's rule numbers) to `current` window and
> returns an RV_* bitmask (RV_NOTHING=1, RV_SOMETHING=2, RV_DELIMITED=4,
> RV_TRACERULE=8). Compute `intersects = current.valid_rules.intersect(rules)` —
> the rules actually applicable to this window. Set `cohort_map[0]` to the window's
> first cohort. Iterate `intersects` (iterator `iter_rules`, re-seekable when the
> rule set mutates). Per rule (a `repeat_rule:` label supports RF_REPEAT/
> should_repeat): skip if not in cmdline `valid_rules`, if K_IGNORE, if mappings/
> corrections disabled for its type, or per enclosure-final gating
> (`did_final_enclosure` vs RF_ENCL_FINAL). Set `current_rule`. Define local
> lambdas used by the callbacks: `reindex` (renumber local_numbers +
> rebuildCohortLinks), `collect_subtree` (gather a WithChild subtree via
> `doesSetMatchCohortNormal`+`isChildOf`), `add_cohort` (build a cohort from the
> rule maplist for ADDCOHORT/MERGECOHORTS incl. dep/relation attachment and
> insertion before/after the target), `rem_cohort` (mark CT_REMOVED, reparent
> children, erase from indexes and cohorts, renumber), plus `reset_cohorts`.
> Provide `reading_cb` and `cohort_cb` that switch on `rule->type` to perform the
> action: SELECT/IFF keep the matched readings and delete the rest; REMOVE/IFF
> delete matched readings (respecting UNMAPLAST/DELAYED/IGNORED); PROTECT/UNPROTECT/
> UNMAP flag readings; ADD/MAP/REPLACE/SUBSTITUTE/APPEND/COPY edit tag lists (using
> FILL_TAG_LIST/APPEND_TAGLIST_TO_READING which also call `updateValidRules`);
> ADDCOHORT/SPLITCOHORT/MERGECOHORTS/COPYCOHORT build cohorts and set
> `reset_cohorts_for_loop`; REMCOHORT removes one; DELIMIT calls `delimitAt` and
> sets `delimited`; JUMP/EXTERNAL/SET-/REM-VARIABLE and the parent/child/relation
> rules (SETPARENT/SETCHILD/ADD|SET|REM RELATION(S), REMPARENT/SWITCHPARENT,
> MOVE/SWITCH) mutate dependency/relation/variable state; TRACE is recorded on
> readings. Run the rule via `rv = runSingleRule(current, *rule, reading_cb,
> cohort_cb)`. If `rv || readings_changed`, mark `section_did_something` (unless
> RF_NOITERATE or single-run) and `rule_did_something`. Handle `should_bail`
> (bailout: zero the rule's hit count and clear `index_ruleCohort_no`) and
> `should_repeat`. If the rule did something, re-seek `iter_rules` and, if its line
> is in `trace_rules`, set RV_TRACERULE. If `delimited`, break. If the rule did
> something and is RF_REPEAT, clear `index_ruleCohort_no` and `goto repeat_rule`.
> If RV_TRACERULE was set, break. A `Sorter` guard re-sorts every
> `rule_to_cohorts` set after each rule when needed. Finally OR RV_SOMETHING if
> `section_did_something` and RV_DELIMITED if `delimited`; return `retval`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
> bool runSingleRule(SingleWindow& current, const Rule& rule, RuleCallback reading_cb, RuleCallback cohort_cb)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn]
> Walks the candidate cohorts of one rule, tests each cohort's readings against
> target+contexts, and invokes the caller-supplied `reading_cb`/`cohort_cb` to
> perform the rule action. Returns whether anything changed. Set
> `finish_cohort_loop=true`, `anything_changed=false`. The candidate set is
> `current.rule_to_cohorts[rule.number]`, unless `in_nested` (WITH), in which case
> a fresh `nested_rule_to_cohorts` is built from the current apply-target plus any
> T_CONTEXT-referenced cohorts. Push the cohortset onto `cohortsets` and a null
> iterator onto `rocits` (popped by a scope guard). Loop `rocit` over the
> cohortset (its live index is exposed via `rocits.back()`): fetch the cohort,
> pre-increment `rocit`; set `finish_reading_loop=true`. Skip the cohort for many
> reasons: it is the initial `>>>` (local_number 0); CT_REMOVED/CT_IGNORED;
> CT_ENCLOSED or belonging to another window; no readings; RESTORE with the
> relevant list empty; the target set not in `possible_sets` (when sub_reading==0);
> single-reading Select/safe-Remove/Iff no-ops; DELIMIT at the final cohort;
> ENCL_INNER/OUTER parenthesis constraints; SETPARENT-SAFE/NOPARENT with an
> existing parent; REMPARENT/SWITCHPARENT with no parent. Then a per-(rule,cohort)
> negative cache `index_ruleCohort_no` keyed by `hash_value(rule.number,
> global_number)` skips cohorts known not to match (inserted eagerly). Reset
> per-cohort scratch (readings_plain, subs_any, regexgrps_*, unif_*). Push a
> Rule_Context for this cohort. First inner loop over readings: descend to the
> sub-reading (`get_sub_reading`), clear its `matched_target`/`matched_tests`, skip
> mapped/noprint/immutable readings per rule type (immutable ones still count for
> Select/Protect etc.). Reuse cached results for readings with an identical plain
> signature (`readings_plain`). Otherwise reset regex/unif context, set
> `same_basic`/`mark`, mark the reading chain active, and if the rule has a target
> and `doesSetMatchReading` matches, run the rule's contextual `tests` in order
> via `runContextualTest` (pushing each result onto the context list, profiling,
> and — unless RF_KEEPORDER — reordering a failing test to the front for the next
> cohort); a reading that matches target and all tests is counted `num_active` and
> its context saved in `reading_contexts`; Iff flips to Select on first success.
> After the reading loop, detect state changes (reading/deleted/delayed/ignored
> counts) to set `anything_changed`. If no reading was a valid target (and not
> Iff), erase this cohort from the cohortset (unless immutable/target-only) and
> continue. If EVERY reading matched, Select and safe-Remove are no-ops (continue).
> Otherwise, second loop over `reading_contexts`: for each whose subreading
> matched (and matched_tests unless Iff), set it as `context_stack.back()` and call
> `reading_cb()`; honor the traversal flags it sets — `finish_cohort_loop=false`
> aborts the whole rule (return), `reset_cohorts_for_loop` re-derives the cohortset
> and re-seeks `rocit`, `finish_reading_loop=false` stops the reading loop. Then
> call `cohort_cb()` once, honoring the same flags. Pop the context and continue.
> Return `anything_changed`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-single-test-fn]
> Cohort* runSingleTest(Cohort* cohort, const ContextualTest* test, uint8_t& rvs, bool* retval, Cohort** deep = nullptr, Cohort* origin = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-test-fn]
> Evaluates a contextual test against ONE cohort and updates traversal-state bits
> `rvs` and `*retval`. Snapshot `regexgrp_ct`. Side-setup from `test->pos`: if
> POS_MARK_SET, `set_mark(cohort)`; if POS_ATTACH_TO and the current attach target
> differs from this cohort, clear `matched_target`/`matched_tests` on every reading
> across the readings/deleted/delayed/ignored lists (gated by POS_LOOK_*); if
> POS_WITH, `merge_with = cohort`; if `deep`, `*deep = cohort`. Build a
> `dSMC_Context context = { test, deep, origin, test->pos }`. Match: if POS_CAREFUL
> use `doesSetMatchCohortCareful` (and, if it fails and POS_SCANFIRST is set, set
> `did_test` and run `doesSetMatchCohortNormal` once just to populate
> `matched_target`); else `doesSetMatchCohortNormal`. Then compute break flags: if
> `origin` and the test has a nonzero offset or is scanning and origin==cohort and
> origin isn't the initial cohort, null the cohort and set TRV_BREAK. If
> matched_target and POS_SCANFIRST, set TRV_BREAK; else if the test is not any of
> SCANALL/SCANFIRST/DEP_DEEP/DEP_GLOB, set TRV_BREAK|TRV_BREAK_DEFAULT. Remember
> `broken`. Reset context fields and set `did_test=true`. Barriers: if
> `test->barrier` and cohort, run a barrier context (non-careful) via
> `doesSetMatchCohortNormal`; if it matches set `seen_barrier=true`, add
> TRV_BREAK|TRV_BARRIER and clear TRV_BREAK_DEFAULT. Similarly `test->cbarrier`
> via `doesSetMatchCohortCareful`. If matched_target and *retval, set TRV_BREAK.
> If not broken and TRV_BARRIER and the test is self-including both directions
> (MASK_SELF_NB), clear TRV_BREAK|TRV_BARRIER. If `!*retval`, restore the
> snapshotted `regexgrp_ct`. Return the (possibly nulled) cohort. The
> `(sWindow, i, ...)` overload bounds-checks `i` against `sWindow->cohorts.size()`
> (out of range -> TRV_BREAK, *retval=false, return 0) then forwards to the cohort
> overload.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-attach-to-fn]
> void set_attach_to(Reading* reading, Reading* subreading = nullptr)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-attach-to-fn]
> Records the attach target on the current rule context. If `context_stack` is
> non-empty, set `context_stack.back().attach_to` fields: `.cohort =
> reading->parent`, `.reading = reading`, `.subreading = subreading` (subreading
> defaults to nullptr). No-op if the stack is empty. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-grammar-fn]
> void GrammarApplicator::setGrammar(Grammar* res)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-grammar-fn+1]
> Attaches a Grammar and derives cached tags/indexes. Sets `grammar = res`.
> Interns the special tags: `tag_begin = addTag(STR_BEGINTAG)`,
> `tag_end = addTag(STR_ENDTAG)`, `tag_subst = addTag(STR_DUMMY)`, and stores
> their hashes into `begintag`, `endtag`, `substtag`. (Port de-warting: the
> `tag_end`/`tag_subst` `TagId` caches are dropped — only `tag_begin` is
> retained, since the run path routes the end/subst tags through the `endtag`/
> `substtag` hash forms — so the port interns all three tags but stores only the
> `tag_begin` id alongside the three hashes.) Builds the mapping-prefix
> tags: `mprefix_key = addTag(u"_MPREFIX")->hash` and
> `mprefix_value = addTag(UString{grammar->mapping_prefix})->hash`. Clears and
> resizes `index_readingSet_yes` and `index_readingSet_no` to
> `grammar->sets_list.size()` each. If `res->text_delimiters` is set, collect its
> tags via `trie_getTagList` over both `trie` and `trie_special`, and for each
> tag compile an ICU regex with `uregex_open(t->tag.data(), size, flags, &pe,
> &status)` where `flags = (t->type & T_CASE_INSENSITIVE) ? UREGEX_CASE_INSENSITIVE
> : 0`, pushing each into `text_delimiters`; on any `uregex_open` error print an
> error and `CG3Quit(1)`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-mark-fn]
> void set_mark(Cohort* cohort)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-mark-fn]
> Sets the `mark` cohort on the current rule context: if `context_stack` is
> non-empty, `context_stack.back().mark = cohort`. No-op if empty. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-options-fn]
> void GrammarApplicator::setOptions(UConverter* conv)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-options-fn]
> Populates all the runtime option members from the global `Options::options`
> table. If no `UConverter* conv` is passed, open the default converter and mark
> it for deletion at the end. Then, for each option's `doesOccur` flag, set the
> corresponding member (resetting several to defaults first): ALWAYS_SPAN->
> always_span; UNICODE_TAGS->unicode_tags (reset false first); UNIQUE_TAGS->
> unique_tags; NOMAPPINGS->apply_mappings=false (default true); NOCORRECTIONS->
> apply_corrections=false; NOBEFORESECTIONS/NOSECTIONS/NOAFTERSECTIONS->the
> no_*_sections flags; UNSAFE->unsafe; ORDERED->ordered; TRACE->trace=true and, if
> it has a value, parse a ranged list into `trace_rules` (base-10);
> TRACE_NAME_ONLY/TRACE_NO_REMOVED/TRACE_ENCL each set trace=true plus their
> flag; PIPE_DELETED->pipe_deleted; DRYRUN->dry_run; SINGLERUN->
> section_max_count=1; MAXRUNS->section_max_count=stoul(value); SECTIONS->parse
> ranged into `sections`; RULES->parse ranged into `valid_rules`; RULE-> if the
> value starts with a digit push_back stoi(value) into valid_rules, else convert
> the value to UChars via `conv` and push every grammar rule whose `name` matches;
> DEBUG_RULES->parse ranged into `debug_rules`; VERBOSE->verbosity_level (value or
> 1); DODEBUG->debug_level (value or 1, prints "Debug level set to N");
> PRINT_IDS->print_ids; PRINT_DEP->has_dep; NUM_WINDOWS/SOFT_LIMIT/HARD_LIMIT->
> stoul into their members; TEXT_DELIMIT->build the rx (default
> STR_TEXTDELIM_DEFAULT or the converted value) and call setTextDelimiter;
> DEP_DELIMIT->dep_delimit (value or 10) and parse_dep=true; DEP_ABSOLUTE->
> dep_absolute; DEP_ORIGINAL->dep_original; DEP_ALLOW_LOOPS->dep_block_loops=false;
> DEP_BLOCK_CROSSING->dep_block_crossing=true; MAGIC_READINGS->
> allow_magic_readings=false; NO_PASS_ORIGIN->no_pass_origin; SPLIT_MAPPINGS->
> split_mappings; SHOW_END_TAGS->show_end_tags; NO_BREAK->add_spacing=false. If
> the converter was opened here, `ucnv_close` it. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-text-delimiter-fn]
> void GrammarApplicator::setTextDelimiter(UString rx)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-text-delimiter-fn]
> Replaces the text-delimiter regex set from a single pattern string `rx`. First
> `uregex_close` every existing entry of `text_delimiters` and clear the vector.
> If `rx` is empty, return. Then optional `/.../flags` unwrapping: if
> `rx.size() >= 3` and the first char is `/`, erase the leading `/`, then repeat:
> while the last char is `/`, `r`, or `i` — if it is `i` set `icase=true`; if it
> is `/` pop it and break; otherwise (an `r`) pop it and continue. (This strips a
> trailing `/`, `/r`, `/i`, `/ri`, etc., recording only case-insensitivity.)
> Compile with `uregex_open(rx.data(), size, flags, &pe, &status)` where
> `flags = icase ? UREGEX_CASE_INSENSITIVE : 0`, push the result into
> `text_delimiters`; on `uregex_open` error print and `CG3Quit(1)`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.split-all-mappings-fn]
> void splitAllMappings(all_mappings_t& all_mappings, Cohort& cohort, bool mapped = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.split-all-mappings-fn]
> Applies splitMappings for every reading in a cohort that has pending mappings.
> If `all_mappings` is empty, return. Snapshot `cohort.readings` into a
> thread_local `readings` list (because splitMappings appends to
> cohort.readings). For each snapshot reading, look it up in `all_mappings`; skip
> if absent; else call `splitMappings(iter->second, cohort, *reading, mapped)`.
> Sort `cohort.readings` by `Reading::cmp_number`. If `grammar->reopen_mappings`
> is non-empty, for each reading whose `mapping` hash is in reopen_mappings, set
> `reading->mapped = false` (so those mappings can be re-applied). Finally clear
> `all_mappings`. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.split-mappings-fn]
> void splitMappings(TagList& mappings, Cohort& cohort, Reading& reading, bool mapped = false)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.split-mappings-fn]
> Given a list of candidate mapping tags for a reading, splits them so each
> mapping tag ends up on its own copy of the reading. First pass over `mappings`:
> for each entry, varstring-expand it fully (while T_VARSTRING, regenerate); if it
> is NOT actually a mapping tag (not T_MAPPING and first char != mapping_prefix),
> add it directly to `reading` via `addTagToReading` and erase it from `mappings`;
> else keep it. If `reading.mapping` is already set, push it onto `mappings` and
> `delTagFromReading` it (so the existing mapping participates in the split).
> Pop the last mapping tag into `tag` (this one stays on the original reading).
> For each remaining mapping tag `ttag` (there are `i = mappings.size()` of them,
> counting down): skip if the cohort already has a reading with the same
> `hash_plain` and an equal mapping (dedup). Otherwise clone the reading
> (`alloc_reading`), set `nr->mapped = mapped`, set `nr->number = reading.number -
> i`, add `ttag` via `addTagToReading` (updating `nr->mapping` to the resolved
> tag), append the new reading to the cohort, and `++numReadings`. Finally set
> `reading.mapped = mapped`, add the held-back `tag` to the original reading, and
> set `reading.mapping` accordingly. No return.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.st-retvals]
> enum ST_RETVALS {
>   TRV_BREAK = (1 << 0);
>   TRV_BARRIER = (1 << 1);
>   TRV_BREAK_DEFAULT = (1 << 2);
> }

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.unmap-reading-fn]
> bool unmapReading(Reading& reading, const uint32_t rule)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.unmap-reading-fn]
> Removes mapping state from a reading. `readings_changed=false`. If
> `reading.mapping` is set: clear `reading.noprint`, call
> `delTagFromReading(reading, reading.mapping->hash)`, set changed=true. If
> `reading.mapped` is true: set it false, changed=true. If anything changed, push
> `rule` onto `reading.hit_by`. Returns `readings_changed`.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.update-rule-to-cohorts-fn]
> bool updateRuleToCohorts(Cohort& c, const uint32_t& rsit)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.update-rule-to-cohorts-fn]
> Registers cohort `c` as a candidate for rule number `rsit` in its window's
> `rule_to_cohorts` index. If `valid_rules` is non-empty and does not contain
> `rsit`, return false. Let `current = c.parent`, `r =
> grammar->rule_by_number[rsit]`; if `!doesWordformsMatch(c.wordform, r->wordform)`,
> return false. If `current->rule_to_cohorts` isn't sized to include `rsit`, call
> `indexSingleWindow(*current)` to (re)build it. Take the target `CohortSet&
> cohortset = current->rule_to_cohorts[rsit]`. Because live iterators (`rocits`)
> may be pointing into this exact cohortset, find every index in `cohortsets`
> equal to `&cohortset`; for those, snapshot each active iterator: if it is at/past
> the end, remember it to reset to the new size, otherwise remember its current
> cohort. Insert `&c`. Fix up the remembered end-iterators to the new size, and if
> the container reallocated (capacity changed) re-find each remembered cohort's new
> position via `find_n`. (Without live iterators, just `cohortset.insert(&c)`.)
> Return `current->valid_rules.insert(rsit)` (true if the rule was newly marked
> valid for this window).

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.update-valid-rules-fn]
> bool updateValidRules(const uint32IntervalVector& rules, uint32IntervalVector& intersects, const uint32_t& hash, Reading& reading)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.update-valid-rules-fn]
> After a tag `hash` has been added to `reading`, discovers newly-applicable rules
> and merges them into the running `intersects` set. Remember `os =
> intersects.size()`. Look up `hash` in `grammar->rules_by_tag`; if found, for each
> rule number `rsit` there, call `updateRuleToCohorts(reading.parent, rsit)` and,
> if that returned true AND the caller's `rules` set contains `rsit`, insert `rsit`
> into `intersects`. Return whether `intersects` grew (`os != intersects.size()`),
> signaling the caller to refresh its rule iterator.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.would-parent-child-cross-fn]
> bool wouldParentChildCross(const Cohort* parent, const Cohort* child)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.would-parent-child-cross-fn]
> Tests whether attaching `parent`↔`child` would produce crossing dependency
> branches. Compute `mn = min(parent->global_number, child->global_number)` and
> `mx = max(...)`. Loop `i` from `mn+1` to `mx-1`: look up `parent->dep_parent` in
> `gWindow->cohort_map`; if found and its `dep_parent != DEP_NO_PARENT`, and that
> grandparent's `dep_parent` is `< mn` or `> mx`, return true. Otherwise return
> false. NOTE (apparent bug, describe as-is, do not fix): the loop variable `i` is
> never used inside the body — every iteration inspects `parent->dep_parent` (not
> the cohort at position `i`), so the check is either constant-true or repeated
> `mx-mn-1` times identically rather than scanning the intervening cohorts.

> [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.would-parent-child-loop-fn]
> bool wouldParentChildLoop(const Cohort* parent, const Cohort* child)

> [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.would-parent-child-loop-fn]
> Tests whether making `parent` the parent of `child` would create a dependency
> loop. Short-circuits: returns true if `parent->global_number ==
> child->global_number`; returns false if `parent->global_number ==
> child->dep_parent`; returns false if `parent->global_number ==
> parent->dep_parent`; returns true if `parent->dep_parent ==
> child->global_number`. Otherwise walk up from `inner = parent` for up to 1000
> iterations: if `inner->dep_parent` is 0 or DEP_NO_PARENT, return false; look up
> `inner->dep_parent` in `gWindow->cohort_map`, move `inner` there if found else
> break; if `inner->dep_parent == child->global_number`, return true. On hitting
> 1000 iterations, warn if `verbosity_level > 0`; returns the accumulated `retval`
> (default false).

> [spec:cg3:def:grammar-applicator.cg3.reading-spec]
> struct ReadingSpec {
>   Cohort* cohort = nullptr;
>   Reading* reading = nullptr;
>   Reading* subreading = nullptr;
> }

> [spec:cg3:def:grammar-applicator.cg3.regexgrps-t]
> typedef std::vector<UnicodeString> regexgrps_t

> [spec:cg3:def:grammar-applicator.cg3.rule-callback]
> typedef std::function<void(void)> RuleCallback

> [spec:cg3:def:grammar-applicator.cg3.rule-context]
> struct Rule_Context {
>   ReadingSpec target;
>   std::vector<Cohort*> context;
>   std::vector<Cohort*> dep_context;
>   ReadingSpec attach_to;
>   Cohort* mark = nullptr;
>   unif_tags_t* unif_tags = nullptr;
>   unif_sets_t* unif_sets = nullptr;
>   uint8_t regexgrp_ct = 0;
>   regexgrps_t* regexgrps = nullptr;
>   bool is_with = false;
> }

> [spec:cg3:def:grammar-applicator.cg3.tmpl-context-t]
> struct tmpl_context_t {
>   Cohort* min = nullptr;
>   Cohort* max = nullptr;
>   std::vector<const ContextualTest*> linked;
>   bool in_template = false;
> }

> [spec:cg3:def:grammar-applicator.cg3.tmpl-context-t.clear-fn]
> void clear()

> [spec:cg3:sem:grammar-applicator.cg3.tmpl-context-t.clear-fn]
> Resets the template-context struct to its default state: sets `min = nullptr`,
> `max = nullptr`, calls `linked.clear()` (empties the vector of pending linked
> ContextualTest pointers), and sets `in_template = false`. No return.

> [spec:cg3:def:grammar-applicator.cg3.unif-sets-t]
> typedef bc::flat_map<uint32_t, uint32SortedVector> unif_sets_t

> [spec:cg3:def:grammar-applicator.cg3.unif-tags-t]
> typedef bc::flat_map<uint32_t, const void*> unif_tags_t

