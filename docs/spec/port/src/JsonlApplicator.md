# src/JsonlApplicator.cpp, src/JsonlApplicator.hpp

> [spec:cg3:def:jsonl-applicator.cg3.json-to-ustring-fn]
> UString json_to_ustring(const json::Value& val)

> [spec:cg3:sem:jsonl-applicator.cg3.json-to-ustring-fn]
> Free function converting a JSON value to a UString (UTF-16). If `val` is a JSON
> string, take its UTF-8 bytes (`GetString`) and length (`GetStringLength`),
> decode via `icu::UnicodeString::fromUTF8(icu::StringPiece(bytes, len))`, and
> return a `UString` built from that UnicodeString's buffer and length. For any
> non-string value (null, number, bool, array, object, or missing), return an
> empty UString. In the Rust port using serde_json, treat this as: `Value::String`
> -> the string decoded to the internal wide representation; anything else -> "".

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator]
> class JsonlApplicator : public virtual GrammarApplicator

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn]
> void JsonlApplicator::buildJsonReading(const Reading* reading, json::Value& reading_json, json::Document::AllocatorType& allocator)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn]
> Builds the JSON object `reading_json` (asserted to already be an object) for one
> reading. The reading object shape is `{"l": <baseform>, "ts": [<tags>], "s":
> {<subreading>}}` where `ts` and `s` are only present when non-empty.
> Baseform ("l"): if `reading->baseform` is non-zero, look it up in
> `grammar->single_tags`; if found, take its tag text and, if it is at least 2
> chars and starts and ends with `"`, strip those surrounding quotes; otherwise
> use the whole tag. UTF-8-encode the result. Always add member `"l"` (an empty
> string when there is no baseform).
> Tags ("ts"): create a JSON array, fill it via `buildJsonTags(reading, ...)`, and
> add it as member `"ts"` only if it is non-empty.
> Subreading ("s"): if `reading->next` is non-null, recursively build a JSON
> object for `reading->next` and add it as member `"s"` only if that object is
> non-empty. (Because it recurses on `->next`, nesting follows the subreading
> linked-list; deleted-ness/noprint of the sub is not checked here.)

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn]
> void JsonlApplicator::buildJsonTags(const Reading* reading, json::Value& tags_json, json::Document::AllocatorType& allocator)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn]
> Fills the JSON array `tags_json` (asserted to already be an array) with a
> reading's printable tags as UTF-8 strings. Iterates `reading->tags_list` in
> order, keeping a `uint32SortedVector unique`. For each tag hash `tter`, skip it
> when: (`!show_end_tags` and `tter == endtag`) or `tter == begintag`; or `tter ==
> reading->baseform`; or the reading has a parent and `tter ==
> reading->parent->wordform->hash`. If `unique_tags` is set and `tter` is already
> in `unique`, skip; otherwise insert it. Look up `tag = grammar->single_tags[tter]`;
> skip if `(tag->type & T_DEPENDENCY)` and `has_dep` and not `dep_original`; skip
> if `(tag->type & T_RELATION)` and `has_relations`. Otherwise convert `tag->tag`
> to UTF-8 and `PushBack` it as a JSON string. Order is preserved from
> `tags_list`. (In the Rust/serde port, note that the C++ builds each string via a
> C-string constructor, so a tag containing an embedded NUL would be truncated at
> the NUL.)

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
> JsonlApplicator::JsonlApplicator(std::ostream& ux_err)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
> Constructor. Delegates to the base `GrammarApplicator(ux_err)` constructor with
> an empty body; no subclass data members. (There is also an explicit empty
> destructor `~JsonlApplicator()` present only to anchor the vtable.) No side
> effects.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn]
> void JsonlApplicator::parseJsonCohort(const json::Value& obj, SingleWindow* cSWindow, Cohort*& cCohort)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn]
> Parses one cohort object `obj` into a new cohort, assigning it into the output
> parameter `cCohort`. Consumes the cohort object shape: `{"w": wordform, "sts":
> [static tags], "z": text, "rs": [readings], "drs": [deleted readings], "ds":
> dep_self, "dp": dep_parent}`.
> Steps: allocate `cCohort = alloc_cohort(cSWindow)`, set `global_number =
> gWindow->cohort_counter++`, `++numCohorts`.
> Wordform ("w"): if member `"w"` exists, `wform_str = json_to_ustring(obj["w"])`;
> else warn "Warning: JSON cohort on line <numLines> missing 'w' (wordform). Using
> empty." Build the wordform tag `"\"<" + wform_str + ">\""` and `cCohort->wordform
> = addTag(...)`.
> Text ("z"): clear `cCohort->wblank`; if member `"z"` exists, set `cCohort->text =
> json_to_ustring(obj["z"])`.
> Static tags ("sts"): if member `"sts"` exists and is an array: if
> `cCohort->wread` is null, allocate it, add the wordform to it, and set its
> `baseform = cCohort->wordform->hash`; then for each array element with a
> non-empty string, `addTag` it and push its hash onto `cCohort->wread->tags_list`
> (pushed directly to the list, NOT via addTagToReading).
> Readings ("rs"): if member `"rs"` exists and is an array, iterate; for each
> element that is not an object, warn "Warning: Non-object found in 'rs' (readings)
> array on line <numLines>. Skipping." and continue; otherwise call
> `parseJsonReading(reading_val, cCohort)`; on success `cCohort->appendReading` and
> `++numReadings`; on failure print "Error: Failed to parse main reading on line
> <numLines>."
> If `cCohort->readings` ends up empty, call `initEmptyCohort(*cCohort)`. Then
> `insert_if_exists(cCohort->possible_sets, grammar->sets_any)`.
> Dependency: if member `"ds"` exists and is an unsigned int, set `cCohort->dep_self
> = obj["ds"].GetUint()`; if member `"dp"` exists and is an unsigned int, set
> `cCohort->dep_parent = obj["dp"].GetUint()`.
> Deleted readings ("drs"): if member `"drs"` exists and is an array, iterate;
> skip non-object elements; otherwise `parseJsonReading(dr_val, cCohort)`; on
> success set `delR->deleted = true` and push onto `cCohort->deleted`; on failure
> print "Error: Failed to parse deleted reading on line <numLines>."

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn]
> Reading* JsonlApplicator::parseJsonReading(const json::Value& reading_obj, Cohort* parentCohort)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn]
> Parses one reading object into a `Reading*`, recursing into subreadings.
> Consumes the object shape `{"l": baseform, "ts": [tags], "s": {subreading}}`.
> If `reading_obj` is not a JSON object, print to stderr "Error: Expected reading
> object, but got different type on line <numLines>." and return `nullptr`.
> Allocate `cReading = alloc_reading(parentCohort)` and `addTagToReading(*cReading,
> parentCohort->wordform)`.
> Baseform ("l"): if member `"l"` exists, `base_str = json_to_ustring(l_val)`; if
> non-empty, build a tag string `"\"" + base_str + "\""` (wrap in quotes),
> `addTag` it, and `addTagToReading`. If `"l"` is present but empty, warn
> "Warning: Empty 'l' (baseform) in reading on line <numLines>." If `"l"` is
> missing, warn "Warning: Reading missing 'l' (baseform) on line <numLines>."
> Tags ("ts"): if member `"ts"` exists and is an array, iterate it; for each
> element, `tag_str = json_to_ustring`; if non-empty, `tag = addTag(tag_str)`;
> if `tag->type & T_MAPPING` OR its first char equals `grammar->mapping_prefix`,
> collect it into a local `mappings` TagList; otherwise `addTagToReading`. After
> the loop, if `mappings` is non-empty call `splitMappings(mappings, *parentCohort,
> *cReading, true)`.
> Subreading ("s"): if member `"s"` exists and is an object, recursively parse it
> with the SAME `parentCohort`; on success set `cReading->next = subReading`, else
> print "Error: Failed to parse subreading object on line <numLines>." If `"s"`
> exists but is not an object, warn "Warning: Value for 's' (sub_reading) is not
> an object on line <numLines>. Skipping."
> Fallback: if `cReading->baseform` is still 0, set it to
> `parentCohort->wordform->hash` and warn "Warning: Reading on line <numLines>
> ended up with no baseform. Using wordform." Return `cReading`.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn]
> void JsonlApplicator::printCohort(Cohort* cohort, std::ostream& output, bool profiling)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn]
> Serializes one cohort as a single JSON object on its own line. Emitted object
> shape: `{"w": wordform, "sts": [...], "z": text, "ds": u, "dp": u, "rs": [...],
> "drs": [...]}` (each optional member is present only under the conditions below;
> RapidJSON emits members in insertion order: w, sts, z, ds, dp, rs, drs).
> If `cohort->local_number == 0` or `cohort->type & CT_REMOVED`, return without
> printing anything. If not `profiling`, call `cohort->unignoreAll()`.
> Wordform ("w"): from `cohort->wordform->tag`; if it has size ≥ 4 and starts with
> `"<` and ends with `>"`, strip both (the substring between them); otherwise use
> the whole tag. UTF-8-encode and add member `"w"`.
> Static tags ("sts"): if `cohort->wread` exists and its `tags_list` is non-empty,
> build an array of each tag's text, skipping the entry equal to the wordform
> hash, applying `unique_tags` dedup; add member `"sts"` only if the array is
> non-empty.
> Text ("z"): if `cohort->text` is non-empty, copy it, pop a single trailing
> `'\n'` if present, and if still non-empty add it (UTF-8) as member `"z"`.
> Dependency: if `has_dep` and not `CT_REMOVED`, add member `"ds"` = `cohort->dep_self`
> if non-zero else `cohort->global_number`; and if `cohort->dep_parent !=
> DEP_NO_PARENT`, add member `"dp"` = `cohort->dep_parent`.
> Readings ("rs"): sort `cohort->readings` by `Reading::cmp_number`; for each
> reading whose `noprint` is false, build a reading object via `buildJsonReading`
> and push it if non-empty; add member `"rs"` only if the array is non-empty.
> (Quirk: a code comment mentions optionally printing only the single best
> reading, but the `break` is commented out, so ALL non-noprint readings are
> emitted.)
> Deleted readings ("drs"): if `cohort->deleted` is non-empty, sort by
> `cmp_number`, build a reading object for each (the `noprint` flag is NOT checked
> here), and add member `"drs"` if non-empty. Deleted readings are emitted with
> the same shape as normal readings (no deleted marker in the object itself).
> Finally serialize the document with a RapidJSON `Writer` and write
> `<json>` + `"\n"` to `output`, then `output.flush()`.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn]
> void JsonlApplicator::printPlainTextLine(UStringView line, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn]
> Emits plain text as one JSONL object `{"t": <line>}`. Builds a JSON object,
> UTF-8-encodes `line`, adds it as member `"t"`, serializes with a RapidJSON
> `Writer`, and writes `<json>` + `"\n"` to `output`. Does NOT flush. (Any
> newlines embedded in `line` are JSON-escaped by the writer, so the output is
> still exactly one physical line.)

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn]
> void JsonlApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn]
> Serializes a whole window as a sequence of JSONL lines. Steps, in order:
> (1) For each `var` in `window->variables_output`, build a stream-command string
> and emit it via `printStreamCommand`. `key = grammar->single_tags[var]`. If
> `var` is in `window->variables_set`: if its value is not `grammar->tag_any`,
> command = `STR_CMD_SETVAR + key->tag + "=" + value->tag + ">"`; else command =
> `STR_CMD_SETVAR + key->tag + ">"`. If `var` is not in `variables_set` (removed),
> command = `STR_CMD_REMVAR + key->tag + ">"`.
> (2) If `window->text` (pre-text) is non-empty, emit it via `printPlainTextLine`.
> (3) For each cohort in `window->all_cohorts`, call `printCohort(cohort, output,
> profiling)`.
> (4) If `window->text_post` is non-empty, emit it via `printPlainTextLine`.
> (5) If `window->flush_after`, emit `printStreamCommand(STR_CMD_FLUSH, output)`.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn]
> void JsonlApplicator::printStreamCommand(UStringView cmd, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn]
> Emits a stream command as one JSONL object `{"cmd": <cmd>}`. Builds a JSON
> object, UTF-8-encodes `cmd`, adds it as member `"cmd"`, serializes with a
> RapidJSON `Writer`, and writes `<json>` + `"\n"` to `output`. Does NOT flush.

> [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn]
> void JsonlApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn]
> Reads JSON-Lines input (one JSON object per line), builds windows, runs the
> grammar, and prints JSONL output. Each input object is one of: a command
> (`{"cmd": ...}`), a plain-text line (`{"t": ...}` with no `"w"`), or a cohort
> (`{"w": ...}`, parsed by `parseJsonCohort`).
> Setup: store `&input`/`&output` in `ux_stdin`/`ux_stdout`. If `!input.good()`
> print "Error: Input is null..." + `CG3Quit(1)`; if `input.eof()` print "Error:
> Input is empty..." + quit; if `!output` print "Error: Output is null..." +
> quit; if `!grammar` print "Error: No grammar provided..." + quit. Warn if the
> grammar lacks hard (and/or soft) delimiters. Call `index()`. Compute `resetAfter
> = (num_windows + 4) * 2 + 1`. `ignoreinput = false`. Null the pointers
> `cSWindow/cCohort/lSWindow/lCohort`. Set `gWindow->window_span = num_windows`.
> Declare LOCAL variable-tracking state `variables_set` (map hash->hash),
> `variables_rem` (set), `variables_output` (sorted vector). Call
> `ux_stripBOM(input)`.
> Main loop: `std::getline(input, line_str)`. `++numLines`. Skip lines that are
> empty or all whitespace (` \t\n\v\f\r`). Parse the line with RapidJSON; on parse
> error print "Warning: Failed to parse JSON on line <numLines>: <error> (offset
> <off>). Skipping line." and continue. If the parsed value is not an object,
> print "Warning: JSON on line <numLines> is not an object. Skipping line." and
> continue.
> Command handling: if the object has member `"cmd"`, `cmd_ustr =
> json_to_ustring(doc["cmd"])`. If empty, warn "Warning: Empty 'cmd' value..." and
> continue. Otherwise:
> - `STR_CMD_FLUSH`: if `verbosity_level > 0` log an Info line. Set `backSWindow =
>   gWindow->back()`; if non-null set `backSWindow->flush_after = true`. If
>   `lCohort` is the last cohort of `cSWindow`, add `endtag` to all of `lCohort`'s
>   readings. Null `lCohort/cSWindow/lSWindow`. Drain buffered windows: while
>   `gWindow->next` non-empty, `shuffleWindowsDown` + `runGrammarOnWindow` +
>   `resetIndexes` when `numWindows % resetAfter == 0` (+ optional progress). Then
>   `shuffleWindowsDown()` once and, while `gWindow->previous` non-empty, pop
>   front, `printSingleWindow`, `free_swindow`, erase. If there was NO back window,
>   emit `printStreamCommand(cmd_ustr, output)`. Then `variables.clear()` (the
>   member map — note the LOCAL `variables_set/rem/output` are NOT cleared here),
>   flush output and stderr.
> - `STR_CMD_IGNORE`: set `ignoreinput = true`, `printStreamCommand(cmd_ustr)`.
> - `STR_CMD_RESUME`: set `ignoreinput = false`, `printStreamCommand(cmd_ustr)`.
> - `STR_CMD_EXIT`: `printStreamCommand(cmd_ustr)` then `goto` the exit label
>   (skips all end-of-stream draining).
> - Prefix `STR_CMD_SETVAR` (`"<STREAMCMD:SETVAR:"`, matched with `u_strncmp` over
>   its length): payload = `cmd_ustr.substr(SETVAR.size(), size - SETVAR.size() -
>   1)` (strips the prefix and the final char, assumed `>`). Find `'='`: if
>   present, `key = payload[0..=)`, `value = payload[=+1..]`, `key_tag =
>   addTag(key)`, `value_hash = addTag(value)->hash`; else `key_tag =
>   addTag(payload)`, `value_hash = grammar->tag_any`. Then `variables_set[key_tag
>   ->hash] = value_hash`, erase from `variables_rem`, insert into
>   `variables_output`.
> - Prefix `STR_CMD_REMVAR` (`"<STREAMCMD:REMVAR:"`): payload strips prefix and
>   final char; `key_tag = addTag(payload)`; erase from `variables_set`, insert
>   into `variables_rem`, insert into `variables_output`.
> After handling any command, `continue`.
> Ignore mode: if `ignoreinput` is set, then if the object has member `"t"` and it
> is a non-empty string, emit it via `printPlainTextLine`; continue (all
> non-command input is passed through as text and cohorts are not built).
> Plain text: else if the object has `"t"` and NOT `"w"`: `t_ustr =
> json_to_ustring(doc["t"])`; if non-empty (optionally logged when `verbosity_level
> > 1`), append it to `lCohort->text` if `lCohort` exists, else to
> `lSWindow->text` if that exists, else emit via `printPlainTextLine`. If empty,
> warn "Warning: Empty 't' value...". Continue.
> Cohort: else if the object has `"w"`: if `cSWindow` is null, allocate a new
> SingleWindow (`allocAppendSingleWindow` + `initEmptySingleWindow`), TRANSFER the
> local variable state into it (`cSWindow->variables_set = variables_set` then
> clear the local, likewise `variables_rem` and `variables_output`), `++numWindows`,
> set `lSWindow = cSWindow`. Call `parseJsonCohort(doc, cSWindow, cCohort)`; if
> `cCohort` is null print "Error: Failed to create cohort from JSON on line
> <numLines>." and continue. Append `cCohort` to `cSWindow`, set `lCohort =
> cCohort`. Delimiting (`did_delim = false`): if `cSWindow->cohorts.size() >=
> soft_limit` and soft delimiters exist and `cCohort` matches
> `grammar->soft_delimiters` (via `doesSetMatchCohortNormal`), optionally log,
> add `endtag` to all readings, null `cSWindow`/`cCohort`, `did_delim = true`.
> Else if `cSWindow->cohorts.size() >= hard_limit` OR (`grammar->delimiters` exists
> and `cCohort` matches them): if the hard limit triggered warn "Warning: Hard
> limit ... forcing break.", add `endtag` to all readings, null
> `cSWindow`/`cCohort`, `did_delim = true`. Then if `did_delim` OR
> `gWindow->next.size() > num_windows`: `shuffleWindowsDown` + `runGrammarOnWindow`
> + `resetIndexes` when `numWindows % resetAfter == 0` (+ optional progress).
> Finally null `cCohort`.
> End of stream: if `cSWindow` exists and is non-empty, add `endtag` to all
> readings of its last cohort. While `gWindow->next` non-empty, `shuffleWindowsDown`
> + `runGrammarOnWindow`. If `gWindow->current` exists, `runGrammarOnWindow`. Then
> `shuffleWindowsDown()` and drain `gWindow->previous` (print + free + erase).
> Flush `output`. Then emit any still-pending GLOBAL variable commands: for each
> `var` in the LOCAL `variables_output`, build a SETVAR/SETVAR-any/REMVAR command
> string (same rule as `printSingleWindow`) from `variables_set` and emit via
> `printStreamCommand`.
> Exit label (`CGCMD_EXIT_JSONL`, reached by EXIT or normal fall-through): if
> `verbosity_level > 0` print a final "Progress: ... - Done." line to stderr.
> Note: no regex is used; commands are matched by exact string equality or
> `u_strncmp` prefix comparison.

> [spec:cg3:def:jsonl-applicator.cg3.ustring-to-utf8-fn]
> std::string ustring_to_utf8(UStringView ustr)

> [spec:cg3:sem:jsonl-applicator.cg3.ustring-to-utf8-fn]
> Free function converting a UString (UTF-16 view) to a UTF-8 `std::string`. Uses
> ICU's `u_strToUTF8` in two passes: first call with a null buffer and size 0 to
> compute `required_length` (the preflight; `status` is expected to come back
> U_BUFFER_OVERFLOW_ERROR and is ignored — it is reset to `U_ZERO_ERROR` before
> the second pass). Resize the output string to `required_length`, then call
> `u_strToUTF8` again to fill it. Returns the UTF-8 bytes. In Rust, this is simply
> the UTF-8 encoding of the (UTF-16) string.

