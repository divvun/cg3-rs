# src/GrammarWriter.cpp, src/GrammarWriter.hpp

> [spec:cg3:def:grammar-writer.cg3.grammar-writer]
> class GrammarWriter {
>   std::ostream* ux_stderr = nullptr;
>   uint32FlatHashSet used_sets;
>   uint32FlatHashSet seen_rules;
>   const Grammar* grammar = nullptr;
>   std::multimap<uint32_t, uint32_t> anchors;
> }

> [spec:cg3:def:grammar-writer.cg3.grammar-writer.grammar-writer-fn]
> GrammarWriter::GrammarWriter(Grammar& res, std::ostream& ux_err)

> [spec:cg3:sem:grammar-writer.cg3.grammar-writer.grammar-writer-fn]
> Constructor `GrammarWriter(Grammar& res, std::ostream& ux_err)`. Stores
> `ux_stderr = &ux_err` and `grammar = &res`. Then builds the `anchors` multimap
> by INVERTING `res.anchors` (a map of anchor-tag-hash → rule-number): for each
> pair `(first, second)` it inserts `(second, first)`, so the multimap is keyed
> by rule number with anchor-tag-hash values, which `printRule` later queries via
> `equal_range(rule.number)`. (The non-specced destructor simply sets `grammar`
> to null.)

> [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-contextual-test-fn]
> void GrammarWriter::printContextualTest(std::ostream& to, const ContextualTest& test)

> [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-contextual-test-fn]
> Emits a contextual test's position, target, barriers, and linked chain in CG-3
> syntax to `to`.
> If `pos & POS_NEGATE` print "NEGATE ". If `(pos & POS_TMPL_OVERRIDE)` OR the
> test has neither a template nor ORs (`!tmpl && ors.empty()`), print the
> position atom by testing pos bits in this order: POS_ALL→"ALL ", POS_NONE→
> "NONE ", POS_NOT→"NOT ", POS_ABSOLUTE→"@"; then POS_SCANALL→"**" else
> (POS_SCANFIRST|POS_DEP_DEEP)→"*"; POS_LEFTMOST→"ll", POS_LEFT→"l",
> POS_RIGHTMOST→"rr", POS_RIGHT→"r", POS_DEP_CHILD→"c"; for POS_DEP_PARENT print
> "p" then (if also POS_DEP_GLOB) another "p" (yielding "pp"), else if
> POS_DEP_GLOB print "cc"; POS_DEP_SIBLING→"s", POS_SELF→"S", POS_NO_BARRIER→"N".
> Then if POS_UNKNOWN print "?", else — only when none of DEP_CHILD/DEP_SIBLING/
> DEP_PARENT/DEP_GLOB/LEFT_PAR/RIGHT_PAR/RELATION/BAG_OF_TAGS are set — print the
> numeric `offset` as "%d". Then POS_CAREFUL→"C", POS_SPAN_BOTH→"W",
> POS_SPAN_LEFT→"<", POS_SPAN_RIGHT→">", POS_PASS_ORIGIN→"o", POS_NO_PASS_ORIGIN→
> "O", POS_LEFT_PAR→"L", POS_RIGHT_PAR→"R", POS_MARK_SET→"X". For POS_JUMP: "x"
> if jump_pos==JUMP_MARK, "jA" if JUMP_ATTACH, "jT" if JUMP_TARGET, else
> "jC<jump_pos>". Then POS_LOOK_DELETED→"D", POS_LOOK_DELAYED→"d", POS_ACTIVE→"T",
> POS_INACTIVE→"t", POS_LOOK_IGNORED→"I", POS_ATTACH_TO→"A", POS_WITH→"w",
> POS_BAG_OF_TAGS→"B". For POS_RELATION print "r:" then
> `printTag(single_tags[relation])`. If `offset_sub` nonzero print "/*" when it
> equals GSR_ANY else "/<offset_sub>". Finally print " ".
> Reference part: if `tmpl` set print "T:<tmpl->hash> "; else if `ors` non-empty,
> print each OR as "(" printContextualTest(or) ")" joined by " OR " between
> elements and a trailing " " after the last. If `target` set print
> "<sets_list[target]->name> ". If `cbarrier` set print "CBARRIER
> <sets_list[cbarrier]->name> ". If `barrier` set print "BARRIER
> <sets_list[barrier]->name> ". If `linked` set print "LINK " then recurse
> `printContextualTest(*linked)`.

> [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-rule-fn]
> void GrammarWriter::printRule(std::ostream& to, const Rule& rule)

> [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-rule-fn]
> Emits one rule in CG-3 text form to `to`; deduplicated via `seen_rules` (if
> `rule.number` already seen, return; else insert it).
> Anchors: for each `anchors.equal_range(rule.number)` entry, resolve the anchor
> tag text `single_tags[hash]->tag`; skip it if it equals `keywords[K_START]`,
> `keywords[K_END]`, or `rule.name`; otherwise print "ANCHOR <tag> ;\n".
> If `rule.wordform` set, `printTag(wordform)` then " ".
> Determine the display keyword: collapse MOVE_BEFORE/MOVE_AFTER→K_MOVE,
> ADDCOHORT_BEFORE/ADDCOHORT_AFTER→K_ADDCOHORT, EXTERNAL_ONCE/EXTERNAL_ALWAYS→
> K_EXTERNAL. Print `keywords[type]`. If `rule.name` is non-empty and does NOT
> start with "_R_" (auto-generated names, tested as name[0]=='_' && name[1]=='R'
> && name[2]=='_'), print ":<name>". Print " ".
> Flags: for i in [0,FLAGS_COUNT), skipping FL_BEFORE/FL_AFTER/FL_WITHCHILD, if
> `rule.flags & (1<<i)` print `g_flags[i]` then " " — except FL_SUB which prints
> "<flag>:<sub_reading> ". If `flags & RF_WITHCHILD` print "WITHCHILD
> <sets_list[childset1]->name> ".
> If type SUBSTITUTE or EXECUTE, print "<sublist->name> ". If `maplist` set,
> print "<maplist->name> ". If `sublist` set and type is one of
> ADDRELATIONS/SETRELATIONS/REMRELATIONS/SETVARIABLE/COPY/COPYCOHORT, print
> "EXCEPT " first when COPY/COPYCOHORT, then "<sublist->name> ".
> For ADD/MAP/SUBSTITUTE/COPY/COPYCOHORT: print "BEFORE " if RF_BEFORE, "AFTER "
> if RF_AFTER, and if `childset1` set print "WITHCHILD " (only for COPYCOHORT)
> then "<sets_list[childset1]->name> ". For ADDCOHORT_BEFORE print "BEFORE ", for
> ADDCOHORT_AFTER print "AFTER ".
> If `rule.target` set, print "<sets_list[target]->name> ". Then for each test in
> `rule.tests` print "(" printContextualTest ") ".
> Trailing keyword: "TO " for SETPARENT/SETCHILD/ADDRELATION(S)/SETRELATION(S)/
> REMRELATION(S)/COPYCOHORT; "AFTER " for MOVE_AFTER; "BEFORE " for MOVE_BEFORE;
> "WITH " for SWITCH/MERGECOHORTS.
> If `rule.dep_target` set: if `childset2` print "WITHCHILD
> <sets_list[childset2]->name> ", then "(" printContextualTest(dep_target) ") ".
> For each `dep_tests` entry print "(" printContextualTest ") ".
> If type K_WITH: print "{\n", then for each sub-rule "\t" printRule(sub) " ;\n",
> then "}\n". (Note: `rule.name[1]`/`name[2]` are read without a length guard.)

> [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-set-fn]
> void GrammarWriter::printSet(std::ostream& output, const Set& curset)

> [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-set-fn]
> Emits one set in CG-3 text form to `output`, recursively and deduplicated. If
> `curset.number` is already in `used_sets`, return.
> If `curset.sets` is empty (a leaf LIST): insert number into `used_sets`; if
> `type & ST_ORDERED` print "O"; print "LIST <name> = "; then for each of the two
> tries `[trie, trie_special]` obtain the ordered tag groups via
> `trie_getTagsOrdered` and for each group: if it has >1 tag print "(", print
> every tag with `printTag` each followed by a space, and if >1 print ") ";
> finally print " ;\n".
> Else (a composite SET): insert number into `used_sets`; recurse `printSet` on
> each referenced subset `grammar->sets_list[s]` (dependencies printed first).
> Then: if the name begins with "$$" or "&&" print "# " (comments the line out);
> if `type & ST_ORDERED` print "O"; print "SET <name> = "; print the first
> subset's name; then for i in [0, sets.size()-1) print "<op> <nextsetname> "
> using `stringbits[set_ops[i]]` and `sets_list[sets[i+1]]->name`; finish with
> " ;\n\n". (Note: `name[0]`/`name[1]` are read without a length guard; the LIST
> branch ends with a single "\n", the SET branch with two.)

> [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-tag-fn]
> void GrammarWriter::printTag(std::ostream& to, const Tag& tag)

> [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-tag-fn]
> Converts the tag to its CG-3 textual form via `tag.toUString(true)` (the `true`
> requests the escaped/round-trippable rendering) and prints it with "%S". No
> trailing space or separator is added here — callers add those.

> [spec:cg3:def:grammar-writer.cg3.grammar-writer.write-grammar-fn]
> int GrammarWriter::writeGrammar(std::ostream& output)

> [spec:cg3:sem:grammar-writer.cg3.grammar-writer.write-grammar-fn]
> Writes the whole grammar to `output` in CG-3 source text; returns 0. If
> `output` is falsy print "Output is null" + CG3Quit(1); if `grammar` is null
> print "No grammar provided" + CG3Quit(1).
> Preamble: print the "# DELIMITERS and SOFT-DELIMITERS do not exist..." note;
> "MAPPING-PREFIX = <char> ;" using `grammar->mapping_prefix`; "SUBREADINGS = LTR
> ;" if `sub_readings_ltr` else "SUBREADINGS = RTL ;"; if `cmdargs` non-empty
> "CMDARGS += <cmdargs> ;"; if `cmdargs_override` non-empty "CMDARGS-OVERRIDE +=
> <...> ;"; if `static_sets` non-empty "STATIC-SETS =" then " <name>" per entry
> then " ;"; if `preferred_targets` non-empty "PREFERRED-TARGETS = " then each tag
> via `printTag(single_tags[iter])` + " " then " ;"; if `parentheses` non-empty
> "PARENTHESES = " then for each pair "(" printTag(first) " " printTag(second)
> ") " then ";"; if `ordered` "OPTIONS += ordered ;"; if `addcohort_attach`
> "OPTIONS += addcohort-attach ;"; then a blank line.
> Set-naming pass: `used_sets.clear()`; for every set in `sets_list` whose name
> is empty, assign one: `STR_DELIMITSET` if it is `grammar->delimiters`,
> `STR_SOFTDELIMITSET` if `soft_delimiters`, `STR_TEXTDELIMITSET` if
> `text_delimiters`, otherwise "S<number>" (via u_sprintf into a 12-char buffer).
> Then if the resulting name `is_internal` (starts with "_G_") prepend "CG3"
> (inserts '3', then 'G', then 'C' at the front). Then print every set via
> `printSet(output, *s)` and a blank line.
> Templates: for each `(key, tmpl)` in `grammar->templates` print "TEMPLATE
> <tmpl->hash> = ", `printContextualTest(*tmpl)`, " ;".
> Rules by section, each section header printed once and only when a matching
> rule exists: "\nBEFORE-SECTIONS\n" for rules with `section == -1`; then for
> each section index in `grammar->sections` a "\nSECTION\n" header for rules with
> `section ==` that index; "\nAFTER-SECTIONS\n" for `section == -2`;
> "\nNULL-SECTION\n" for `section == -3`. Each matching rule is emitted with
> `printRule(output, r)` followed by " ;". Return 0.

