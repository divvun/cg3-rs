# src/TextualParser.cpp, src/TextualParser.hpp

> [spec:cg3:def:textual-parser.cg3.freq-sorter]
> struct freq_sorter {
>   const bc::flat_map<Tag*, size_t>& tag_freq;
> }

> [spec:cg3:def:textual-parser.cg3.freq-sorter.freq-sorter-fn]
> freq_sorter(const bc::flat_map<Tag*, size_t>& tag_freq)

> [spec:cg3:sem:textual-parser.cg3.freq-sorter.freq-sorter-fn]
> Constructor for the `freq_sorter` comparator functor. Takes a const
> reference to a `bc::flat_map<Tag*, size_t>` mapping each tag to its
> frequency count and stores it by reference in the member `tag_freq`;
> empty body. Used to reorder tag vectors by descending frequency.

> [spec:cg3:def:textual-parser.cg3.freq-sorter.operator-fn]
> bool operator()(Tag* a, Tag* b) const

> [spec:cg3:sem:textual-parser.cg3.freq-sorter.operator-fn]
> `bool operator()(Tag* a, Tag* b) const` - comparator that sorts
> highest-frequency-first. Looks up `a` and `b` in the referenced
> `tag_freq` map (dereferencing `find(...)->second` with no end-check,
> so both keys must be present) and returns
> `tag_freq[a] > tag_freq[b]` (a orders before b when strictly more
> frequent). Used with `std::sort` for cheap trie compression.

> [spec:cg3:def:textual-parser.cg3.is-mapping-list-fn]
> bool is_mapping_list(Grammar* result, Set* s)

> [spec:cg3:sem:textual-parser.cg3.is-mapping-list-fn]
> Recursively decide whether Set `s` qualifies as a "mapping list" (a
> plain LIST-style set usable as a MAP/ADD/SUBSTITUTE maplist),
> returning bool. Local `is_list = true`. First branch: if the set is
> NOT (empty `trie` AND empty `trie_special` AND not a unification set
> of type ST_TAG_UNIFY|ST_SET_UNIFY|ST_CHILD_UNIFY) - i.e. it has
> direct tag content or is a unification set - treat it as a leaf:
> iterate over both tries, skip empties, get their tag vectors via
> `trie_getTags`, and for every tag in every vector, if the tag has
> T_FAILFAST or T_REGEXP_LINE, return false immediately; if none
> disqualify, return `is_list` (true). Otherwise (a pure composite of
> sub-sets, no direct tags, not unification): if any operator in
> `s->set_ops` is not `S_OR`, set is_list=false and break; then for
> each sub-set hash in `s->sets`, fetch it with `result->getSet(i)` and
> recurse - if any recursion returns false, set is_list=false and
> break. Return is_list. Net: a set is a mapping list iff built only
> from OR of LIST-like leaves and containing no failfast/regex-line
> tags.

> [spec:cg3:def:textual-parser.cg3.textual-parser]
> class TextualParser : public IGrammarParser {
>   const char* filebase = nullptr;
>   uint32SortedVector strict_tags;
>   uint32SortedVector list_tags;
>   Profiler* profiler = nullptr;
>   UChar nearbuf[32]{};
>   uint32_t verbosity_level = 0;
>   uint32_t sets_counter = 100;
>   uint32_t seen_mapping_prefix = 0;
>   flags_t section_flags;
>   bool option_vislcg_compat = false;
>   bool in_section = false, in_before_sections = true, in_after_sections = false, in_null_section = false, in_nested_rule = false;
>   bool no_isets = false, no_itmpls = false, strict_wforms = false, strict_bforms = false, strict_second = false, strict_regex = false, strict_icase = false;
>   bool self_no_barrier = false;
>   bool safe_setparent = false;
>   bool only_sets = false;
>   Rule* nested_rule = nullptr;
>   const char* filename = nullptr;
>   UChar* cur_grammar = nullptr;
>   uint32_t cur_grammar_n = 0;
>   uint32_t num_grammars = 0;
>   deferred_t deferred_tmpls;
>   std::vector<std::unique_ptr<UString>> grammarbufs;
>   int error_counter = 0;
> }

> [spec:cg3:def:textual-parser.cg3.textual-parser.add-rule-to-grammar-fn]
> void TextualParser::addRuleToGrammar(Rule* rule)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.add-rule-to-grammar-fn]
> Assign the rule to the correct grammar section from parser state,
> then register it. If `in_nested_rule`: set `rule->section = -3`,
> `result->addRule(rule)`, and append the rule to
> `nested_rule->sub_rules`. Else if `in_section`:
> `rule->section = SI32(result->sections.size()) - 1` (index of the
> current numbered section), `addRule`. Else if `in_after_sections`:
> section = -2, `addRule`. Else if `in_null_section`: section = -3,
> `addRule`. Else (before-sections, the default): section = -1,
> `addRule`. (Section encoding: -1 before, >=0 numbered sections, -2
> after, -3 null/nested.)

> [spec:cg3:def:textual-parser.cg3.textual-parser.add-tag-fn]
> Tag* TextualParser::addTag(Tag* tag)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.add-tag-fn]
> Thin delegate: `return result->addTag(tag);` - hands the tag to the
> Grammar for deduplication/registration and returns the canonical
> Tag*. Implements the `addTag` hook of the `State` concept used by the
> free `parseTag`/`parseSet` helpers.

> [spec:cg3:def:textual-parser.cg3.textual-parser.deferred-t]
> typedef std::unordered_map<ContextualTest*, std::pair<size_t, UString>> deferred_t

> [spec:cg3:def:textual-parser.cg3.textual-parser.error-fn]
> void TextualParser::error(const char* str, const UChar* p)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.error-fn]
> The `(const char* str, const UChar* p)` overload of the error
> reporter (one of nine overloads sharing this behavior). Copies up to
> 20 UChars of context starting at `p` into the member buffer `nearbuf`
> via `ux_bufcpy` (which also maps embedded CR/LF to visible U+240x
> control-picture glyphs and NUL-terminates). Then prints via
> `u_fprintf(ux_stderr, str, filebase, result->lines, nearbuf)` - the
> format string receives file base name, current line number, and the
> near-context string. Finally calls `incErrorCount()`, which is
> [[noreturn]] (it throws), so this never returns. Backs the many
> "Error: ... on line %u near `%S`" messages.

> [spec:cg3:def:textual-parser.cg3.textual-parser.get-grammar-fn]
> Grammar* get_grammar()

> [spec:cg3:sem:textual-parser.cg3.textual-parser.get-grammar-fn]
> Trivial accessor `Grammar* get_grammar() { return result; }` -
> returns the `result` grammar pointer (from base `IGrammarParser`)
> that the parser is populating. Provides the `get_grammar` hook of the
> `State` interface consumed by the free `parseTag`/`parseSet` helpers.

> [spec:cg3:def:textual-parser.cg3.textual-parser.inc-error-count-fn]
> void TextualParser::incErrorCount()

> [spec:cg3:sem:textual-parser.cg3.textual-parser.inc-error-count-fn]
> Central error-count/bailout routine, [[noreturn]]. Flushes
> `ux_stderr`, increments `error_counter`. If `error_counter >= 10`,
> prints "Too many errors - giving up..." and calls `CG3Quit(1)`
> (process exit). Otherwise `throw error_counter;` (throws an `int`) -
> this unwinds up to the per-statement `try/catch(int)` in
> `parseFromUChar`, which recovers by skipping to the next line. So
> each `error(...)` aborts the current construct but parsing continues,
> up to 10 accumulated errors.

> [spec:cg3:def:textual-parser.cg3.textual-parser.maybe-parse-rule-fn]
> bool TextualParser::maybeParseRule(UChar*& p)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.maybe-parse-rule-fn]
> Keyword dispatcher: examines the text at `p` and, if it begins with a
> rule keyword (case-insensitive via `IS_ICASE`, which also requires
> the following char to be a non-alnum boundary), calls
> `parseRule(p, K_...)` for that keyword and returns true; otherwise
> returns false without consuming input. The checks form an ordered
> if/else-if chain; order matters where one name is a prefix of
> another, so longer names come first: ADDRELATIONS/SETRELATIONS/
> REMRELATIONS before ADDRELATION/SETRELATION/REMRELATION; then
> SETVARIABLE, REMVARIABLE, SETPARENT, SETCHILD, REMPARENT,
> SWITCHPARENT, RESTORE, IFF, MAP, ADD, APPEND, SELECT, REMOVE,
> REPLACE, SUBSTITUTE, COPYCOHORT before COPY, UNMAP, PROTECT,
> UNPROTECT, DELIMIT, JUMP, MOVE, SWITCH, EXECUTE, EXTERNAL, REMCOHORT,
> ADDCOHORT, SPLITCOHORT, MERGECOHORTS, RESTORE (a dead duplicate
> branch, never reached), WITH. Each maps to the corresponding KEYWORDS
> enum value passed to `parseRule`. If none match, return false.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-anchorish-fn]
> void TextualParser::parseAnchorish(UChar*& p, bool rule_flags)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-anchorish-fn]
> Parse an optional anchor/section name plus optional section-level
> rule flags after a section header (SECTION/BEFORE-SECTIONS/MAPPINGS/
> ANCHOR/etc.). Signature `(UChar*& p, bool rule_flags=true)`.
> If `*p != ':'` (a name is present; a leading `:` is reserved for
> section flags): read a token from `p` up to whitespace via
> `SKIPTOWS(n, 0, true)` (allowhash) into `gbuffers[0]`. If not
> `only_sets`, register it as an anchor with
> `result->addAnchor(name, UI32(result->rule_by_number.size()), true)`
> - i.e. the anchor targets the current rule count. Advance `p` past
> the name. Then `SKIPWS(p, ':')` up to a `:`. If `rule_flags` is true
> and `*p == ':'`, consume the `:` and parse section-wide default flags
> into the member `section_flags` via `parseRuleFlags(p)`. Then
> `SKIPWS(p, ';')`; if `*p != ';'` -> error "Expected closing ; ...
> after anchor/section name". Does not consume the `;` (the caller
> handles it).

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-dependency-tests-fn]
> void TextualParser::parseContextualDependencyTests(UChar*& p, Rule* rule)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-dependency-tests-fn]
> Parse one dependency-side contextual test and attach it to
> `rule->dep_tests`. Calls `parseContextualTestList(p, rule)` to build
> test `t`. If `option_vislcg_compat` and `t` has POS_NOT: convert NOT
> to NEGATE (clear POS_NOT, set POS_NEGATE) - legacy vislcg semantics.
> Then `rule->addContextualTest(t, rule->dep_tests)`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn]
> ContextualTest* TextualParser::parseContextualTestList(UChar*& p, Rule* rule, bool in_tmpl)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn]
> Parse a full contextual test (position/target/barriers, plus the
> template forms and any LINKed continuation) and return the
> registered `ContextualTest*`. `p` is advanced; `rule` may be null
> (e.g. TEMPLATE); `in_tmpl` marks parsing inside a template/inline-
> template.
> Allocate a ContextualTest `t` (keep `ot = t` as the head), set
> `t->line = result->lines`. Then, each preceded by `SKIPWS`, consume
> optional leading modifier keywords in this order, OR-ing bits into
> `t->pos`: `STR_TEXTNEGATE` ("NEGATE") -> POS_NEGATE; `STR_ALL`
> ("ALL") -> POS_ALL; `STR_NONE` ("NONE") -> POS_NONE; `STR_TEXTNOT`
> ("NOT") -> POS_NOT (each matched case-insensitively via
> `ux_simplecasecmp`, skipped by its length).
> Peek the token up to `(` into `gbuffers[0]` (via `SKIPTOWS(n,'(')`)
> and branch:
> 1. If the token is empty/whitespace (`ux_isEmpty`): an inline
>    template (a parenthesized OR-group). If `no_itmpls` -> error
>    "Inline template spotted". Loop: require `*p=='('` (else error
>    "Expected '('"), `++p`, recurse
>    `parseContextualTestList(p, rule, true)` into `ored`, `++p`
>    (consume the closing `)`), push `ored` into `t->ors`; `SKIPWS`; if
>    next is `STR_OR` ("OR") consume it and continue, else break. After
>    the loop, if `t->ors.size()==1` and verbosity>0, warn (either
>    "only make sense if you OR them" or, if the single alternative
>    itself has >=2 ors, "do not need () around the whole expression").
> 2. Else if the token starts with `[` (`gbuffers[0][0]=='['`):
>    template shorthand `[set, set, ...]`. `++p`, `SKIPWS`, parse a set
>    via `parseSetInlineWrapper` -> `t->offset=1`, `t->target=hash`.
>    Then while `*p==','`: `++p`, `SKIPWS`, allocate a linked
>    ContextualTest `lnk`, parse a set, `lnk->offset=1`,
>    `lnk->target=hash`, chain `t->linked=lnk; t=lnk`. Require closing
>    `]` (else error), then `++p`.
> 3. Else if the token starts with `T:` (`gbuffers[0][0]=='T' &&
>    [1]==':'`): `goto label_parseTemplateRef` (the template-reference
>    handler below).
> 4. Else (a normal test): `parseContextualTestPosition(p, *t)` to
>    parse the position, then `p = n` (the token boundary computed by
>    the earlier peek). If pos has DEP_CHILD/DEP_PARENT/DEP_SIBLING ->
>    `result->has_dep = true`; if POS_RELATION -> `result->has_relations
>    = true`. `SKIPWS`. Then:
>    - If the next chars are `T:` this is a template override: set
>      POS_TMPL_OVERRIDE and (at `label_parseTemplateRef`) `p+=2`, read
>      the template name up to whitespace/`)` into `gbuffers[0]`,
>      compute `cn = hash_value(name)`, stash `t->tmpl =
>      reinterpret_cast<ContextualTest*>(cn)` (a placeholder holding
>      the hash), and record `tmpl_data = {result->lines, name}` for
>      deferred resolution. `SKIPWS`.
>    - Else parse the target set via `parseSetInlineWrapper` ->
>      `t->target = hash`.
>    - `SKIPWS`; if next is `STR_CBARRIER` ("CBARRIER") consume it,
>      `SKIPWS`, parse a set -> `t->cbarrier = hash`. `SKIPWS`; if next
>      is `STR_BARRIER` ("BARRIER") consume, parse set ->
>      `t->barrier = hash`. `SKIPWS`.
>    - If a (c)barrier was set but the position is not a scanning/self
>      test (no MASK_POS_SCAN and no POS_SELF): warn "Barriers only make
>      sense for scanning or self tests" and clear both barriers.
> After the branch: `SKIPWS`. If next is `STR_AND` ("AND") -> error
> (deprecated; use LINK 0 or `+`). If next is `STR_LINK` ("LINK")
> consume it and set `linked=true`. `SKIPWS`. If linked:
> `t->linked = parseContextualTestList(p, rule, in_tmpl)`; if
> `t->pos & POS_NONE` -> error "does not make sense to LINK from a NONE
> test". Else if not `in_tmpl` and pos has POS_SCANALL but not
> POS_CAREFUL: warn "** without LINK or C doesn't make sense".
> If `rule` non-null, propagate its look-flags into the test:
> RF_LOOKDELETED->POS_LOOK_DELETED, RF_LOOKDELAYED->POS_LOOK_DELAYED,
> RF_LOOKIGNORED->POS_LOOK_IGNORED.
> Register: `t = result->addContextualTest(ot)` (dedup on the whole
> structure). If `profiler`, record the test's byte span. If `t->tmpl`
> is set, record `deferred_tmpls[t] = tmpl_data` so the name-hash is
> resolved to a real template after the whole grammar is parsed. Return
> `t`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn]
> void TextualParser::parseContextualTestPosition(UChar*& p, ContextualTest& t)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn]
> Parse the position specifier of a contextual test (the leading token
> such as `-1`, `*`, `**C`, `p`, `cc`, `r:rel`, `jC3`) into `t.pos`,
> `t.offset`, `t.offset_sub`, `t.relation`, `t.jump_pos`. `n=p`
> remembers the start for error context; local flags `negative` and
> `had_digits`.
> Main loop (bounded to 100 iterations, `tries`) running until `*p` is
> a space, `(`, or `/`: it applies a long sequence of INDEPENDENT `if`
> tests, each of which, when its char matches, ORs a POS_* bit and
> advances `p`. Because they are separate `if`s (not else-if), one
> iteration can consume several adjacent specifier letters and ORDER
> matters. In source order: `**`->POS_SCANALL (p+=2); `*`->POS_SCANFIRST;
> `C`->POS_CAREFUL; `c`->POS_DEP_CHILD and an immediately following `c`
> clears DEP_CHILD and sets POS_DEP_GLOB; `p`->POS_DEP_PARENT and a
> following `p` adds POS_DEP_GLOB; `s`->POS_DEP_SIBLING; `S`->POS_SELF;
> `N`->POS_NO_BARRIER; `<`->POS_SPAN_LEFT; `>`->POS_SPAN_RIGHT;
> `W`->POS_SPAN_BOTH; `@`->POS_ABSOLUTE; `O`->POS_NO_PASS_ORIGIN;
> `o`->POS_PASS_ORIGIN; `L`->POS_LEFT_PAR; `R`->POS_RIGHT_PAR;
> `X`->POS_MARK_SET; `x`->POS_JUMP; `D`->POS_LOOK_DELETED;
> `d`->POS_LOOK_DELAYED; `I`->POS_LOOK_IGNORED; `A`->POS_ATTACH_TO;
> `w`->POS_WITH; `?`->POS_UNKNOWN; `f`->POS_NUMERIC_BRANCH;
> `T`->POS_ACTIVE; `t`->POS_INACTIVE; `B`->sets
> `result->has_bag_of_tags=true` and POS_BAG_OF_TAGS; `-`->set local
> `negative=true`; digits->set had_digits and accumulate decimal into
> `t.offset` (`t.offset = t.offset*10 + (*p-'0')`); `r:`->POS_RELATION,
> then read a tag name up to whitespace/`(` (`SKIPTOWS(n,'(')`),
> `parseTag` it, `t.relation = tag->hash`; plain `r`->POS_RIGHT and a
> following `r` clears POS_RIGHT and sets POS_RIGHTMOST; `l`->POS_LEFT
> and a following `l` sets POS_LEFTMOST; `j` jump forms: `jM`->POS_JUMP,
> `jA`->POS_JUMP + jump_pos=JUMP_ATTACH, `jT`->POS_JUMP +
> jump_pos=JUMP_TARGET, `jC<digit>`->POS_JUMP + jump_pos=that digit.
> After the loop: if `negative`, `t.offset = -abs(t.offset)`.
> If `*p == '/'`: parse a secondary subreading offset - `++p`, then a
> second up-to-100 loop until space/`(`: `**` or `*`->offset_sub=GSR_ANY;
> `-`->negative; digits accumulate into `t.offset_sub`. If negative,
> `t.offset_sub = -abs(t.offset_sub)`.
> Post-processing/validation:
> - if `self_no_barrier` option and POS_SELF set: toggle POS_NO_BARRIER
>   (clear if present, else set).
> - if pos has (DEP_CHILD|DEP_SIBLING) and (SCANFIRST|SCANALL): clear
>   both scan bits and set POS_DEP_DEEP.
> - if `tries >= 100` -> error "unknown specifier %C"; else if
>   `tries >= 20` -> warning "took many loops".
> - if the stop char is not whitespace (`!ISSPACE(*p)`) -> error
>   "garbage data".
> - if exactly one char was consumed and it was `o` or `O` -> error
>   (stand-alone o/O - maybe you meant 0).
> - if had_digits: error if combined with dependency
>   (DEP_CHILD/SIBLING/PARENT), with enclosures (LEFT_PAR/RIGHT_PAR),
>   or with relations (POS_RELATION).
> - if POS_BAG_OF_TAGS combined with anything other than
>   NOT/NEGATE/SPAN_BOTH/SPAN_LEFT/SPAN_RIGHT, or with digits -> error
>   "bag of tags may only be combined with window spanning".
> - if POS_DEP_PARENT without POS_DEP_GLOB and with LEFTMOST/RIGHTMOST
>   -> error (leftmost/rightmost requires ancestor not parent).
> - if POS_PASS_ORIGIN and POS_NO_PASS_ORIGIN both -> error; LEFT_PAR
>   and RIGHT_PAR both -> error; POS_ALL and POS_NONE both -> error;
>   POS_UNKNOWN combined with anything else or with digits -> error.
> - if POS_SCANALL and POS_NOT -> warning "mixing NOT and **".
> - finally, if `t.pos > POS_64BIT` set POS_64BIT (marks that high bits
>   are in use). All `error(...)` calls throw to abort the test.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-tests-fn]
> void TextualParser::parseContextualTests(UChar*& p, Rule* rule)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-tests-fn]
> Parse one contextual test for the main (non-dependency) test set of a
> rule. Calls `parseContextualTestList(p, rule)` -> `t`. If
> `option_vislcg_compat` and `t` has POS_NOT: convert to POS_NEGATE
> (clear POS_NOT, set POS_NEGATE). Then
> `rule->addContextualTest(t, rule->tests)`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-from-u-char-fn]
> void TextualParser::parseFromUChar(UChar* input, const char* fname)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-from-u-char-fn]
> Top-level directive parser over a null-terminated UTF-16 buffer
> `input` with optional filename `fname`. If `input` is null or empty
> -> print error and `CG3Quit(1)`. If `profiler` set, force
> `parse_ast=true`. Assign a grammar id `id = ++num_grammars` (or, when
> profiling, `id = profiler->addGrammar(fname, utf8-of-input)`); set
> `cur_grammar=input`, `cur_grammar_n=id`, `p=input`, `result->lines=1`;
> open the root AST `Grammar` node; `filebase = basename(fname)`.
> Main loop while `*p`, each iteration wrapped in
> `try { ... } catch(int) { result->lines += SKIPLN(p); }` so any
> thrown `error()` skips to the next line and continues. Each iteration:
> optionally print progress every 500 lines when verbosity>0;
> `result->lines += SKIPWS(p)` (skip whitespace/`#`-comments/blank
> lines, counting newlines). Then a big if/else-if chain matching
> keywords via `IS_ICASE`. Each directive parses its payload, validates
> a trailing `;` where applicable, and builds AST nodes:
> - DELIMITERS: error if already defined; allocate `result->delimiters`
>   named STR_DELIMITSET; require `=`; `parseTagList` into it; addSet;
>   error if empty; require `;`.
> - SOFT-DELIMITERS: same into `result->soft_delimiters`.
> - TEXT-DELIMITERS: same into `result->text_delimiters`, then every
>   tag in it must be T_REGEXP else error "had non-regex tag".
> - MAPPING-PREFIX: error if seen before; read one token; set
>   `result->mapping_prefix = gbuffers[0][0]` (its FIRST char only);
>   error if empty; `;`.
> - PREFERRED-TARGETS: `=`; loop reading tags (MAYBE_QUOTED +
>   SKIPTOWS(';')) pushing each `tag->hash` into
>   `result->preferred_targets`; error if none; `;`.
> - REOPEN-MAPPINGS: like above, inserting hashes into
>   `result->reopen_mappings`.
> - STATIC-SETS: `=`; loop reading whitespace-delimited names into
>   `result->static_sets` (as UStrings); error if none; `;`.
> - CMDARGS-OVERRIDE / CMDARGS: require `+=`; collect raw text up to `;`
>   (honoring quoted spans), convert UTF-16->UTF-8, store into
>   `result->cmdargs_override` if the matched length `icn==16` else
>   `result->cmdargs`; `;`.
> - UNDEF-SETS: `=`; loop reading names, call `result->undefSet(name)`,
>   warn if a name wasn't defined; error if none; `;`.
> - SETS: just `p += 4` (bare section header, no payload).
> - LIST-TAGS: `+=`; swap `list_tags` out to a temp, read tags into a
>   temp `uint32SortedVector` (by hash), error if empty, `;`, then swap
>   the temp back into `list_tags`.
> - LIST / OLIST: allocate a Set; OLIST sets `ordered=true` and skips
>   the leading `O`; `p+=4`; read the name (trim trailing `,`/`]`);
>   `setName`; handle `+=` append (target set must already exist else
>   error) vs `=`; `parseTagList(p, s, ordered)`; `s->rehash()`; then
>   `appendToSet` (append) or `addSet` (with a verbosity>0 warning on
>   duplicate identical definitions); error if empty; `;`.
> - SET: allocate Set; `p+=3`; read name; `=`; `parseSetInline(p, s)`
>   with `no_isets` temporarily false; `rehash`; warn on identical
>   existing set; else if it is a single-set alias
>   (`sets.size()==1 && !ST_TAG_UNIFY`) record `result->set_alias` and
>   destroy the wrapper, reusing the aliased set; `addSet`; error if
>   empty; `;`.
> - MAPPINGS / CORRECTIONS / BEFORE-SECTIONS: unless `only_sets`, set
>   `in_before_sections=true` and the other three section flags false;
>   if a name follows on the same line, call `parseAnchorish`.
> - SECTION / CONSTRAINTS: push `result->lines` into `result->sections`,
>   set `in_section=true`; optional `parseAnchorish`.
> - AFTER-SECTIONS: `in_after_sections=true`; optional `parseAnchorish`.
> - NULL-SECTION: `in_null_section=true`; optional `parseAnchorish`.
> - SUBREADINGS: `=`; L/LTR -> `sub_readings_ltr=true`, R/RTL -> false,
>   else error; `;`.
> - OPTIONS: `+=`; loop matching option keywords against a table that
>   sets booleans (no_isets, no_itmpls, strict_wforms, strict_bforms,
>   strict_second, strict_regex, strict_icase, self_no_barrier,
>   result->ordered, result->addcohort_attach, safe_setparent); error
>   on unknown; if addcohort_attach set `result->has_dep`; `;`.
> - STRICT-TAGS: `+=`; like LIST-TAGS but into `strict_tags`.
> - ANCHOR: `p+=6`; `parseAnchorish(p, false)` (no section flags).
> - INCLUDE: `p+=7`; optional `STATIC` keyword forces `only_sets` for
>   the include; read filename; `;`; convert to UTF-8; shell-expand
>   with `wordexp` if it contains `~`/`$`/`*` (first match only);
>   resolve relative to the current file's dir if not absolute; stat +
>   read the file (strip UTF-8 BOM), convert to UTF-16 into a new
>   `grammarbufs` entry at offset 4, and recursively `parseFromUChar`
>   with swapped-in line/filebase/cur_grammar state.
> - TEMPLATE: `p+=8`; read name; `=`;
>   `parseContextualTestList(p, nullptr, true)` with `no_itmpls`
>   temporarily false; set its line; `result->addTemplate(t, name)`;
>   `;`.
> - PARENTHESES: `=`; loop over `( left right )` pairs, each a
>   CompositeTag of exactly two `parseTag`s; store
>   `result->parentheses[left->hash]=right->hash` and the reverse map;
>   error on malformed; error if none; `;`.
> - END: if preceded by newline/space AND followed by NUL/newline/space
>   -> `break` out of the whole parse loop; otherwise `++p` (treat as
>   ordinary text).
> - Else `maybeParseRule(p)` - if it recognizes a rule keyword it
>   parses the rule (kept last so e.g. MAPPINGS is not mis-parsed as
>   MAP PINGS).
> - Else (no keyword): skip; if `*p` is `;` or `"` handle quoted spans
>   / skip to whitespace; if non-terminator garbage remains -> error
>   "Garbage data encountered"; count a newline; `++p`.
> After the loop, AST-close the Grammar node with `id`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-grammar-fn]
> int TextualParser::parse_grammar(UString& data)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn]
> The private `parse_grammar(UString& data)` driver: sets up magic
> tags/sets, runs the directive parser, then finalizes. `data` is the
> fully-decoded UTF-16 buffer whose real text starts at index 4 (with
> 4 leading NULs so look-back code is safe). Steps:
> 1. Register the START anchor at rule number 0
>    (`result->addAnchor(keywords[K_START], 0, true)`).
> 2. Create built-in magic objects: the `*` any-tag
>    (`result->tag_any = parseTag(STR_ASTERIK)->hash`); the dummy set
>    (`allocateDummySet`); singleton magic sets `_TARGET_`, `_MARK_`,
>    `_ATTACHTO_`, `_LEFT_`, `_RIGHT_`, `_ENCL_`, `_SAME_BASIC_` each
>    containing their same-named magic tag; the `_PAREN_` set =
>    `(_LEFT_) OR (_RIGHT_)`; and context sets `_C1_`.._C9_` each with
>    their tag. All at line 0.
> 3. `parseFromUChar(&data[4], filename)` to parse the actual grammar
>    text.
> 4. Register the END anchor at the last rule number
>    (`rule_by_number.size()-1`).
> 5. For every named rule add an anchor `name -> number`.
> 6. Validate JUMP rules: for each K_JUMP rule take the first tag of
>    its maplist; if not T_SPECIAL, look it up in `result->anchors`; if
>    missing -> print "JUMP ... could not find anchor" and
>    `++error_counter` (does NOT throw).
> 7. For each single tag: if T_REGEXP_LINE force `result->ordered=true`;
>    then for T_VARSTRING tags scan the text for `{...}` groups and for
>    each one `parseSet` the inner name, appending the resolved Set to
>    `tag->vs_sets` and the `{name}` literal to `tag->vs_names`
>    (allocating those vectors on first use).
> 8. Resolve deferred template refs: for each `deferred_tmpls` entry,
>    hash the stored name; if `result->templates` has no such template
>    -> error message + `++error_counter`; else set the context's
>    `tmpl` pointer to the real template.
> 9. Numeric-branch splitting: iterate `result->contexts`; for any
>    context with POS_NUMERIC_BRANCH, erase it, build a CAREFUL "safe"
>    copy over a numeric-stripped target set (caching target->stripped
>    via `result->removeNumericTags`), clear POS_NUMERIC_BRANCH on the
>    original "unsafe" one, register both, build `orc = (safe OR
>    unsafe)`, re-point every context/rule dep_target/test that
>    referenced the old context to `orc`, copy profiler span if
>    present, and restart the scan.
> 10. Set `result->num_tags = single_tags_list.size()`.
> Return `error_counter`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-rule-flags-fn]
> flags_t TextualParser::parseRuleFlags(UChar*& p)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-flags-fn]
> Parse the sequence of rule option flags at `p`, returning a
> `flags_t {flags, sub_reading}`. `SKIPWS`; remember `lp=p` for errors.
> Outer loop `while (setflag)`: reset `setflag=false`, then inner
> `for i in [0, FLAGS_COUNT)`: if the text at `p` case-insensitively
> matches `g_flags[i]` (`ux_simplecasecmp`): advance past it, set bit
> `1<<i` in `rv.flags`, `setflag=true`. Special-case `FL_SUB`: require
> a following `:` (else `goto undo_flag`), consume it, read the
> following token; if it is `*` set `rv.sub_reading = GSR_ANY` else
> `u_sscanf(..., "%d", &rv.sub_reading)`. Validity guard: if the char
> after the matched flag is not `(`, `;`, or whitespace, this "flag"
> is really part of something else - at `undo_flag:` clear the bit,
> restore `p=op`, `setflag=false`, break. Otherwise emit a RuleFlag AST
> node. After each attempt, `SKIPWS`, and if the next char is
> `(`/`T`/`t`/`;` there can be no more flags -> `setflag=false`, break.
> After the inner loop, if any of RF_WITHCHILD/RF_NOCHILD/RF_BEFORE/
> RF_AFTER is set, break the outer loop (these must be last, since a
> set follows). Then validate mutual exclusions: for each group in
> `flag_excls`, if more than one bit of the group is set -> error
> listing the offending flag names. Also SAFE+UNMAPLAST -> error.
> Post-adjust: UNMAPLAST implies RF_UNSAFE; REMEMBERX implies
> RF_KEEPORDER; ENCL_FINAL sets `result->has_encl_final`. Final
> `SKIPWS`; return `rv`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-rule-fn]
> void TextualParser::parseRule(UChar*& p, KEYWORDS key)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-fn]
> Parse a full rule of keyword type `key` and (unless filtered out) add
> it to the grammar. Allocate `rule`, set `rule->line`,
> `rule->type=key`.
> Leading wordform: `lp=p; BACKTONL(lp)` to the start of the logical
> line, `SKIPWS(lp)`; if `lp<p` a token precedes the keyword -> parse
> it (MAYBE_QUOTED + SKIPTOWS) as a tag into `rule->wordform`.
> Consume the keyword (`p += keywords[key].size()`), `SKIPWS`. Optional
> `:name` - if `*p==':'`, read the name up to `(`; empty name ->
> warning, else `rule->setName`.
> If key==K_EXTERNAL: expect ONCE (->K_EXTERNAL_ONCE) or ALWAYS
> (->K_EXTERNAL_ALWAYS) else error; then read the command (optionally
> quoted, quotes stripped) into a tag, `rule->varname = tag->hash`.
> Flags: `flags = parseRuleFlags(p)`; copy `flags.flags->rule->flags`
> and `flags.sub_reading->rule->sub_reading`. Merge section defaults:
> for each set bit in `section_flags.flags` not excluded by
> `_flags_excls[i]`, OR it into `rule->flags`; if `section_flags.
> sub_reading` and the rule has none, inherit it.
> Default iteration: if neither RF_ITERATE nor RF_NOITERATE and key is
> NOT one of SELECT/REMOVE/IFF/DELIMIT/REMCOHORT/MOVE/SWITCH -> set
> RF_NOITERATE. K_UNMAP with no SAFE/UNSAFE -> RF_SAFE. K_SETPARENT
> with no SAFE/UNSAFE -> RF_SAFE if `safe_setparent` else RF_UNSAFE.
> If RF_WITHCHILD: parse a set -> `rule->childset1` (has_dep=true).
> Else if RF_NOCHILD: `childset1=0`.
> If key is SUBSTITUTE or EXECUTE: parse a "sublist" set (no_isets
> temporarily false), `reindex`, `rule->sublist`; error if empty or not
> `is_mapping_list`.
> SUB:* guard: if `rule->sub_reading==GSR_ANY` and key in
> {MAP,ADD,REPLACE,SUBSTITUTE,COPY,COPYCOHORT} -> error.
> If key needs a maplist (MAP/ADD/REPLACE/APPEND/SUBSTITUTE/COPY/
> COPYCOHORT/ADDRELATION(S)/SETRELATION(S)/REMRELATION(S)/SETVARIABLE/
> REMVARIABLE/ADDCOHORT/JUMP/SPLITCOHORT/MERGECOHORTS/RESTORE): parse a
> set -> `rule->maplist` (reindex); error if empty or not a mapping
> list.
> If (COPY/COPYCOHORT/REPLACE) and next is EXCEPT: consume it, set
> `copy_except`.
> If key in {ADDRELATIONS/SETRELATIONS/REMRELATIONS/SETVARIABLE} OR
> `copy_except`: parse another set -> `rule->sublist` (reindex); error
> if empty/not-list.
> If key==ADDCOHORT: expect AFTER (->K_ADDCOHORT_AFTER) or BEFORE
> (->K_ADDCOHORT_BEFORE) else error.
> If key in {ADD,MAP,SUBSTITUTE,COPY,COPYCOHORT}: optional AFTER
> (->RF_AFTER) or BEFORE (->RF_BEFORE); and unless COPYCOHORT, if
> BEFORE/AFTER set, parse a set -> `childset1`.
> `SKIPWS`; optional TARGET keyword (skipped). `SKIPWS`. Optional
> WITHCHILD (parse set -> childset1, set RF_WITHCHILD, clear RF_NOCHILD,
> has_dep) or NOCHILD (RF_NOCHILD, childset1=0).
> Parse the mandatory target set -> `rule->target`. `SKIPWS`; optional
> IF keyword. `SKIPWS`.
> Contexts: while `*p=='('`: `++p`, `SKIPWS`,
> `parseContextualTests(p, rule)`, `SKIPWS`, require `)` else error,
> `++p`.
> If key needs a dependency/second target (SETPARENT/SETCHILD/
> ADDRELATION(S)/SETRELATION(S)/REMRELATION(S)/MOVE/SWITCH/
> MERGECOHORTS/COPYCOHORT): expect a direction keyword - MOVE: AFTER
> (->K_MOVE_AFTER) or BEFORE (->K_MOVE_BEFORE); SWITCH/MERGECOHORTS:
> WITH; else TO or FROM (FROM sets RF_REVERSE). For COPYCOHORT
> non-reverse: optional AFTER/BEFORE flag. For MOVE/COPYCOHORT:
> optional WITHCHILD (->childset2, has_dep) or NOCHILD (childset2=0).
> Then parse dependency-target contexts: while `*p=='('` ->
> `parseContextualDependencyTests`; error if none collected; unless
> MERGECOHORTS, pop the last dep test into `rule->dep_target`.
> Grammar flags: SETPARENT/SETCHILD/SPLITCOHORT/MERGECOHORTS ->
> has_dep; relation keywords + MERGECOHORTS -> has_relations. COPYCOHORT
> with no BEFORE/AFTER -> RF_AFTER.
> Mark-jump detection: if not RF_REMEMBERX and any test (or dep_target)
> has POS_JUMP with jump_pos==JUMP_MARK -> set RF_REMEMBERX|RF_KEEPORDER.
> If key==K_WITH: set RF_KEEPORDER; if `{` follows, parse a
> `{ ...subrules... }` block - set in_nested_rule/nested_rule,
> repeatedly `maybeParseRule` (error if none) until `}`, restoring the
> nested state afterward.
> `rule->reverseContextualTests()`.
> Rule filtering (REGEX SITE): `destroy = only_sets`. If `nrules` (the
> --num/rule-name include regex) is set: `uregex_setText(nrules,
> rule->name, ...)` then `uregex_find(nrules, -1, &status)` - an
> UNANCHORED search over the rule name; if it does NOT match ->
> destroy=true. If `nrules_inv` (exclude regex) is set and it DOES
> match -> destroy=true.
> `SKIPWS` to `;`; warn (non-fatal) if not at `;`. If destroy ->
> `result->destroyRule(rule)`. Else `addRuleToGrammar(rule)` (plus
> profiler span and AST close with `rule->number+1`).

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-fn]
> Set* TextualParser::parseSet(const UChar* name, const UChar* p)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-fn]
> Thin wrapper: `return ::CG3::parseSet(name, p, *this);` - delegates
> to the free `parseSet` helper (parser_helpers), passing `*this` as
> the `State` so it uses this parser's grammar, `strict_tags`/
> `list_tags`, and error/addTag hooks. See
> parser-helpers.cg3.parse-set-fn for the full behavior.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-inline-fn]
> Set* TextualParser::parseSetInline(UChar*& p, Set* s)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-fn]
> Parse an inline set expression (operands joined by set operators,
> e.g. `A - (foo bar) | B`) into a Set. `s` may be a pre-allocated
> target Set to fill, else one is created. Local `set_ops` and `sets`
> vectors accumulate operator codes and operand hashes; `wantop`
> alternates between expecting an operand and expecting an operator.
> Loop while `*p` and not `;`/`)`: `SKIPWS`; if now at `;`/`)` stop. If
> `!wantop` (expecting an operand):
> - If `*p=='('`: a composite/inline tag set. If `no_isets` and the
>   char after `(` is not `*` -> error "Inline set spotted". Allocate
>   `set_c` with ST_ORDERED, auto-name it (`sets_counter++`). Read tags
>   until `)` (each MAYBE_QUOTED + SKIPTOWS(')') -> parseTag), require
>   `)`; if 0 tags -> error "Empty inline set ... use (*)"; if 1 tag
>   `addTagToSet`; else trie_insert into `trie_special` if any tag is
>   T_SPECIAL else `trie`. `addSet(set_c)`; push its hash into `sets`.
> - Else: read a set name up to whitespace/`)` (trimming trailing
>   `,`/`]`), `parseSet` it, push its hash.
> - Then, if the previous operator (`set_ops.back()`) is one of the
>   eager binary ops S_SET_DIFF / S_SET_ISECT_U / S_SET_SYMDIFF_U:
>   immediately MATERIALIZE it over the two most recent operand sets -
>   fetch both sets' tag-vector-sets and compute, respectively,
>   `set_difference` (in order b,a because order matters),
>   `set_intersection`, or `set_symmetric_difference`; pop the op and
>   both operands; build a new `set_c` from the result (frequency-
>   sorted, trie-inserted the same way as parseTagList), `addSet`, push
>   its hash. (These operators are evaluated eagerly here; `|`/`+`/`-`
>   are just stored for later evaluation.)
> - Set `wantop=true`.
> Else (`wantop`, expecting an operator): read the next token (special
> case: a `\` immediately followed by whitespace advances just one, so
> the `\` set-difference operator can be written `\ `); `ux_isSetOp` it;
> if valid (!=S_IGNORE) push it to `set_ops`, `wantop=false`, advance;
> else break out of the loop (end of expression). If at operand
> position with an empty token -> error "Expected set".
> After the loop: if no target `s` and `sets` empty -> error "Expected
> set". If no `s` and exactly one operand -> `s =
> result->getSet(sets.back())` (the operand itself, no wrapper). Else:
> allocate `s` if needed and `swap` `sets`->`s->sets` and
> `set_ops`->`s->set_ops`. Return `s`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-inline-wrapper-fn]
> Set* TextualParser::parseSetInlineWrapper(UChar*& p)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-wrapper-fn]
> Wrapper around `parseSetInline` guaranteeing the returned set is
> lined, named, and registered. Save `tmplines = result->lines`;
> `s = parseSetInline(p)` (no target, so it allocates or returns an
> existing set); if `s->line` is 0 set it to `tmplines`; if `s->name`
> is empty assign an auto name (`sets_counter++`); `result->addSet(s)`;
> return `s`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-tag-fn]
> Tag* TextualParser::parseTag(const UChar* to, const UChar* p)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-fn]
> Wrapper over the free `::CG3::parseTag(to, p, *this)` that adds
> varstring and strict-tag policy checks. First build the tag via the
> helper. Then: if the tag is T_VARSTRING but its text contains neither
> `{` nor `$` -> error "Varstring tag had no variables". Then, if
> `strict_tags` is non-empty and does NOT contain `tag->plain_hash`,
> enforce the allowlist with exceptions: always allow tags of type
> T_ANY/T_VARSTRING/T_VSTR/T_META/T_VARIABLE/T_LOCAL_VARIABLE/T_SET/
> T_PAR_LEFT/T_PAR_RIGHT/T_ENCL/T_TARGET/T_MARK/T_ATTACHTO/T_SAME_BASIC,
> and the literals `>>>`/`<<<` (STR_BEGINTAG/STR_ENDTAG); for
> T_REGEXP/T_REGEXP_ANY error only if `strict_regex`; T_CASE_INSENSITIVE
> only if `strict_icase`; T_WORDFORM only if `strict_wforms`;
> T_BASEFORM only if `strict_bforms`; a `<...>`-delimited secondary tag
> only if `strict_second`; otherwise error "Tag not on the strict-tags
> list". Return the tag.

> [spec:cg3:def:textual-parser.cg3.textual-parser.parse-tag-list-fn]
> void TextualParser::parseTagList(UChar*& p, Set* s, bool ordered)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-list-fn]
> Parse a LIST/DELIMITERS-style tag list into set `s`. `ordered`
> controls whether tags within a composite are sorted+uniq'd. Uses a
> `taglists` set (dedup of tag-vectors) and a `tag_freq` frequency map.
> Loop while `*p` and not `;`/`)`: `SKIPWS`; if a real token remains,
> build a `TagVector tags`:
> - If `*p=='('` (composite entry): `++p`, then loop reading tags until
>   `)`: each via MAYBE_QUOTED + SKIPTOWS(')') -> parseTag pushed into
>   `tags`; require closing `)` else error; `++p`.
> - Else a single tag: MAYBE_QUOTED + SKIPTOWS(0) -> parseTag pushed
>   into `tags`.
> - If not `ordered`: `std::sort(tags, compare_Tag())` then
>   `std::unique(..., equal_Tag())` to sort+dedupe. If this exact
>   tag-vector is newly inserted into `taglists`, increment each member
>   tag's `tag_freq`.
> After collecting all entries, build the trie via a `freq_sorter` over
> `tag_freq`: for each unique tag-vector `tvc` in `taglists`: if size 1,
> `result->addTagToSet(tvc[0], s)`. Else, if not `ordered`, sort the
> vector by descending frequency (freq_sorter) for cheap trie
> compression; determine `special` if any member has T_SPECIAL; then
> `trie_insert` into `s->trie_special` (special) or `s->trie`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.print-ast-fn]
> void TextualParser::print_ast(std::ostream& out)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.print-ast-fn]
> If `ast.cs` is empty, return (nothing captured, e.g. AST disabled).
> Otherwise emit an XML declaration and three comment lines documenting
> the node attributes (`l`=line; `b`/`e`=absolute UTF-16 code-unit
> offsets in the file, not code points; `u`=deduplicated object id),
> then recursively serialize the root node via the free
> `::print_ast(out, ast.cs.front().b, 0, ast.cs.front())`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.set-compatible-fn]
> void TextualParser::setCompatible(bool f)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.set-compatible-fn]
> Setter: `option_vislcg_compat = f;` - enables/disables vislcg
> legacy-compatibility mode (which later converts context POS_NOT into
> POS_NEGATE in parseContextualTests/parseContextualDependencyTests).

> [spec:cg3:def:textual-parser.cg3.textual-parser.set-verbosity-fn]
> void TextualParser::setVerbosity(uint32_t level)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.set-verbosity-fn]
> Setter: `verbosity_level = level;` - controls progress printing
> (every 500 lines) and the various non-fatal warnings emitted only
> when `verbosity_level > 0`.

> [spec:cg3:def:textual-parser.cg3.textual-parser.textual-parser-fn]
> TextualParser::TextualParser(Grammar& res, std::ostream& ux_err, bool _dump_ast)

> [spec:cg3:sem:textual-parser.cg3.textual-parser.textual-parser-fn]
> Constructor `TextualParser(Grammar& res, std::ostream& ux_err, bool
> _dump_ast)`. Forwards `res` and `ux_err` to the base
> `IGrammarParser(res, ux_err)` (storing the target grammar `result`
> and the error stream `ux_stderr`), then sets the inherited
> `parse_ast = _dump_ast` to enable/disable AST capture. All other
> members keep their in-class defaults.

