# src/GrammarApplicator_runGrammar.cpp

> [spec:cg3:def:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-cohort-fn]
> Reading* GrammarApplicator::initEmptyCohort(Cohort& cCohort)

> [spec:cg3:sem:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-cohort-fn]
> Gives a cohort that has no readings a single "magic" placeholder reading, and
> returns it. Allocate a Reading in `cCohort` (alloc_reading). Set its baseform:
> if `allow_magic_readings` is true, `baseform =
> makeBaseFromWord(cCohort.wordform)->hash` (derive a `"..."` baseform tag from
> the `"<...>"` wordform and use its hash); otherwise `baseform =
> cCohort.wordform->hash` (reuse the wordform hash directly). Call
> `insert_if_exists(possible_sets, grammar->sets_any)` on the cohort. Add the
> cohort's wordform tag to the reading (addTagToReading). Set the reading's
> `noprint = true` so it is suppressed on output unless a later rule clears the
> flag. Append the reading to the cohort, increment `numReadings`, and return
> the new reading pointer.

> [spec:cg3:def:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-single-window-fn]
> void GrammarApplicator::initEmptySingleWindow(SingleWindow* cSWindow)

> [spec:cg3:sem:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-single-window-fn]
> Builds the leading `>>>` boundary cohort for a fresh SingleWindow. Allocate a
> Cohort in `cSWindow` (alloc_cohort). Set its `global_number` to
> `gWindow->cohort_counter++` (read then post-increment the global counter). Set
> its `wordform` to `tag_begin` (the `>>>` boundary wordform tag). Allocate a
> Reading in that cohort; set the reading's `baseform` to `begintag` (hash of
> the `>>>` tag). Call `insert_if_exists(cReading->parent->possible_sets,
> grammar->sets_any)` to flag the ANY set on the cohort's possible-set bitset if
> that set exists. Add the `begintag` tag to the reading via addTagToReading.
> Append the reading to the cohort (cCohort->appendReading), then append the
> cohort to the window (cSWindow->appendCohort). Does not touch numReadings/
> numCohorts counters. This cohort is always local_number 0 and is skipped by
> rule application.

> [spec:cg3:def:grammar-applicator-run-grammar.cg3.grammar-applicator.run-grammar-on-text-fn]
> void GrammarApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:grammar-applicator-run-grammar.cg3.grammar-applicator.run-grammar-on-text-fn]
> The main CG stream driver: reads the standard "VISL CG-3" text format line by
> line, builds Windows -> Cohorts -> Readings, runs the grammar window by window
> as enough windows accumulate, and writes results. Setup: store `&input` in
> `ux_stdin` and `&output` in `ux_stdout`. Validate and CG3Quit(1) on any of:
> `!input.good()` ("Input is null"), `input.eof()` ("Input is empty"),
> `!output` ("Output is null"), `!grammar` ("No grammar provided"). If there are
> no hard delimiters (and possibly no soft delimiters) warn about the hard/soft
> cohort limits. Allocate `line` (UString 1024) and `cleaned` (line.size()+1)
> buffers; flags ignoreinput/did_soft_lookback/is_deleted = false. Call
> `index()`. Compute `resetAfter = (num_windows+4)*2+1`; lines=0. Null out the
> current and last SingleWindow/Cohort/Reading pointers. Set
> `gWindow->window_span = num_windows`. Create empty locals `variables_set`
> (map hash->value), `variables_rem` (set), `variables_output` (sorted vector),
> `indents` (vector of {indent-size, Reading*}) and `all_mappings`. Strip a BOM
> from input. Define `adopt_variables` (merge the three locals into the current
> window's variables_set/rem/output then clear them) and `binary_maybe_window`
> (only when `fmt_output==CG3SF_BINARY`: allocate+append a SingleWindow, init it
> empty, set lSWindow); call binary_maybe_window once.
>
> Main loop `while (!input.eof())`: increment `lines`; `packoff =
> get_line_clean(line, cleaned, input)` reads one raw line into `line` and a
> trimmed/normalized copy into `cleaned`. Strip trailing whitespace from
> `cleaned` (zeroing chars and decrementing packoff). If `ignoreinput`, jump
> straight to the text/command handling (istext). Otherwise dispatch on the
> shape of `cleaned`:
>
> (1) Cohort line — `cleaned[0]=='"' && cleaned[1]=='<'`: scan `space` forward
> to the terminating `>"`, tolerating embedded quotes/escapes (SKIPTO_NOSPAN /
> SKIPTOWS). If the token does not actually end in `>"` (space[0]!='"' ||
> space[-1]!='>'), warn "looked like a cohort but wasn't - treated as text" and
> jump to istext. Null-terminate the wordform at space[1]. If a pending
> `cCohort` has no readings, initEmptyCohort it. Then enforce limits in order:
> (a) soft-limit lookback — if the window has >= soft_limit cohorts, soft
> delimiters exist, and lookback hasn't run, set did_soft_lookback and scan the
> window's cohorts in reverse for one matching `soft_delimiters`; on a hit,
> delimitAt() there, advance cSWindow to the produced next window, repoint
> cCohort->parent, and (verbose) warn; (b) soft-delimiter on the current cohort
> — if >= soft_limit and cCohort matches soft_delimiters, add `endtag` to all
> its readings, splitAllMappings, append cCohort to cSWindow, set line_number,
> close the window (cSWindow=cCohort=null, ++numCohorts); (c) hard break — if
> cCohort exists and (window >= hard_limit OR (not dep_delimit and cCohort
> matches `delimiters`)): if hard_limit hit and not is_conv, warn "Hard limit
> ... forcing break"; add endtag, splitAllMappings, append, close window. If no
> current window exists, allocate+append a new SingleWindow, initEmptySingleWindow
> it, set lSWindow, ++numWindows. If a pending cCohort exists, splitAllMappings
> and append it to the window. If `gWindow->next.size() > num_windows+1`,
> shuffleWindowsDown(), runGrammarOnWindow(), and every `numWindows % resetAfter
> == 0` call resetIndexes() (plus verbose progress). If this window's
> all_cohorts.size()==1 (first real cohort), call adopt_variables(). Finally
> allocate the new cCohort (global_number = cohort_counter++), set its wordform
> to addTag(cleaned wordform), remember it as lCohort, clear indents,
> ++numCohorts, set line_number. If tokens follow the wordform on the same line,
> build a `wread` (word-level reading) from the wordform tag plus each following
> quote-aware space-separated tag.
>
> (2) Reading line — `cleaned[0]==' ' && cleaned[1]=='"'` with a current
> cCohort: set is_deleted=false, readings=&cCohort->readings, fall into
> `got_reading`. got_reading: count leading whitespace of `line` as `indent`;
> pop `indents` entries whose level >= indent. If indents is non-empty and
> indent > its back level, this is a sub-reading: if the back reading already
> has a `next`, warn "each reading currently only can have one sub-reading",
> null cReading and `continue`; else allocate the reading as
> back.second->allocateReading(...) and link back.second->next to it. Otherwise
> allocate a normal reading in cCohort. Flag sets_any on possible_sets and add
> the wordform tag. Parse the baseform quote (retrying a raw, non-escaping scan
> to handle baseforms containing `\` before the closing `"`); if it still
> doesn't end in `"`, warn "looked like a reading but wasn't - treated as text",
> unlink the sub-reading if it was linked, free_reading, (if is_deleted re-add
> the leading ';' to cleaned and line) and jump to istext. Set
> cReading->deleted = is_deleted. Tokenize the remaining space-separated,
> quote-aware fields: addTag each; if a tag is a mapping (T_MAPPING or first
> char == grammar->mapping_prefix) mark it T_MAPPING and push into
> all_mappings[cReading], else addTagToReading. Warn if no baseform resulted.
> For a top-level reading (indents empty or indent <= back level) append it to
> `*readings`; for a sub-reading, discard extra mapping tags beyond the first
> (warn), splitMappings the remaining, erase from all_mappings, and rehash the
> parent reading. Push {indent, cReading} to indents; ++numReadings. Then the
> `--dep-delimit` check: if not deleted, dep_delimit>0, dep_highest_seen>0 and
> (cCohort->dep_self <= dep_highest_seen or dep_self - dep_highest_seen >
> dep_delimit), reflowDependencyWindow(cCohort->global_number), add endtag to
> the last cohort's readings, start a new window (copy the local variable maps
> into it, then clear them), ++numWindows, reset did_soft_lookback and
> dep_highest_seen=0, and if grammar->has_bag_of_tags reparent+reflow the
> cohort's readings.
>
> (3) Deleted-reading line — `pipe_deleted` and `cleaned` starts with `; "` with
> a current cCohort: set is_deleted=true, readings=&cCohort->deleted, erase the
> leading ';' from cleaned and line, and jump to got_reading.
>
> (4) Everything else / `istext`: if it looked like a reading with no containing
> cohort, (verbose) warn. If `line` is non-empty, recognize stream commands by
> comparing `cleaned`: FLUSH (mark the back window flush_after, close and append
> the pending cohort adding endtag, drain all pending windows via
> shuffleWindowsDown+runGrammarOnWindow, print+free all previous windows, emit
> the FLUSH command if there was no back window, clear `line`, clear
> `variables`, flush all streams); IGNORE (set ignoreinput=true, echo the
> command, clear line); RESUME (ignoreinput=false, echo, clear line); EXIT (echo
> the command, goto CGCMD_EXIT); `<SETVAR:...>` prefix (parse identifier[=value]
> items separated by ',' and '=' into variables_set/variables_rem/variables_output,
> defaulting a missing identifier or value to grammar->tag_any '*' with a
> warning, and also writing global `variables` when no window exists yet);
> `<REMVAR:...>` prefix (parse comma-separated names, erase from variables_set,
> insert into variables_rem and variables_output). If after command handling
> `line` is still non-empty: if lSWindow and lCohort and testStringAgainst(line,
> text_delimiters) is true, treat the line as a text delimiter — append it to
> lSWindow->text_post, add endtag to the pending cohort's readings,
> splitAllMappings, append the cohort, set line_number, and close the window;
> else if lCohort append the line to lCohort->text; else if lSWindow append to
> text_post (if that is non-empty) or otherwise to text; else if not a command,
> printPlainTextLine. At the end of each iteration ++numLines and reset
> line[0]=cleaned[0]=0.
>
> After the loop: set input_eof=true. If a pending cCohort+cSWindow remain,
> splitAllMappings, append the cohort, initEmptyCohort if it has no readings,
> add endtag to all its readings, and null the pointers. If binary output and
> variables_output is non-empty, binary_maybe_window()+adopt_variables(). Drain
> the remaining windows: while `gWindow->next` non-empty, shuffleWindowsDown() +
> runGrammarOnWindow() (+ verbose progress). shuffleWindowsDown() once more, then
> print+free every window in `gWindow->previous` in order. Flush output. For each
> hash in variables_output, emit a `<SETVAR:key[=value]>` (value omitted when it
> is tag_any) or `<REMVAR:key>` stream command reflecting the final state of
> variables_set. CGCMD_EXIT label: if verbose, print the "Did N lines, N
> windows, N cohorts, N readings" summary.

> [spec:cg3:def:grammar-applicator-run-grammar.cg3.test-string-against-fn]
> inline bool testStringAgainst(const UString& str, std::vector<URegularExpression*>& rxs)

> [spec:cg3:sem:grammar-applicator-run-grammar.cg3.test-string-against-fn]
> Free function. Tests whether `str` matches any of the pre-compiled ICU regexes
> in `rxs`, with a move-to-front (MRU) reordering side effect. Set rv=false.
> Iterate i from 0 to rxs.size()-1: reset the ICU error status; call
> `uregex_setText(rxs[i], str.data(), str.size(), &status)` to bind the subject
> text; if status is not U_ZERO_ERROR, CG3Quit(1) (fatal). Reset status; call
> `uregex_find(rxs[i], -1, &status)` — start index -1 means "search the whole
> region", and find is UNANCHORED, so it succeeds if the pattern matches
> anywhere in `str`. If it returns true: set rv=true; if i != 0, swap rxs[0]
> with rxs[i] so the matching regex moves to the front for faster future hits;
> break out of the loop. If after the find status is not U_ZERO_ERROR,
> CG3Quit(1). Return rv. In the port this must map to `regex` crate `is_match`
> (unanchored search) over each pattern, preserving the front-swap heuristic.
> Used by runGrammarOnText to detect text-delimiter lines via `text_delimiters`.

