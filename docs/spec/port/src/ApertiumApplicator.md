# src/ApertiumApplicator.cpp, src/ApertiumApplicator.hpp

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator]
> class ApertiumApplicator : public virtual GrammarApplicator {
>   bool wordform_case = false;
>   bool print_word_forms = true;
>   bool print_only_first = false;
>   bool delimit_lexical_units = true;
>   bool surface_readings = false;
> }

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.apertium-applicator-fn]
> ApertiumApplicator::ApertiumApplicator(std::ostream& ux_err)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.apertium-applicator-fn]
> Constructor. Takes an error output stream `ux_err` and forwards it to the
> base `GrammarApplicator(ux_err)` constructor; the body is empty. All
> Apertium-specific member flags keep their in-class defaults: `wordform_case
> = false`, `print_word_forms = true`, `print_only_first = false`,
> `delimit_lexical_units = true` (cohorts surrounded by `^`...`$`),
> `surface_readings = false`. No other initialization is performed here.

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.merge-mappings-fn]
> void ApertiumApplicator::mergeMappings(Cohort& cohort)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.merge-mappings-fn]
> Collapses readings of `cohort` that are byte-for-byte identical, including
> mapping tags. Builds a `std::map<uint32_t, ReadingList> mlist` keyed by a
> composite hash: for each reading `r` in `cohort.readings`, start `hp =
> r->hash` (the full hash, which includes mapping tags — the comment notes
> this is deliberately `hash`, not `hash_plain`); if `trace` is on, fold every
> value in `r->hit_by` into `hp` via `hash_value(iter_hb, hp)`; then walk the
> sub-reading chain `sub = r->next`, folding `hash_value(sub->hash, hp)` for
> each, and (if `trace`) folding each `sub->hit_by` too; append `r` to
> `mlist[hp]`. If the number of distinct keys equals `cohort.readings.size()`
> (all readings unique), return immediately without changes. Otherwise clear
> `cohort.readings`; for each hash group, keep the FIRST reading of the group
> (push into a temporary `order` vector) and `free_reading` every other
> reading in the group. Sort `order` by `Reading::cmp_number` and insert it at
> the beginning of `cohort.readings`. Iteration order over `mlist` is by
> ascending key (std::map), but the final order is re-sorted by cmp_number.

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.parse-stream-var-fn]
> void ApertiumApplicator::parseStreamVar(const SingleWindow* cSWindow, UString& cleaned, uint32FlatHashMap& variables_set, uint32FlatHashSet& variables_rem, u...

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.parse-stream-var-fn]
> Parses a stream-command variable directive out of `cleaned` (a UString like
> `<STREAMCMD:SETVAR:a=b,c` with NO trailing `>`; the caller already stripped
> the surrounding `[` and `>]`). The function MUTATES `cleaned` in place by
> writing NUL terminators, uses `addTag` to intern identifiers/values, and
> updates the three collections passed by reference. It mirrors the SETVAR/
> REMVAR handling in GrammarApplicator_runGrammar.cpp.
>
> If `cleaned` begins with `STR_CMD_SETVAR` (`<STREAMCMD:SETVAR:`, 18 UChars),
> let `s` point just past that prefix, `c = u_strchr(s, ',')`, `d =
> u_strchr(s, '=')`. Case (a): if both `c` and `d` are null there is a single
> bare identifier — intern `tag = addTag(s)`, set `variables_set[tag->hash] =
> grammar->tag_any`, `variables_rem.erase(tag->hash)`,
> `variables_output.insert(tag->hash)`, and ONLY IF `cSWindow == nullptr` also
> set the live member map `variables[tag->hash] = grammar->tag_any`. Case (b):
> otherwise loop `while (c || d)`: if `d` exists and (`d < c` or `c` is null)
> — i.e. an `=` comes before the next `,` — set `*d = 0`; if the identifier
> before `=` is empty, warn "SETVAR ... had no identifier before the =!
> Defaulting to identifier *." and use `a = grammar->tag_any`, else `a =
> addTag(s)->hash`; if `c` exists set `*c = 0` and advance `s = c+1`; if the
> value after `=` is empty (`d[1] == 0`), warn "... had no value after the =!
> Defaulting to value *." and `b = grammar->tag_any`, else `b = addTag(d+1)
> ->hash`; if there was no comma, set `d = nullptr` and `s = nullptr`; then
> record `variables_set[a] = b`, `variables_rem.erase(a)`,
> `variables_output.insert(a)`. Else if `c` exists and (`c < d` or `d` null) —
> a comma-separated bare identifier — set `*c = 0`; if empty warn "... had no
> identifier after the ,! Defaulting to identifier *." and `a = tag_any`, else
> `a = addTag(s)->hash`; advance `s = c+1`; record `variables_set[a] =
> grammar->tag_any`, erase from rem, insert into output. After each iteration,
> if `s` is non-null recompute `c` and `d` on the new `s`; if both are now
> null the remainder is a final bare identifier — `a = addTag(s)->hash`, set
> it to `tag_any`, erase/insert as above, and set `s = nullptr` to end.
> Note: in case (b) the member `variables` map is NEVER updated (only case
> (a)'s single-identifier form touches it, and only when `cSWindow` is null).
>
> Else if `cleaned` begins with `STR_CMD_REMVAR` (`<STREAMCMD:REMVAR:`), let
> `s` point past the prefix and `c = u_strchr(s, ',')`. While `c && *c`: set
> `*c = 0`; if `s[0]` is non-empty, `a = addTag(s)->hash`, `variables_set.erase
> (a)`, `variables_rem.insert(a)`, `variables_output.insert(a)`; advance `s =
> c+1`, recompute `c`. After the loop, if `s && s[0]` process the trailing
> identifier the same way (erase from set, insert into rem and output).
>
> If `cleaned` matches neither prefix, the function does nothing.

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.print-cohort-fn]
> void ApertiumApplicator::printCohort(Cohort* cohort, std::ostream& output, bool profiling)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-cohort-fn]
> Prints one cohort in Apertium `^wordform/reading.../reading$` form to
> `output`. `profiling` toggles cleanup behavior.
>
> First, if `cohort->local_number == 0` (the boundary/BOS cohort) OR
> `cohort->type & CT_REMOVED`, print `cohort->text` verbatim if non-empty and
> return early — such cohorts emit only their trailing text.
>
> If `!profiling`: call `cohort->unignoreAll()`, and if `!split_mappings`,
> call `mergeMappings(*cohort)` to collapse identical readings.
>
> If `cohort->wblank` is non-empty, print it verbatim (the word-bound blank,
> e.g. `[[...]]`, that precedes the token). If `delimit_lexical_units`, print
> `^`.
>
> If `print_word_forms`: take the wordform tag text and drop the wrapping
> `"<` and `>"` (build a UnicodeString from `wordform->tag.data()+2` with
> length `size-4`). Escape it character-by-character: prepend `\` before any of
> `^ \ / $ [ ] { } < >`, and additionally prepend `\` before `@` when
> `cohort->type & CT_AP_UNKNOWN`; append each char; print the escaped result.
> Then, if `cohort->wread` (static reading) exists, for each tag hash in
> `cohort->wread->tags_list`, skip the one equal to `cohort->wordform->hash`
> and print the rest as `<tag>`.
>
> Set `need_slash = print_word_forms`. Sort `cohort->readings` by
> `Reading::cmp_number`. For each reading: skip if `reading->noprint`; if
> `need_slash` print `/`; set `need_slash = true`; if `grammar->sub_readings_ltr
> && reading->next` replace `reading` with `reverse(reading)`; call
> `printReading(reading, output)` (the 2-arg overload that derives casing);
> if `print_only_first`, break after the first printed reading.
>
> If `trace`: sort `cohort->delayed` by cmp_number and print each non-noprint
> one prefixed with `/` followed by the `not_sign` (¬) character (respecting
> the reverse-on-sub_readings_ltr rule); then do the same for
> `cohort->deleted`. Each such reading is separated by `/¬`.
>
> If `delimit_lexical_units`, print `$`. Finally, if `cohort->text` is
> non-empty, print it verbatim (trailing blank/superblank text).

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.print-reading-fn]
> void ApertiumApplicator::printReading(const Reading* reading, std::ostream& output, ApertiumCasing casing, int32_t firstlower)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-reading-fn]
> Prints one reading (and its sub-reading chain) in Apertium form. This is the
> 4-argument core; `casing`/`firstlower` control lemma casing.
>
> There is also a 2-argument overload `printReading(reading, output)` (the one
> that `printCohort`/`testPR` actually call), which derives the arguments as
> follows: `casing = Nochange`; if `wordform_case`, walk to the last sub-reading
> that has a baseform (`last`), then over the cohort's wordform tag text
> (indices `2 .. size-4-1+2`, i.e. the wordform between `"<` and `>"`) count
> alphabetic chars (`u_isUAlphabetic`) and uppercase chars (`u_isUUppercase`);
> if `uppercaseseen == alphabeticsseen && uppercaseseen >= 2` set `casing =
> Upper`; else if the first wordform char (`wftag[2]`) is upper and
> `uppercaseseen == 1` set `casing = Title`. It then calls the 4-arg form with
> `firstlower = 0`.
>
> 4-arg body: if `reading->next` is set, RECURSE into `printReading(reading->
> next, output, casing, firstlower)` first, then print `+` — so the innermost
> sub-reading prints first and segments are joined by `+`.
>
> If `reading->baseform`: build a UnicodeString `bf` from the baseform tag,
> dropping the surrounding `"` quotes (`data()+1`, length `size-2`). If
> `wordform_case`: when `casing == Upper` call `bf.toUpper()`; when `casing ==
> Title && !reading->next`, uppercase only the single char at index `firstlower`
> (via a length-1 substring `toUpper` then `setCharAt`). Escape `bf` into
> `bf_escaped`: prepend `\` before any of `^ \ / $ [ ] { } < >`, and prepend `\`
> before `@` when `reading->parent->type & CT_AP_UNKNOWN`; append each char. If
> `surface_readings && bf.length() > 0 && bf_escaped[0] == '@'`, overwrite the
> first char with `#`. Print `bf_escaped`.
>
> If `surface_readings && !trace`, RETURN now (like `lt-proc -g`: don't print
> tags for surface output unless tracing).
>
> Reorder tags so MAPPING tags precede the multiword join: iterate
> `reading->tags_list`; a tag whose text starts with `+` sets `multi = true`, a
> `T_MAPPING` tag sets `multi = false`; a `T_DEPENDENCY` tag is skipped entirely
> when `has_dep && !dep_original`; tags with `multi` true go into
> `multitags_list`, others into `tags_list`; finally append `multitags_list`
> after `tags_list`. Set `escape = surface_readings ? "\\" : ""`. For each tag
> hash `tter`: if `unique_tags`, skip already-seen hashes (tracked in a sorted
> `used_tags`); skip `endtag` and `begintag`; fetch `tag`; if it is neither
> `T_BASEFORM` nor `T_WORDFORM`, then: if its text starts with `+` print the tag
> text verbatim (`%S`); else if it starts with `&` print `escape<escapeTEXT
> escape>` using `substr(tag->tag, 2)` (drop the `&` plus one char) as TEXT;
> else print `escape<escapeTAGescape>` (a normal `<tag>`), where `escape` is the
> `\` prefix/suffix only in surface mode.
>
> Dependency output: if `has_dep && !(parent->type & CT_REMOVED) &&
> !reading->next`: if `parent->dep_self == 0` set it to `parent->global_number`;
> determine the parent cohort `pr` (default `parent`): if `parent->dep_parent !=
> DEP_NO_PARENT`, then if `dep_parent == 0` use `parent->parent->cohorts[0]`,
> else if `dep_parent` is in `gWindow->cohort_map` use that cohort. Print
> `<#LOCAL→PARENTLOCAL>` using the pattern `<#%u→%u>` (U+2192 RIGHTWARDS
> ARROW) with `parent->local_number` and `pr->local_number`.
>
> If `trace`: for each entry in `reading->hit_by`, print `<`, then
> `printTrace(output, entry)`, then `>`.

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.print-single-window-fn]
> void ApertiumApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-single-window-fn]
> Prints an entire `SingleWindow`. If `window->text` is non-empty, print it
> verbatim first (leading blank text). Then iterate `window->all_cohorts` in
> order, calling `printCohort(cohort, output, profiling)` and flushing the
> output stream (`u_fflush`) after each cohort. If `window->text_post` is
> non-empty, print it verbatim and flush. Finally, if `window->flush_after` is
> set, write a NUL byte (`u_fputc('\0', output)`) to terminate the window (used
> for null-flush streaming).

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.process-reading-fn]
> void ApertiumApplicator::processReading(Reading* cReading, UChar* p, Tag* wform)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.process-reading-fn]
> Parses one Apertium analysis string (already extracted between `/` and the
> next `/` or `$`) into a CG `Reading`, including sub-readings. `p` is a mutable
> `UChar*` cursor into the string, `wform` the cohort's wordform tag. There is
> a thin overload taking `UString&` that just calls this with `&reading_string
> [0]`. Examples handled: `venir<vblex><imp><p2><sg>`, `venir<vblex><inf>+lo
> <prn><enc><p3><nt><sg>`, `be<vblex><inf># happy`, `be# happy<vblex><inf>`.
>
> Start by `addTagToReading(*cReading, wform)`. Maintain `TagList taglist`
> (accumulated ordered tags across segments), `UString bf` initialized to `"`
> (the running baseform, quote-wrapped), and per-segment `TagList tags` and
> `prefix_tags`.
>
> Main scan loop `while (*p)`: set `n = p` and advance `n` while `*n` is not
> one of `# + <`; while advancing, any char equal to `esc_lt` (the sentinel
> `'\1'`, which the caller substituted for an escaped `\<`) is REWRITTEN in
> place to `'<'` so it becomes literal baseform text rather than a tag opener.
> If `n != p`, the span `[p,n)` is baseform text: temporarily NUL-terminate at
> `n`, append it to `bf`, restore, and set `p = n`. Then, if `*n == '<'` (tag
> start): set `p = n+1`, advance `n` to the closing `>`; if no `>` is found,
> warn "Did not find matching > to close the tag on line %u." and `continue`;
> otherwise NUL-terminate at `>`, and if `bf.size() == 1` (still just the
> opening quote — no baseform seen yet in this segment) push `addTag(p)` into
> `prefix_tags`, else push into `tags`; restore and set `p = n+1`. Then, if
> `*n == '#'` (multiword marker): set `p = n`, advance `n` until `<` or `+`,
> NUL-terminate, append `[p,n)` to `bf` (so `# happy` becomes part of the
> baseform), restore, `p = n`. Then, if `*n == '+'` (sub-reading delimiter):
> close the current segment — `bf += '"'`, push `addTag(bf)` (a `T_BASEFORM`
> tag) into `taglist`, then append `tags` then `prefix_tags` to `taglist`;
> reset `bf` to just `"` (`resize(1)`), clear `tags` and `prefix_tags`, set
> `p = n+1`.
>
> After the loop, if anything remains (`bf` non-empty, which is always true
> since it holds the quote, or `tags`/`prefix_tags` non-empty), close the final
> segment the same way: `bf += '"'`, push baseform tag, append `tags` then
> `prefix_tags`.
>
> Now assign tags to reading(s), scanning from the BACK of `taglist`. Loop
> `while (!taglist.empty())`: set `reading = cReading` and reverse-iterate; at
> the first `T_BASEFORM` tag found: if `reading` already has a baseform, create
> a new sub-reading (`allocateReading(reading->parent)`), link `reading->next =
> nr`, descend into it, and `addTagToReading(*reading, wform)`; collect tags
> from that baseform position forward to `taglist.end()` — tags that are
> `T_MAPPING` or whose text starts with `grammar->mapping_prefix` go into a
> `mappings` list, all others are added via `addTagToReading`; if `mappings` is
> non-empty call `splitMappings(mappings, *reading->parent, *reading, true)`;
> then pop from the back of `taglist` all trailing non-baseform tags and pop the
> baseform tag itself. The net effect: the LAST baseform segment fills
> `cReading`, and each earlier segment becomes a further `next` sub-reading; the
> caller reverses the chain when `sub_readings_ltr`. Ends with `assert(taglist.
> empty())`. Per-segment tag order placed into taglist is: baseform, then the
> `tags` seen after the baseform, then the `prefix_tags` seen before it.

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.run-grammar-on-text-fn]
> void ApertiumApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.run-grammar-on-text-fn]
> Parses an Apertium stream (`^wordform/reading.../reading$` cohorts with
> superblanks between them) and runs the constraint grammar over it, streaming
> results to `output`. Character-by-character state machine.
>
> Setup: set `ux_stdin`/`ux_stdout`. Validate and `CG3Quit(1)` with an error
> message if: `!input.good()` ("Input is null"), `input.eof()` ("Input is
> empty"), `!output` ("Output is null"), or `!grammar` ("No grammar provided").
> If the grammar has no hard delimiters (`!grammar->delimiters ||
> empty`): warn about the hard-limit cohort break if there are also no soft
> delimiters, else warn about the soft-limit break. Initialize state: `c = 0`,
> booleans `in_blank`, `in_wblank`, `in_cohort` all false, empty UStrings
> `blank`, `wblank`, `token`. Call `index()`. Compute `resetAfter =
> (num_windows + 4) * 2 + 1`. Constants `wb_start = "[["`, `wb_end = "]]"`. Set
> `begintag = addTag(">>>")->hash`, `endtag = addTag("<<<")->hash`. Null out
> `cSWindow`, `cCohort`, `lSWindow`, `lCohort`. Set `gWindow->window_span =
> num_windows`. Create empty `variables_set` (map), `variables_rem` (set),
> `variables_output` (sorted vector). Call `ux_stripBOM(input)`.
>
> Define lambda `ensure_endtag`: if `lSWindow` exists, has cohorts, and the
> last cohort's front reading lacks `endtag`, add `endtag` to every reading of
> that last cohort.
>
> Define lambda `flush(bool n=false)`: call `ensure_endtag`; let `backSWindow =
> n ? gWindow->back() : nullptr` and if it exists set its `flush_after = true`.
> If `blank` is non-empty, attach it to `lCohort->text` if `lCohort`, else to
> the last cohort of `lSWindow`, else to `lSWindow->text`, else print it via
> `printPlainTextLine(blank, output)`; then clear `blank`. Drain windows: while
> `gWindow->next` non-empty, `shuffleWindowsDown()` then `runGrammarOnWindow()`;
> then `shuffleWindowsDown()` once more; while `gWindow->previous` non-empty,
> `printSingleWindow(front)`, `free_swindow(front)`, erase it. If `c && c !=
> 0xffff`, print the single leftover char via `printPlainTextLine` (source notes
> this path is untested). If `n && !backSWindow`, write a NUL byte. Flush
> output. Then reset all state: booleans false, `lSWindow`/`lCohort`/`cSWindow`/
> `cCohort` null, clear `token`, clear `variables_rem`/`variables_set`/
> `variables_output` and the member `variables` map.
>
> Main loop `while ((c = u_fgetc(input)) != U_EOF)`:
> - If `c == '\n'`, `++numLines`.
> - If `c == '\\'`: read the next char `n`; append `c` then `n` (the backslash
>   and the escaped char, kept literally) to `token` if `in_cohort`, else to
>   `blank`; `continue`. Escapes thus never drive the state machine.
> - If `c == 0` (NUL): call `flush(true)` and `continue` (null-flush boundary).
> - If `!in_cohort && c == '['`: if already `in_blank`, set `in_wblank = true`
>   (nested `[` opens a word-bound blank); set `in_blank = true`. Else if
>   `!in_blank && c == '^'`: set `in_cohort = true`.
> - Append `c` to `blank` if `!in_cohort`, else to `token`.
> - If `in_wblank && c == ']'`: `in_wblank = false`. Else if `in_blank && c ==
>   ']'`: `in_blank = false`, and if `blank.size() > 14 && blank[1] == '<' &&
>   blank[size-2] == '>'` (looks like `[<STREAMCMD:...>]`), compute `cleaned =
>   blank.substr(1, size-3)` (drops leading `[` and trailing `>]`, leaving no
>   trailing `>`) and call `parseStreamVar(cSWindow, cleaned, variables_set,
>   variables_rem, variables_output)`. The stream-command text is NOT removed
>   from `blank`; it stays and is later emitted as cohort/window text.
> - Else if `!in_blank && c == '$'` (end of cohort; note `token` already
>   contains the trailing `$` because the append above ran while still
>   `in_cohort`): if `!in_cohort`, error "$ found without prior ^ on line %u."
>   and `CG3Quit(1)`. Set `in_cohort = false`. Word-bound-blank extraction: if
>   `blank` non-empty, find `b = blank.find("[[")` and `e = blank.find("]]",
>   b)`; if both found and NOT (`e == b+3 && blank[b+2] == '/'`) — i.e. not a
>   bare closing `[[/]]` blank — move the tail `wblank = blank.substr(b)` out
>   of `blank` (`blank.erase(b, npos)`). If `wblank` non-empty and it does not
>   end in `]]`, error "Word-bound blank was not immediately prior to token on
>   line %u" and `CG3Quit(1)`.
>
>   Attach leftover `blank` to the nearest text sink: `cCohort->text` if
>   `cCohort`, else `lCohort->text`, else `lSWindow->text` (clearing `blank`
>   each time). If there is no current window (`!cSWindow`): `ensure_endtag()`,
>   `cSWindow = gWindow->allocAppendSingleWindow()`, `initEmptySingleWindow`,
>   MOVE the three variable collections into `cSWindow->variables_set/rem/
>   output` (clearing the locals), set `lSWindow = cSWindow`, `lSWindow->text =
>   blank`, clear `blank`, `++numWindows`. Allocate the cohort: `lCohort =
>   cCohort = alloc_cohort(cSWindow)`, `global_number = gWindow->cohort_counter
>   ++`, `++numCohorts`; set `cCohort->text = blank` (clear), `cCohort->wblank =
>   wblank` (clear).
>
>   Parse the wordform: reset `blank = "\"<"`, set `p = &token[1]` (skip `^`),
>   loop while `*p` and `*p` not in `/ < $`: on `\\` skip it (`++p`), then
>   append `*p`; append `>\"`; set `cCohort->wordform = addTag(blank)`, clear
>   `blank`. Static reading: if `*p == '<'`: `++p`, clear `blank`, allocate
>   `cCohort->wread`; loop while `*p` not in `/ $`: on `\\` skip and append next
>   char; on `<` skip; on `>` intern `addTag(blank)` and add to `wread`, clear;
>   else append. Readings: if `*p == '/'`: `++p`, clear `blank`, loop while
>   `*p`: on `\\`, `++p`, and if the escaped char is `<` append the sentinel
>   `esc_lt` (`'\1'`) else append the char; on `/` or `$` (reading boundary)
>   allocate `cReading`, call `processReading(cReading, blank, cCohort->
>   wordform)`, reverse it if `sub_readings_ltr && cReading->next`, push into
>   `cCohort->deleted` if `cReading->deleted` else `appendReading`, `++
>   numReadings`, warn "Cohort %u on line %u had no valid baseform." if it has
>   none, clear `blank`; else append `*p`.
>
>   If `cCohort->readings` is empty, `initEmptyCohort(*cCohort)` (magic
>   reading). `insert_if_exists(cCohort->possible_sets, grammar->sets_any)`;
>   `cSWindow->appendCohort(cCohort)`. If `cCohort->wordform->tag[2] == '@'`
>   (first wordform char is `@`), set `cCohort->type |= CT_AP_UNKNOWN`.
>
>   Delimiter handling: `did_delim = false`. If `cCohort` and
>   `cSWindow->cohorts.size() >= soft_limit` and soft delimiters exist and
>   `doesSetMatchCohortNormal(*cCohort, soft_delimiters->number)`: add `endtag`
>   to all readings, set `lSWindow = cSWindow`, null `cSWindow`/`cCohort`,
>   `did_delim = true`. Then if `cCohort` still set and (`cohorts.size() >=
>   hard_limit` or delimiters match): if `!is_conv && size >= hard_limit`, warn
>   "Hard limit of %u cohorts reached ... forcing break."; add `endtag` to all
>   readings, set `lSWindow`, null `cSWindow`/`cCohort`, `did_delim = true`. If
>   `did_delim && gWindow->next.size() > num_windows`: `shuffleWindowsDown()`,
>   `runGrammarOnWindow()`, and if `numWindows % resetAfter == 0`,
>   `resetIndexes()`. Clear `token`.
>
> After the input loop, call `flush()` (with `n = false`).

> [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.test-pr-fn]
> void ApertiumApplicator::testPR(std::ostream& output)

> [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.test-pr-fn]
> Debug/self-test routine that round-trips six hard-coded Apertium analysis
> strings through `processReading`/`printReading`. The strings are: `venir
> <vblex><imp><p2><sg>`, `venir<vblex><inf>+lo<prn><enc><p3><nt><sg>`, `be
> <vblex><inf># happy`, `sellout<vblex><imp><p2><sg># ouzh+indirect<prn><obj>
> <p3><m><sg>`, `be# happy<vblex><inf>`, and `aux3<tag>+aux2<tag>+aux1<tag>
> +main<tag>`. Loops `for i` in `[0,6)`: builds a UString `text` from the ASCII
> bytes of `texts[i]`, allocates a reading via `alloc_reading()`, calls
> `processReading(reading, text, grammar->single_tags[grammar->tag_any])`
> (using the "any" tag as the wordform), reverses the sub-reading chain if
> `grammar->sub_readings_ltr && reading->next`, prints it with `printReading
> (reading, output)`, prints a newline, then `free_reading(reading)`. Emits
> nothing else. Note the array has exactly 6 elements and the loop is bounded
> by the literal `6`.

> [spec:cg3:def:apertium-applicator.cg3.apertium-casing]
> enum ApertiumCasing {
>   Nochange;
>   Title;
>   Upper;
> }

