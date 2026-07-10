# src/GrammarApplicator_runContextualTest.cpp

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.get-cohort-in-window-fn]
> Cohort* getCohortInWindow(SingleWindow*& sWindow, size_t position, const ContextualTest* test, int32_t& pos)

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.get-cohort-in-window-fn]
> Free function. Resolves a plain positional test to a concrete cohort,
> possibly hopping one window boundary. `sWindow` and `pos` are in/out
> references. Set cohort=nullptr and `pos = position + test->offset`. First, if
> the test is absolute (POS_ABSOLUTE) AND has a left/right span
> (POS_SPAN_LEFT|POS_SPAN_RIGHT): if a previous window exists and SPAN_LEFT is
> set, move sWindow to sWindow->previous; else if a next window exists and
> SPAN_RIGHT is set, move to sWindow->next; else return nullptr (cohort left
> null). Next, if POS_ABSOLUTE: recompute pos — for a negative offset,
> `pos = sWindow->cohorts.size() + test->offset` (count from the end); for a
> non-negative offset, `pos = test->offset` (absolute index from the window
> start). Then handle single-boundary spill: if pos >= 0 and pos is past the end
> (pos >= cohorts.size()) and the test has SPAN_RIGHT or SPAN_BOTH and a next
> window exists, move sWindow to next and set pos=0; if pos < 0 and the test has
> SPAN_LEFT or SPAN_BOTH and a previous window exists, move to previous and set
> pos = previous.cohorts.size()-1. Finally, if 0 <= pos < sWindow->cohorts.size()
> set cohort = sWindow->cohorts[pos]. Return cohort (nullptr if out of range).
> Note it only ever crosses one window boundary; an offset overshooting by more
> than one window yields nullptr.

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.pos-output-helper-fn]
> bool GrammarApplicator::posOutputHelper(const SingleWindow* sWindow, size_t position, const ContextualTest* test, const Cohort* cohort, const Cohort* cdeep)

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.pos-output-helper-fn]
> Validates that a template match landed at the position the overriding test
> demands. Set good=false. Build a 4-element array cs = {cohort, cdeep, cohort,
> cdeep}; if `tmpl_cntx.min` is set overwrite cs[2] with it, if `tmpl_cntx.max`
> is set overwrite cs[3] with it. std::sort cs with compare_Cohort (less_Cohort:
> by local_number, tie-broken by parent window number), so cs[0] is the leftmost
> and cs[3] the rightmost of the entry/exit/min/max cohorts. If the test's pos
> includes any of SCANFIRST, SCANALL, or ABSOLUTE, the override includes `*`/`@`
> so offsets are irrelevant: good=true. Otherwise, for a positive offset good is
> true iff `cs[0]->local_number - position == test->offset` (leftmost must sit
> exactly offset to the right); for a negative offset good is true iff
> `cs[3]->local_number - position == test->offset` (rightmost must sit exactly
> offset to the left). Then two vetoes: if the test has no span flag
> (SPAN_BOTH|SPAN_LEFT|SPAN_RIGHT) and `cdeep->parent != sWindow`, force
> good=false (the deep result left the window). And unless POS_PASS_ORIGIN is
> set: for a negative offset if `cs[3]->local_number > position` force
> good=false, or for a positive offset if `cs[0]->local_number < position`
> force good=false (the match must not straddle/pass the origin position).
> Return good. Note comparisons mix signed (`SI32(...)`) and unsigned
> `local_number` — the veto comparisons use raw unsigned local_number.

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn]
> Cohort* GrammarApplicator::runContextualTest(SingleWindow* sWindow, size_t position, const ContextualTest* test, Cohort** deep, Cohort* origin)

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn]
> The central contextual-test dispatcher. Returns the matched cohort on success
> or nullptr on failure; when the test succeeds but has no natural cohort (e.g.
> NONE tests) it returns the window's cohort[0] as a truthy sentinel. Steps:
> if the test has POS_UNKNOWN ('?' position with no override), print an error
> and CG3Quit(1). Init cohort=nullptr, retval=true, orgSWin=sWindow.
>
> Jump handling (POS_JUMP): resolve a jump anchor `j` from test->jump_pos —
> JUMP_MARK uses get_mark(); JUMP_ATTACH uses get_attach_to().cohort; JUMP_TARGET
> walks context_stack in reverse and takes `it.target.cohort` from the deepest
> `is_with` context found (overwritten each match, no break); a positive jump_pos
> `n` indexes the parent context (context_stack[size-2].context[n-1]) if
> context_stack has more than one frame and that context has at least n entries.
> If `j` was found, set sWindow=j->parent and position=j->local_number; else set
> retval=false. Compute `pos = position + test->offset`. If retval is already
> false (jump target missing), skip straight to the finalize label.
>
> Choose the primary lookup: if test->tmpl is set, call runContextualTest_tmpl
> with test->tmpl (capturing cdeep, forwarding to *deep). Else if test->ors is
> non-empty, iterate the OR-alternative templates: before each, clear
> dep_deep_seen, call runContextualTest_tmpl with that alternative, and break on
> the first that returns a cohort; forward cdeep to *deep. Else (a plain
> positional test) call getCohortInWindow(sWindow, position, test, pos).
>
> If no cohort was found, retval=false. If the lookup was tmpl/ors, do nothing
> more. Otherwise (a concrete positional cohort): if POS_PASS_ORIGIN, reset
> origin = sWindow->cohorts[0]; if deep, *deep = cohort; and if inside a template
> (tmpl_cntx.in_template) extend tmpl_cntx.min/max to include this cohort (and
> *deep) using a 64-bit key make_64(parent->number, local_number) with min taking
> the smaller and max the larger. Then pick an evaluation mode by position flags:
> - POS_DEP_PARENT && POS_DEP_GLOB -> a DepAncestorIter (depAncestorIters pool,
>   counter ci_depths[5]).
> - POS_DEP_PARENT -> a DepParentIter (depParentIters, ci_depths[3]).
> - POS_DEP_GLOB -> a DepDescendentIter (depDescendentIters, ci_depths[4]).
> - POS_DEP_CHILD|POS_DEP_SIBLING -> call runDependencyTest(sWindow, cohort,
>   test, deep, origin, 0); on a returned cohort set cohort=that, retval=true,
>   sWindow=cohort->parent, else retval=false; if POS_NONE, negate retval. (no
>   iterator)
> - POS_LEFT_PAR|POS_RIGHT_PAR -> runParenthesisTest; set cohort/retval from it.
> - POS_RELATION -> runRelationTest; set cohort/retval; if POS_NONE negate retval.
> - POS_BAG_OF_TAGS -> match = doesSetMatchReading(sWindow->bag_of_tags,
>   test->target, true); if no match and a span flag is set, walk previous
>   windows leftward and/or next windows rightward testing each window's
>   bag_of_tags until one matches; if POS_NOT negate match; on match, if
>   test->linked, recurse runContextualTest on the linked test, else retval=false.
> - else if test->offset == 0 and (SCANFIRST|SCANALL): a symmetric bidirectional
>   scan. Start right=left=sWindow, rpos=lpos=pos. If POS_SELF, first
>   runSingleTest the current cohort (and, if it failed only via the default
>   break, clear that break); if that yields a break with retval, finish. Then
>   for i=1,2,...: while `left` is live, runSingleTest(left, lpos-i, ...) — on
>   break+retval finish; on break stop scanning left (and if POS_NOT also stop
>   right); when lpos-i reaches 0, hop to the previous window if SPAN_LEFT/
>   SPAN_BOTH or always_span (updating lpos to i+prev.size()), else stop left.
>   Symmetrically, while `right` is live, runSingleTest(right, rpos+i, ...) — on
>   break+retval finish; on break stop right (POS_NOT stops left); when rpos+i
>   reaches right.size()-1, hop to the next window if SPAN_RIGHT/SPAN_BOTH or
>   always_span (rpos = (0-i)-1), else stop right. Loop while either side is live.
> - else if offset < 0 -> a TopologyLeftIter (topologyLeftIters, ci_depths[1]).
> - else if offset > 0 -> a TopologyRightIter (topologyRightIters, ci_depths[2]).
> - else -> a plain CohortIterator (cohortIterators, ci_depths[0]).
>
> If an iterator was selected, it->reset(cohort, test, always_span); nc=nullptr,
> rvs=0, seen=0. If POS_SELF and (no MASK_POS_LORR flag, or DEP_PARENT without
> DEP_GLOB): ++seen; assert position is inside orgSWin; runSingleTest the self
> cohort orgSWin->cohorts[position] (clearing a pure default break on failure).
> Unless rvs already broke, iterate the CohortIterator from `cohort`: `current`
> starts at cohort; for each `**it` until the iterator equals the null sentinel,
> ++seen, then: if POS_LEFT and less_Cohort(current, **it) (moved rightward past
> current) set nc=null,retval=false and break; if POS_RIGHT and !less_Cohort
> (current, **it) same break; runSingleTest(**it, ...); if POS_ALL and !retval
> set nc=null and break; if POS_NONE and retval set nc=null and break; if
> rvs&TRV_BREAK break; advance current=**it. After the loop: if seen==0,
> retval=false; if !retval and POS_NONE, set retval=true and nc=cohort; set
> cohort=nc.
>
> Finalize (label_gotACohort): if cohort is null, retval=false. If cohort is null
> and POS_NOT and there is no linked continuation, negate retval. If POS_NEGATE,
> negate retval. If !retval, cohort=nullptr; else if cohort is null (truthy but
> no cohort) set cohort = sWindow->cohorts[0]. Return cohort. (The commented-out
> profiler block is inert.)

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
> Cohort* GrammarApplicator::runContextualTest_tmpl(SingleWindow* sWindow, size_t position, const ContextualTest* test, ContextualTest* tmpl, Cohort*& cdeep, C...

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn]
> Runs one template (`tmpl`) on behalf of the outer `test`, optionally imposing
> the outer test's position onto the template, then validating the result.
> Snapshot `tmpl_cntx.min`, `.max`, and `.in_template` into locals; set
> `tmpl_cntx.in_template = true`; if the outer test has a `linked` continuation,
> push it onto `tmpl_cntx.linked`. Snapshot the template's own pos/offset/
> cbarrier/barrier. If the outer test has POS_TMPL_OVERRIDE: copy the outer
> test's pos into tmpl->pos but clear POS_NEGATE|POS_NOT|POS_JUMP from it; set
> tmpl->offset = test->offset; if test->offset != 0 and the outer test has none
> of (SCANFIRST|SCANALL|ABSOLUTE), OR POS_SCANALL into tmpl->pos (a non-zero
> offset with no scan flag becomes a scan-all); if the outer test has a
> cbarrier/barrier, copy each into the template. Call `cohort =
> runContextualTest(sWindow, position, tmpl, &cdeep, origin)` (recursion), where
> `cdeep` is the out-parameter for the deepest reached cohort. If
> POS_TMPL_OVERRIDE was applied: restore the template's saved pos/offset/
> cbarrier/barrier, and if a cohort was found, cdeep is set, test->offset != 0,
> and posOutputHelper(sWindow, position, test, cohort, cdeep) returns false,
> reject the match by setting cohort=nullptr. If the outer test had a linked
> continuation, pop tmpl_cntx.linked. If no cohort resulted (failure), roll back
> `tmpl_cntx.min/max/in_template` to the snapshots. Return cohort.

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
> Cohort* GrammarApplicator::runDependencyTest(SingleWindow* sWindow, Cohort* current, const ContextualTest* test, Cohort** deep, Cohort* origin, const Cohort*...

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-dependency-test-fn]
> Traverses dependency children/parents/siblings from `current`, testing each,
> optionally recursing (deep). `self` is the recursion origin: if `self` is
> passed and equals `current`, return 0 immediately (don't retest the origin);
> if `self` is null, set self=current. Loop guard for POS_DEP_DEEP: if
> dep_deep_seen already contains the pair {test->hash, current->global_number}
> return 0, otherwise insert it. If POS_SELF and no MASK_POS_LORR flag, first
> runSingleTest(current, ...): if it matches (retval), return that cohort; if its
> rvs has TRV_BARRIER, return 0.
>
> Pick the `deps` set of global numbers to walk: if POS_DEP_CHILD, deps =
> &current->dep_children. Otherwise (parent/sibling): if current->dep_parent==0
> deps = the root cohort's dep_children (current->parent->cohorts[0]->
> dep_children); else look up current->dep_parent in the window-group cohort_map,
> and if found with non-empty dep_children use those, else (verbose) warn "did
> not have any siblings" and return 0. If any of MASK_POS_LORR (l/r/ll/rr) is
> set, rebuild deps into a local sorted `tmp_deps` by scanning the whole
> cohort_map: for each mapped cohort whose global_number is in `deps`, include it
> if POS_LEFT and less_Cohort(it, current), or POS_RIGHT and less_Cohort(current,
> it), or (neither L nor R) unconditionally; if POS_SELF also insert
> current->global_number; if POS_RIGHTMOST reverse tmp_deps; then deps=&tmp_deps.
>
> Iterate each global number `dter` in *deps: skip it if it equals
> current->global_number and POS_SELF is not set. If `dter` is not in the
> cohort_map, (verbose) warn that the child/sibling dependency does not exist and
> continue. Fetch the cohort; skip it if CT_REMOVED. Compute `good`: if the
> cohort is in a different window than current, it's only good if the crossing
> direction is allowed — cohort in an earlier window requires SPAN_BOTH|SPAN_LEFT,
> in a later window requires SPAN_BOTH|SPAN_RIGHT, else good=false. If good,
> runSingleTest(cohort, ...) to get retval. Then: if POS_ALL, a failure sets
> rv=nullptr and breaks, a success sets rv=cohort and continues (all must pass);
> else if retval, set rv=cohort and break; else if rvs has TRV_BARRIER, continue;
> else if POS_DEP_DEEP, recurse runDependencyTest(cohort->parent, cohort, test,
> deep, origin, self) and if it returns a cohort set rv to it and break. Return
> rv (nullptr if nothing matched).

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-parenthesis-test-fn]
> Cohort* GrammarApplicator::runParenthesisTest(SingleWindow* sWindow, const Cohort* current, const ContextualTest* test, Cohort** deep, Cohort* origin)

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-parenthesis-test-fn]
> Tests one edge of the currently-unwrapped enclosure (parentheses). If
> `current->local_number` is outside the active enclosure range [par_left_pos,
> par_right_pos], return 0. Otherwise select the edge cohort: for POS_LEFT_PAR
> use sWindow->cohorts[par_left_pos], else use sWindow->cohorts[par_right_pos].
> Run runSingleTest on that edge cohort (retval/rvs locals). If it matched
> (retval), return that cohort; otherwise return nullptr. `par_left_pos`/
> `par_right_pos` are set by runGrammarOnWindow while unpacking enclosures.

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-relation-test-fn]
> Cohort* GrammarApplicator::runRelationTest(SingleWindow* sWindow, Cohort* current, const ContextualTest* test, Cohort** deep, Cohort* origin)

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-relation-test-fn]
> Follows named relations (`r:name`) from `current` to related cohorts, testing
> each. If `current` is not CT_RELATED or has no relations, return 0. Save
> `regexgrpz = context_stack.back().regexgrp_ct`. Resolve the relation tag:
> rtag = grammar->single_tags[test->relation]; while it is a T_VARSTRING, expand
> it via generateVarstringTag. Collect the target cohorts into a sorted CohortSet
> `rels`, three ways: (a) if rtag->hash == grammar->tag_any, for every relation
> the cohort has, map each stored target global_number through the window-group
> cohort_map and insert the found cohorts; (b) if rtag is T_REGEXP, get its
> capture count via uregex_groupCount, and for every relation whose NAME tag
> matches via doesTagMatchRegexp(relation-name-hash, *rtag, caps!=0) (UNANCHORED
> ICU find on the relation name), insert its mapped targets and set regexgrp_ct =
> min(current regexgrp_ct, regexgrpz + caps); (c) otherwise look up rtag->hash
> exactly in current->relations and insert its mapped targets. Then order/filter
> `rels`: POS_LEFT keeps only members strictly before current (begin ..
> lower_bound(current)); POS_RIGHT keeps members at/after current (lower_bound ..
> end); POS_SELF inserts current; POS_LEFTMOST collapses to just the first
> member; POS_RIGHTMOST collapses to just the last. Iterate `rels` in order:
> runSingleTest(iter, ...); if POS_ALL, a failure sets rv=nullptr and breaks
> while a success sets rv=iter and continues; otherwise the first success sets
> rv=iter and breaks. If rv is null at the end, restore
> context_stack.back().regexgrp_ct = regexgrpz. Return rv.

> [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-single-test-fn]
> Cohort* GrammarApplicator::runSingleTest(Cohort* cohort, const ContextualTest* test, uint8_t& rvs, bool* retval, Cohort** deep, Cohort* origin)

> [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-single-test-fn]
> The atomic "does this one cohort match the test" step; also computes barrier
> and scan-break signals. Save `regexgrpz = context_stack.back().regexgrp_ct`.
> Side effects driven by position flags: if POS_MARK_SET, set_mark(cohort); if
> POS_ATTACH_TO and the current attach target (get_attach_to().cohort) is not
> this cohort, clear `matched_target` and `matched_tests` on every reading in
> cohort->readings and, conditionally, in cohort->deleted (POS_LOOK_DELETED),
> cohort->delayed (POS_LOOK_DELAYED), and cohort->ignored (POS_LOOK_IGNORED); if
> POS_WITH, set `merge_with = cohort`; if `deep` non-null, `*deep = cohort`.
> Build `dSMC_Context context = { test, deep, origin, test->pos }`. Target
> matching: if POS_CAREFUL, `*retval = doesSetMatchCohortCareful(cohort,
> test->target, &context)` and, when the target didn't match yet
> (`!context.matched_target`) and POS_SCANFIRST is set, set context.did_test=true
> and call doesSetMatchCohortNormal once more solely to populate
> context.matched_target (its return value is discarded); otherwise `*retval =
> doesSetMatchCohortNormal(cohort, test->target, &context)`.
>
> Then compute the traversal flags `rvs` (a bitmask of TRV_BREAK,
> TRV_BREAK_DEFAULT, TRV_BARRIER): if `origin` is set and (offset != 0 or
> SCANALL/SCANFIRST) and origin == cohort and origin->local_number != 0, the
> scan has looped back to its origin — set cohort=nullptr and rvs|=TRV_BREAK. If
> context.matched_target and POS_SCANFIRST, rvs|=TRV_BREAK; else if the test has
> none of (SCANALL|SCANFIRST|DEP_DEEP|DEP_GLOB), rvs |= TRV_BREAK|TRV_BREAK_DEFAULT
> (a single-position test always breaks after one cohort). Remember `broken =
> rvs&TRV_BREAK`. Reset context's test/deep/origin to null and did_test=true.
> Barriers (only if cohort is still non-null): if test->barrier, run
> doesSetMatchCohortNormal(cohort, test->barrier, ...) with a fresh context whose
> options are `test->pos & ~POS_CAREFUL` and in_barrier=true; if it matches set
> seen_barrier=true, rvs|=TRV_BREAK|TRV_BARRIER, and clear TRV_BREAK_DEFAULT.
> Likewise if test->cbarrier, run doesSetMatchCohortCareful(cohort,
> test->cbarrier, ...) with options `test->pos | POS_CAREFUL`, same effect on
> match. If context.matched_target and *retval, rvs|=TRV_BREAK. If NOT broken and
> (rvs&TRV_BARRIER) and `(test->pos & MASK_SELF_NB) == MASK_SELF_NB` (both POS_SELF
> and POS_NO_BARRIER set), clear TRV_BREAK and TRV_BARRIER (the `N` no-barrier
> self test ignores the barrier). Finally, if the test failed (`!*retval`),
> restore `context_stack.back().regexgrp_ct = regexgrpz` to discard any regex
> capture-group count accumulated during the failed match. Return cohort (which
> the caller reads together with the by-reference `rvs`/`*retval`). There is also
> a sibling overload taking (sWindow, index i): if i is out of range it sets
> rvs|=TRV_BREAK, *retval=false and returns 0, else it forwards
> sWindow->cohorts[i] to this function.

