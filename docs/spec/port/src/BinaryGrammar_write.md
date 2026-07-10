# src/BinaryGrammar_write.cpp

> [spec:cg3:def:binary-grammar-write.cg3.binary-grammar.write-binary-grammar-fn]
> int BinaryGrammar::writeBinaryGrammar(std::ostream& output)

> [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-binary-grammar-fn]
> Serializes `grammar` to `output` as a `.cg3b` blob, byte-compatible with the
> current revision. If `output` is falsy print "Output is null" and CG3Quit(1);
> if `grammar` is null print "No grammar provided" and CG3Quit(1). Open a UTF-8
> UConverter for encoding strings via `ucnv_fromUChars` into `cbuffers[0]`.
> Integers are written big-endian with `writeBE`.
> Header: write the 4 raw bytes "CG3B", then `writeBE(CG3_FEATURE_REV)` (=13898).
> Build the top-level `fields` bitset from grammar state: BINF_DEP if has_dep;
> BINF_PREFIX if mapping_prefix nonzero; BINF_SUB_LTR if sub_readings_ltr;
> BINF_TAGS if single_tags_list nonempty; BINF_REOPEN_MAP if reopen_mappings
> nonempty; BINF_PREF_TARGETS if preferred_targets nonempty; BINF_ENCLS if
> parentheses nonempty; BINF_ANCHORS if anchors nonempty; BINF_SETS if sets_list
> nonempty; BINF_DELIMS if delimiters set; BINF_SOFT_DELIMS if soft_delimiters
> set; BINF_CONTEXTS if contexts nonempty; BINF_RULES if rule_by_number nonempty;
> BINF_RELATIONS if has_relations; BINF_BAG if has_bag_of_tags; BINF_ORDERED if
> ordered; BINF_TEXT_DELIMS if text_delimiters set; BINF_ADDCOHORT_ATTACH if
> addcohort_attach. Then `writeBE(fields)`.
> If mapping_prefix: encode the single UChar to UTF-8, write its byte length
> (u32) then the bytes.
> Unconditionally (the current format always includes cmdargs, since
> CG3_FEATURE_REV == BIN_REV_CMDARGS): write u32 length of `cmdargs` then its raw
> bytes if nonzero, then the same for `cmdargs_override`.
> Tags: if `num_tags` nonzero write it as u32. Then for i in [0,num_tags): take
> `single_tags_list[i]`, build a per-tag u32 field mask and a temp buffer,
> writing only nonzero/relevant fields into the buffer: bit0 number, bit1 hash,
> bit2 plain_hash, bit3 seed, bit4 type, bit5 comparison_hash, bit6
> comparison_op (as u32); (bit7 is intentionally NOT reused — reserved until a
> hard format break); bit12 comparison_val if nonzero, written as a double
> (writeBE(double) = uint64 mantissa from frexp(value)*INT64_MAX, then uint32
> exponent — 12 bytes); bit8 tag text if non-empty (u32 UTF-8 byte length +
> bytes); bit9 regex if `t->regexp` set (fetch pattern via `uregex_pattern`,
> UTF-8 encode, write u32 length + bytes — regex FLAGS are NOT stored, they are
> re-derived on read from T_CASE_INSENSITIVE in `type`); bit10 vs_sets if present
> (u32 count then each referenced set's `number`); bit11 vs_names if present (u32
> count then each name as u32 UTF-8 length + bytes); bit13 variable_hash if `type
> & (T_VARIABLE|T_LOCAL_VARIABLE)` and variable_hash nonzero; bit14
> context_ref_pos if `type & T_CONTEXT`. Write the u32 field mask to output, then
> the temp buffer's bytes.
> reopen_mappings: if nonempty write u32 size then each u32.
> preferred_targets: if nonempty write u32 size then each u32.
> parentheses: if nonempty write u32 size then each (first u32, second u32).
> anchors: if nonempty write u32 size then each (first u32, second u32).
> Sets: if sets_list nonempty write u32 size. For each set build a mask+buffer:
> bit0 number if nonzero; then EITHER bit1 = write u32 type when `type >=
> ST_ORDERED` OR bit2 = write u8 type otherwise (exactly one of bit1/bit2 is
> always set); bit3 if `getNonEmpty()` non-empty: write u32 `trie.size()` +
> `trie_serialize(trie)` then u32 `trie_special.size()` +
> `trie_serialize(trie_special)`; bit4 set_ops if non-empty (u32 count + each
> u32); bit5 sets if non-empty (u32 count + each u32); bit6 name if `type &
> ST_STATIC` (u32 UTF-8 length + bytes). Write the u32 mask then the buffer.
> delimiters/soft_delimiters/text_delimiters: if each is set write its set
> `number` (u32) — no per-field flag here; presence is governed by the top-level
> BINF_*DELIMS bits.
> Contexts: `seen_uint32.clear()`; if contexts non-empty write u32
> `contexts.size()`; then iterate the contexts map calling `writeContextualTest`
> on each (which dedups by hash and recurses into dependencies first). NOTE the
> written record count is `contexts.size()`, so every context reachable via
> tmpl/ors/linked MUST also be a distinct entry of the contexts map, otherwise
> more records get emitted than the count and the stream desyncs on read.
> Rules: if rule_by_number non-empty write u32 size. For each rule build a
> mask+buffer: bit0 section(int32) if nonzero; bit1 type(u32) if nonzero; bit2
> line(u32) if nonzero; bit3 flags if nonzero — additionally set bit16 and write
> u64 when flags > UINT32_MAX, else write u32; bit4 name if non-empty (u32 UTF-8
> length + bytes); bit5 target(u32) if nonzero; bit6 wordform =
> `wordform->number`(u32) if set; bit7 varname(u32) if nonzero; bit8 varvalue(u32)
> if nonzero; bit9 sub_reading if nonzero, written as `abs(sub_reading)` with
> bit31 OR'd in when negative; bit10 childset1(u32) if nonzero; bit11
> childset2(u32) if nonzero; bit12 maplist->number(u32) if set; bit13
> sublist->number(u32) if set; bit14 number(u32) if nonzero; bit15 set (flag
> only) if sub_rules non-empty. Write the u32 mask, then the buffer. Then
> unconditionally: write u32 = `dep_target->hash` or 0 if none. Call
> `r->reverseContextualTests()` — SIDE EFFECT: reverses the rule's `tests` and
> `dep_tests` lists in place — then write u32 `dep_tests.size()` and each test's
> hash, then u32 `tests.size()` and each hash. If sub_rules non-empty write u32
> count and each sub-rule's `number`.
> Finally `ucnv_close(conv)` and return 0.

> [spec:cg3:def:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
> void BinaryGrammar::writeContextualTest(ContextualTest* t, std::ostream& output)

> [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
> Writes one ContextualTest, deduplicating and emitting dependencies first. If
> `t->hash` is already in `seen_uint32`, return immediately (already written).
> Otherwise insert `t->hash` into `seen_uint32`. Recurse to write dependencies
> BEFORE this node: if `t->tmpl` write it; for each `t->ors` write it; if
> `t->linked` write it — guaranteeing referenced contexts precede the referrer in
> the stream.
> Build a temp buffer + u32 field mask: bit0 hash — REQUIRED; if `t->hash`==0
> print "Context on line <line> had hash 0!" and CG3Quit(1). bit1 pos if nonzero:
> write low 32 bits `UI32(pos & 0xFFFFFFFF)`, and if `pos & POS_64BIT` also write
> the high 32 bits `UI32((pos>>32) & 0xFFFFFFFF)`. bit2 offset(int32) if nonzero.
> bit3 `tmpl->hash`(u32) if tmpl set. bit4 target(u32) if nonzero. bit5 line(u32)
> if nonzero. bit6 relation(u32) if nonzero. bit7 barrier(u32) if nonzero. bit8
> cbarrier(u32) if nonzero. bit9 offset_sub(int32) if nonzero. bit10 set (flag
> only) if `ors` non-empty. bit11 set (flag only) if linked. bit12 jump_pos(int8)
> if nonzero. Write the u32 mask, then the buffer.
> After the buffer: if `ors` non-empty write u32 `ors.size()` then each or's
> hash; if `linked` set write `linked->hash`(u32). (On read these trailing
> fields are consumed by the bit10/bit11 handlers, while jump_pos/bit12 is part
> of the fixed buffer read before them.)

