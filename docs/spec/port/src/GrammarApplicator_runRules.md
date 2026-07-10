# src/GrammarApplicator_runRules.cpp

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.does-wordforms-match-fn]
> bool GrammarApplicator::doesWordformsMatch(const Tag* cword, const Tag* rword)

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.does-wordforms-match-fn]
> Decides whether a rule's wordform restriction `rword` admits a cohort's
> wordform `cword`. If `rword` is non-null and not pointer-identical to `cword`:
> if `rword` is a T_REGEXP tag, return false unless doesTagMatchRegexp(cword->
> hash, *rword) matches (UNANCHORED ICU find of the rule's regex against the
> cohort's wordform string); else if `rword` is T_CASE_INSENSITIVE, return false
> unless doesTagMatchIcase(cword->hash, *rword) matches (case-insensitive string
> equality); else (a plain, non-equal wordform) return false. In all other cases
> — `rword` null (no restriction), identical pointers, or a successful regex/
> icase match — return true.

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.get-sub-reading-fn]
> Reading* GrammarApplicator::get_sub_reading(Reading* tr, int sub_reading)

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.get-sub-reading-fn]
> Selects a sub-reading from a reading chain (readings are singly linked via
> `->next`, each `next` being a deeper sub-reading). If `sub_reading == 0`,
> return `tr` unchanged (the primary reading). If `sub_reading == GSR_ANY`
> (32767): if `tr->next` is null there are no sub-readings so return `tr`;
> otherwise build and return an amalgamated reading — append a fresh Reading to
> the scratch list `subs_any`, copy `*tr` into it, null its `next`, then walk the
> `tr->next` chain and for each deeper reading append a 0 separator to
> `tags_list` followed by that reading's tags_list, union its tags/tags_plain/
> tags_textual (with their bloom filters) and tags_numerical, and OR in its
> mapped/mapping/matched_target/matched_tests; finally rehash() and return the
> amalgam. If `sub_reading > 0`, step `->next` that many times (stopping early at
> null) and return the result (nullptr if the chain is shorter). If
> `sub_reading < 0`, first compute `ntr` as the negative of the chain length
> (loop advancing a copy to the end, decrementing ntr per link); if `tr->next`
> is null set tr=nullptr; then advance `tr` from index `ntr` up toward
> `sub_reading` (i.e. count from the deepest end, where -1 is the last
> sub-reading) and return it. Returns nullptr when the requested depth doesn't
> exist.

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
> void GrammarApplicator::getTagList(const Set& theSet, TagList& theTags, bool unif_mode) const

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
> Flattens a Set into an ordered TagList `theTags` (appending, not clearing), 
> honoring unification. This is the by-reference overload; a sibling overload
> `getTagList(theSet, unif_mode)` simply constructs a local TagList, calls this,
> and returns it. Logic by set type: if the set is ST_SET_UNIFY, fetch the
> recorded unified subsets for this set number from the current context
> (`(*context_stack.back().unif_sets)[theSet.number]`), take the parent set
> `grammar->sets_list[theSet.sets[0]]`, and for each of its member sets that is
> present in the unified-subset record, recurse getTagList on it. If ST_TAG_UNIFY,
> recurse over every member set with unif_mode forced true. Else if the set has
> member sets, recurse over each with the incoming `unif_mode`. Else if
> `unif_mode` is true, look up this set's number in the context's `unif_tags`;
> if a unified value is recorded, emit tags from `theSet.trie` and
> `theSet.trie_special` filtered by that value (trie_getTagList with the value).
> Else (leaf, no unification), emit all tags from `theSet.trie` and
> `theSet.trie_special`. After collecting, remove only CONSECUTIVE duplicate tag
> pointers (walk the list; for each element, erase immediately-following elements
> that are exactly one step away and equal) — non-adjacent duplicates are kept
> deliberately, because AddCohort/Append may legitimately repeat tags across
> readings.

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.index-single-window-fn]
> void GrammarApplicator::indexSingleWindow(SingleWindow& current)

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.index-single-window-fn]
> (Re)builds the window's rule-to-cohort index from scratch. Clear
> `current.valid_rules`. Resize `current.rule_to_cohorts` to
> `grammar->rule_by_number.size()` and clear each per-rule CohortSet. Then for
> every cohort `c` in `current.cohorts`, iterate the set-bit positions `psit` of
> its `possible_sets` bitset: for each bit that is set, look up
> `grammar->rules_by_set[psit]` (the rules referencing that set); if present,
> call updateRuleToCohorts(*c, rsit) for each rule number `rsit` in that list.
> Net effect: after this call, `rule_to_cohorts[r]` contains exactly the cohorts
> whose possible-set membership (and wordform, per updateRuleToCohorts) make them
> candidates for rule r, and `current.valid_rules` lists the rules with at least
> one candidate cohort in this window.

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn]
> uint32_t GrammarApplicator::runRulesOnSingleWindow(SingleWindow& current, const uint32IntervalVector& rules)

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn]
> Applies a set of rules to one window, driving runSingleRule per rule and
> supplying the per-rule-type action callbacks. Returns a bitmask over
> {RV_NOTHING=1, RV_SOMETHING=2, RV_DELIMITED=4, RV_TRACERULE=8}. Init
> retval=RV_NOTHING, section_did_something=false, delimited=false. Compute
> `intersects = current.valid_rules.intersect(rules)` (the rules applicable to
> this window from `rules`, in ascending order). Register the boundary cohort:
> current.parent->cohort_map[0] = current.cohorts.front().
>
> Iterate `iter_rules` over `intersects` (a live cursor the callbacks may
> re-seat). Each iteration builds a `Sorter` RAII guard that, if its do_sort flag
> is set, re-sorts every rule_to_cohorts CohortSet when the rule finishes.
> `repeat_rule:` rule_did_something=false; j = *iter_rules. Skip (continue) when
> the cmdline `valid_rules` filter is non-empty and lacks j. Set current_rule =
> rule = grammar->rule_by_number[j]. Skip K_IGNORE; skip MAP/ADD/REPLACE when
> !apply_mappings; skip SUBSTITUTE/APPEND when !apply_corrections. When the window
> has enclosures, skip non-final rules until the final enclosure pass and skip
> RF_ENCL_FINAL rules before it (gated on did_final_enclosure). Reset
> readings_changed/should_repeat/should_bail. Define the action lambdas (below),
> clear `removed` and `selected`, then call rv = runSingleRule(current, *rule,
> reading_cb, cohort_cb).
>
> After the call: if rv or readings_changed, then (unless RF_NOITERATE with
> section_max_count!=1) set section_did_something and always set rule_did_something.
> If should_bail, goto bailout (zero rule_hits[rule] and clear
> index_ruleCohort_no). If should_repeat, goto repeat_rule. If rule_did_something,
> re-seat iter_rules to intersects.find(rule->number)/end and, if trace_rules
> contains rule->line, OR in RV_TRACERULE. If `delimited`, break the rule loop. If
> rule_did_something and RF_REPEAT, clear index_ruleCohort_no and goto repeat_rule.
> If retval has RV_TRACERULE, break. After the loop: if section_did_something OR
> in RV_SOMETHING; if delimited OR in RV_DELIMITED; return retval.
>
> The callbacks and helper lambdas (closures over current/rule/intersects/etc.):
> - reindex(which=current): renumber a window's cohorts' local_number to their
>   index and gWindow->rebuildCohortLinks().
> - collect_subtree(cs, head, cset): when cset!=0, gather `head` plus every
>   cohort whose dep_parent is head and that matches set cset, then transitively
>   all descendants of those (isChildOf), excluding re-descending from head; when
>   cset==0 just insert head.
> - add_cohort(cohort, spacesInAddedWf, withs): materialize a new cohort from
>   rule->maplist (varstring-expanded) — the first T_WORDFORM tag becomes its
>   wordform, subsequent T_BASEFORM tags start new readings, a `*` tag expands to
>   the target cohort's own tags; build the readings (splitting mapping tags,
>   updateValidRules per added tag), register it in cohort_map/dep_window, attach
>   dependencies/relations for ADDCOHORT (attachParentChild when addcohort_attach)
>   or MERGECOHORTS, init an empty cohort if it ended up reading-less, then insert
>   it into current.cohorts/all_cohorts before/after the collected subtree of
>   `cohort` (per ADDCOHORT_BEFORE/AFTER), renumber and rebuild links; returns the
>   new cohort.
> - rem_cohort(cohort): mark all its readings deleted + hit_by, erase it from
>   every rule_to_cohorts set, forward its dep_children up to its parent (or
>   root), flag CT_REMOVED, detach, erase from cohort_map, remove from
>   current.cohorts and renumber; if that empties the window (and it isn't the
>   current window), splice its text/all_cohorts into the neighbour window and
>   free the emptied window; finally rebuildCohortLinks.
> - ignore_cohort(cohort): mark readings hit_by, erase from rule_to_cohorts, flag
>   CT_IGNORED, detach, erase from cohort_map, remove from current.cohorts.
> - make/add/set/rem_relation_rtag: build/maintain the textual `R:name:id` tags
>   that mirror named relations on a cohort's readings.
> - insert_taglist_to_reading(iter, taglist, reading, mappings): insert
>   varstring-expanded tags at a position (stop at `*`), routing mapping tags to
>   `mappings`, calling updateValidRules per tag, then reflowReading.
>
> cohort_cb (runs once per matched cohort) dispatches on rule->type:
> SELECT (and IFF once a match occurred) keeps only the `selected` readings,
> moving the rest into deleted/delayed/ignored and tracing each; REMOVE/IFF moves
> the `removed` readings into deleted/delayed/ignored (guarding the safe-remove
> conditions) and re-inits an empty cohort if all readings were removed; JUMP
> looks up the anchor named by maplist and re-seats iter_rules there
> (finish_cohort_loop=false, should_repeat=true); REMVARIABLE/SETVARIABLE mutate
> the global `variables` map (regex/icase name matching for removal), optionally
> flagging variables_output; DELIMIT calls delimitAt at the cohort and sets
> delimited; EXTERNAL_ONCE/ALWAYS pipes the window through an external process and
> re-indexes; REMCOHORT removes (or, RF_IGNORED, ignores) the cohort subtree,
> re-adds `<<<` to the new last cohort, and requests a cohort-loop reset.
>
> reading_cb (runs per matched reading) dispatches on rule->type: SELECT pushes
> the reading to `selected`; REMOVE/IFF push to `removed` (or, RF_UNMAPLAST on the
> last reading, unmap it); PROTECT/UNPROTECT toggle immutable; UNMAP unmaps;
> ADD/MAP append/insert maplist tags (childset1 chooses an insertion spot, split
> mappings, set mapped for MAP); RESTORE moves matching readings out of deleted/
> delayed/ignored back into readings; REPLACE clears the reading to wordform+maplist
> (preserving `sublist` excepts and baseform); SUBSTITUTE removes the sublist tags
> (regex/icase-expanded) and inserts maplist tags at the removal spot, handling a
> changed wordform; APPEND adds new readings from maplist; COPY clones the reading
> then applies sublist/childset like ADD; ADDCOHORT_BEFORE/AFTER and SPLITCOHORT
> and MERGECOHORTS and COPYCOHORT build/split/merge cohorts (indexSingleWindow +
> reset afterward); SETPARENT/SETCHILD/ADD|SET|REMRELATION(S) attach dependencies
> or add/set/remove named relations, iterating the dep_target scan with barrier/
> RF_NEAREST/loop-avoidance handling; REMPARENT/SWITCHPARENT rewire dependencies;
> MOVE_AFTER/MOVE_BEFORE/SWITCH relocate/swap cohort subtrees within the window
> (with an endless-loop bailout when a move keeps changing state); WITH runs its
> sub-rules in a nested pass (in_nested) repeating per RF_REPEAT; any other type
> just traces. Each mutating action typically clears index_ruleCohort_no, sets
> readings_changed, calls updateValidRules for new tags (re-seating iter_rules on
> growth), and may set reset_cohorts_for_loop to restart cohort iteration.

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
> bool GrammarApplicator::runSingleRule(SingleWindow& current, const Rule& rule, RuleCallback reading_cb, RuleCallback cohort_cb)

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn]
> Iterates one rule over its candidate cohorts in the window, decides which
> readings are valid targets (matching both the target set and all contextual
> tests), and invokes the caller-supplied `reading_cb`/`cohort_cb` to perform the
> action. Returns whether anything changed. It does NOT itself mutate readings
> beyond match bookkeeping; the callbacks (defined in runRulesOnSingleWindow) do.
>
> Setup: `finish_cohort_loop = true`; anything_changed=false; type=rule.type;
> set = grammar->sets_list[rule.target]; cohortset = &current.rule_to_cohorts
> [rule.number]. `override_cohortset` lambda: when `in_nested` (running inside a
> WITH), replace cohortset with current.nested_rule_to_cohorts holding just the
> WITH apply-to cohort plus, for each special trie tag that is T_CONTEXT with
> context_ref_pos <= the context size, the referenced context cohort. Push
> cohortset onto `cohortsets` and a null onto `rocits`, with a scope_guard that
> pops both on return.
>
> Cohort loop `for rocit=0; rocit < cohortset->size();`: set rocits.back()=&rocit,
> cohort = cohortset->at(rocit), then ++rocit; finish_reading_loop=true. Skip
> (continue) the cohort when: local_number==0 (the `>>>` boundary); type is
> CT_REMOVED or CT_IGNORED; CT_ENCLOSED or cohort->parent != &current; readings
> empty; for K_RESTORE when the relevant source list (delayed for RF_DELAYED,
> ignored for RF_IGNORED, else deleted) is empty; when rule.sub_reading==0 and
> the target set index is out of range or not set in possible_sets; when
> readings.size()==1 and the rule can't affect a lone reading (K_SELECT always;
> K_REMOVE/K_IFF when the sole reading is noprint or the remove is "safe"; K_UNMAP
> with RF_SAFE); K_DELIMIT at the final cohort; RF_ENCL_INNER when there is no
> active enclosure or the cohort is outside [par_left_pos,par_right_pos];
> RF_ENCL_OUTER when the cohort is inside the enclosure; K_SETPARENT|RF_SAFE or
> RF_NOPARENT when a parent already exists; K_REMPARENT/K_SWITCHPARENT when no
> parent exists. Then the no-match cache: ih = hash_value(rule.number,
> cohort->global_number); if index_ruleCohort_no contains ih, continue; else
> insert ih (the cohort is assumed non-matching until proven otherwise; this
> cache is cleared whenever a rule changes window state).
>
> Per-cohort scratch reset: counters num_active=num_iff=num_immutable=0; a
> reading_contexts vector; if rule.type==K_IFF set the working `type`=K_REMOVE
> (Iff is treated as Remove until a match promotes it to Select). Clear
> readings_plain, subs_any, regexgrps_z/regexgrps_c, unif_tags_rs/unif_sets_rs;
> used_regex=0; grow regexgrps_store/unif_*_store to fit the readings; used_unif=0.
> Push a fresh Rule_Context (target.cohort=cohort, is_with = rule.type==K_WITH)
> onto context_stack. Snapshot the four list sizes (readings, deleted, delayed,
> ignored) for change detection.
>
> Reading-match loop over cohort->readings[i]: reading = get_sub_reading
> (readings[i], rule.sub_reading); if null, clear that reading's matched flags and
> continue. Set the context target reading/subreading; clear the subreading's
> matched_target/matched_tests. Skip the reading when: it is mapped and the rule
> is MAP/ADD/REPLACE; mapped and RF_NOMAPPED; noprint and !allow_magic_readings.
> If the reading is immutable and the rule isn't K_UNPROTECT: for the
> protect/add/map/replace/select/remove/iff/substitute/unmap rules ++num_active,
> and for K_SELECT also mark it matched and push its context; ++num_iff,
> ++num_immutable; continue. Plain-signature cache: if the set is not
> ST_SPECIAL/ST_MAPPING/ST_CHILD_UNIFY and a previously-seen reading in
> readings_plain has the same hash_plain, copy its matched_target/matched_tests
> (++num_active if it matched tests), copy its regex-group and unif pointers into
> the context, mark did_test, push the context, and continue. Otherwise establish
> fresh per-reading regex and unification state (regexgrp_ct=0, fresh regexgrps/
> unif_tags/unif_sets slots registered in unif_*_rs under both hash_plain and
> hash; clear unif_last_wordform/baseform/textual; same_basic=hash_plain), set the
> MARK to the parent WITH's mark if any else the cohort, mark the whole reading
> chain active, and test target membership: if rule.target is set and
> doesSetMatchReading(*reading, rule.target, special-flag) matches, set
> matched_target on the reading (and the local matched_target). Then, unless a
> cached did_test says otherwise, run the rule's contextual tests in order: for
> each test, reset the MARK to the cohort unless RF_REMEMBERX (RF_RESETX forces
> reset), clear seen_barrier/dep_deep_seen/ci_depths/tmpl_cntx, run
> runContextualTest(&current, cohort_local_number, test, deep[, origin=cohort])
> where `deep` is used and merge_with reset only for K_WITH and origin=cohort is
> passed when the test must not pass origin (no_pass_origin or POS_NO_PASS_ORIGIN,
> and not POS_PASS_ORIGIN); push the resulting cohort (or merge_with) into the
> context's context vector; if the test fails, mark not-good and — unless this is
> the first test or RF_KEEPORDER — move the failing test to the front of
> rule.tests (a self-optimizing reorder) and break. If all tests pass and the rule
> is K_IFF, promote the working `type` to K_SELECT (and if the grammar has any
> Protect rules, also mark earlier immutable readings as matched); mark the
> subreading matched_tests, ++num_active, and optionally propagate captured regex
> groups from an earlier reading. Record the reading in readings_plain, clear the
> chain's active flags, copy matched flags back to readings[i] if a sub-reading
> was used, persist any regex groups, and push the context onto reading_contexts.
>
> After the reading loop: if any of the four snapshotted list sizes changed, set
> anything_changed and clear CT_NUM_CURRENT on the cohort. If no reading was
> active (num_active==0 and not an IFF-with-iff-hits case): when nothing even
> matched the target (num_immutable==0 and !matched_target) step rocit back and
> erase this cohort from the rule's cohortset (erase_n) so the rule won't revisit
> it; pop the context and continue. If every reading matched (num_active ==
> readings.size()): for K_SELECT there is nothing to remove (pop, continue); for a
> safe K_REMOVE likewise (pop, continue). Otherwise run the actions: for each
> saved ctx in reading_contexts whose subreading matched_target (and matched_tests
> unless K_IFF), set it as the current context, reset reset_cohorts_for_loop, and
> call reading_cb(); if reading_cb cleared finish_cohort_loop, pop context and
> return anything_changed; if it set reset_cohorts_for_loop, call the
> reset_cohorts helper (re-fetch the cohortset and reposition rocit just past the
> apply-to cohort) and break; if it cleared finish_reading_loop, break. Then call
> cohort_cb() once (same finish_cohort_loop/reset_cohorts handling). Pop the
> context and move to the next cohort. Return anything_changed.

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.update-rule-to-cohorts-fn]
> bool GrammarApplicator::updateRuleToCohorts(Cohort& c, const uint32_t& rsit)

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.update-rule-to-cohorts-fn]
> Registers cohort `c` as a candidate for rule number `rsit` in its window,
> keeping any live rule iterators valid across the insert. If the cmdline rule
> filter `valid_rules` is non-empty and does not contain `rsit`, return false. Let
> current = c.parent and r = grammar->rule_by_number[rsit]. If
> doesWordformsMatch(c.wordform, r->wordform) is false, return false (the rule's
> wordform restriction excludes this cohort). If current->rule_to_cohorts is
> shorter than rsit+1, call indexSingleWindow(*current) to size it. Take
> cohortset = current->rule_to_cohorts[rsit]. Scan the global `cohortsets` stack
> (the CohortSets currently being iterated by active runSingleRule frames) for
> indices `csi` whose entry points at this same cohortset. If any exist: record
> the current capacity; for each such active iterator, if its position
> (*rocits[csi]) is at/after cohortset.size() note it as an "end" iterator, else
> remember the pair (its position pointer, the cohort currently at that
> position). Insert &c into the sorted cohortset. Set every "end" iterator to the
> new cohortset.size() (keep them parked at the end). If the capacity changed (a
> reallocation happened), recompute each remembered iterator's position with
> find_n on its remembered cohort so ongoing iteration stays consistent. If no
> active iterator referenced this cohortset, just insert &c. Finally return
> `current->valid_rules.insert(rsit)` — true iff `rsit` was newly added to the
> window's valid-rule set.

> [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.update-valid-rules-fn]
> bool GrammarApplicator::updateValidRules(const uint32IntervalVector& rules, uint32IntervalVector& intersects, const uint32_t& hash, Reading& reading)

> [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.update-valid-rules-fn]
> After a tag `hash` has just been added to `reading`, discovers rules keyed on
> that tag and activates any that belong to the working rule set. Record os =
> intersects.size(). Look up `hash` in grammar->rules_by_tag; if found, let c =
> *reading.parent and for each rule number `rsit` associated with that tag: call
> updateRuleToCohorts(c, rsit), and if that returns true AND `rules` (the set of
> rules under consideration) contains `rsit`, insert `rsit` into `intersects`.
> Return whether `intersects` grew (os != intersects.size()); callers use a true
> return to re-derive their iterators into `intersects` since it may have
> reallocated.

> [spec:cg3:def:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-single-window-fn]
> uint32_t GrammarApplicator::runGrammarOnSingleWindow(SingleWindow& current)

> [spec:cg3:sem:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-single-window-fn]
> Runs the grammar's BEFORE-SECTIONS, numbered SECTIONS, and AFTER-SECTIONS over
> one window, returning early on delimit or trace. If the grammar has
> before_sections and !no_before_sections, run runRulesOnSingleWindow(current,
> runsections[-1]); if the result has RV_DELIMITED or RV_TRACERULE, return it.
> Then, if the grammar has rules and !no_sections, iterate the ordered
> `runsections` map (keyed by section number; negative keys are the before/after
> pseudo-sections). Maintain a per-section `counter`. With a `pass` counter
> starting at 0 and incremented each loop turn: skip entries whose key is
> negative or whose counter has reached section_max_count (advance the iterator,
> continue). Otherwise run runRulesOnSingleWindow(current, iter->second) (the
> cumulative rule set for that section), ++counter[key]; if the result has
> RV_DELIMITED or RV_TRACERULE, return it. If the result does NOT have
> RV_SOMETHING (the section made no change), advance to the next section and
> reset pass=0; otherwise stay on the same section so it re-runs (it keeps
> re-running while it changes the window). If `pass` reaches 1000, warn about an
> endless loop (dumping the window's wordforms) and break out. Finally, if the
> grammar has after_sections and !no_after_sections, run
> runRulesOnSingleWindow(current, runsections[-2]) and return it if it has
> RV_DELIMITED or RV_TRACERULE. Return 0 if none of the early-exit conditions
> fired.

> [spec:cg3:def:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-window-fn]
> void GrammarApplicator::runGrammarOnWindow()

> [spec:cg3:sem:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-window-fn]
> Prepares and repeatedly runs the grammar on `gWindow->current`, handling
> parenthesis enclosures and delimit-driven restarts. Set current =
> gWindow->current, did_final_enclosure=false. Apply the window's variable deltas
> to the global `variables` map: assign each of current->variables_set, erase
> each of current->variables_rem, then set variables[mprefix_key]=mprefix_value.
> If has_dep, reflowDependencyWindow() and, when not at input_eof and the next
> window (gWindow->next.back()) has more than one cohort, register its cohorts in
> gWindow->dep_window. If has_relations, reflowRelationWindow().
>
> Enclosure wrapping (only if grammar->parentheses is non-empty): label
> `scanParentheses` — walk current->cohorts in reverse; for a cohort whose
> is_pleft is set and whose id has a matching right-paren id in
> grammar->parentheses, scan forward collecting cohorts up to the one whose
> is_pright equals the expected id; if found, remove that enclosed span from the
> `cohorts` vector (shifting the trailing cohorts left and renumbering their
> local_number), shrink the vector, mark each enclosed cohort CT_ENCLOSED and
> bump its `enclosed` depth, set current->has_enclosures, and restart the scan.
>
> Reset par_left_tag/par_right_tag/par_left_pos/par_right_pos=0 and pass=0. Label
> `runGrammarOnWindow_begin`: while there are more than num_windows previous
> windows, print and free the oldest. Clear rule_hits and index_ruleCohort_no;
> re-fetch current=gWindow->current; indexSingleWindow(*current); clear
> hit_external; rebuildCohortLinks. ++pass; if pass>1000, warn endless loop
> (dump wordforms) and return. If trace_encl, tag every reading with a per-pass
> hit marker. Run rv = runGrammarOnSingleWindow(*current); if rv has RV_DELIMITED,
> goto runGrammarOnWindow_begin to restart from scratch.
>
> Label `unpackEnclosures`: if current->has_enclosures, find the next enclosed
> group (a cohort with enclosed==1) and splice it back into the visible `cohorts`
> vector at its original slot — locate the insertion anchor to its left, count
> the run of enclosed/removed/ignored cohorts, decrement each `enclosed` depth
> and clear CT_ENCLOSED for those reaching 0, make room in `cohorts` and reinsert
> the now-revealed cohorts (renumbering local_number and reparenting), set
> par_left_tag/par_right_tag from the enclosure's edge is_pleft/is_pright and
> par_left_pos/par_right_pos to its span, then goto runGrammarOnWindow_begin (or
> back to unpackEnclosures if rv had RV_TRACERULE). When no enclosed group
> remains and did_final_enclosure is still false, clear the par_* fields, set
> did_final_enclosure=true and goto runGrammarOnWindow_begin once more so
> RF_ENCL_FINAL rules get their final pass.
>
> Finally, restore any CT_IGNORED cohorts: scan all_cohorts in reverse and for
> each ignored cohort reinsert it into `cohorts` after the nearest
> non-removed/enclosed/ignored cohort, clear CT_IGNORED, and re-register it in
> cohort_map, flagging should_reflow. If anything was restored, renumber all
> cohorts' local_number and reflowDependencyWindow().

