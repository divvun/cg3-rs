# src/FSTApplicator.cpp, src/FSTApplicator.hpp

> [spec:cg3:def:fst-applicator.cg3.fst-applicator]
> class FSTApplicator : public virtual GrammarApplicator {
>   bool did_warn_statictags = false;
>   double wfactor = 1.0;
>   UString wtag{'W'};
>   UString sub_delims{'#'};
> }

> [spec:cg3:def:fst-applicator.cg3.fst-applicator.fst-applicator-fn]
> FSTApplicator::FSTApplicator(std::ostream& ux_err)

> [spec:cg3:sem:fst-applicator.cg3.fst-applicator.fst-applicator-fn]
> Constructor. Delegates to the base `GrammarApplicator(ux_err)` constructor and
> has an empty body of its own. The subclass data members therefore take their
> in-class defaults: `did_warn_statictags = false`, `wfactor = 1.0`, `wtag` = the
> one-character string `"W"`, `sub_delims` = the one-character string `"#"`. No
> other side effects.

> [spec:cg3:def:fst-applicator.cg3.fst-applicator.print-cohort-fn]
> void FSTApplicator::printCohort(Cohort* cohort, std::ostream& output, bool profiling)

> [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-cohort-fn]
> Serializes one cohort to `output`. Control flow uses a `removed:` label so that
> removed cohorts still get their trailing text printed.
> If `cohort->local_number == 0` (the magic 0th/boundary cohort) or `cohort->type
> & CT_REMOVED`, jump straight to `removed:` (skip the wordform/readings block).
> Otherwise: if `cohort->wblank` is non-empty, print it verbatim, and if its last
> character is not a newline (`ISNL`), print a `'\n'`. If `cohort->wread` is set
> (there are static tags) and `did_warn_statictags` is still false, print to
> stderr `"Warning: FST CG format cannot output static tags! You are losing
> information!"`, flush stderr, and set `did_warn_statictags = true` (once per run;
> static tags are otherwise dropped from FST output).
> If not `profiling`: call `cohort->unignoreAll()`, and if `split_mappings` is
> false call `mergeMappings(*cohort)`.
> Then print the wordform and readings. Let `wform = cohort->wordform->tag`. The
> wordform is printed stripped of its `"<` prefix and `>"` suffix: it emits
> `wform.size() - 4` UChars starting at `wform.data() + 2`. If `cohort->readings`
> is empty, or has exactly one reading whose `noprint` is set, print
> `"<wordform>\t+?\n"` (the FST "no analysis" marker `+?`). Otherwise, for each
> reading in `cohort->readings` (in current vector order — note printCohort does
> NOT sort readings), print the stripped wordform, a tab, then call
> `printReading(reading, output)`, then a newline. After the readings block, print
> one extra blank line (`'\n'`).
> `removed:` — if `cohort->text` is non-empty AND contains at least one character
> not in the whitespace set `ws` (space, tab, newline), print `cohort->text`
> verbatim and, if its last char is not a newline, print `'\n'`.

> [spec:cg3:def:fst-applicator.cg3.fst-applicator.print-reading-fn]
> void FSTApplicator::printReading(const Reading* reading, std::ostream& output)

> [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-reading-fn]
> Serializes one `Reading` to `output` in FST-lookup analysis syntax (a
> `+`-joined chain of baseform then tags), recursing into subreadings. Steps:
> If `reading->noprint` or `reading->deleted`, return immediately (print nothing).
> If `reading->next` is non-null (a subreading exists), first recurse
> `printReading(reading->next, output)`, then emit the subreading delimiter
> `sub_delims` (default `"#"`). Because the recursion happens before printing the
> current reading, the innermost subreading is printed first and the top reading
> last, separated by `#`.
> Then, if `reading->baseform` is non-zero, look up its tag in
> `grammar->single_tags` and print the baseform text WITHOUT its surrounding
> quotes: it prints `tag.size() - 2` UChars starting at `tag.data() + 1` (i.e.
> strips the leading and trailing `"`).
> Then iterate `reading->tags_list` in order, maintaining a `uint32SortedVector
> unique`. For each tag hash `tter`, skip it when: `tter == endtag` and
> `show_end_tags` is false; or `tter == begintag`; or `tter == reading->baseform`;
> or `tter == reading->parent->wordform->hash`. If `unique_tags` is set and `tter`
> is already in `unique`, skip; otherwise insert it. Look up `tag =
> grammar->single_tags[tter]`; skip it if `(tag->type & T_DEPENDENCY)` and
> `has_dep` and not `dep_original`; skip if `(tag->type & T_RELATION)` and
> `has_relations`. Otherwise print `"+"` followed by the tag's raw text
> (`tag->tag`). No trailing newline is written here (the caller adds it).

> [spec:cg3:def:fst-applicator.cg3.fst-applicator.print-single-window-fn]
> void FSTApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-single-window-fn]
> Serializes a whole `SingleWindow`. Steps: if `window->text` (pre-window text) is
> non-empty, print it verbatim and, if its last char is not a newline (`ISNL`),
> print `'\n'`. Then iterate `window->all_cohorts` in order and call
> `printCohort(cohort, output, profiling)` for each. Then if `window->text_post`
> is non-empty, print it verbatim and add a `'\n'` if it does not already end in a
> newline. Finally always print one blank line (`'\n'`) after the window, and
> flush `output`. The `profiling` flag is only forwarded to `printCohort`.

> [spec:cg3:def:fst-applicator.cg3.fst-applicator.run-grammar-on-text-fn]
> void FSTApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:fst-applicator.cg3.fst-applicator.run-grammar-on-text-fn]
> Reads the FST-lookup text format (one `wordform<TAB>analysis[<TAB>weight]`
> per line; a wordform's readings appear on consecutive lines; a blank/other line
> ends the cohort), builds windows, runs the grammar, and prints results. No regex
> is used anywhere; parsing is manual character scanning plus `strtof`.
> Setup: store `&input` in `ux_stdin`, `&output` in `ux_stdout`. If
> `!input.good()` print "Error: Input is null..." and `CG3Quit(1)`; if
> `input.eof()` print "Error: Input is empty..." and quit; if `!output` print
> "Error: Output is null..." and quit; if `!grammar` print "Error: No grammar
> provided..." and quit. If the grammar has no hard delimiters, warn about the
> hard limit (or, if it also has no soft delimiters, warn about both). Allocate a
> reusable UString `line` of 1024 zero UChars and `cleaned` of the same size.
> `ignoreinput=false`, `did_soft_lookback=false`. Call `index()`. Compute
> `resetAfter = (num_windows + 4) * 2 + 1`; `lines = 0`. Set the running pointers
> `cSWindow`, `cCohort`, `cReading`, `lSWindow`, `lCohort` to null. Set
> `gWindow->window_span = num_windows`. Call `ux_stripBOM(input)`.
> Main loop: `while (!input.eof())`. Each iteration: `++lines`;
> `packoff = get_line_clean(line, cleaned, input, true)` reads one logical line
> into `line` and a whitespace-collapsed copy into `cleaned` (runs of spaces
> collapse to a single space; runs containing a tab collapse to a single tab
> because `keep_tabs` is true; the returned `packoff` is the length of `cleaned`).
> Trim trailing whitespace: while `cleaned[0]` is set and `cleaned[packoff-1]` is
> whitespace (`ISSPACE`), zero it and decrement `packoff`.
> If NOT `ignoreinput` and `cleaned[0]` is non-zero (a non-empty content line):
> set `space = &cleaned[0]` and advance it with `SKIPTO_NOSPAN_RAW(space, '\t')`
> (moves to the first TAB, stopping at NUL or a newline; no escape handling). If
> `space[0] != '\t'` (no tab was found) the line is not a cohort: if
> `cleaned[0] != '<'` (does not look like inline markup) print to stderr
> "Warning: <text> on line <numLines> looked like a cohort but wasn't - treated
> as text." and flush; then `goto istext`. Otherwise set `space[0] = 0` to
> terminate the wordform at the tab. Build the wordform tag `tag = "\"<" + cleaned
> + ">\""`.
> Cohort creation: if `cCohort` is null, then if `cSWindow` is also null allocate
> a new SingleWindow via `gWindow->allocAppendSingleWindow()`,
> `initEmptySingleWindow`, set `lSWindow=cSWindow`, `++numWindows`,
> `did_soft_lookback=false`; then allocate `cCohort` via `alloc_cohort(cSWindow)`,
> set `global_number = gWindow->cohort_counter++`, `wordform = addTag(tag)`,
> `lCohort=cCohort`, `++numCohorts`. (Because the cohort is only created when
> `cCohort` is null, consecutive input lines all feed readings into the SAME
> cohort until a blank/text line clears `cCohort`; the wordform is taken from the
> first line only.)
> Reading parse: advance `++space` past the NUL to the analysis field, then run
> `while (space && *space && !(space[0]=='+' && space[1]=='?' && space[2]==0))`.
> This body builds exactly one reading (it runs once because `space` becomes null
> once the analysis is fully consumed); it is skipped entirely when the analysis
> is exactly `"+?"` (the FST "no analysis" marker). Inside: `tab =
> u_strchr(space, '\t')` finds the analysis/weight separator; if `tab` exists and
> `tab[1]=='+' && tab[2]=='?'` (the FST re-emitted the input as a non-match)
> `break` without making a reading. Allocate `cReading = alloc_reading(cCohort)`,
> `insert_if_exists(cReading->parent->possible_sets, grammar->sets_any)`, and
> `addTagToReading(*cReading, cCohort->wordform)`. Set `base = space` (start of
> analysis), empty `mappings` TagList, `wtag_tag = nullptr`, `weight = 0.0`.
> Weight handling (only if `tab` was found): set `tab[0]=0`, `++tab` to point at
> the weight text; if it contains a comma replace the FIRST comma with `.`
> (locale decimal). Copy up to 31 characters of the weight text into a `char[32]`
> `buf` (each UChar truncated to `char`), NUL-terminate. If `buf == "inf"` format
> `buf` as `"%f"` of `NUMERIC_MAX` (= 2^48-1 = 281474976710655). Otherwise
> `weight = strtof(buf, 0)`, then `weight *= wfactor`, then reformat `buf` as
> `"%f"` of `weight`. Build the weight tag string `wtag_buf = "<" + wtag + ":" +
> buf + ">"` (e.g. `"<W:1.500000>"`) and `wtag_tag = addTag(wtag_buf)`.
> Baseform/tag tokenization over the analysis (`base`..): first, `plus =
> u_strchr(space, '+')`; if found, `++plus`, count leading `'+'` runs via
> `u_strspn`, then set `space = plus + run - 1` (this positions the scan so a
> baseform that is itself `+` or ends in `+` is handled). Then loop
> `while (space && *space && (space = u_strchr(space, '+')) != 0)` splitting the
> analysis on `'+'`: for each segment `[base, space)` when `base[0]` is set,
> compute `f = u_strcspn(base, sub_delims.data())` (offset of the first
> subreading delimiter `#`). If `f` is non-zero and `base+f < space` (a `#` occurs
> within this segment before the `+`), grow `cleaned` by one NUL, re-derive the
> `hash`/`base` pointers after the reallocation, shift the tail right by one to
> insert a NUL at the `#` position, set that position to 0, and set `space = hash`
> so the `#` acts as the segment end. Set `space[0] = 0` to terminate the segment.
> If `cReading->baseform == 0` this first segment is the baseform: wrap it as
> `tag = "\"" + base + "\""` and point `base` at that. If `base[0] == 0` set
> `base = "_"` and warn "Line <numLines> had empty tag." Create `Tag* t =
> addTag(base)`; if `t` is a mapping tag (`t->type & T_MAPPING` or its first char
> equals `grammar->mapping_prefix`) push it onto `mappings`, otherwise
> `addTagToReading(*cReading, t)`. If a `#` split occurred (`hash && hash[0]==0`),
> this ends a subreading: if `wtag_tag` is set add it to the current reading, then
> allocate `nr = cReading->allocateReading(cReading->parent)`, set `nr->next =
> cReading`, make `cReading = nr` (new innermost reading), and `++space`. Finally
> `base = ++space` to advance past the `+`. After the loop, the trailing segment
> (`base` non-empty): if `cReading->baseform == 0` wrap it in quotes as baseform,
> `addTag`, and route to `mappings` or `addTagToReading` the same way.
> After tokenization: if `wtag_tag` is set, add it to `cReading`. If
> `cReading->baseform` is still 0, set it to `cCohort->wordform->hash` and warn
> "Line <numLines> had no valid baseform." If the baseform tag text has size 2
> (i.e. an empty `""` baseform), remove it via `delTagFromReading` and set
> `cReading->baseform = makeBaseFromWord(cCohort->wordform->hash)->hash` (derive
> baseform from the wordform). If `mappings` is non-empty call
> `splitMappings(mappings, *cCohort, *cReading, true)`. If `grammar->sub_readings_ltr`
> and `cReading->next` is set, reverse the subreading chain via `cReading =
> reverse(cReading)`. Then `cCohort->appendReading(cReading)` and `++numReadings`.
> istext branch (blank line, ignored line, or a line with no tab): if `cCohort`
> exists and has no readings, `initEmptyCohort(*cCohort)`. If `is_conv`
> (cg-conv/conversion mode): if `cCohort` exists set its `local_number = 1`, call
> `printCohort(cCohort, output)`, `free_cohort`, and null `cCohort`; if the source
> line had content print it with `printPlainTextLine(&line[0], output)`; then
> `continue`.
> Otherwise apply delimiting. (1) Soft-delimiter lookback: if `cSWindow` exists,
> its cohort count ≥ `soft_limit`, soft delimiters exist, and `did_soft_lookback`
> is false: set `did_soft_lookback=true`, then scan `cSWindow->cohorts` in reverse
> for the first cohort matching `grammar->soft_delimiters` (via
> `doesSetMatchCohortNormal`); if found, set `did_soft_lookback=false`, split the
> window at it via `delimitAt(*cSWindow, c)` (returns the first cohort of the new
> window), set `cSWindow` to that cohort's parent's `next`, re-parent `cCohort` to
> the new `cSWindow`, optionally warn, and break. (2) If `cCohort` exists, count ≥
> `soft_limit`, soft delimiters exist, and `cCohort` itself matches a soft
> delimiter: optionally warn, add `endtag` to all of `cCohort`'s readings, append
> `cCohort` to `cSWindow`, set `lSWindow/lCohort`, null `cSWindow`, clear
> `did_soft_lookback`. (3) If `cCohort` exists and (count ≥ `hard_limit` OR (not
> `dep_delimit` and `cCohort` matches a hard delimiter)): if hitting the hard
> limit (and not `is_conv`) warn "Hard limit ... forcing break"; add `endtag` to
> all readings, append `cCohort`, update `lSWindow/lCohort`, null `cSWindow`,
> clear `did_soft_lookback`.
> Window (re)allocation: if `cSWindow` is null now, allocate a new SingleWindow,
> `initEmptySingleWindow`, set `lSWindow=cSWindow`, `lCohort = cSWindow->cohorts[0]`
> (the empty 0th cohort), null `cCohort`, `++numWindows`, clear
> `did_soft_lookback`. If `cCohort` and `cSWindow` both exist, append `cCohort` to
> `cSWindow` and set `lCohort=cCohort`. If `gWindow->next.size() > num_windows`,
> `shuffleWindowsDown()`, `runGrammarOnWindow()`, and if `numWindows % resetAfter
> == 0` call `resetIndexes()`; optionally print progress. Then null `cCohort`.
> Attach text: if the source `line` had content, append `&line[0]` to
> `lCohort->text` if `lCohort` exists, else to `lSWindow->text` if that exists,
> else print it via `printPlainTextLine`.
> End of each iteration: `++numLines`; zero `line[0]` and `cleaned[0]`.
> After the loop: if `cCohort` and `cSWindow` still exist, append `cCohort`,
> `initEmptyCohort` if it has no readings, add `endtag` to all its readings, and
> null `cReading/cCohort/cSWindow`. Drain buffered windows: while
> `gWindow->next` is non-empty, `shuffleWindowsDown` + `runGrammarOnWindow`. Then
> `shuffleWindowsDown()` once more and, while `gWindow->previous` is non-empty,
> pop the front window, `printSingleWindow(tmp, output)`, `free_swindow(tmp)`,
> erase it. Finally flush `output`.

