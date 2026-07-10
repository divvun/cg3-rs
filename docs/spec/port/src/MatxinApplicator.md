# src/MatxinApplicator.cpp, src/MatxinApplicator.hpp

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator]
> class MatxinApplicator : public virtual GrammarApplicator {
>   bool wordform_case = false;
>   bool print_word_forms = true;
>   bool print_only_first = false;
>   struct Node { int self = 0; UString lemma; UString form; UString pos; UString mi; UString si; };
>   std::map<int, Node> nodes;
>   std::map<int, std::vector<int>> deps;
>   bool nullFlush = false;
>   bool runningWithNullFlush = false;
> }

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.get-null-flush-fn]
> bool MatxinApplicator::getNullFlush()

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.get-null-flush-fn]
> Trivial getter: returns the `nullFlush` member boolean. No side effects.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.matxin-applicator-fn]
> MatxinApplicator::MatxinApplicator(std::ostream& ux_err)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.matxin-applicator-fn]
> Constructor. Takes an error stream `ux_err` and forwards it to the base
> `GrammarApplicator(ux_err)` constructor; the body is empty. Members keep
> their in-class defaults: `wordform_case = false`, `print_word_forms = true`,
> `print_only_first = false`, empty `nodes` and `deps` maps, `nullFlush =
> false`, `runningWithNullFlush = false`. No other work is done.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.merge-mappings-fn]
> void MatxinApplicator::mergeMappings(Cohort& cohort)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.merge-mappings-fn]
> Collapses byte-for-byte-identical readings (including mapping tags) of
> `cohort`, mirroring the Apertium version but differing in how the survivor is
> produced. Build `std::map<uint32_t, ReadingList> mlist`: for each reading `r`
> in `cohort.readings`, start `hp = r->hash`; if `trace`, fold every `r->hit_by`
> value into `hp` via `hash_value`; walk the `r->next` sub-reading chain folding
> `hash_value(sub->hash, hp)` (and, if `trace`, each `sub->hit_by`); append `r`
> to `mlist[hp]`. If the key count equals `cohort.readings.size()` (all unique),
> return unchanged. Otherwise clear `cohort.readings`; for each hash group,
> allocate a COPY of the group's first reading via `alloc_reading(*(clist.
> front()))` and push it into `order` (no mapping-tag merging is done). Sort
> `order` by `Reading::cmp_number` and insert at the start of `cohort.readings`.
> Note: unlike the Apertium `mergeMappings`, this does NOT `free_reading` the
> original readings — it allocates fresh copies and drops the originals from the
> vector, so the pooled originals are effectively orphaned.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.node]
> struct Node {
>   int self = 0;
>   UString lemma;
>   UString form;
>   UString pos;
>   UString mi;
>   UString si;
> }

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.print-reading-fn]
> void MatxinApplicator::printReading(Reading* reading, Node& node, std::ostream& output)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.print-reading-fn]
> Fills a Matxin `Node` from one reading's lemma and tags (and writes any `+`
> multiword tags directly to `output`). Does NOT emit `<NODE>` markup itself —
> that is done later by `procNode`.
>
> If `reading->noprint`, return. If `reading->next` is set, Matxin cannot
> represent sub-readings: print `"Error: input contains sub-readings!"` to
> stderr, write `  </SENTENCE>\n` then `</corpus>\n` to `output`, and `exit(-1)`
> (hard process exit). If `!reading->baseform`, return.
>
> Otherwise build UnicodeString `bf` from the baseform tag with the surrounding
> `"` quotes stripped (`data()+1`, length `size-2`) and set `node.lemma =
> bf.getTerminatedBuffer()`.
>
> Reorder tags exactly as in the Apertium printer: iterate `reading->tags_list`;
> a tag whose text starts with `+` sets `multi = true`, a `T_MAPPING` tag sets
> `multi = false`; `multi` tags go into `multitags_list`, others into
> `tags_list`; then append `multitags_list` after `tags_list`.
>
> Build `mi` (pipe-joined morphology). With `first = true` and a `used_tags`
> sorted set for `unique_tags` dedup, iterate `tags_list`: skip already-seen
> hashes when `unique_tags`; skip `endtag`/`begintag`; for a tag that is neither
> `T_BASEFORM` nor `T_WORDFORM`: if its text starts with `+`, print the tag text
> verbatim to `output` (mid-markup, an oddity); else if it starts with `@`, set
> `node.si = tag->tag` (syntactic-function tag; later procNode drops the leading
> `@`); else append the tag to `mi` (first one directly, subsequent ones
> prefixed with `|`). Finally `node.mi = mi`. `node.form`, `node.self`,
> `node.pos` are set by the caller, not here.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.print-single-window-fn]
> void MatxinApplicator::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.print-single-window-fn]
> Emits one window as a Matxin `<SENTENCE>...</SENTENCE>` block containing a
> nested dependency tree.
>
> Print `  <SENTENCE ord="%d" alloc="0">\n` with `window->number`. Then iterate
> `window->all_cohorts`: skip any cohort with `local_number == 0` or
> `CT_REMOVED`. If `!profiling`: `cohort->unignoreAll()`, and if `!split_mappings`
> call `mergeMappings(*cohort)`. Build a `Node n`: take the wordform text with
> the `"<` / `>"` wrapper stripped (`data()+2`, length `size-4`) and XML-escape
> it into `wf_escaped` — for each char, if `&` append `"&amp;"`, else if `"`
> append `"&quot;"`, then UNCONDITIONALLY append the original char afterward.
> (BUG: because the raw char is always appended after the entity, `&` becomes
> `&amp;&` and `"` becomes `&quot;"` — the entity and the literal are both
> emitted.) Set `n.self = cohort->global_number` and `n.form = wf_escaped`.
>
> Take only the FIRST reading (`cohort->readings[0]`) and call `printReading
> (reading, n, output)` to fill `n.lemma`/`n.mi`/`n.si`. Determine a fallback
> root `r = (int)nodes.size()` (the running count of nodes = "last word"), and
> if `deps[0]` is non-empty override `r = deps[0][0]`. Store `nodes[cohort->
> global_number] = n`. Record the dependency edge: if `cohort->dep_parent ==
> DEP_NO_PARENT`, `deps[r].push_back(global_number)`; else
> `deps[cohort->dep_parent].push_back(global_number)`. Flush after each cohort.
>
> After all cohorts, set `depth = 0` and call `procNode(depth, nodes, deps, 0,
> output)` to recursively print the tree rooted at virtual node 0. Then print
> `  </SENTENCE>\n`.
>
> Note: `nodes` and `deps` are member maps that are NEVER cleared between
> windows, so successive `<SENTENCE>` blocks accumulate and cross-contaminate
> node/dependency state; the "last word" fallback `nodes.size()` also grows
> across windows.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.proc-node-fn]
> void MatxinApplicator::procNode(int& depth, std::map<int, Node>& nodes, std::map<int, std::vector<int>>& deps, int n, std::ostream& output)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.proc-node-fn]
> Recursive depth-first printer of the Matxin dependency tree. Parameters:
> `depth` (by reference, mutated), the `nodes` and `deps` maps, current node id
> `n`, and `output`.
>
> Look up `node = nodes[n]` and `v = deps[n]` (both may default-construct empty
> entries via `operator[]`). Increment `depth`. Compute `si = node.si.data() +
> !node.si.empty()`, i.e. the `si` string with its first character skipped when
> non-empty (dropping the leading `@`), or the empty string when empty.
>
> If `n != 0`: print `depth * 2` spaces of indentation, then print a `<NODE>`
> element. If `v` (this node's children) is non-empty, print an OPEN tag
> `<NODE ord="%d" alloc="0" form="%S" lem="%S" mi="%S" si="%S">\n` with
> `node.self`, `node.form`, `node.lemma`, `node.mi`, `si`. Otherwise print a
> SELF-CLOSING `<NODE .../>` with the same attributes and DECREMENT `depth`.
>
> Then scan `deps`: set `found = true` iff some entry has `first == n` and a
> non-empty vector; if not found, RETURN early. Otherwise recurse into each
> child: `for it in v: procNode(depth, nodes, deps, it, output)`.
>
> After recursing, if `n != 0`, print `depth * 2` spaces and a closing
> `</NODE>\n`. Finally decrement `depth` and return. Node 0 is the virtual root
> and prints no markup of its own, only its children.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.process-reading-fn]
> void MatxinApplicator::processReading(Reading* cReading, const UChar* reading_string)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.process-reading-fn]
> Parses one Matxin/Apertium-style analysis string (`const UChar*
> reading_string`) into `cReading`. A convenience overload taking `const
> UString&` calls this with `.data()`. Two cursors `m` and `c` both start at the
> string.
>
> `insert_if_exists(cReading->parent->possible_sets, grammar->sets_any)`.
>
> Pass 1 (find multiword suffix into `suf`): with flags `tags=false`,
> `multi=false`, walk `m` to the NUL. `<` sets `tags = true`. A `#` seen AFTER
> tags (`tags == true`) sets `multi = true`. A `+` while `multi` sets `multi =
> false`. While `multi` is true, append `*m` to `suf` (this includes the `#`
> itself, since `multi` is set before the append at that char). Net: `suf` is
> the `#...` lemq segment from the first post-tags `#` up to the next `+` or end
> (e.g. `# happy`). `join_idx` is initialized to `'0'` here but is only ever
> incremented later and never read (dead).
>
> Build the baseform `base`, wrapped in `"`: walk `c` from the start; a `*`
> anywhere sets `unknown = true`; stop at the first `<` or NUL; append every
> other char to `base`. If `suf` is non-empty, append it to `base` (moving the
> multiword lemq onto the baseform, as pretransfer would). Close with `"`.
>
> Intern `tag = addTag(base)`. If `unknown`: set `cReading->baseform =
> tag->hash`, `addTagToReading(*cReading, tag)`, and RETURN (unknown words carry
> only the baseform).
>
> Otherwise push `tag` into `TagVector taglist` and read the tags with flags
> `joiner=false`, `intag=false` and accumulator `tmptag`. Walk `c` to NUL: a `+`
> sets `multi=false`, `joiner=true`, `++join_idx`. A `#` while `!intag` sets
> `multi=true`. On `<`: set `multi=false`; if already `intag`, print error
> "The Matxin stream format does not allow '<' in tag names.", skip the char and
> continue; else set `intag=true`, and if `joiner`, first flush the pending
> joined baseform: build `bf = "\""`, then append `tmptag` minus a leading `+`
> if present, close `"`, push `addTag(bf)` into `taglist`, clear `tmptag`, clear
> `joiner`; advance. On `>`: set `multi=false`; if `!intag`, print error "...
> does not allow '>' outside tag names.", skip and continue; else set
> `intag=false`, push `addTag(tmptag)` into `taglist`, clear `tmptag`, clear
> `joiner`, advance. If `multi` is true (inside a multiword queue outside a tag),
> skip the char. Otherwise append `*c` to `tmptag`.
>
> Finally assign tags to reading(s) with the same back-to-front baseform scan as
> the Apertium `processReading`: while `taglist` non-empty, reverse-scan for the
> first `T_BASEFORM` tag; if the current reading already has a baseform, create
> a sub-reading (`allocateReading`, link via `next`) — but note this Matxin
> variant does NOT re-add the wordform tag to the new sub-reading; collect tags
> from the baseform forward, routing `T_MAPPING`/`mapping_prefix` tags to
> `splitMappings(mappings, *reading->parent, *reading, true)` and adding the
> rest via `addTagToReading`; then pop the trailing non-baseform tags and the
> baseform itself. Ends with `assert(taglist.empty())`.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-fn]
> void MatxinApplicator::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-fn]
> Parses an Apertium-style stream and emits a Matxin XML `<corpus>` of
> dependency-tree `<SENTENCE>` blocks. Character-by-character; the inner reading
> loops call `u_fgetc` directly.
>
> Setup: set `ux_stdin`/`ux_stdout`. If `getNullFlush()` is true, delegate to
> `runGrammarOnTextWrapperNullFlush(input, output)` and return. Then the same
> validation/`CG3Quit(1)` checks as the Apertium applicator (`!input.good()`,
> `input.eof()`, `!output`, `!grammar`) and the same soft/hard-delimiter
> warnings. Init: `inchar = 0`, `superblank = false`, `incohort = false`, empty
> `firstblank`; `index()`; `resetAfter = (num_windows+4)*2+1`; `begintag =
> addTag(">>>")->hash`, `endtag = addTag("<<<")->hash`; null `cSWindow`,
> `cCohort`, `cReading`, `lSWindow`; `gWindow->window_span = num_windows`;
> `ux_stripBOM(input)`.
>
> Main loop `while ((inchar = u_fgetc(input)) != 0)`; break immediately if
> `input.eof()`.
> - `[` sets `superblank = true`; `]` sets `superblank = false`
>   (unconditionally, even inside a cohort).
> - If `inchar == '\\' && !incohort && !superblank`: read the next char and
>   append BOTH the backslash and that char to `cCohort->text` if `cCohort`,
>   else `lSWindow->text` if `lSWindow`, else print both directly to `output`;
>   `continue`.
> - `^` sets `incohort = true`.
> - If `superblank || inchar == ']' || !incohort`: this char is blank/markup —
>   append `inchar` to `cCohort->text` if `cCohort`, else `lSWindow->text` if
>   `lSWindow`, else `firstblank`; `continue`.
> - Otherwise (we are at the start of a cohort): if `cCohort` has no readings,
>   `initEmptyCohort(*cCohort)`. Soft-limit break: if `cCohort` and
>   `cSWindow->cohorts.size() >= soft_limit` and soft delimiters match, add
>   `endtag` to all readings, `appendCohort(cCohort)`, set `lSWindow = cSWindow`,
>   null `cSWindow`/`cCohort`, `++numCohorts`. Hard-limit break: analogously with
>   `hard_limit`/hard delimiters, warning "Hard limit ... forcing break." when
>   `!is_conv && size >= hard_limit`. If `!cSWindow`, create a new window:
>   `allocAppendSingleWindow`, then a 0th BOS cohort (`alloc_cohort`,
>   `global_number = cohort_counter++`, `wordform = tag_begin`, a reading with
>   `baseform = begintag`, `insert_if_exists` sets_any, `addTagToReading
>   (begintag)`, append reading, append cohort), set `lSWindow = cSWindow`,
>   `lSWindow->text = firstblank`, clear `firstblank`, null `cCohort`,
>   `++numWindows`. If `cCohort && cSWindow`, `cSWindow->appendCohort(cCohort)`
>   (append the PREVIOUS cohort). If `gWindow->next.size() > num_windows`:
>   `shuffleWindowsDown()`, `runGrammarOnWindow()`, and `resetIndexes()` when
>   `numWindows % resetAfter == 0`.
>
>   Allocate the new cohort (`alloc_cohort`, `global_number = cohort_counter++`).
>   Read the wordform: `wordform = "\"<"`; loop forever reading chars until `/`
>   or `<` breaks (on `\\`, read and append the next char literally, else append
>   the char); append `>\"`; `cCohort->wordform = addTag(wordform)`;
>   `++numCohorts`. If the breaking char was `<`, read the static reading into a
>   fresh `cCohort->wread`: loop reading chars — `\\` reads+appends next into
>   `tag`; `<` is skipped; `>` interns `addTag(tag)` and adds it to `wread` then
>   clears `tag`; `/` or `$` breaks; any other char appends to `tag` — until `/`
>   or `$`.
>
>   Read the readings: `while (incohort)` read a char: on `\\`, read the next
>   char and append it to `current_reading` (source TODO note: `\<` in
>   baseforms is mishandled). On `$`: allocate `cReading`, `insert_if_exists`
>   sets_any, `addTagToReading(cCohort->wordform)`, `processReading(cReading,
>   current_reading)`, reverse if `sub_readings_ltr && next`, append the reading,
>   `++numReadings`, clear `current_reading`, set `incohort = false`. On `/`
>   (end of one reading): allocate a (locally-shadowed) `cReading`,
>   `addTagToReading(wordform)`, `processReading`, reverse if needed, append,
>   `++numReadings`, clear `current_reading`, continue. Otherwise append the
>   char to `current_reading`. After the readings loop, if `!cReading->baseform`
>   warn "Line %u had no valid baseform." Then `++numLines`.
>
>   NOTE: `++numLines` runs once per fully-processed cohort (all the blank/
>   escape branches `continue` before reaching it), so `numLines` here counts
>   cohorts, not text lines — a quirk that skews line numbers in warnings.
>
> After the loop: if `firstblank` non-empty, print it and clear. If `cCohort &&
> cSWindow`: `appendCohort(cCohort)`; if it has no readings `initEmptyCohort`;
> add `endtag` to all its readings; null `cReading`/`cCohort`/`cSWindow`. Then
> print `<corpus>\n`; drain windows (`while gWindow->next: shuffleWindowsDown +
> runGrammarOnWindow`); `shuffleWindowsDown()`; while `gWindow->previous`:
> `printSingleWindow(front)`, `free_swindow`, erase. If `inchar && inchar !=
> 0xffff`, print `inchar` (e.g. a final newline). Print `</corpus>\n` and flush.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-wrapper-null-flush-fn]
> void MatxinApplicator::runGrammarOnTextWrapperNullFlush(std::istream& input, std::ostream& output)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-wrapper-null-flush-fn]
> Drives repeated grammar runs for null-flush mode. First calls `setNullFlush
> (false)` (so the nested `runGrammarOnText` does NOT re-enter this wrapper) and
> sets `runningWithNullFlush = true`. Then loops `while (!input.eof())`: call
> `runGrammarOnText(input, output)`, write a NUL byte (`u_fputc('\0', output)`),
> and flush the output. After the loop, set `runningWithNullFlush = false`. Each
> iteration therefore processes one null-terminated chunk and emits its own
> trailing NUL separator.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.set-null-flush-fn]
> void MatxinApplicator::setNullFlush(bool pNullFlush)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.set-null-flush-fn]
> Trivial setter: assigns the argument `pNullFlush` to the `nullFlush` member.
> No other side effects.

> [spec:cg3:def:matxin-applicator.cg3.matxin-applicator.test-pr-fn]
> void testPR(std::ostream& output)

> [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.test-pr-fn]
> `void testPR(std::ostream& output)` is only DECLARED (in MatxinApplicator.hpp,
> as a public method) — there is no definition anywhere in the source tree, so
> it has no behavior to reimplement. It is never called, so the program still
> links; taking its address or calling it would be an unresolved-symbol error.
> The Rust port should treat this as a no-op stub (or omit it entirely); the
> corresponding functional test debug routine lives only in the Apertium
> applicator's `testPR`.

