# src/PlaintextApplicator.cpp, src/PlaintextApplicator.hpp

> [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator]
> class PlaintextApplicator : public virtual GrammarApplicator {
>   bool add_tags = false;
> }

> [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.plaintext-applicator-fn]
> PlaintextApplicator::PlaintextApplicator(std::ostream& ux_err)

> [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.plaintext-applicator-fn]
> Constructor. Takes a reference to an error output stream `ux_err` and
> forwards it to the base `GrammarApplicator(ux_err)` constructor. Its body
> sets the inherited flag allow_magic_readings = true (so cohorts created
> during parsing get magic/synthetic baseform readings). The class field
> add_tags keeps its default `false`. Nothing is returned.

> [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.print-cohort-fn]
> void PlaintextApplicator::printCohort(Cohort* cohort, std::ostream& output, bool)

> [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-cohort-fn]
> Prints a single cohort as bare text. The third bool parameter (profiling)
> is unnamed and ignored. If cohort->local_number == 0 (the window-boundary
> cohort) return immediately, printing nothing. If cohort->type has the
> CT_REMOVED bit set, return immediately. Otherwise print "%.*S " — the
> wordform's inner text followed by a single trailing space, using length
> wordform->tag.size()-4 starting at wordform->tag.data()+2 (the wordform
> stored as `"<word>"` with the leading `"<` and trailing `>"` stripped,
> leaving `word`). No readings, tags, or baseform are printed. Returns void.

> [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.print-single-window-fn]
> void PlaintextApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-single-window-fn]
> Prints an entire window as one line of space-separated wordforms. For each
> cohort in window->all_cohorts (in order) call printCohort(cohort, output,
> profiling) — each surviving cohort emits its wordform text plus a trailing
> space. After all cohorts, print one '\n' and call u_fflush(output). Note
> the boundary cohort (local_number 0) and CT_REMOVED cohorts print nothing,
> and window->text / window->text_post are NOT emitted. Returns void.

> [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.run-grammar-on-text-fn]
> void PlaintextApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.run-grammar-on-text-fn]
> Tokenizes raw plaintext from `input` into cohorts (one cohort per
> whitespace/punctuation-delimited token), runs the grammar, and writes the
> result to `output`. Overrides the base runGrammarOnText.
>
> Setup is identical to the Niceline/base variant: store &input/&output in
> ux_stdin/ux_stdout; fatal-error (message + CG3Quit(1)) if !input.good(),
> input.eof(), !output, or grammar null; warn if no hard/soft delimiters;
> allocate `line`(1024 zeros) and `cleaned`(same size); ignoreinput=false,
> did_soft_lookback=false; index(); resetAfter=((num_windows+4)*2+1);
> lines=0; null cSWindow/cCohort/cReading/lSWindow/lCohort;
> gWindow->window_span=num_windows; ux_stripBOM(input).
>
> Main loop: while !input.eof(): ++lines; packoff =
> get_line_clean(line, cleaned, input) — keep_tabs defaults to false, so
> TABs are collapsed to single spaces along with other whitespace runs. Trim
> trailing whitespace as in the other variants (while cleaned[0] and
> ISSPACE(cleaned[packoff-1]) zero and --packoff).
>
> Cohort branch — when !ignoreinput AND cleaned[0] != 0 AND cleaned[0]!='<':
>   If cCohort exists and cCohort->readings is empty, initEmptyCohort(*cCohort).
>   Soft-limit lookback: same as base — if cSWindow exists and its
>   cohorts.size() >= soft_limit and soft_delimiters exists and
>   !did_soft_lookback: set did_soft_lookback=true, scan cohorts in reverse,
>   and on the first soft-delimiter match delimitAt() to split, set cSWindow
>   to the split point's parent->next, reparent cCohort onto it, optionally
>   warn, break.
>   Soft-limit break: if cCohort and cohorts.size()>=soft_limit and
>   soft_delimiters and cCohort matches soft_delimiters: optionally warn;
>   append endtag to each reading; appendCohort(cCohort); set lSWindow=cSWindow,
>   lCohort=cCohort; null cSWindow/cCohort; ++numCohorts; clear did_soft_lookback.
>   Hard break: if cCohort and (cohorts.size()>=hard_limit OR (!dep_delimit
>   and delimiters and cCohort matches delimiters)): if !is_conv and
>   size>=hard_limit warn; append endtag to each reading; appendCohort;
>   lSWindow=cSWindow, lCohort=cCohort; null cSWindow/cCohort; ++numCohorts;
>   clear did_soft_lookback.
>   New window: if !cSWindow, allocAppendSingleWindow + initEmptySingleWindow;
>   lSWindow=cSWindow; lCohort = cSWindow->cohorts[0] (the boundary cohort);
>   cCohort=null; ++numWindows; clear did_soft_lookback.
>   Window flush: if gWindow->next.size() > num_windows: shuffleWindowsDown();
>   runGrammarOnWindow(); if numWindows % resetAfter == 0 resetIndexes(); if
>   verbosity_level>0 print a Progress line.
>   QUIRK: because cCohort is set to null after every token (see below), at
>   the top of each line cCohort is null, so the three cCohort-gated break
>   blocks and the empty-readings init never fire, and cSWindow (once set) is
>   never nulled — in practice the entire plaintext input accumulates into a
>   single SingleWindow (only the cCohort-independent soft-lookback delimitAt
>   path can split it).
>
>   Raw split on spaces: base=&cleaned[0], space=base. While space && *space
>   && (space=u_strchr(space,' '))!=null: set space[0]=0; if base && base[0]
>   push base into vector tokens_raw; base=++space. After the loop, if
>   base && base[0] push base. (Empty runs are skipped.)
>
>   Punctuation splitting: build vector<UnicodeString> tokens. For each raw
>   token p (len = u_strlen(p)): while *p && u_ispunct(p[0]) push a
>   single-char token UnicodeString(p[0]), ++p, --len (peel LEADING
>   punctuation chars into their own tokens); record tkz = tokens.size();
>   while *p && u_ispunct(p[len-1]) push single-char token
>   UnicodeString(p[len-1]), set p[len-1]=0, --len (peel TRAILING punctuation,
>   which are appended in reverse order); if *p is still non-empty, insert the
>   remaining middle UnicodeString(p) at position tkz (before the trailing
>   punctuation tokens). Net effect: leading-punct tokens, then the core
>   token, then trailing-punct tokens in original order.
>
>   Cohort creation: for each `token` in tokens: compute case flags over the
>   token — first_upper = u_isupper(token[0]) != 0; all_upper starts equal to
>   first_upper and is cleared if any char at index >=1 is NOT upper;
>   mixed_upper = true if any char at index >=1 IS upper. Allocate
>   cCohort=alloc_cohort(cSWindow); global_number=gWindow->cohort_counter++;
>   build wordform tag u"\"<" + token + u">\"" and set cCohort->wordform =
>   addTag(tag); lCohort=cCohort; ++numCohorts. cReading =
>   initEmptyCohort(*cCohort) (a magic reading whose baseform is derived from
>   the wordform); set cReading->noprint = !add_tags (so by default the
>   reading is not printed). If add_tags: add the tag "<cg-conv>" to cReading.
>   If add_tags && (first_upper||all_upper||mixed_upper):
>   delTagFromReading(*cReading, cReading->baseform) to drop the magic
>   baseform, token.toLower(), add a new baseform `"<lowercased-token>"`
>   written as `"` + token + `"`, then add case tags: "<all-upper>" if
>   all_upper, "<first-upper>" if first_upper, "<mixed-upper>" if mixed_upper
>   && !all_upper. Finally cSWindow->appendCohort(cCohort) and set
>   cCohort=null.
>
> Text branch — otherwise: if cleaned[0] && line[0]: if lCohort append
> &line[0] to lCohort->text; else if lSWindow append to lSWindow->text; else
> printPlainTextLine(&line[0], output).
>
> End of iteration: ++numLines; line[0]=cleaned[0]=0.
>
> Finalization is identical to the base/Niceline variant: if cCohort &&
> cSWindow, appendCohort + initEmptyCohort-if-empty + endtag on each reading,
> null pointers (in practice unreached, cCohort is null); drain gWindow->next
> with shuffleWindowsDown()+runGrammarOnWindow(); a final shuffleWindowsDown()
> then, while gWindow->previous non-empty, printSingleWindow + free_swindow +
> erase the front; u_fflush(output). Returns void.

