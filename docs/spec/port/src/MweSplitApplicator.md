# src/MweSplitApplicator.cpp, src/MweSplitApplicator.hpp

> [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator]
> class MweSplitApplicator : public virtual GrammarApplicator

> [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.maybe-wf-tag-fn]
> const Tag* MweSplitApplicator::maybeWfTag(const Reading* r)

> [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.maybe-wf-tag-fn]
> Returns the first "extra" wordform-type tag on reading `r`, or nullptr if
> none. Iterate r->tags_list in order. Skip a tag hash `tter` if
> (!show_end_tags && tter == endtag) or tter == begintag. Skip if tter ==
> r->baseform or tter == r->parent->wordform->hash (the reading's own
> baseform and its cohort's wordform). Look up tag = grammar->single_tags[tter];
> if tag->type has the T_WORDFORM bit set, return that Tag* immediately.
> After exhausting the list, return nullptr. In other words it finds a
> `"<...>"` wordform tag embedded in the reading that is not the cohort's own
> wordform — its presence signals that this (head or sub) reading carries a
> component word of a multi-word expression that should be split out.

> [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.mwe-split-applicator-fn]
> MweSplitApplicator::MweSplitApplicator(std::ostream& ux_err)

> [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.mwe-split-applicator-fn]
> Constructor. Forwards `ux_err` to the base GrammarApplicator(ux_err). Its
> body builds and installs a minimal dummy grammar so the base parsing
> machinery has something to run: allocate `grammar = new Grammar`; set
> grammar->ux_stderr = ux_stderr; call grammar->allocateDummySet();
> grammar->delimiters = grammar->allocateSet(); add a dummy tag to that
> delimiters set via grammar->addTagToSet(grammar->allocateTag(STR_DUMMY),
> grammar->delimiters) where STR_DUMMY is the literal
> u"__CG3_DUMMY_STRINGBIT__" (a sentinel that will never match real input, so
> nothing is ever treated as a delimiter); grammar->reindex(); setGrammar(grammar).
> Then set owns_grammar = true (so this applicator deletes the grammar it
> created) and is_conv = true (marks it a format converter, suppressing
> hard-limit warnings and enabling converter behaviors). Nothing is returned.

> [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.print-single-window-fn]
> void MweSplitApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.print-single-window-fn]
> Prints a window, MWE-splitting every cohort on the way out. `profiling`
> defaults to false.
>
> Variables block: for each var (a tag hash) in window->variables_output (a
> sorted set): key = grammar->single_tags[var]; look up var in
> window->variables_set. If present: if its value != grammar->tag_any, let
> value = grammar->single_tags[value-hash] and print "%S%S=%S>\n" with
> STR_CMD_SETVAR (u"<STREAMCMD:SETVAR:"), key->tag, value->tag — e.g.
> "<STREAMCMD:SETVAR:key=value>\n"; else print "%S%S>\n" with STR_CMD_SETVAR
> and key->tag — "<STREAMCMD:SETVAR:key>\n". If NOT present in variables_set,
> print "%S%S>\n" with STR_CMD_REMVAR (u"<STREAMCMD:REMVAR:") and key->tag —
> "<STREAMCMD:REMVAR:key>\n".
>
> If window->text is non-empty, print it verbatim and, if its last char is
> not a newline (ISNL), print '\n'.
>
> Cohorts: let cs = UI32(window->cohorts.size()). For c in 0..cs (over
> window->cohorts, NOT all_cohorts): cohort = window->cohorts[c]; compute the
> split vector via splitMwe(cohort); for each resulting cohort call
> printCohort(iter, output, profiling) (the inherited
> GrammarApplicator::printCohort). Note splitMwe appends its newly created
> cohorts to window->cohorts, but the loop bound `cs` was captured before the
> loop, so the freshly appended split cohorts are not re-iterated.
>
> If window->text_post is non-empty, print it and, if its last char is not a
> newline, print '\n'. Then print one '\n'. If window->flush_after is set,
> print "%S\n" with STR_CMD_FLUSH (u"<STREAMCMD:FLUSH>") — "<STREAMCMD:FLUSH>\n".
> Finally u_fflush(output). Returns void.

> [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.run-grammar-on-text-fn]
> void MweSplitApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.run-grammar-on-text-fn]
> One-line delegating override: calls
> GrammarApplicator::runGrammarOnText(input, output) directly, passing both
> streams through unchanged. All input reading, window/cohort/reading
> building, grammar execution, and output happen in the base implementation;
> the MWE splitting is applied only at print time because the base output
> path dispatches to this class's overridden printSingleWindow. Returns void.

> [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.split-mwe-fn]
> std::vector<Cohort*> MweSplitApplicator::splitMwe(Cohort* cohort)

> [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.split-mwe-fn]
> Splits one multi-word-expression cohort into a vector of new cohorts, one
> per component word, or returns the original cohort unchanged if it cannot
> or should not be split. A component word is encoded as a wordform tag
> (maybeWfTag) on a reading in the cohort's sub-reading chains: the head
> reading is the LAST word of the MWE and each deeper sub-reading (->next)
> is an earlier word. Returns std::vector<Cohort*>.
>
> Constants: rtrimblank = the char set {space,'\n','\r','\t'}; textprefix =
> the single char ':'.
>
> Eligibility check: iterate cohort->readings (the head readings); count
> n_goodreadings = total head readings, and n_wftags = head readings for
> which maybeWfTag(r) != nullptr. If n_wftags < n_goodreadings (not every
> head reading has a wordform tag) do NOT split: if n_wftags > 0 warn "Line
> %u: Some but not all main-readings of %S had wordform-tags (not completely
> mwe-disambiguated?), not splitting." (using cohort->line_number and
> cohort->wordform->tag); push the original cohort into the result and
> return it. (If n_wftags == 0, return unchanged with no warning.)
>
> Splitting: maintain UString `pretext` (initially empty) that carries a
> leading blank captured from one word to prefix the NEXT allocated cohort's
> text. For each head reading r in cohort->readings: set pos =
> SIZE_MAX and prev = nullptr, then walk the sub-reading chain
> (for sub = r; sub; sub = sub->next):
>   Compute wfTag = maybeWfTag(sub). If wfTag == nullptr, execute
>   prev = prev->next (this relies on the head reading always having a
>   wfTag, guaranteed by the eligibility check, so prev is non-null before
>   any such step; a head reading without a wfTag would dereference null).
>   Else (wfTag found): ++pos (first time SIZE_MAX+1 wraps to 0). Ensure a
>   cohort exists at index pos: while cos.size() < pos+1, allocate
>   c=alloc_cohort(cohort->parent), c->global_number=gWindow->cohort_counter++,
>   cohort->parent->appendCohort(c) (appends to the SAME SingleWindow), and if
>   pretext is non-empty set c->text=pretext then clear pretext; push c into
>   cos. Then c = cos[pos].
>
>   Reconstruct the trimmed wordform from wfTag->tag (which looks like
>   `"<...content...>"`): wfBeg=2 (index just after `"<`); spBeg0 =
>   wfTag->tag.find_first_not_of(rtrimblank, wfBeg) (first non-blank content
>   index); spBeg = sub->next ? spBeg0 : wfBeg (for the DEEPEST reading, i.e.
>   sub->next==null, the first/leftmost word, do not trim leading blank / do
>   not emit pretext); wfEnd = wfTag->tag.size()-3 (index of the last content
>   char, just before `>"`); spEnd = 1 + wfTag->tag.find_last_not_of(rtrimblank,
>   wfEnd) (one past the last non-blank content char). Build wf =
>   substr(0,wfBeg) [`"<`] + substr(spBeg, spEnd-spBeg) [trimmed content] +
>   substr(wfEnd+1) [`>"`].
>
>   Ambiguity guard: if c->wordform != 0 (already set from a previous head
>   reading at this pos) and wf != c->wordform->tag, warn "Line %u: Ambiguous
>   wordform-tags for same cohort, '%S' vs '%S', not splitting." (numLines,
>   wf, existing), then cos.clear(), push the original cohort, and return it.
>   Otherwise set c->wordform = addTag(wf).
>
>   Blank/text handling: if spBeg > wfBeg (there was a trimmed leading blank),
>   set pretext = textprefix + wfTag->tag.substr(wfBeg, spBeg-wfBeg) (i.e.
>   ":" + the leading blank), to be attached to the next cohort allocated. If
>   spEnd < wfEnd+1 (trimmed trailing blank), set c->text = textprefix +
>   wfTag->tag.substr(spEnd, wfEnd+1-spEnd) (":" + the trailing blank).
>
>   Reading migration: rNew = alloc_reading(*sub) — a deep copy of sub AND its
>   entire sub-reading ->next chain. Iterate rNew->tags_list and erase every
>   entry equal to wfTag->hash or to rNew->parent->wordform->hash (removing
>   both from tags_list and from the tags set) — strips the component-word
>   wordform tag and the original MWE cohort's wordform tag from the copy.
>   cos[pos]->appendReading(rNew); set rNew->parent = cos[pos]. If prev !=
>   nullptr, free_reading(prev->next) — frees the leftover sub-reading chain
>   hanging off the previously created reading, flattening each new cohort's
>   reading to a single (non-sub) reading. Set prev = rNew.
>
> After all head readings: if cos.size() == 0 warn "Line %u: Tried splitting
> %S, but got no new cohorts; shouldn't happen." and push the original
> cohort. Set cos[0]->text = cohort->text (cos[0] corresponds to the head
> reading = the LAST word, so the original cohort's trailing text moves onto
> it). Then std::reverse(cos) so the returned order runs first-word to
> last-word. Return cos.

