# src/NicelineApplicator.cpp, src/NicelineApplicator.hpp

> [spec:cg3:def:niceline-applicator.cg3.niceline-applicator]
> class NicelineApplicator : public virtual GrammarApplicator {
>   bool did_warn_statictags = false;
>   bool did_warn_subreadings = false;
> }

> [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.niceline-applicator-fn]
> NicelineApplicator::NicelineApplicator(std::ostream& ux_err)

> [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.niceline-applicator-fn]
> Constructor. Takes a reference to an error output stream `ux_err` and
> forwards it unchanged to the base `GrammarApplicator(ux_err)` constructor;
> it has no body of its own. The two class members `did_warn_statictags` and
> `did_warn_subreadings` keep their in-class default value `false` (they are
> one-shot latches used later to warn at most once that Niceline output
> cannot represent static tags and sub-readings). No other state is set and
> nothing is returned.

> [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-cohort-fn]
> void NicelineApplicator::printCohort(Cohort* cohort, std::ostream& output, bool profiling)

> [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-cohort-fn]
> Prints one cohort in Niceline format. `profiling` defaults to false.
> Control flow: if cohort->local_number == 0 (the window-boundary cohort) or
> cohort->type has the CT_REMOVED bit set, jump straight to the `removed`
> tail (skip the word and readings, only emit the trailing newline and any
> cohort text). Otherwise: if cohort->wblank is non-empty, print it verbatim
> ("%S"), and if its last character is not a newline (ISNL) print a '\n'.
> Then print the wordform's inner text with "%.*S" using length
> wordform->tag.size()-4 starting at wordform->tag.data()+2 — i.e. the
> wordform tag stored as `"<word>"` with the leading `"<` (2 chars) and
> trailing `>"` (2 chars) stripped, leaving `word`. If cohort->wread is set
> and did_warn_statictags is still false, warn once to ux_stderr "Niceline
> CG format cannot output static tags! You are losing information!" and set
> did_warn_statictags=true (static tags are silently dropped — a lossy
> quirk). If !profiling: call cohort->unignoreAll(), and if !split_mappings
> call mergeMappings(*cohort) to fold mapped readings back together. If
> cohort->readings is empty, print a single '\t'. Then for each reading in
> cohort->readings (in order) call printReading(reading, output). At the
> `removed` label: print '\n'. Finally, if cohort->text is non-empty AND it
> contains at least one character not in `ws` (the two-char set {space,tab},
> i.e. find_first_not_of(ws) != npos), print cohort->text verbatim and, if
> its last char is not a newline, print '\n'. Returns void.

> [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-reading-fn]
> void NicelineApplicator::printReading(const Reading* reading, std::ostream& output)

> [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-reading-fn]
> Prints one reading (its tags, dependency, relations, trace) on the tail of
> the current cohort line. If reading->noprint is true, or reading->deleted
> is true, return immediately (print nothing). Otherwise print a leading
> '\t'. If reading->baseform != 0, look up the baseform tag in
> grammar->single_tags by that hash and print "[%.*S]" with length
> tag.size()-2 starting at tag.data()+1 — i.e. the baseform stored as
> `"base"` printed as `[base]` (both surrounding quotes stripped, wrapped in
> square brackets).
>
> Tag loop: maintain a local uint32SortedVector `unique`. For each tag hash
> `tter` in reading->tags_list in order: skip if (!show_end_tags && tter ==
> endtag) or tter == begintag; skip if tter == reading->baseform or tter ==
> reading->parent->wordform->hash; if unique_tags is set, skip if tter is
> already in `unique`, else insert it. Look up tag = grammar->single_tags[tter].
> Skip if (tag->type & T_DEPENDENCY) && has_dep && !dep_original. Skip if
> (tag->type & T_RELATION) && has_relations. Otherwise print " %S"
> (space + tag text).
>
> Dependency block: if has_dep && !(reading->parent->type & CT_REMOVED): if
> parent->dep_self == 0, set parent->dep_self = parent->global_number. Set
> pr = parent; if parent->dep_parent != DEP_NO_PARENT then: if dep_parent ==
> 0 set pr = parent->parent->cohorts[0] (the window's root cohort), else if
> dep_parent is a key in gWindow->cohort_map set pr to that cohort. Choose a
> format pattern: " #%u->%u" normally, or " #%u→%u" (space,'#',num,U+2192,num)
> if unicode_tags is set. Then: if dep_absolute, print pattern with
> (parent->global_number, pr->global_number); else if !dep_has_spanned,
> print with (parent->local_number, pr->local_number); else if dep_parent ==
> DEP_NO_PARENT print with (parent->dep_self, parent->dep_self), otherwise
> print with (parent->dep_self, parent->dep_parent).
>
> Relations block: if reading->parent->type & CT_RELATED, print " ID:%u"
> with parent->global_number; then if parent->relations is non-empty, for
> each (relation-name-hash, target-set) pair and each target number in the
> set, print " R:%S:%u" with the relation tag text (single_tags lookup of
> the name hash) and the target number.
>
> Trace block: if `trace` is set, for each entry iter_hb in reading->hit_by:
> print ' ' then call printTrace(output, iter_hb).
>
> Sub-reading warning: if reading->next is non-null and did_warn_subreadings
> is still false, warn once to ux_stderr "Niceline CG format cannot output
> sub-readings! You are losing information!" and set
> did_warn_subreadings=true. The sub-readings themselves are NOT printed —
> they are silently dropped (a lossy quirk). Returns void; the caller emits
> the terminating newline.

> [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-single-window-fn]
> void NicelineApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-single-window-fn]
> Prints an entire window. `profiling` defaults to false. If window->text is
> non-empty, print it verbatim ("%S") and, if its last char is not a newline
> (ISNL), print '\n'. Then for each cohort in window->all_cohorts (in order),
> call printCohort(cohort, output, profiling). Then if window->text_post is
> non-empty, print it verbatim and, if its last char is not a newline, print
> '\n'. Finally print one '\n' (blank line separating windows) and call
> u_fflush(output). Returns void.

> [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.run-grammar-on-text-fn]
> void NicelineApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.run-grammar-on-text-fn]
> Reads the Niceline stream from `input`, builds windows/cohorts/readings,
> runs the grammar, and prints results to `output`. It is a Niceline-specific
> reimplementation of GrammarApplicator::runGrammarOnText with a distinct
> per-line tokenizer: each cohort occupies one line of the form
> "wordform<TAB>reading<TAB>reading...", each reading being space-separated
> tags with the baseform written as `[base]`.
>
> Setup: store &input in ux_stdin and &output in ux_stdout. Fatal-error
> (message to ux_stderr then CG3Quit(1)) if !input.good(), if input.eof(),
> if !output, or if grammar is null. If grammar has no hard delimiters
> (delimiters null or empty): if it also has no soft delimiters, warn that
> the hard_limit may break windows unintentionally, else warn the soft_limit
> may. Allocate UString `line`(1024 zeros) and UString `cleaned`(same size).
> Set ignoreinput=false, did_soft_lookback=false. Call index(). Compute
> resetAfter = ((num_windows+4)*2+1); lines=0. Null the running pointers
> cSWindow, cCohort, cReading, lSWindow, lCohort. Set gWindow->window_span =
> num_windows. Call ux_stripBOM(input) to consume a leading BOM.
>
> Main loop: while !input.eof(): ++lines; packoff =
> get_line_clean(line, cleaned, input, true) reads one logical line into
> `line` and a whitespace-collapsed copy into `cleaned` (runs of spaces
> collapse to one; TABs are PRESERVED because keep_tabs=true), returning
> `cleaned`'s length. Trim trailing whitespace: while cleaned[0] != 0 and
> ISSPACE(cleaned[packoff-1]), zero that char and --packoff.
>
> Cohort branch — taken when !ignoreinput AND cleaned[0] != 0 AND
> cleaned[0] != '<':
>   Set space = &cleaned[0]; SKIPTO_NOSPAN(space,'\t') advances space to the
>   first unescaped TAB, stopping early at end-of-string or any ISNL char.
>   After: if space[0] != 0 and space[0] != '\t' (it stopped on a newline,
>   i.e. there was no TAB) warn "%S on line %u looked like a cohort but
>   wasn't - treated as text." and goto istext (text branch below). If
>   space[0] == 0 (end reached, no TAB) set space[1]=0 (guard so the reading
>   loop starts on an empty region). Set space[0]=0 to terminate the
>   wordform text at the TAB.
>
>   Soft-limit lookback: if cSWindow exists and cSWindow->cohorts.size() >=
>   soft_limit and grammar->soft_delimiters exists and !did_soft_lookback:
>   set did_soft_lookback=true, scan cSWindow->cohorts in reverse, and on the
>   first cohort matching the soft_delimiters set
>   (doesSetMatchCohortNormal(*c, soft_delimiters->number)) clear
>   did_soft_lookback, call delimitAt(*cSWindow, c) to split the window
>   there, set cSWindow = returned-cohort->parent->next, and if cCohort exists
>   set cCohort->parent = cSWindow; optionally warn (verbosity_level>0);
>   break.
>
>   Soft-limit break: if cCohort exists and cSWindow->cohorts.size() >=
>   soft_limit and soft_delimiters exists and cCohort matches soft_delimiters:
>   optionally warn; append endtag to every reading of cCohort
>   (addTagToReading); appendCohort(cCohort) onto cSWindow; set
>   lSWindow=cSWindow; null cSWindow and cCohort; ++numCohorts; clear
>   did_soft_lookback.
>
>   Hard break: if cCohort exists and (cSWindow->cohorts.size() >= hard_limit
>   OR (!dep_delimit and grammar->delimiters and cCohort matches delimiters)):
>   if !is_conv and size >= hard_limit, warn "Hard limit of %u cohorts reached
>   at cohort %S (#%u) on line %u - forcing break."; append endtag to each
>   reading; appendCohort(cCohort); lSWindow=cSWindow; null cSWindow/cCohort;
>   ++numCohorts; clear did_soft_lookback.
>
>   New window: if !cSWindow, cSWindow = gWindow->allocAppendSingleWindow(),
>   initEmptySingleWindow(cSWindow) (creates the boundary cohort 0 carrying
>   begintag), lSWindow=cSWindow, cCohort=null, ++numWindows, clear
>   did_soft_lookback.
>
>   If cCohort and cSWindow both set, appendCohort(cCohort) onto cSWindow.
>
>   Window flush: if gWindow->next.size() > num_windows: shuffleWindowsDown();
>   runGrammarOnWindow(); if numWindows % resetAfter == 0 call resetIndexes();
>   if verbosity_level>0 print a "Progress:" line to ux_stderr.
>
>   Build wordform: tag = u"\"<" + &cleaned[0] (the text before the TAB) +
>   u">\"". Allocate cCohort = alloc_cohort(cSWindow); set global_number =
>   gWindow->cohort_counter++; wordform = addTag(tag); lCohort = cCohort;
>   ++numCohorts.
>
>   Reading loop: advance ++space past the (nulled) TAB. While space && space[0]:
>   allocate cReading = alloc_reading(cCohort); insert grammar->sets_any into
>   cReading->parent->possible_sets. Set base=space; if *space=='"' do
>   ++space then SKIPTO_NOSPAN(space,'"') (skip the quoted baseform); then if
>   *space=='[' SKIPTO_NOSPAN(space,']'). Find the next TAB via
>   u_strchr(space,'\t'); if found, set tab[0]=0 so this reading ends before
>   the next reading. Token loop: while space && *space &&
>   (space=u_strchr(space,' ')) != null: set space[0]=0; if base && base[0]:
>   if base[0]=='[' and space[-1]==']', rewrite both to '"' (turning `[x]`
>   into `"x"`, a baseform string); tag=addTag(base); if the tag is a mapping
>   ((tag->type & T_MAPPING) OR tag->tag[0]==grammar->mapping_prefix) push it
>   on a local `mappings` TagList, else addTagToReading(*cReading, tag). Then
>   base=++space and again: if *space=='"' skip to matching '"', if
>   *space=='[' skip to ']'. After the loop, process the trailing token
>   `base` identically (bracket→quote rewrite, addTag, mapping-vs-reading).
>   If cReading->baseform is still 0, set it to
>   cReading->parent->wordform->hash and warn "Line %u had no valid
>   baseform." If `mappings` is non-empty, call splitMappings(mappings,
>   *cCohort, *cReading, true) to expand mapping tags into multiple readings.
>   appendReading(cReading); ++numReadings. If a TAB was found, set
>   space=++tab and continue to the next reading. After the reading loop, if
>   cCohort->readings is empty, call initEmptyCohort(*cCohort).
>
> Text branch (istext) — taken otherwise (ignoreinput, empty cleaned,
> cleaned starting with '<', or via the goto): if cleaned[0] and line[0] are
> both non-zero: if lCohort exists append &line[0] to lCohort->text; else if
> lSWindow exists append to lSWindow->text; else print immediately via
> printPlainTextLine(&line[0], output).
>
> End of iteration: ++numLines; reset line[0]=cleaned[0]=0.
>
> Finalization after EOF: if cCohort && cSWindow: appendCohort(cCohort); if
> it has no readings call initEmptyCohort; append endtag to each of its
> readings; null cReading/cCohort/cSWindow. While gWindow->next is non-empty:
> shuffleWindowsDown(); runGrammarOnWindow(). Then one final
> shuffleWindowsDown(), and while gWindow->previous is non-empty: take the
> front window, printSingleWindow(tmp, output), free_swindow(tmp), erase it
> from previous. Finally u_fflush(output). Returns void.

