# src/BinaryGrammar_read.cpp

> [spec:cg3:def:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn]
> int BinaryGrammar::parse_grammar(std::istream& input)

> [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn]
> Reads a whole `.cg3b` binary grammar from `input` into `grammar`. All
> multi-byte integers are big-endian via `readBE<T>` (raw read + byte swap);
> strings are UTF-8 decoded through an ICU `UConverter` opened for "UTF-8" into
> the thread-local `gbuffers[0]` (UChar) / `cbuffers[0]` (byte) scratch buffers.
> First enable `input.exceptions(failbit|eofbit|badbit)` so any short read
> throws.
> Header: read 4 bytes into `cbuffers[0]`; on read failure print to std::cerr
> and CG3Quit(1). If not `is_cg3b` (bytes != 'C','G','3','B') print "Grammar
> does not begin with magic bytes" to ux_stderr and CG3Quit(1). Read
> `bin_revision` = readBE<uint32_t>. If `bin_revision <= BIN_REV_ANCIENT`
> (10297): if verbosity>=1 print the "please recompile" warning, `seekg(0)`,
> and return `readBinaryGrammar_10043(input)` (the excluded legacy path). If
> `bin_revision < CG3_TOO_OLD` (10373) print "requires 10373 or later" and
> CG3Quit(1). If `bin_revision > CG3_FEATURE_REV` (13898) print "only knows up
> to 13898" and CG3Quit(1).
> Set `grammar->is_binary = true` and read the top-level feature bitset `fields`
> = readBE<uint32_t>. Decode: has_dep=BINF_DEP(1<<0),
> sub_readings_ltr=BINF_SUB_LTR(1<<2), has_relations=BINF_RELATIONS(1<<13),
> has_bag_of_tags=BINF_BAG(1<<14), ordered=BINF_ORDERED(1<<15),
> addcohort_attach=BINF_ADDCOHORT_ATTACH(1<<17). Full BINF bit map: DEP=1<<0,
> PREFIX=1<<1, SUB_LTR=1<<2, TAGS=1<<3, REOPEN_MAP=1<<4, PREF_TARGETS=1<<5,
> ENCLS=1<<6, ANCHORS=1<<7, SETS=1<<8, DELIMS=1<<9, SOFT_DELIMS=1<<10,
> CONTEXTS=1<<11, RULES=1<<12, RELATIONS=1<<13, BAG=1<<14, ORDERED=1<<15,
> TEXT_DELIMS=1<<16, ADDCOHORT_ATTACH=1<<17.
> If BINF_PREFIX: read u32 length, read that many bytes, decode UTF-8 into
> `grammar->mapping_prefix` (exactly one UChar, capacity 1).
> If `bin_revision >= BIN_REV_CMDARGS` (13898; always true for the current
> format): read u32 len; if nonzero resize `grammar->cmdargs` to len and read
> len raw bytes (a byte std::string, not decoded); then the same for
> `grammar->cmdargs_override`.
> Tags: `num_single_tags` = (BINF_TAGS ? read u32 : 0); set `grammar->num_tags`
> and resize `single_tags_list`. For each tag: `allocateTag`, read a per-tag u32
> field mask, then conditionally read in this order: bit0 number(u32), bit1
> hash(u32), bit2 plain_hash(u32), bit3 seed(u32), bit4 type(u32), bit5
> comparison_hash(u32), bit6 comparison_op(u32 cast to C_OPS), bit7
> comparison_val as int32 (if <=INT32_MIN set NUMERIC_MIN, if >=INT32_MAX set
> NUMERIC_MAX — the legacy integer form, never emitted by the current writer),
> bit12 comparison_val as a 12-byte double: read sizeof(uint64_t)+sizeof(int32_t)
> =12 bytes into a stringstream then `readBE<double>` (uint64 mantissa BE then
> int32 exponent BE; value = (double)(int64)mant / INT64_MAX, then ldexp by
> exp). bit8 tag text: read u32 len, if nonzero read len bytes, UTF-8 decode,
> assign `t->tag`. bit9 regex: read u32 len, if nonzero UTF-8 decode the pattern
> and `uregex_open` it with flags `UREGEX_CASE_INSENSITIVE` iff `t->type &
> T_CASE_INSENSITIVE` else 0; on ICU error print + CG3Quit(1); store `t->regexp`
> (the pattern text is stored, flags are re-derived from `type`, not stored).
> bit10 vs_sets: read u32 count, `allocateVsSets()`, and record the `count` set
> numbers into a deferred map `tag_varsets[t->number]` (resolved after sets
> load). bit11 vs_names: read u32 count, `allocateVsNames()`, loop reading u32
> len then len UTF-8 bytes pushed to `vs_names` (skipped when len==0). bit13
> variable_hash(u32). bit14 context_ref_pos(u32). Then register
> `single_tags[t->hash]=t`, `single_tags_list[t->number]=t`, and if the tag text
> is exactly "*" set `grammar->tag_any = t->hash`.
> reopen_mappings: count=(BINF_REOPEN_MAP?u32:0); read that many u32 and insert
> each into `grammar->reopen_mappings`.
> preferred_targets: count=(BINF_PREF_TARGETS?u32:0); push_back each u32.
> parentheses: count=(BINF_ENCLS?u32:0); each is (left u32, right u32) →
> `parentheses[left]=right` and `parentheses_reverse[right]=left`.
> anchors: count=(BINF_ANCHORS?u32:0); each (left,right) → `anchors[left]=right`.
> Sets: count=(BINF_SETS?u32:0); resize `sets_list`. For each set: `allocateSet`,
> read per-set field mask: bit0 number(u32); bit1 type = UI16(read u32) [16-bit
> type, used when type >= ST_ORDERED]; bit2 type = read u8 [8-bit type]; bit3
> tries: read u32 n1, if nonzero `trie_unserialize(s->trie,input,*grammar,n1)`,
> read u32 n2, if nonzero `trie_unserialize(s->trie_special,...,n2)` (n is the
> trie's top-level entry count); bit4 set_ops: read u32 count then that many
> u32; bit5 sets: read u32 count then that many u32; bit6 name: read u32 len, if
> nonzero UTF-8 decode + `s->setName`. Register `sets_list[s->number]=s`.
> Resolve varstring tag sets: for each `tag_varsets` entry, find the tag by
> number and push each `sets_list[num]` into its `vs_sets`.
> If BINF_DELIMS read u32 → `grammar->delimiters = sets_list[u32]`; if
> BINF_SOFT_DELIMS → `soft_delimiters`; if BINF_TEXT_DELIMS → `text_delimiters`.
> Contexts: count=(BINF_CONTEXTS?u32:0); for each call `readContextualTest(input)`
> and store `grammar->contexts[t->hash]=t`. (The writer emits contexts
> dependency-first, so a test's `linked` reference — resolved inline by hash —
> is already present when read.)
> Rules: count=(BINF_RULES?u32:0); resize `rule_by_number`. For each rule:
> `allocateRule`, read per-rule field mask: bit0 section(int32), bit1
> type(u32→KEYWORDS), bit2 line(u32), bit3 flags = (bit16 set ? read u64 : read
> u32), bit4 name(u32 len + UTF-8), bit5 target(u32), bit6 wordform =
> `single_tags_list[read u32]`, bit7 varname(u32), bit8 varvalue(u32), bit9
> sub_reading: read u32, if bit31 set clear it and negate → signed sub_reading,
> bit10 childset1(u32), bit11 childset2(u32), bit12 maplist = `sets_list[read
> u32]`, bit13 sublist = `sets_list[read u32]`, bit14 number(u32). Then
> unconditionally: read u32 dep hash; if nonzero `r->dep_target =
> grammar->contexts[hash]`. Read u32 num_dep_tests; for each read a context hash,
> look it up in `grammar->contexts`, and `addContextualTest(t, r->dep_tests)`
> (which push_fronts). Read u32 num_tests and do the same into `r->tests`. If
> bit15: read u32 count of sub-rule numbers, each `rule_by_number[num]` pushed to
> `r->sub_rules`. Apply the optional name filters: if `nrules` set, set its text
> to `r->name` and if `uregex_find` fails set `r->type = K_IGNORE`; if
> `nrules_inv` set and `uregex_find` succeeds set `r->type = K_IGNORE`. Store
> `rule_by_number[r->number]=r`. NOTE: push_front stores tests in reverse of file
> order; the writer compensates by reversing them before writing (so a round
> trip preserves order).
> Bind deferred template refs: for each `deferred_tmpls` entry set `test->tmpl =
> grammar->contexts.find(hash)->second`. Bind deferred ORs: for each
> `deferred_ors` entry reserve and push each `contexts.find(hash)->second` into
> `test->ors`.
> Finally `ucnv_close(conv)` and return 0.

> [spec:cg3:def:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn]
> ContextualTest* BinaryGrammar::readContextualTest(std::istream& input)

> [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn]
> Reads one ContextualTest record and returns a freshly `allocateContextualTest`
> pointer owned by `grammar`. Read a u32 field mask, then conditionally (in this
> exact source order): bit0 hash(u32); bit1 pos = read u32, and if `pos &
> POS_64BIT` (POS_64BIT = 1ull<<31, i.e. bit 31 of the low word) read a second
> u32 and OR it in as the high 32 bits (`pos |= UI64(hi) << 32`); bit2
> offset(int32); bit3 read a u32 template hash and record it in
> `deferred_tmpls[t]` (bound after all contexts load, not resolved here); bit4
> target(u32); bit5 line(u32); bit6 relation(u32); bit7 barrier(u32); bit8
> cbarrier(u32); bit9 offset_sub(int32); bit12 jump_pos(int8) — NOTE bit12 is
> read BEFORE bit10 and bit11; bit10 ors: read u32 count and append each of
> `count` u32 hashes to `deferred_ors[t]` (bound after all contexts load); bit11
> linked: read u32 hash and set `t->linked = grammar->contexts[u32]`, resolved
> immediately via map `operator[]` (a missing hash would insert/return a null
> pointer; the writer emits linked children first so it is normally present).
> Return `t`.

