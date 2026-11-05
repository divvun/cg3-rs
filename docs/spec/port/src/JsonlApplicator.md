# src/JsonlApplicator.cpp, src/JsonlApplicator.hpp

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator]
> class JsonlApplicator : public virtual GrammarApplicator

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.add-single-mapping-fn]
> void JsonlApplicator::addSingleMapping(Reading& reading, TagList& mappings)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.add-single-mapping-fn]
> Gives a reading that cannot be split into several readings (a sub-reading
> or a deleted reading) the first of the mapping tags collected for it. If
> `mappings` is empty, return. Otherwise, for each mapping tag after the
> first, in order, warn "Warning: Mapping tag <tag> on line <numLines> will be
> discarded, as this reading can only have one.", where `<tag>` is the tag's
> text. Then `addTagToReading(reading, mappings.front())` and clear
> `mappings`.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn]
> void JsonlApplicator::buildJsonReading(const Reading* reading, json::Value& reading_json, json::Document::AllocatorType& allocator)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn+1]
> Fills the JSON object `reading_json` (asserted to already be an object) for
> one reading. Members are added in this order, each only under its
> condition: `"l"`, `"ts"`, `"tr"`, `"s"`.
> Baseform (`"l"`, always added): if `reading->baseform` is non-zero and
> `grammar->single_tags` has it, take that tag's text, and if the text is at
> least 2 characters long and both starts and ends with `"`, drop the first
> and the last character. With no baseform, or a hash that is not in
> `single_tags`, the value is the empty string.
> Tags (`"ts"`): an array filled by `buildJsonTags(reading, ...)`, added only
> if it is non-empty.
> Trace (`"tr"`): only if `trace` is set and `reading->hit_by` is non-empty.
> An array with one string per entry of `hit_by`, in order: the text
> `printTrace` writes for that entry, such as `SELECT:12` or
> `ADDRELATION(subj):8`.
> Sub-reading (`"s"`): if `reading->next` is non-null, an object built by
> recursing on `reading->next`. The sub-reading of a sub-reading is nested in
> its own `"s"`. The recursion does not test the sub-reading's `deleted` or
> `noprint` flag.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn]
> void JsonlApplicator::buildJsonTags(const Reading* reading, json::Value& tags_json, json::Document::AllocatorType& allocator)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn+1]
> Fills the JSON array `tags_json` (asserted to already be an array) with a
> reading's printable tags as strings, mapping tags last. Walk
> `reading->tags_list` in order, keeping a `uint32SortedVector unique` and a
> `TagList mappings`. For each tag hash `tter`, skip it when:
> (`!show_end_tags` and `tter == endtag`) or `tter == begintag`; or `tter ==
> reading->baseform`; or `tter == reading->parent->wordform->hash`. If
> `unique_tags` is set, skip `tter` when it is already in `unique`, else
> insert it. Look up `tag = grammar->single_tags[tter]`; skip it if
> `(tag->type & T_DEPENDENCY)` and `has_dep` and not `dep_original`; skip it
> if `(tag->type & T_RELATION)` and `has_relations`. If `(tag->type &
> T_MAPPING)`, append `tag` to `mappings` and go on to the next hash;
> otherwise push the tag's text. After the walk, push the text of each tag in
> `mappings`, in order. The other tags keep their `tags_list` order, and each
> string is the tag's whole text, any NUL character included.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.get-json-string-fn]
> bool JsonlApplicator::getJsonString(const json::Value& obj, const char* key, UString& out)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.get-json-string-fn]
> Reads the string member `key` of the JSON object `obj` into `out`. Look the
> member up with `FindMember`, which finds the first member of that name. If
> there is none, return false, with no warning. If its value is not a string,
> warn "Warning: '<key>' on line <numLines> is not a string - ignored." and
> return false. Otherwise set `out` to the value decoded from UTF-8 over its
> full length (`GetString`, `GetStringLength`), so a NUL character inside the
> string is kept, and return true. `out` is unchanged when the result is
> false. In the port the value is a UTF-8 string already and is taken as it
> is.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
> JsonlApplicator::JsonlApplicator(std::ostream& ux_err)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
> Constructor. Delegates to the base `GrammarApplicator(ux_err)` constructor with
> an empty body; no subclass data members. (There is also an explicit empty
> destructor `~JsonlApplicator()` present only to anchor the vtable.) No side
> effects.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+1]
> Cohort* JsonlApplicator::parseJsonCohort(const json::Value& obj, SingleWindow* cSWindow)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+2]
> Builds a cohort of `cSWindow` from the cohort object `obj` and returns it.
> The caller has checked that `obj` has a non-empty string `"w"`, and appends
> the cohort to the window itself. Members are read in this order, which is
> also the order of their warnings: `"w"`, `"wb"`, `"z"`, `"sts"`, `"rs"`,
> `"id"`, `"rels"`, `"drs"`, `"ds"`, `"dp"`. Other members are ignored.
> Setup: `cCohort = alloc_cohort(cSWindow)`, `cCohort->global_number =
> gWindow->cohort_counter++`, `cCohort->line_number = numLines`,
> `++numCohorts`.
> Wordform (`"w"`): read with `getJsonString`; `cCohort->wordform =
> addTag("\"<" + w + ">\"")`.
> Word-bound blank (`"wb"`): if `getJsonString` reads it, `cCohort->wblank`
> is its value.
> Text (`"z"`): if `getJsonString` reads it, `cCohort->text` is its value
> followed by `"\n"`, so an empty `"z"` is one blank line.
> Static tags (`"sts"`): if present and not an array, warn "Warning: 'sts'
> (static tags) on line <numLines> is not an array - ignored.". Otherwise,
> for each element in order: if it is not a string, warn "Warning:
> Non-string found in 'sts' (static tags) array on line <numLines> -
> skipping." and go on; skip an empty string; if `cCohort->wread` is null,
> set it to `alloc_reading(cCohort)` and `addTagToReading` the wordform to
> it; then `addTagToReading(*cCohort->wread, addTag(str))`. Without a
> non-empty string, `wread` stays null.
> Readings (`"rs"`): if present and not an array, warn "Warning: 'rs'
> (readings) on line <numLines> is not an array - ignored.". Otherwise, with
> an empty `all_mappings_t all_mappings`, for each element in order: if it is
> not an object, warn "Warning: Non-object found in 'rs' (readings) array on
> line <numLines> - skipping." and go on; else, with a fresh `TagList
> mappings`, `reading = parseJsonReading(element, cCohort, mappings)`,
> `cCohort->appendReading(reading)`, set `all_mappings[reading] = mappings`
> if `mappings` is non-empty, and `++numReadings`. After the loop,
> `splitAllMappings(all_mappings, *cCohort, true)`: a reading with several
> mapping tags becomes one reading per mapping tag.
> Cohort id (`"id"`): if present and not an unsigned 32-bit integer, or 0,
> warn "Warning: 'id' on line <numLines> is not a positive integer -
> ignored.". Otherwise `gWindow->relation_map[id] = cCohort->global_number`.
> Relations (`"rels"`): if present and not an object, warn "Warning: 'rels'
> (relations) on line <numLines> is not an object - ignored.". Otherwise, for
> each member in order, repeated names included: if its value is not an
> array, warn "Warning: Relation targets on line <numLines> are not an array
> - ignored." and go on; else `name = addTag(<member name>)`, and for each
> element in order: if it is not an unsigned 32-bit integer, warn "Warning:
> Relation target on line <numLines> is not a non-negative integer -
> skipping."; else insert it into `cCohort->relations_input[name->hash]`.
> After the members, set `has_relations = true` and call
> `cCohort->setRelated()`. An object `"rels"`, even `{}`, therefore marks the
> cohort as related. `"id"` and `"rels"` are read before the next step so
> that `setRelated()` does not make the placeholder reading printable.
> Empty cohort: if `cCohort->readings` is empty, `initEmptyCohort(*cCohort)`.
> Then `insert_if_exists(cCohort->possible_sets, grammar->sets_any)`.
> Deleted readings (`"drs"`): if present and `pipe_deleted` (`--deleted`) is
> not set, ignore it, and if `verbosity_level > 0` print "Info: Ignoring
> 'drs' (deleted readings) on line <numLines>; use --deleted to read them."
> to stderr. Else if it is not an array, warn "Warning: 'drs' (deleted
> readings) on line <numLines> is not an array - ignored.". Otherwise, for
> each element in order: if it is not an object, warn "Warning: Non-object
> found in 'drs' (deleted readings) array on line <numLines> - skipping." and
> go on; else, with a fresh `TagList mappings`, `delR =
> parseJsonReading(element, cCohort, mappings)`, `addSingleMapping(*delR,
> mappings)`, set `deleted = true` on `delR` and on every reading of its
> `next` chain, `cCohort->appendReading(delR, cCohort->deleted)`, and
> `++numReadings`.
> Dependency (`"ds"`, `"dp"`): if `"ds"` is present: if it is not an unsigned
> 32-bit integer, or is 0, warn "Warning: 'ds' (dependency self) on line
> <numLines> is not a positive integer - ignored." and do not read `"dp"`.
> Otherwise set `cCohort->dep_self` to it; then, if `"dp"` is present: if it
> is not an unsigned 32-bit integer, warn "Warning: 'dp' (dependency parent)
> on line <numLines> is not a non-negative integer - ignored."; else, if it
> differs from `dep_self`, set `cCohort->dep_parent` to it. A `"dp"` equal to
> `"ds"` leaves `dep_parent` at `DEP_NO_PARENT`. Then set `has_dep = true`.
> If `"ds"` is absent and `"dp"` is present, warn "Warning: 'dp' (dependency
> parent) on line <numLines> without 'ds' (dependency self) - ignored.".
> Return `cCohort`.
>
> PORT DIVERGENCE: a number that the flat hash containers reserve as a
> sentinel is refused with a run error naming the member, before it is
> stored (`[spec:cg3:req:robustness.reserved-keys]`): a `"ds"` or `"id"` of
> `UINT32_MAX` or `UINT32_MAX-1`, a `"dp"` of `UINT32_MAX-1`, and a relation
> target of either value. `"ds"` keys the window's `dep_map` and `"dp"`
> probes it; `"id"` keys `gWindow->relation_map` and a relation target probes
> it. The C++ stores them, and a debug build asserts where a release build
> corrupts the map. A `"dp"` of `UINT32_MAX` is `DEP_NO_PARENT` and still
> means no parent. A relation whose name is empty is a run error naming the
> line, where the C++ `addTag`s the empty string
> (`[spec:cg3:req:robustness.empty-tag]`).

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn+1]
> Reading* JsonlApplicator::parseJsonReading(const json::Value& reading_obj, Cohort* cohort, TagList& mappings)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn+1]
> Builds a reading of `cohort` from the reading object `reading_obj`,
> recursing into its sub-reading, and returns it. The caller has checked that
> `reading_obj` is an object. The reading's mapping tags are not added to it
> but appended to `mappings`, for the caller to split (`splitAllMappings`) or
> to reduce to one (`addSingleMapping`). Members are read in the order
> `"l"`, `"ts"`, `"s"`; others are ignored, `"tr"` included.
> Setup: `cReading = alloc_reading(cohort)`, `addTagToReading(*cReading,
> cohort->wordform)`.
> Baseform (`"l"`): if absent, warn "Warning: Reading missing 'l' (baseform)
> on line <numLines>.". Otherwise read it with `getJsonString`; if that
> succeeds with an empty value, warn "Warning: Empty 'l' (baseform) in
> reading on line <numLines>."; if it succeeds with a non-empty value,
> `addTagToReading(*cReading, addTag("\"" + l + "\""))`.
> Tags (`"ts"`): if present and not an array, warn "Warning: 'ts' (tags) on
> line <numLines> is not an array - ignored.". Otherwise, for each element in
> order: if it is not a string, warn "Warning: Non-string found in 'ts'
> (tags) array on line <numLines> - skipping." and go on; skip an empty
> string; `tag = addTag(str)`; if `tag->type & T_MAPPING`, or the tag's first
> character is `grammar->mapping_prefix`, set `T_MAPPING` in `tag->type` and
> append `tag` to `mappings`; otherwise `addTagToReading(*cReading, tag)`.
> Sub-reading (`"s"`): if present and an object, parse it by recursion with
> the same `cohort` and a fresh `TagList sub_mappings`, set `cReading->next`
> to the result, and `addSingleMapping(*cReading->next, sub_mappings)`. If
> present and not an object, warn "Warning: Value for 's' (sub-reading) is
> not an object on line <numLines> - skipping.". The sub-reading is parsed
> before the next step, so its warnings come first.
> Fallback: if `cReading->baseform` is still 0, set it to
> `cohort->wordform->hash` and warn "Warning: Reading on line <numLines>
> ended up with no baseform. Using wordform.".
> Finally call `cReading->rehash()`, because the hash covers the sub-reading,
> which was attached after the tags, and return `cReading`.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn]
> void JsonlApplicator::printCohort(Cohort* cohort, std::ostream& output, bool profiling)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn+1]
> Writes one cohort as one JSON object on its own line. If
> `cohort->local_number == 0` (the window's `>>>` cohort) or `cohort->type &
> CT_REMOVED`, write no object: call `printText(cohort->text, output)`, so
> that the text after the cohort is still written, and return. If not
> `profiling`, call `cohort->unignoreAll()`. Members are added in this order,
> each only under its condition: `"w"`, `"wb"`, `"sts"`, `"z"`, `"ds"`,
> `"dp"`, `"id"`, `"rels"`, `"rs"`, `"drs"`.
> Wordform (`"w"`, always added): the wordform tag's text; if it is at least
> 4 characters long, starts with `"<` and ends with `>"`, the first two and
> the last two characters are dropped.
> Word-bound blank (`"wb"`): `cohort->wblank`, if it is non-empty.
> Static tags (`"sts"`): if `cohort->wread` exists, an array of the text of
> each tag in `wread->tags_list`, in order, except the wordform's hash, with
> no `unique_tags` filtering; added if non-empty.
> Text (`"z"`): if `cohort->text` contains a character not in `ws` (space and
> tab, and `\n` as well unless `add_spacing`), the text without its last
> character when that is a newline character (`ISNL`). Newlines inside the
> text stay, so several lines of text are one `"z"` with `\n` between them.
> Dependency (`"ds"`, `"dp"`): only if `has_dep`. The parent cohort `pr` is:
> none if `cohort->dep_parent == DEP_NO_PARENT`; the window's `>>>` cohort
> (`cohort->parent->cohorts[0]`) if `dep_parent == 0`; else the cohort that
> `gWindow->cohort_map` maps `dep_parent` to, or none if the map does not
> have it. If `dep_absolute` or `dep_has_spanned`: `"ds"` is
> `cohort->global_number` and, if `pr` exists, `"dp"` is 0 when
> `pr->local_number == 0`, else `pr->global_number`. Otherwise `"ds"` is
> `cohort->local_number` and, if `pr` exists, `"dp"` is `pr->local_number`.
> The root is therefore always 0, and `"dp"` is left out when the cohort has
> no parent.
> Cohort id (`"id"`): `cohort->global_number`, if `has_relations` or
> `print_ids` is set.
> Relations (`"rels"`): if `cohort->type & CT_RELATED`, an object, added even
> when it is empty. For each entry of `cohort->relations`, in its order
> (ascending by the relation name's hash), the member name is the name tag's
> text and the value is the array of the entry's target numbers, ascending.
> Readings (`"rs"`): sort `cohort->readings` in place by
> `Reading::cmp_number`; for each reading whose `noprint` is false, append
> the object `buildJsonReading` builds for it; added if non-empty.
> Deleted readings (`"drs"`): only if `trace` is set and `trace_no_removed`
> is not, the condition under which the CG writer prints delayed and
> deleted readings. Sort `cohort->delayed` and `cohort->deleted` in place by
> `Reading::cmp_number`; then, for `delayed` followed by `deleted`, for each
> reading whose `noprint` is false, append the object `buildJsonReading`
> builds for it; added if non-empty. Deleted readings have the same shape as
> the other readings.
> Write the object with `write_json_line`, then flush `output`.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn]
> void JsonlApplicator::printPlainTextLine(UStringView line, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn+1]
> Writes text as `{"t": ...}` objects, one per line of the text. If `line` is
> non-empty and its last character is a newline character (`ISNL`), drop that
> character. Split what remains at each `\n`, and write each piece, empty
> pieces included, with `write_json_line` as an object whose only member is
> `"t"`. Text without a `\n` is one object, and empty text is one object
> with an empty `"t"`. Only `\n` separates lines: other `ISNL` characters
> inside the text stay in the strings. Does not flush. This is the format's
> override of the virtual `printPlainTextLine`, so the text that other
> readers pass through is written the same way.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn]
> void JsonlApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn+1]
> Writes a whole window. In order: (1) `printVariables(window->variables_output,
> window->variables_set, output)`; (2) `printText(window->text, output)`; (3)
> `printCohort(cohort, output, profiling)` for each cohort in
> `window->all_cohorts`, the `>>>` cohort and removed cohorts included, which
> `printCohort` reduces to their text; (4) `printText(window->text_post,
> output)`; (5) if `window->flush_after`, `printStreamCommand(STR_CMD_FLUSH,
> output)`. Then flush `output`.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn]
> void JsonlApplicator::printStreamCommand(UStringView cmd, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn+1]
> Writes a stream command as one JSONL object `{"cmd": <cmd>}`: an object
> whose only member is `"cmd"`, holding the whole of `cmd`, any NUL character
> included, written with `write_json_line`. Does not flush.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-text-fn]
> void JsonlApplicator::printText(const UString& text, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-text-fn]
> Writes text that is not only whitespace. If `text` is non-empty and
> contains a character not in `ws` (space and tab, and `\n` as well unless
> `add_spacing`), call `printPlainTextLine(text, output)`; otherwise write
> nothing. The CG writer applies the same test to window and cohort text.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-variables-fn]
> void JsonlApplicator::printVariables(const uint32SortedVector& vars_output, const uint32FlatHashMap& vars_set, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-variables-fn]
> Writes one stream command per variable in `vars_output`, in its order
> (ascending by the variable name's hash). For each hash `var`, `key =
> grammar->single_tags[var]`. If `vars_set` maps `var` to a value other than
> `grammar->tag_any`, the command is `STR_CMD_SETVAR + key->tag + "=" +
> value->tag + ">"`, where `value` is the tag of that value's hash; if it
> maps `var` to `tag_any`, the command is `STR_CMD_SETVAR + key->tag + ">"`;
> if `vars_set` does not have `var`, the command is `STR_CMD_REMVAR +
> key->tag + ">"`. Each command is written with `printStreamCommand`. A
> SETVAR or REMVAR that named several variables therefore comes out as one
> command per variable.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn]
> void JsonlApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+2]
> Reads a JSONL stream, one JSON object per line, builds windows from it, runs
> the grammar on them, and writes them out. Each object is a stream command
> (`"cmd"`), a cohort (`"w"`) or a line of text (`"t"`), tested in that
> order, so an object with both `"w"` and `"t"` is a cohort and its `"t"` is
> ignored.
>
> Setup: store `&input` and `&output` in `ux_stdin` and `ux_stdout`. If
> `!input.good()`, print "Error: Input is null - nothing to parse!" and
> `CG3Quit(1)`; if `input.eof()`, print "Error: Input is empty - nothing to
> parse!" and quit; if `!output`, print "Error: Output is null - cannot write
> to nothing!" and quit; if `!grammar`, print "Error: No grammar provided -
> cannot continue! Hint: call setGrammar() first." and quit. If
> `grammar->delimiters` is null or empty: if `grammar->soft_delimiters` is
> null or empty too, warn "Warning: No soft or hard delimiters defined in
> grammar. Hard limit of <hard_limit> cohorts may break windows."; else warn
> "Warning: No hard delimiters defined in grammar. Soft limit of <soft_limit>
> cohorts may break windows.". Call `index()`. `resetAfter = (num_windows +
> 4) * 2 + 1`. Local state, all false or null: `ignoreinput`,
> `did_soft_lookback`, `cSWindow` (the window being filled, null once it has
> been delimited), and `lSWindow` and `lCohort` (the last window and the
> last cohort, which receive the text that follows them). Set
> `gWindow->window_span = num_windows`. Local variable state, empty:
> `variables_set` (`uint32FlatHashMap`), `variables_rem`
> (`uint32FlatHashSet`), `variables_output` (`uint32SortedVector`). Call
> `ux_stripBOM(input)`.
>
> Helpers:
> - `add_end_tag(cohort)`: add `endtag` to each reading in
>   `cohort->readings` whose `tags` does not have it yet.
> - `new_window()`: `cSWindow = gWindow->allocAppendSingleWindow()`,
>   `initEmptySingleWindow(cSWindow)`, `lSWindow = cSWindow`, `++numWindows`,
>   `did_soft_lookback = false`. Then move the local variable state into the
>   window: insert `variables_set`, `variables_rem` and `variables_output`
>   into the window's members of the same names, and clear the locals.
> - `check_dep_span(window)`: for each cohort in `window->cohorts` whose
>   `dep_parent` is neither 0 nor `DEP_NO_PARENT` and whose type has
>   `CT_DEP_DONE`, look `dep_parent` up in `gWindow->cohort_map`; if it is
>   there and that cohort's window is not this cohort's window, set
>   `dep_has_spanned = true` and stop. JSONL has no marker for dependencies
>   that span windows, so the reader detects them once they are resolved.
>   Windows are written a few runs later, so the CG writer can still switch
>   to its spanning numbering.
> - `run_window()`: `gWindow->shuffleWindowsDown()`, `runGrammarOnWindow()`.
>   If `has_dep` is set and `dep_has_spanned` is not, `check_dep_span` on
>   `gWindow->current` and on each window in `gWindow->next`. If `numWindows
>   % resetAfter == 0`, `resetIndexes()`. If `verbosity_level > 0`, print
>   "Progress: L:<numLines>, W:<numWindows>, C:<numCohorts>,
>   R:<numReadings>\r" to stderr and flush it.
> - `flush_windows()`: while `gWindow->next` is non-empty, `run_window()`.
>   Then `gWindow->shuffleWindowsDown()`, and while `gWindow->previous` is
>   non-empty, `printSingleWindow` its first window to `output`,
>   `free_swindow` it, and erase it.
> - `add_text(text)`: if `lSWindow` and `lCohort` are set and
>   `testStringAgainst(text, text_delimiters)` matches (`--text-delimit`),
>   append `text` to `lSWindow->text_post`, and if `cSWindow == lSWindow`,
>   `add_end_tag(cSWindow->cohorts.back())` and null `cSWindow`; then null
>   `lCohort`. Else if `lCohort` is set, append `text` to `lCohort->text`.
>   Else if `lSWindow` is set, append `text` to `lSWindow->text_post` if that
>   is non-empty, otherwise to `lSWindow->text`. Else write it at once with
>   `printPlainTextLine(text, output)`.
>
> Main loop: read each line with `std::getline` and `++numLines`. Skip a line
> that is empty or holds only the characters ` \t\n\v\f\r`, with no warning.
> Parse the line as one JSON document (RapidJSON `Document::Parse` over the
> line's bytes and length, default flags). On a parse error, warn "Warning:
> Failed to parse JSON on line <numLines>: <message> (offset <offset>).
> Skipping line.", where `<message>` is RapidJSON's English text for the
> error code (`GetParseError_En`, such as "Missing a name for object
> member.") and `<offset>` is the byte offset of the error, and go to the
> next line. If the document is not an object, warn "Warning: JSON on line
> <numLines> is not an object. Skipping line." and go to the next line. A
> member name that occurs more than once in an object is read at its first
> occurrence, since `HasMember` and `FindMember` find the first.
>
> Commands, if the object has a `"cmd"` member: read it with
> `getJsonString`, and go to the next line if that fails. If the command is
> empty, warn "Warning: Empty 'cmd' value on line <numLines>." and go to the
> next line. Commands are handled whether or not `ignoreinput` is set. A
> command starts with a prefix when it is longer than the prefix, ends with
> `>`, and begins with the prefix. The first case that applies is taken:
> - `STR_CMD_FLUSH` (exact): if `verbosity_level > 0`, print "Info: FLUSH
>   encountered on line <numLines>. Flushing..." to stderr. `backSWindow =
>   gWindow->back()`; if it is non-null, set its `flush_after`. If
>   `cSWindow` is set, `add_end_tag(cSWindow->cohorts.back())`. Null
>   `lCohort`, `cSWindow` and `lSWindow`, and call `flush_windows()`. If
>   there was no `backSWindow`, `printStreamCommand(STR_CMD_FLUSH, output)`.
>   Clear the member `variables`; the local variable state is kept. Flush
>   `output` and stderr.
> - `STR_CMD_IGNORE` (exact): if `verbosity_level > 0`, print "Info: IGNORE
>   encountered on line <numLines>. Passing through all input..." to stderr.
>   Set `ignoreinput` and `printStreamCommand(STR_CMD_IGNORE, output)`.
> - `STR_CMD_RESUME` (exact): if `verbosity_level > 0`, print "Info: RESUME
>   encountered on line <numLines>. Resuming CG..." to stderr. Clear
>   `ignoreinput` and `printStreamCommand(STR_CMD_RESUME, output)`.
> - `STR_CMD_EXIT` (exact): if `verbosity_level > 0`, print "Info: EXIT
>   encountered on line <numLines>. Exiting..." to stderr.
>   `printStreamCommand(STR_CMD_EXIT, output)`, then go to the exit label,
>   so buffered windows are neither run nor written.
> - Starts with `STR_CMD_SETVAR`: the payload is the text between the prefix
>   and the final `>`. Split it at each `,` into items; an empty payload is
>   one empty item, and a trailing `,` gives a last, empty item. For each
>   item, the key is the text before its first `=`, or the whole item if it
>   has none, and the variable `a` and the value `v` both start as
>   `grammar->tag_any`. If the key is empty, warn "Warning: SETVAR on line
>   <numLines> had an empty identifier! Defaulting to identifier *."; else `a
>   = addTag(key)->hash`. If the item has a `=`: if nothing follows it, warn
>   "Warning: SETVAR on line <numLines> had no value after the =! Defaulting
>   to value *."; else `v = addTag(<the text after the first =>)->hash`.
>   Then `variables_set[a] = v`, erase `a` from `variables_rem`, and insert
>   `a` into `variables_output`. So `<STREAMCMD:SETVAR:a=1,b>` sets `a` to
>   `1` and `b` to `*`.
> - Starts with `STR_CMD_REMVAR`: the payload is found and split the same
>   way, and empty items are skipped. For each other item, `a =
>   addTag(item)->hash`; erase `a` from `variables_set`, and insert it into
>   `variables_rem` and into `variables_output`.
> - Otherwise: warn "Warning: Unknown or malformed stream command <cmd> on
>   line <numLines> - treated as text." and `add_text(cmd + "\n")`. A SETVAR
>   or REMVAR without its closing `>` is handled here.
>
> Then go to the next line.
>
> Cohorts, if the object has a `"w"` member:
> - If `ignoreinput` is set, the line is passed through as text: decode the
>   whole line from UTF-8, append `"\n"`, `add_text` it, and go to the next
>   line. `"w"` is not checked.
> - Read `"w"` with `getJsonString`. If that fails or the value is empty,
>   warn "Warning: Cohort on line <numLines> has an empty or invalid 'w'
>   (wordform). Skipping line." and go to the next line.
> - Soft-limit look-back: if `cSWindow` is set,
>   `cSWindow->cohorts.size() > soft_limit`, `grammar->soft_delimiters` is
>   set, and `did_soft_lookback` is not: set `did_soft_lookback`, and walk
>   `cSWindow->cohorts` from last to first. At the first cohort `c` that
>   matches the soft delimiters (`doesSetMatchCohortNormal(*c,
>   grammar->soft_delimiters->number)`), clear `did_soft_lookback`, set
>   `cSWindow = delimitAt(*cSWindow, c)->parent->next` (the window that
>   `delimitAt` splits off after `c`) and `lSWindow = cSWindow`; if
>   `verbosity_level > 0`, warn "Warning: Soft limit of <soft_limit> cohorts
>   reached at line <numLines> but found suitable soft delimiter in
>   buffer."; and stop the walk.
> - If `cSWindow` is null, `new_window()`.
> - If `gWindow->next.size() > num_windows + 1`, `run_window()`, so rules
>   see `num_windows` windows ahead.
> - `cCohort = parseJsonCohort(doc, cSWindow)`.
> - Dependency delimiting (`--dep-delimit`): if `dep_delimit`,
>   `dep_highest_seen` and `cCohort->dep_self` are all non-zero,
>   `cSWindow->cohorts.size() > 1`, and either `cCohort->dep_self <=
>   dep_highest_seen` or `cCohort->dep_self - dep_highest_seen >
>   dep_delimit`: `reflowDependencyWindow(cCohort->global_number)`,
>   `add_end_tag(cSWindow->cohorts.back())`, `new_window()`,
>   `dep_highest_seen = 0`, `cCohort->parent = cSWindow`, and, if
>   `grammar->has_bag_of_tags`, `reflowReading` each of `cCohort`'s readings.
>   A cohort without a `"ds"` never starts a new window here.
> - `cSWindow->appendCohort(cCohort)` and `lCohort = cCohort`.
> - Delimiting, with the new cohort already counted: if
>   `cSWindow->cohorts.size() > soft_limit`, `grammar->soft_delimiters` is
>   set, and `cCohort` matches the soft delimiters: if `verbosity_level > 0`,
>   warn "Warning: Soft limit of <soft_limit> cohorts reached at line
>   <numLines> but found suitable soft delimiter."; `add_end_tag(cCohort)`;
>   null `cSWindow`. Else if `cSWindow->cohorts.size() > hard_limit`, or
>   `dep_delimit` is 0, `grammar->delimiters` is set, and `cCohort` matches
>   the delimiters: if `is_conv` is not set and the hard limit was passed,
>   warn "Warning: Hard limit of <hard_limit> cohorts reached at line
>   <numLines> - forcing break."; `add_end_tag(cCohort)`; null `cSWindow`.
>
> Then go to the next line.
>
> Text, if the object has a `"t"` member: if `getJsonString` reads it,
> `add_text(t + "\n")`, so an empty `"t"` is a blank line. Text is handled
> the same way whether or not `ignoreinput` is set. Then go to the next line.
>
> Any other object: warn "Warning: JSON object on line <numLines> has none of
> 'w', 't' or 'cmd'. Skipping line.".
>
> End of stream: set `input_eof = true`. If `cSWindow` is set,
> `add_end_tag(cSWindow->cohorts.back())` and null it. Call
> `flush_windows()` and flush `output`. Then `printVariables(variables_output,
> variables_set, output)`: the variables set or removed after the last window
> was made.
>
> Exit label (`CGCMD_EXIT_JSONL`, reached by EXIT or at the end of the
> stream): if `verbosity_level > 0`, print "Progress: L:<numLines>,
> W:<numWindows>, C:<numCohorts>, R:<numReadings> - Done." and a newline to
> stderr, and flush it.
>
> PORT DIVERGENCE: a line that is not UTF-8 MUST be a run error naming the
> input and the line (`[spec:cg3:req:robustness.stream-invalid-utf8]`). The
> C++ does not check: RapidJSON's default parse does not validate the bytes
> inside strings, and decoding them through ICU replaces each invalid
> sequence with U+FFFD.

> [spec:cg3:def:jsonl-applicator.cg3.write-json-line-fn]
> void write_json_line(const json::Document& doc, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.write-json-line-fn]
> Free function in an anonymous namespace. Serialises `doc` with a RapidJSON
> `Writer` into a `StringBuffer`, writes the result followed by `"\n"` to
> `output`, and does not flush. The `Writer` writes no whitespace between
> tokens, object members in the order they were added, array elements in
> order, and unsigned integers in decimal. In strings it writes characters
> from U+0080 up as raw UTF-8; it escapes `"` and `\` with a backslash,
> writes U+0008, U+0009, U+000A, U+000C and U+000D as `\b`, `\t`, `\n`, `\f`
> and `\r`, and writes every other character below U+0020, NUL included, as
> `\u00XX` with uppercase hex digits. `/` and U+007F are not escaped. A
> serialised document holds no raw newline, so each call writes exactly one
> line.
>
> serde_json's own serialiser differs in two ways that the port has to undo
> to write the same bytes: its `\u00xx` escapes use lowercase hex digits,
> and without the `preserve_order` feature a `Map` writes its members sorted
> by name rather than in the order they were added.

`[spec:cg3:def:jsonl-applicator.cg3.ustring-to-utf8-fn]` and its `sem` rule
stood here, naming `std::string ustring_to_utf8(UStringView ustr)`. They are
obsolesced, not unmet: the function converts the C++'s UTF-16 strings to UTF-8
for the JSON writer, and the port's strings are UTF-8 already, so the port
item was the identity and its callers now pass their strings straight through.

The C++ behaviour, recorded for completeness: two `u_strToUTF8` passes, the
first with a null buffer to measure the required length (its overflow status
ignored), the second to fill a string resized to that length.

The C++ keeps `ustring_to_utf8` and `write_json_line` in an anonymous
namespace with two more helpers, which have no rules of their own either.
`UString utf8_to_ustring(const char* str, size_t len)` is the reverse
conversion, decoding `len` bytes through ICU's `UnicodeString::fromUTF8`; it
is the identity in the port for the same reason. `json::Value
to_json(UStringView str, json::Document::AllocatorType& allocator)` builds a
JSON string from the UTF-8 form of `str` with an explicit length, so a NUL
character inside `str` is kept. The rules of the members that call them state
what they contribute.
