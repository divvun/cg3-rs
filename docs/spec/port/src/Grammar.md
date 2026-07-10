# src/Grammar.cpp, src/Grammar.hpp

> [spec:cg3:def:grammar.cg3.grammar]
> class Grammar {
>   std::ostream* ux_stderr = nullptr;
>   std::ostream* ux_stdout = nullptr;
>   bool has_dep = false;
>   bool has_bag_of_tags = false;
>   bool has_relations = false;
>   bool has_encl_final = false;
>   bool has_protect = false;
>   bool is_binary = false;
>   bool sub_readings_ltr = false;
>   bool ordered = false;
>   bool addcohort_attach = false;
>   size_t grammar_size = 0;
>   size_t num_tags = 0;
>   UChar mapping_prefix = '@';
>   uint32_t lines = 0;
>   uint32_t verbosity_level = 0;
>   mutable double total_time = 0;
>   std::string cmdargs;
>   std::string cmdargs_override;
>   std::vector<Tag*> single_tags_list;
>   Taguint32HashMap single_tags;
>   std::vector<Set*> sets_list;
>   SetSet sets_all;
>   uint32FlatHashMap sets_by_name;
>   set_name_seeds_t set_name_seeds;
>   Setuint32HashMap sets_by_contents;
>   uint32FlatHashMap set_alias;
>   SetSet maybe_used_sets;
>   static_sets_t static_sets;
>   regex_tags_t regex_tags;
>   icase_tags_t icase_tags;
>   contexts_t templates;
>   contexts_t contexts;
>   rules_by_set_t rules_by_set;
>   rules_by_tag_t rules_by_tag;
>   sets_by_tag_t sets_by_tag;
>   uint32IntervalVector* rules_any = nullptr;
>   boost::dynamic_bitset<>* sets_any = nullptr;
>   Set* delimiters = nullptr;
>   Set* soft_delimiters = nullptr;
>   Set* text_delimiters = nullptr;
>   uint32_t tag_any = 0;
>   uint32Vector preferred_targets;
>   uint32SortedVector reopen_mappings;
>   parentheses_t parentheses;
>   parentheses_t parentheses_reverse;
>   uint32Vector sections;
>   uint32FlatHashMap anchors;
>   RuleVector rule_by_number;
>   RuleVector before_sections;
>   RuleVector rules;
>   RuleVector after_sections;
>   RuleVector null_section;
>   RuleVector wf_rules;
> }

> [spec:cg3:def:grammar.cg3.grammar.add-anchor-fn]
> void Grammar::addAnchor(const UChar* to, uint32_t at, bool primary)

> [spec:cg3:sem:grammar.cg3.grammar.add-anchor-fn]
> Registers a named section anchor. Interns the anchor name `to` by calling
> allocateTag(to) and taking the resulting Tag's `hash` as the key `ah`. Looks
> `ah` up in the `anchors` map. If `primary` is true AND `ah` already exists,
> prints "Error: Redefinition attempt for anchor '<to>' on line <lines>!" to
> ux_stderr and calls CG3Quit(1). Next, if `at` > rule_by_number.size(), prints
> "Warning: No corresponding rule available for anchor '<to>' on line <lines>!"
> and clamps `at` to rule_by_number.size() (cast via UI32). Finally, only if the
> anchor did NOT already exist (it == anchors.end()), stores anchors[ah] = at.
> Quirks: a non-primary re-add of an existing anchor is silently ignored (the
> stored position is never overwritten); the clamp test uses strict `>`, so
> at == size() is left as-is (one past the last rule). Uses the grammar's
> current `lines` field for message line numbers.

> [spec:cg3:def:grammar.cg3.grammar.add-contextual-test-fn]
> ContextualTest* Grammar::addContextualTest(ContextualTest* t)

> [spec:cg3:sem:grammar.cg3.grammar.add-contextual-test-fn]
> Interns a ContextualTest into the `contexts` map, deduplicating structurally
> equal tests and returning the canonical pointer. If `t` is nullptr, returns
> nullptr immediately. Otherwise: recomputes t->hash via t->rehash(); recursively
> interns t->linked (replacing t->linked with the returned canonical pointer);
> and for each entry in t->ors, replaces it in place with its interned form. Then
> linear-probes seeds 0..999: candidate key = t->hash + seed, looked up in
> `contexts`. (a) If the key is absent, stores contexts[t->hash+seed] = t, then
> does t->hash += seed and t->seed = seed, optionally warns "Context on line
> <t->line> got hash seed <seed>" when verbosity_level > 1 && seed != 0, and
> breaks. (b) If the found entry is the same pointer `t`, breaks (already
> interned). (c) If the found entry compares equal via operator== (*t ==
> *cit->second), deletes `t`, sets t = cit->second (the canonical entry), and
> breaks. Returns `t`. Note: t->tmpl is NOT interned here (only linked and ors
> are); and if all 1000 seeds collide with distinct non-equal tests the loop
> exits without inserting, returning `t` un-interned.

> [spec:cg3:def:grammar.cg3.grammar.add-rule-fn]
> void Grammar::addRule(Rule* rule)

> [spec:cg3:sem:grammar.cg3.grammar.add-rule-fn]
> Appends a rule to the grammar's master list. Sets rule->number to the current
> rule_by_number.size() (i.e. the index the rule is about to occupy, cast via
> UI32), then pushes `rule` onto rule_by_number. Rules retain insertion order and
> `number` is the 0-based index into rule_by_number.

> [spec:cg3:def:grammar.cg3.grammar.add-set-fn]
> void Grammar::addSet(Set*& to)

> [spec:cg3:sem:grammar.cg3.grammar.add-set-fn]
> Registers a fully-built Set into the grammar, canonicalizing it by content and
> by name, and rewrites the in/out reference `to` to point at the canonical set.
> Steps:
> (1) Delimiter capture: if `delimiters` is null and to->name == STR_DELIMITSET
> ("_S_DELIMITERS_"), set delimiters = to; else-if `soft_delimiters` null and
> name == STR_SOFTDELIMITSET, set soft_delimiters = to; else-if `text_delimiters`
> null and name == STR_TEXTDELIMITSET, set text_delimiters = to.
> (2) If verbosity_level > 0 and name[0]=='T' && name[1]==':', warn that the set
> name looks like a mis-attempt of template usage.
> (3) SET->LIST folding: if to->sets is non-empty AND to->type has NONE of
> ST_TAG_UNIFY|ST_CHILD_UNIFY|ST_SET_UNIFY, test whether the set is just an
> OR-chain of single-tag lists: iterate the component sets; set all_tags=false
> and break if any join operator (set_ops[i-1]) is not S_OR, or a component has
> non-empty sets, or a component has BOTH trie and trie_special non-empty, or the
> component's getNonEmpty() trie does not have size 1 or is not trie_singular. If
> all_tags survives: for each component hash i, s=getSet(i); insert s into
> maybe_used_sets; tv = trie_getTagList(s->getNonEmpty()); if tv has 1 tag call
> addTagToSet(tv[0], to); else if any tag in tv is T_SPECIAL do
> trie_insert(to->trie_special, tv) else trie_insert(to->trie, tv). Then clear
> to->sets and to->set_ops, call to->reindex(*this), and if verbosity_level > 1
> and !is_internal(name) log "SET ... changed to a LIST".
> (4) Fail-fast splitting: if to->ff_tags is non-empty AND ff_tags.size() <
> (trie.size() + trie_special.size()) (i.e. failfast tags don't comprise the
> whole set), split into positive minus negative: allocate `positive` and
> `negative` sets, name them "_G_"+name+"_POSITIVE" and "_G_"+name+"_NEGATIVE".
> Swap to->trie into positive->trie and to->trie_special into
> positive->trie_special. For each ff tag: if it appears as a terminal node in
> positive->trie_special, delete its sub-trie (if any) and erase that entry; then
> make a copy `new Tag(*iter)`, clear its T_FAILFAST bit, addTag it, and
> addTagToSet it into `negative`. reindex both positive and negative, then
> addSet(positive) and addSet(negative). Clear to->ff_tags, push
> positive->hash and negative->hash onto to->sets and push S_MINUS onto
> to->set_ops, then to->reindex(*this). If verbosity_level > 1, log "LIST ... was
> split into two sets".
> (5) Name registration: chash = to->rehash(). Then a `for(;;)` block that always
> breaks after one pass (so it runs at most once) unless is_internal(name) is true
> (in which case the block is skipped entirely): compute nhash =
> hash_value(name). If sets_by_name has nhash and the set it resolves to (via
> sets_by_contents[sets_by_name[nhash]]) is `to` or shares to->hash, break. If
> set_name_seeds has `name`, add its seed to nhash. If sets_by_name lacks nhash,
> store sets_by_name[nhash] = chash and break. Else if chash differs from the
> existing set's hash (real name+content clash): let `a` be the existing set; if
> a->name == to->name, print "Error: Set ... already defined at line ...
> Redefinition attempted at line ...!" and CG3Quit(1); otherwise search seeds
> 0..999 for a free nhash+seed, optionally warn "Set ... got hash seed ..."
> (verbosity_level>0 && !is_internal), record set_name_seeds[name]=seed and
> sets_by_name[nhash+seed]=chash. Then break.
> (6) Content registration: if sets_by_contents lacks chash, store
> sets_by_contents[chash] = to. Otherwise let `a` = sets_by_contents[chash]; if
> a != to, reindex both a and to, and if their masked types
> (ST_SPECIAL|ST_TAG_UNIFY|ST_CHILD_UNIFY|ST_SET_UNIFY) differ, or their set_ops
> sizes, sets sizes, trie sizes, or trie_special sizes differ, print "Error:
> Content hash collision between set ...!" and CG3Quit(1); else destroySet(to)
> (the incoming set is a duplicate). Finally set `to = sets_by_contents[chash]`,
> making the caller's pointer refer to the canonical set.

> [spec:cg3:def:grammar.cg3.grammar.add-set-to-list-fn]
> void Grammar::addSetToList(Set* s)

> [spec:cg3:sem:grammar.cg3.grammar.add-set-to-list-fn]
> Depth-first appends a set (and its component sets) to sets_list, assigning each
> its `number` = its index in sets_list. Acts only if s->number == 0 (not yet
> numbered) AND s is not already sets_list[0] (guard: skip when sets_list is
> non-empty and its first element is s — protects the reserved dummy slot 0). If
> the guard passes and s->sets is non-empty, first recurse addSetToList on each
> component (resolved via getSet(hash)) so children receive lower numbers. Then
> push s onto sets_list and set s->number = UI32(sets_list.size()-1). Note: any
> already-numbered set (number != 0) is skipped, and a set legitimately occupying
> index 0 is never re-added.

> [spec:cg3:def:grammar.cg3.grammar.add-tag-fn]
> Tag* Grammar::addTag(Tag* tag)

> [spec:cg3:sem:grammar.cg3.grammar.add-tag-fn]
> Interns a Tag into single_tags/single_tags_list, deduplicating by hash and
> text, and returns the canonical Tag pointer. Computes hash = tag->rehash().
> Linear-probes seeds 0..9999: ih = hash + seed. (a) If single_tags contains ih:
> let t = the stored tag. If t is the same pointer as `tag`, return `tag` (already
> registered). If t->tag (the text) equals tag->tag, this is a duplicate parked at
> a seeded slot — set hash += seed, delete the incoming `tag`, and break (so the
> existing tag is returned). Otherwise (hash collision, different text) continue
> probing. (b) If ih is free: optionally warn "Tag ... got hash seed <seed>" when
> verbosity_level > 0 && seed != 0; set tag->seed = seed; recompute hash =
> tag->rehash() (rehash adds `seed`, yielding base+seed == ih); push tag onto
> single_tags_list; set tag->number = UI32(single_tags_list.size()-1); store
> single_tags[hash] = tag; break. Returns single_tags[hash]. Because rehash folds
> seed into the hash, the final `hash` matches the probed slot both when inserting
> and when deduplicating.

> [spec:cg3:def:grammar.cg3.grammar.add-tag-to-set-fn]
> void Grammar::addTagToSet(Tag* rtag, Set* set)

> [spec:cg3:sem:grammar.cg3.grammar.add-tag-to-set-fn]
> Adds a single tag to `set` as a length-1 trie path and updates set type flags.
> If rtag is T_ANY, OR ST_ANY into set->type. If rtag is T_FAILFAST, insert rtag
> into set->ff_tags. If rtag is T_SPECIAL, OR ST_SPECIAL into set->type and create
> the trie node set->trie_special[rtag] with terminal = true; otherwise create
> set->trie[rtag] with terminal = true. Note: because failfast tags are also
> T_SPECIAL (T_FAILFAST is in MASK_TAG_SPECIAL), a failfast tag is added to BOTH
> ff_tags and trie_special.

> [spec:cg3:def:grammar.cg3.grammar.add-template-fn]
> void Grammar::addTemplate(ContextualTest* test, const UChar* name)

> [spec:cg3:sem:grammar.cg3.grammar.add-template-fn]
> Registers a named template (TEMPLATE definition). cn = hash_value(name). If
> `templates` already contains cn, print "Error: Redefinition attempt for template
> '<name>' on line <lines>!" and CG3Quit(1). Otherwise store templates[cn] = test.
> Quirk: keyed purely by name hash with no seed/collision handling, so a genuine
> hash collision between two distinct template names would be misreported as a
> redefinition.

> [spec:cg3:def:grammar.cg3.grammar.allocate-contextual-test-fn]
> ContextualTest* Grammar::allocateContextualTest()

> [spec:cg3:sem:grammar.cg3.grammar.allocate-contextual-test-fn]
> Allocates and returns a fresh default-constructed ContextualTest on the heap
> (`new ContextualTest`). It is not registered anywhere; the caller is responsible
> for later interning it via addContextualTest (or freeing it).

> [spec:cg3:def:grammar.cg3.grammar.allocate-dummy-set-fn]
> void Grammar::allocateDummySet()

> [spec:cg3:sem:grammar.cg3.grammar.allocate-dummy-set-fn]
> Creates the reserved dummy set that occupies sets_list index 0. Allocates a set
> (allocateSet), sets line = 0, names it STR_DUMMY ("__CG3_DUMMY_STRINGBIT__"),
> allocates a tag with that same text (allocateTag(STR_DUMMY)) and adds it via
> addTagToSet, then registers the set with addSet. Sets set_c->number to
> std::numeric_limits<uint32_t>::max() (the sentinel that reindex treats as
> always-used and never renumbered), and inserts set_c at the FRONT of sets_list
> (sets_list.begin()). Called once at grammar setup so real user sets are numbered
> starting at index 1.

> [spec:cg3:def:grammar.cg3.grammar.allocate-rule-fn]
> Rule* Grammar::allocateRule()

> [spec:cg3:sem:grammar.cg3.grammar.allocate-rule-fn]
> Allocates and returns a fresh default-constructed Rule on the heap (`new Rule`).
> It is not added to rule_by_number here; registration is done separately by
> addRule.

> [spec:cg3:def:grammar.cg3.grammar.allocate-set-fn]
> Set* Grammar::allocateSet()

> [spec:cg3:sem:grammar.cg3.grammar.allocate-set-fn]
> Allocates a fresh default-constructed Set (`new Set`), inserts its pointer into
> the `sets_all` ownership registry (a sorted_vector<Set*>), and returns it.
> sets_all tracks every allocated set for eventual teardown regardless of whether
> it ends up in sets_list.

> [spec:cg3:def:grammar.cg3.grammar.allocate-tag-fn]
> Tag* Grammar::allocateTag(const UChar* txt)

> [spec:cg3:sem:grammar.cg3.grammar.allocate-tag-fn]
> Interns a tag from raw text `txt`, returning the canonical Tag pointer. Error
> checks first: if txt[0] == 0 (empty), print "Error: Empty tag on line <lines>!
> Forgot to fill in a ()?" and CG3Quit(1); if txt[0] == '(', print "Error: Tag
> '<txt>' cannot start with ( ..." and CG3Quit(1). Fast path: thash =
> hash_value(txt); if single_tags[thash] exists AND its stored tag's text is
> non-empty AND equals txt, return that existing Tag directly (skips re-parsing).
> This fast path only checks the un-seeded slot. Otherwise allocate `new Tag()`,
> call tag->parseTagRaw(txt, this) to populate its type/flags/regexp/etc., and
> return addTag(tag) to intern it (which handles seeding and full dedup). Uses
> `lines` in the error messages.

> [spec:cg3:def:grammar.cg3.grammar.append-to-set-fn]
> void Grammar::appendToSet(Set*& to)

> [spec:cg3:sem:grammar.cg3.grammar.append-to-set-fn]
> Implements set append (LIST `+=`): extends the already-defined set named
> to->name with the new content carried in `to`, rewriting `to` to the merged
> result. Steps:
> (1) tset = undefSet(to->name) — pulls the currently-registered set of that name
> out of the name index (renaming/unregistering it). Precondition: that set must
> exist; if undefSet returns nullptr, the following addSet(tset) dereferences null
> (crash).
> (2) addSet(tset) — re-registers the pulled-out old set under fresh keys.
> (3) If tset->sets is non-empty (tset is a composite SET): fset =
> getSet(tset->sets[0]). If fset is NOT a generated positive half (name does not
> start with STR_GPREFIX "_G_", OR does not contain STR_POSITIVE "POSITIVE"),
> wrap-in-OR: allocate `ns`, name it to->name and copy to->line; rename `to` to an
> internal numeric name UI32(sets_by_contents.size()+1) and addSet(to); then set
> ns->sets = [tset->hash, to->hash] with set_ops = [S_OR]; assign to = ns (so the
> caller's set becomes "old OR new"). Otherwise (tset IS a positive-minus-negative
> split): copy the positive half back into `to` — trie_getTags(positive->trie) and
> trie_getTags(positive->trie_special) inserted into to->trie/to->trie_special;
> then for the negative half tset->sets[1], flatten both its tries via
> trie_getTagList, and for each tag make a copy Tag(*t), OR in T_FAILFAST, addTag
> it, and addTagToSet it into `to` (re-establishing failfast tags so a later
> addSet re-splits it).
> (4) Else (tset is a plain LIST): merge tset->trie tags into to->trie and
> tset->trie_special tags into to->trie_special (via trie_getTags + trie_insert),
> and insert tset->ff_tags into to->ff_tags.
> (5) addSet(to) — register the merged/renamed set.
> (6) Delimiter capture: if to->name equals STR_DELIMITSET / STR_SOFTDELIMITSET /
> STR_TEXTDELIMITSET, set delimiters / soft_delimiters / text_delimiters = to
> (unconditionally overwriting, unlike addSet which only sets when null).

> [spec:cg3:def:grammar.cg3.grammar.context-adjust-target-fn]
> void Grammar::contextAdjustTarget(ContextualTest* test)

> [spec:cg3:sem:grammar.cg3.grammar.context-adjust-target-fn]
> Rewrites a ContextualTest's set references from content-hash keys to sets_list
> index numbers, recursively, exactly once per test. Guard: if !test->is_used,
> return immediately (skips tests not marked used AND prevents double-processing).
> Sets test->is_used = false (consuming the flag that markUsed set during
> reindex). For each of test->target, test->barrier, test->cbarrier that is
> non-zero, look up the Set in sets_by_contents by that content hash and replace
> the field with set->number. Then recurse contextAdjustTarget over every entry in
> test->ors, over test->tmpl (if set), and over test->linked (if set). Relies on
> sets_by_contents still being populated (reindex calls this before clearing it).
> Note: `is_used` doubles as a visited marker here.

> [spec:cg3:def:grammar.cg3.grammar.contexts-t]
> typedef std::unordered_map<uint32_t, ContextualTest*> contexts_t

> [spec:cg3:def:grammar.cg3.grammar.destroy-rule-fn]
> void Grammar::destroyRule(Rule* rule)

> [spec:cg3:sem:grammar.cg3.grammar.destroy-rule-fn]
> Frees a rule (`delete rule`). Does not remove it from rule_by_number or any
> other container; caller must ensure no dangling references remain.

> [spec:cg3:def:grammar.cg3.grammar.destroy-set-fn]
> void Grammar::destroySet(Set* set)

> [spec:cg3:sem:grammar.cg3.grammar.destroy-set-fn]
> Removes `set` from the sets_all registry (sets_all.erase(set)) and then deletes
> it (`delete set`). Erasing from sets_all first prevents a later double-free in
> the destructor's sets_all sweep.

> [spec:cg3:def:grammar.cg3.grammar.destroy-tag-fn]
> void Grammar::destroyTag(Tag* tag)

> [spec:cg3:sem:grammar.cg3.grammar.destroy-tag-fn]
> Frees a tag (`delete tag`). Does not unregister it from single_tags or
> single_tags_list; caller is responsible for consistency.

> [spec:cg3:def:grammar.cg3.grammar.get-set-fn]
> Set* Grammar::getSet(uint32_t which) const

> [spec:cg3:sem:grammar.cg3.grammar.get-set-fn]
> Resolves a Set* from `which`, which may be either a content hash or a name hash.
> First tries sets_by_contents.find(which); if found, return that set directly.
> Otherwise treat `which` as a name hash: look up sets_by_name[which] to get a
> content hash chash; look chash up in sets_by_contents to get the candidate set.
> If that candidate set's name has a seed registered in set_name_seeds, recurse as
> getSet(chash + seed) (i.e. re-resolve with the seed folded in). Otherwise return
> the candidate. If nothing resolves at any step, return nullptr (0). Note: the
> recursion re-invokes getSet with (the sets_by_name value) + (the found set's
> name seed), so the same argument is interpreted first as a content hash and then
> as a name hash across recursive calls; this is how seeded name-collision entries
> get resolved.

> [spec:cg3:def:grammar.cg3.grammar.get-tag-list-any-fn]
> void Grammar::getTagList_Any(const Set& theSet, TagList& theTags) const

> [spec:cg3:sem:grammar.cg3.grammar.get-tag-list-any-fn]
> Collects all tags of a set into `theTags` (a TagList/vector, appended in place).
> Three cases: (a) if theSet is a unify set (type has ST_SET_UNIFY | ST_TAG_UNIFY),
> CLEAR theTags and push the single "any" tag single_tags[tag_any] — note the
> clear discards anything accumulated so far by callers/recursion; (b) else if
> theSet.sets is non-empty, recurse over each component via
> sets_list[iter] — treating theSet.sets entries as set NUMBERS (indices), so this
> variant is for post-reindex use; (c) else flatten this set's own tries via
> trie_getTagList(theSet.trie, ...) then trie_getTagList(theSet.trie_special, ...),
> which append EVERY tag key at every trie depth including non-terminal
> intermediate nodes. The sibling one-arg overload getTagList_Any(theSet) simply
> creates an empty TagList and delegates here. Contrast getTags, which resolves
> component sets by content hash via getSet rather than by number.

> [spec:cg3:def:grammar.cg3.grammar.get-tags-fn]
> void Grammar::getTags(const Set& set, TagVectorSet& rv) const

> [spec:cg3:sem:grammar.cg3.grammar.get-tags-fn]
> Collects the set's tag combinations into `rv` (a TagVectorSet = set of sorted
> TagVector paths). First recurses over each component set (resolved via getSet by
> content hash) accumulating into the same rv — note the source ToDo comment
> "getTags() ought to account for other operators than OR": all set operators are
> treated as union, other operators (minus, etc.) are ignored. Then appends this
> set's own trie paths using a reused TagVector buffer `tv`: trie_getTags(set.trie,
> rv, tv), then tv.clear(), then trie_getTags(set.trie_special, rv, tv). Each
> terminal node contributes its accumulated (sorted-by-hash) tag path to rv.

> [spec:cg3:def:grammar.cg3.grammar.grammar-fn]
> Grammar::~Grammar()

> [spec:cg3:sem:grammar.cg3.grammar.grammar-fn]
> Destructor. Frees all owned objects in order: (1) for each set in sets_list call
> destroySet (which also erases it from sets_all); (2) for each remaining set in
> sets_all, `delete` it (catches sets that were allocated but never placed in
> sets_list — the sets_list sweep already removed the ones it handled, so there is
> no double free); (3) delete every Tag value in single_tags; (4) delete every
> Rule in rule_by_number; (5) delete every ContextualTest value in `contexts`.
> Note: `templates` are NOT explicitly deleted here — used templates that were
> retained during reindex (and are not also owned via `contexts`) can leak; and
> single_tags_list is not iterated (single_tags owns the tag pointers).

> [spec:cg3:def:grammar.cg3.grammar.icase-tags-t]
> typedef TagSortedVector icase_tags_t

> [spec:cg3:def:grammar.cg3.grammar.index-set-to-rule-fn]
> void Grammar::indexSetToRule(uint32_t r, Set* s)

> [spec:cg3:sem:grammar.cg3.grammar.index-set-to-rule-fn]
> Populates rules_by_tag for a rule: records which tags can trigger rule number
> `r` through target set `s`. If s->type has ST_SPECIAL or ST_TAG_UNIFY, also
> index tag_any -> r via indexTagToRule(tag_any, r) (so the rule is reachable for
> any tag). Then walk both tries with trie_indexToRule(s->trie, ...) and
> trie_indexToRule(s->trie_special, ...) — each maps every tag hash at every trie
> node to r. Finally recurse over each component set: set = sets_list[i] (s->sets
> entries are set numbers post-reindex) and indexSetToRule(r, set). Unlike
> indexSets, this continues descending into children even for special sets.

> [spec:cg3:def:grammar.cg3.grammar.index-sets-fn]
> void Grammar::indexSets(uint32_t r, Set* s)

> [spec:cg3:sem:grammar.cg3.grammar.index-sets-fn]
> Populates sets_by_tag: maps each tag hash to the bitset of set numbers whose set
> contains that tag. For set number `r` and set `s`: if s->type has ST_SPECIAL or
> ST_TAG_UNIFY, index tag_any -> r via indexTagToSet(tag_any, r) and RETURN
> immediately (does NOT descend into tries or children — a key difference from
> indexSetToRule, which continues). Otherwise walk both tries with
> trie_indexToSet(s->trie, ...) and trie_indexToSet(s->trie_special, ...) — setting
> bit r in sets_by_tag[taghash] for every tag — then recurse over each component
> set = sets_list[i] (numbers) with indexSets(r, set).

> [spec:cg3:def:grammar.cg3.grammar.index-tag-to-rule-fn]
> void Grammar::indexTagToRule(uint32_t t, uint32_t r)

> [spec:cg3:sem:grammar.cg3.grammar.index-tag-to-rule-fn]
> Inserts rule number `r` into the uint32IntervalVector rules_by_tag[t] (creating
> the entry if absent). `t` is a tag hash (or tag_any). The interval-vector keeps
> the rule numbers compactly and de-duplicated/sorted.

> [spec:cg3:def:grammar.cg3.grammar.index-tag-to-set-fn]
> void Grammar::indexTagToSet(uint32_t t, uint32_t r)

> [spec:cg3:sem:grammar.cg3.grammar.index-tag-to-set-fn]
> Sets bit `r` (a set number) in the boost::dynamic_bitset sets_by_tag[t]. If the
> entry for tag hash `t` does not exist yet, first create it and resize it to
> sets_list.size() bits, then set bit r. Assumes r < sets_list.size() (no bounds
> growth beyond the initial resize).

> [spec:cg3:def:grammar.cg3.grammar.parentheses-t]
> typedef bc::flat_map<uint32_t, uint32_t> parentheses_t

> [spec:cg3:def:grammar.cg3.grammar.regex-tags-t]
> typedef std::set<URegularExpression*> regex_tags_t

> [spec:cg3:def:grammar.cg3.grammar.reindex-fn]
> void Grammar::reindex(bool unused_sets, bool used_tags)

> [spec:cg3:sem:grammar.cg3.grammar.reindex-fn]
> The core finalization pass: after parsing (or binary load), it marks used
> sets/tags/contexts, numbers the sets, builds all runtime indexes, and rewrites
> hash-based references into number-based ones. Two optional flags trigger
> diagnostic behavior. Steps, in source order:
> (1) Reset set state: for every set in sets_by_contents, if its number ==
> uint32_max (the dummy sentinel) OR in ST_USED and skip it; else clear ST_USED
> (unless the set is ST_STATIC) and set number = 0.
> (2) Static sets: for each name `sset` in static_sets: sh = hash_value(sset); if
> set_alias has sh, error "Static set ... is an alias; only real sets may be made
> static!" and CG3Quit(1). s = getSet(sh); if null, warn (verbosity_level>0) "Set
> ... was not defined, so cannot make it static." and continue. If s->name !=
> sset, rename it. s->markUsed(*this); OR in ST_STATIC.
> (3) Clear/reset containers: set_alias, sets_by_name, rules, before_sections,
> after_sections, null_section, sections all cleared. If !is_binary, resize
> sets_list to 1 (keep only the dummy) and set sets_list[0]->number = 0. Clear
> set_name_seeds. sets_any = nullptr, rules_any = nullptr.
> (4) Populate regex_tags / icase_tags from single_tags_list: for each tag, if
> tag->regexp is non-null AND !is_textual(tag->tag), insert tag->regexp into
> regex_tags (a std::set<URegularExpression*>); if (type & T_CASE_INSENSITIVE) AND
> !is_textual(tag->tag), insert the tag into icase_tags. If is_binary, continue.
> Else if the tag has vs_sets, markUsed each set in *vs_sets. (is_textual means the
> text is a "..." or <...> literal — such literals are excluded from regex/icase
> matching.)
> (5) Propagate T_TEXTUAL: for each tag in single_tags_list not already T_TEXTUAL:
> for each regex in regex_tags, uregex_setText(regex, tag->tag) and if status OK
> and uregex_find(regex, -1, &status) returns true, OR in T_TEXTUAL; for each
> icase tag, if ux_strCaseCompare(tag->tag, icaseTag->tag) is true, OR in
> T_TEXTUAL. (uregex_find with startIndex -1 is an UNANCHORED search — a match
> anywhere in the tag text suffices; the Rust `regex` port must use find/search
> semantics, not full-match.)
> (6) Mark parenthesis and preferred-target tags used: for each (a,b) in
> `parentheses`, markUsed single_tags[a] and single_tags[b]; for each hash in
> preferred_targets, markUsed single_tags[it].
> (7) Rule pre-pass: for each rule in rule_by_number: if rule->wordform, push onto
> wf_rules; if rule->type == K_PROTECT, set has_protect = true. If is_binary,
> continue. Else markUsed the target set (getSet(rule->target)), and, when
> present, childset1, childset2, maplist, sublist, dep_target, every test in
> rule->tests, and every test in rule->dep_tests.
> (8) If !is_binary: markUsed delimiters/soft_delimiters/text_delimiters (each if
> non-null). Filter templates: keep only entries whose ContextualTest->is_used is
> true (build `tosave`, then swap). Filter contexts: keep is_used entries, delete
> the ContextualTest of every non-used entry, then swap.
> (9) If the `unused_sets` flag is set: print "Unused sets:" to ux_stdout, then for
> every set in sets_by_contents that is !ST_USED with a non-empty name, not in
> maybe_used_sets, and not is_internal, print "Line <line> set <name>"; finish with
> "End of unused sets." (diagnostic only; does not stop).
> (10) Build sets_list: for each set in sets_by_contents that is ST_USED, call
> addSetToList (depth-first numbering).
> (11) Recompute mapping flag: for each tag in single_tags, if tag->tag[0] ==
> mapping_prefix (default '@'), OR in T_MAPPING else clear T_MAPPING.
> (12) If !is_binary: reindex every set in sets_list (Set::reindex recomputes
> ST_SPECIAL / unify / mapping from tries and children), then setAdjustSets every
> set (rewrites their sets[] from content hashes to numbers). Then ALWAYS (binary
> too): for each set in sets_list call indexSets(set->number, set) to build
> sets_by_tag.
> (13) Rule finalization: create a uint32SortedVector `sects`. For each rule in
> rule_by_number: dispatch by rule->section — -1 -> before_sections, -2 ->
> after_sections, -3 -> null_section, else insert section into `sects` and push
> the rule onto `rules`. If rule->target: obtain the target Set — if is_binary,
> set = sets_list[target] (already a number); else set =
> sets_by_contents[target] and rewrite rule->target = set->number. Call
> indexSetToRule(rule->number, set) and rules_by_set[rule->target].insert(
> rule->number). If no target, warn "Rule on line ... had no target." If (maplist
> is ST_CHILD_UNIFY) or (sublist is ST_CHILD_UNIFY), OR RF_CAPTURE_UNIF into
> rule->flags. If is_binary, continue. Else rewrite childset1 and childset2 from
> content hash to number (sets_by_contents lookup); if dep_target, contextAdjust-
> Target it; contextAdjustTarget every test in tests and in dep_tests.
> (14) If `sects` is non-empty, fill sections with a contiguous 0..sects.back()
> inclusive (gaps between section numbers are filled in).
> (15) Cache any-tag indexes: if sets_by_tag has tag_any, sets_any =
> &sets_by_tag[tag_any]; if rules_by_tag has tag_any, rules_any =
> &rules_by_tag[tag_any].
> (16) Clear sets_by_contents (sets are henceforth referenced only by number via
> sets_list).
> (17) Re-register static-set names by NUMBER: for each set in sets_list with
> ST_STATIC: nhash = hash_value(name), cnum = number. If sets_by_name lacks nhash,
> sets_by_name[nhash] = cnum. Else if the existing entry's set number differs: if
> the existing set's name equals this name, error "Static set ... already defined.
> Redefinition attempted!" and CG3Quit(1); otherwise probe seeds 0..999 for a free
> nhash+seed, optionally warn (verbosity_level>0), record set_name_seeds[name]=seed
> and sets_by_name[nhash+seed] = cnum. (Now sets_by_name maps name-hash -> set
> number, whereas during parsing it mapped name-hash -> content hash.)
> (18) Compute `sets_vstr` (dynamic_bitset over sets_list): iterate to fixpoint —
> for each unset set, set its bit if any component set (by number) already has its
> bit set, or if trie_hasType(trie, T_VARSTRING) or trie_hasType(trie_special,
> T_VARSTRING). Marks sets that transitively contain varstring tags.
> (19) Compute `nk` (a flat_set of ContextualTest* that use unification or
> varstrings): iterate to fixpoint over `contexts` — add a test t to nk if any of:
> t->tmpl is in nk; t->linked is in nk; t->target refers to a set with
> MASK_ST_UNIFY (sets_list[target]->type) or sets_vstr[target]; likewise for
> t->barrier or t->cbarrier. (Each check `continue`s after adding.)
> (20) Auto-KEEPORDER: for each rule not already RF_KEEPORDER, set needs=true if
> (sublist && sets_vstr[sublist->number]) or (maplist && sets_vstr[maplist->
> number]) or (dep_target in nk) or any test in tests is in nk or any dep_test is
> in nk; if needs, OR RF_KEEPORDER into rule->flags.
> (21) If the `used_tags` flag is set: for each tag in single_tags with T_USED,
> print tag->toUString(true) to ux_stdout, then call exit(0) — terminating the
> whole process (diagnostic dump mode).

> [spec:cg3:def:grammar.cg3.grammar.remove-numeric-tags-fn]
> uint32_t Grammar::removeNumericTags(uint32_t s)

> [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn]
> Returns the hash of a variant of set `s` with all T_NUMERICAL tags removed,
> building a new "_G_<name>_B_" set only if something was actually removed (this
> is the "C branch" set used where numeric comparisons must be dropped). set =
> getSet(s).
> Composite case (set->sets non-empty): copy set->sets into `sets`; for each child
> hash i: ns = removeNumericTags(i). If ns == 0, that child became empty — print
> "Error: Removing numeric tags for branch resulted in set <name> on line <line>
> being empty!" and CG3Quit(1). If ns != i, write ns back into the copy and mark
> did=true. If did, allocate a new set: copy type and line, name = STR_GPREFIX +
> set->name + u"_B_", sets = the modified copy, set_ops = set->set_ops; addSet it;
> set = it.
> Leaf case (no component sets): iterate tries {trie, trie_special} (skipping
> empty ones): ctags = trie_getTags(trie); for each tag path, clear a `tags`
> buffer and call fill_tagvector(path, tags, did, special) — which copies only the
> non-numeric tags, sets did=true if any numeric tag was skipped, and special=true
> if any copied tag is T_SPECIAL; if `tags` is non-empty, record ntags[tags] =
> special (a std::map<TagVector,bool>). Also process set->ff_tags the same way. If
> did: if ntags ended up empty, replace with the "*" any tag — push
> single_tags[tag_any] as a single path with ntags[tags]=true, and if
> verbosity_level>0 warn "Set <name> was empty and replaced with the * set in the
> C branch on line <line>." Allocate a new set (copy type/line, name
> STR_GPREFIX+name+u"_B_"); for each (tagvec, special) in ntags: if special and
> tagvec is a single T_FAILFAST tag, insert into ns->ff_tags; else if special,
> trie_insert(ns->trie_special, tagvec); else trie_insert(ns->trie, tagvec).
> addSet it; set = it.
> Returns set->hash (unchanged input set's hash when nothing was removed, i.e.
> did==false).

> [spec:cg3:def:grammar.cg3.grammar.rules-by-set-t]
> typedef std::unordered_map<uint32_t, uint32IntervalVector> rules_by_set_t

> [spec:cg3:def:grammar.cg3.grammar.rules-by-tag-t]
> typedef std::unordered_map<uint32_t, uint32IntervalVector> rules_by_tag_t

> [spec:cg3:def:grammar.cg3.grammar.set-adjust-sets-fn]
> void Grammar::setAdjustSets(Set* s)

> [spec:cg3:sem:grammar.cg3.grammar.set-adjust-sets-fn]
> Rewrites a set's `sets` vector from content hashes to set numbers, recursively,
> once per set. Guard: if !(s->type & ST_USED) return (skips unused sets AND
> prevents revisiting, since the flag is cleared on entry). Clear ST_USED from
> s->type. For each entry i in s->sets (by reference): look up the Set by content
> hash in sets_by_contents (sets_by_contents.find(i)->second), replace i with
> set->number, and recurse setAdjustSets(set). ST_USED here is consumed as a
> visited marker (it was set by markUsed earlier in reindex).

> [spec:cg3:def:grammar.cg3.grammar.set-name-seeds-t]
> typedef std::unordered_map<UString, uint32_t, hash_ustring> set_name_seeds_t

> [spec:cg3:def:grammar.cg3.grammar.sets-by-tag-t]
> typedef std::unordered_map<uint32_t, boost::dynamic_bitset<>> sets_by_tag_t

> [spec:cg3:def:grammar.cg3.grammar.static-sets-t]
> typedef std::vector<UString> static_sets_t

> [spec:cg3:def:grammar.cg3.grammar.undef-set-fn]
> Set* Grammar::undefSet(const UString& _name)

> [spec:cg3:sem:grammar.cg3.grammar.undef-set-fn]
> Removes the set(s) named `_name` (and its unification-prefixed variants) from
> the name index and returns the plain-named one. Iterates three prefixes in order
> {u"$$", u"&&", u""}: for each, builds name = prefix + _name; nhash =
> hash_value(name); tset = getSet(nhash). If tset is found, rename it to an
> internal numeric name via tset->setName(UI32(sets_by_contents.size())) (so it no
> longer collides by name). Then if set_name_seeds has `name`, add its seed to
> nhash and erase that seed entry. Then if sets_by_name has nhash, erase it.
> Returns `tset` — but because tset is reassigned by getSet each iteration, the
> return value is whatever the LAST prefix ("") resolved to: the plain-named set,
> or nullptr if no plain-named set exists. Quirk: the "$$" and "&&" variants are
> still renamed and unregistered as a side effect even though they are never
> returned, and if only a prefixed variant exists this returns nullptr while
> having mutated that variant.

> [spec:cg3:def:grammar.cg3.trie-index-to-rule-fn]
> inline void trie_indexToRule(const trie_t& trie, Grammar& grammar, uint32_t r)

> [spec:cg3:sem:grammar.cg3.trie-index-to-rule-fn]
> Free function. Recursively walks `trie`; for every (tag, node) entry at every
> depth, calls grammar.indexTagToRule(tag->hash, r), then descends into
> node.trie (if present). Effect: every tag appearing anywhere in the trie —
> including intermediate nodes of multi-tag paths, not just terminals — is mapped
> to rule number `r` in rules_by_tag.

> [spec:cg3:def:grammar.cg3.trie-index-to-set-fn]
> inline void trie_indexToSet(const trie_t& trie, Grammar& grammar, uint32_t r)

> [spec:cg3:sem:grammar.cg3.trie-index-to-set-fn]
> Free function. Identical shape to trie_indexToRule but for the set index: walks
> `trie` recursively and for every (tag, node) entry at every depth calls
> grammar.indexTagToSet(tag->hash, r), descending into node.trie if present. Here
> `r` is a set NUMBER, so this sets bit r in sets_by_tag[tag->hash] for every tag
> in the trie including intermediate nodes.

> [spec:cg3:def:grammar.cg3.trie-unserialize-fn]
> inline void trie_unserialize(trie_t& trie, std::istream& input, Grammar& grammar, uint32_t num_tags)

> [spec:cg3:sem:grammar.cg3.trie-unserialize-fn]
> Free function. Deserializes a tag-trie from a binary-grammar input stream,
> mirroring trie_serialize. Loops `num_tags` times; each iteration: read a
> big-endian uint32 tag index and obtain the Tag* via
> grammar.single_tags_list[index], then get/create the trie node keyed by that tag
> (trie[tag]). Read a big-endian uint8 terminal flag and set node.terminal =
> (byte != 0). Read a big-endian uint32 child-count; if non-zero, ensure
> node.trie exists (reset to a new trie_t if null) and recurse
> trie_unserialize(*node.trie, input, grammar, childCount). Assumes
> single_tags_list is already fully populated (tags are read before tries in the
> binary format) and performs no bounds-checking on the tag index.

