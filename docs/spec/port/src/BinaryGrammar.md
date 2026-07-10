# src/BinaryGrammar.cpp, src/BinaryGrammar.hpp

> [spec:cg3:def:binary-grammar.cg3.binary-grammar]
> class BinaryGrammar : public IGrammarParser {
>   Grammar* grammar = nullptr;
>   deferred_t deferred_tmpls;
>   deferred_ors_t deferred_ors;
>   uint32FlatHashSet seen_uint32;
> }

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.binary-grammar-fn]
> BinaryGrammar::BinaryGrammar(Grammar& res, std::ostream& ux_err)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.binary-grammar-fn]
> Constructor `BinaryGrammar(Grammar& res, std::ostream& ux_err)`. Delegates to
> the base `IGrammarParser(res, ux_err)` constructor (which sets `ux_stderr =
> &ux_err` and `result = &res`, leaving `nrules`/`nrules_inv` null and
> `verbosity` 0). Its own body then sets the member `grammar = result`, i.e.
> `grammar` aliases the same Grammar object as the inherited `result` pointer. No
> allocation or I/O occurs.

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.deferred-ors-t]
> typedef std::unordered_map<ContextualTest*, std::vector<uint32_t>> deferred_ors_t

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.deferred-t]
> typedef std::unordered_map<ContextualTest*, uint32_t> deferred_t

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.parse-grammar-fn]
> int BinaryGrammar::parse_grammar(const char* filename)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.parse-grammar-fn]
> `int parse_grammar(const char* filename)` — the file-path entry point. If
> `grammar` is null, print "Error: Cannot parse into nothing - hint: call
> setResult() before trying." to `ux_stderr` and `CG3Quit(1)`. Call
> `stat(filename, &_stat)`; if it returns non-zero, print "Error: Cannot stat
> <filename> due to error <n> - bailing out!" and `CG3Quit(1)`; otherwise store
> `grammar->grammar_size = (size_t)_stat.st_size`. Open a `std::ifstream` in
> binary mode with the exception mask `failbit|eofbit|badbit` enabled, then
> return the result of the istream overload `parse_grammar(input)` (defined in
> BinaryGrammar_read.cpp). The stream is configured to throw on any read
> error/EOF. Note the sibling in-memory overloads: `(const std::string&)` calls
> `(const char*, size_t)`, which writes into a stringstream and seeks to 0 before
> calling the istream overload; the `(const UChar*, size_t)` and `(UString&)`
> overloads unconditionally throw (binary grammars are byte-oriented).

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn]
> int readBinaryGrammar_10043(std::istream& input)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn]
> OUT OF SCOPE for the Rust port (the legacy `_10043` reader, implemented in
> BinaryGrammar_read_10043.cpp, is intentionally excluded). Documented for
> completeness: reads pre-10298 binary grammars (revisions >= 10043, up to
> BIN_REV_ANCIENT=10297), dispatched from `parse_grammar(istream)` after
> `seekg(0)`. Uses thread-local statics `contexts_list` (vector<ContextualTest*>)
> and `templates` (contexts map) to resolve references by 1-based index and by
> template number. Steps: null-check input and grammar (error+CG3Quit(1) each);
> read 4 magic bytes and verify `is_cg3b`; read u32 revision, if < 10043
> error+quit; set is_binary=true. Read a top-level u32 `fields` with a DIFFERENT
> bit layout than the modern reader: has_dep=1<<0, mapping_prefix=1<<1,
> sub_readings_ltr=1<<2, tags=1<<3, pref_targets=1<<5, parentheses=1<<6,
> anchors=1<<7, sets=1<<8, delimiters=1<<9, soft_delimiters=1<<10,
> contexts=1<<11, rules=1<<12, has_relations=1<<13. Prefix (1<<1) = u32 len +
> UTF-8 bytes into one UChar. Tags (count from 1<<3): per-tag mask bits 0..11
> only (number, hash, plain_hash, seed, type, comparison_hash, comparison_op,
> comparison_val as clamped int32, tag text, regex opened case-insensitively iff
> T_CASE_INSENSITIVE, vs_sets deferred, vs_names) — no double/variable_hash/
> context_ref fields; registers single_tags[hash] and single_tags_list[number],
> sets tag_any for "*". Then preferred_targets (1<<5), parentheses (1<<6, with
> reverse map), anchors (1<<7). Sets (1<<8): per-set mask bit0 number, bit1 =
> `s->hash` (NOT a 16-bit type as in the modern format), bit2 = u8 type, bit3
> tries, bit4 set_ops, bit5 sets, bit6 name; stored in `sets_by_contents[hash]`
> and `sets_list[number]`. Resolve deferred varstring tag sets. delimiters
> (1<<9)/soft_delimiters (1<<10) resolved via `sets_by_contents.find(hash)`.
> Contexts (1<<11): resize `contexts_list`, read each via
> `readContextualTest_10043`, store in `grammar->contexts[hash]` and
> `contexts_list[i]`. Rules (1<<12): per-rule mask bits 0..14 (flags always
> 32-bit; no 64-bit/bit16, no sub_rules/bit15). dep_target and every
> dep_test/test reference is a 1-based index into `contexts_list`
> (`contexts_list[u32-1]`), NOT a hash lookup. Apply nrules/nrules_inv K_IGNORE
> filtering; store rule_by_number[number]. Finally bind deferred templates via
> the `templates` map (`templates.find(hash)->second`), `ucnv_close`, call
> `grammar->allocateDummySet()`, RESET `is_binary=false`, and return 0.

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-contextual-test-10043-fn]
> ContextualTest* readContextualTest_10043(std::istream& input)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-10043-fn]
> OUT OF SCOPE for the Rust port (legacy `_10043`, implemented in
> BinaryGrammar_read_10043.cpp); documented for completeness. Allocates a
> ContextualTest and reads a u32 field mask, then: bit0 hash; bit1 pos (+ high 32
> bits if `pos & POS_64BIT`); bit2 offset(int32); bit3 reads a u32 into local
> `tmpl` but assigns `t->tmpl = reinterpret_cast<ContextualTest*>(u32tmp)` where
> `u32tmp` is still 0 at that point — a BUG that stores null instead of the read
> value; the real template number is kept in local `tmpl` and deferred, so the
> assignment is effectively dead. bit4 target; bit5 line; bit6 relation; bit7
> barrier; bit8 cbarrier; bit9 offset_sub(int32); bit12 reads a u32 template
> number and registers `templates[number] = t` (this test IS that template);
> bit10 ors: read u32 count then push each `contexts_list[u32-1]` to `t->ors`;
> bit11 linked = `contexts_list[u32-1]`. If local `tmpl` is nonzero, record
> `deferred_tmpls[t] = tmpl`. Return t. References use 1-based `contexts_list`
> indices, unlike the modern hash-keyed scheme.

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-contextual-test-fn]
> ContextualTest* readContextualTest(std::istream& input)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-fn]
> Private helper declared here, IMPLEMENTED in BinaryGrammar_read.cpp. Reads one
> ContextualTest record and returns a fresh `grammar->allocateContextualTest`
> pointer. Read a u32 field mask, then conditionally in this exact source order:
> bit0 hash(u32); bit1 pos = read u32, and if `pos & POS_64BIT` (1ull<<31) read a
> second u32 and OR it in as the high 32 bits; bit2 offset(int32); bit3 read a
> u32 template hash and record `deferred_tmpls[t] = hash` (bound later, not
> resolved here); bit4 target(u32); bit5 line(u32); bit6 relation(u32); bit7
> barrier(u32); bit8 cbarrier(u32); bit9 offset_sub(int32); bit12 jump_pos(int8)
> — read BEFORE bit10/bit11; bit10 ors: read u32 count then append each hash to
> `deferred_ors[t]`; bit11 linked: read u32 hash and set `t->linked =
> grammar->contexts[u32]` (inline map lookup — resolves because the writer emits
> linked children first). Return `t`. (See the detailed spec under
> BinaryGrammar_read.md for the same function.)

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.set-compatible-fn]
> void BinaryGrammar::setCompatible(bool)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-compatible-fn]
> `void setCompatible(bool)` override with an empty body — a no-op. The
> compatibility flag argument is accepted and discarded; binary grammar loading
> has no "compatible" mode.

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.set-verbosity-fn]
> void BinaryGrammar::setVerbosity(uint32_t v)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-verbosity-fn]
> `void setVerbosity(uint32_t v)` override. Stores `verbosity = v` (the inherited
> member). Higher values enable optional warnings, e.g. the "please recompile the
> binary grammar" notice emitted when loading an ancient (<= 10297) revision.

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.write-binary-grammar-fn]
> int writeBinaryGrammar(std::ostream& output)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.write-binary-grammar-fn]
> Public method declared here, IMPLEMENTED in BinaryGrammar_write.cpp. Serializes
> `grammar` to `output` as a `.cg3b` blob byte-compatible with the current
> revision, returning 0. Guards: null `output` → "Output is null" + CG3Quit(1);
> null `grammar` → "No grammar provided" + CG3Quit(1). Writes big-endian integers
> (`writeBE`) and UTF-8-encoded strings. Layout: 4 raw bytes "CG3B";
> `writeBE(CG3_FEATURE_REV)` (=13898); a u32 top-level BINF_* feature bitset built
> from grammar state; the single-UChar mapping prefix (u32 len + UTF-8) when set;
> `cmdargs` and `cmdargs_override` (each u32 len + raw bytes, always present); the
> tag table (u32 count then per-tag u32 field mask + fields, incl. comparison_val
> as a 12-byte double via writeBE(double), tag text, regex PATTERN only with
> flags re-derived on read, vs_sets, vs_names); reopen_mappings; preferred_targets;
> parentheses; anchors; the set table (u32 count then per-set mask+fields incl.
> serialized tries with u32 size prefixes); delimiters/soft_delimiters/
> text_delimiters set numbers; the context table (`seen_uint32.clear()`, u32
> `contexts.size()`, each via `writeContextualTest`, dependency-first + dedup);
> and the rule table (u32 count then per-rule mask+fields, then dep_target hash,
> then dep_tests and tests hash lists after calling `reverseContextualTests()`).
> `ucnv_close` and return 0. NOTE the SIDE EFFECT: `reverseContextualTests()`
> reverses each rule's `tests`/`dep_tests` in place. (See BinaryGrammar_write.md
> for the exhaustive field-by-field wire layout.)

> [spec:cg3:def:binary-grammar.cg3.binary-grammar.write-contextual-test-fn]
> void writeContextualTest(ContextualTest* t, std::ostream& output)

> [spec:cg3:sem:binary-grammar.cg3.binary-grammar.write-contextual-test-fn]
> Private helper declared here, IMPLEMENTED in BinaryGrammar_write.cpp. Writes one
> ContextualTest, deduplicated via `seen_uint32` (return early if `t->hash`
> already seen; else insert). Recurses to write dependencies FIRST: `t->tmpl`,
> then each `t->ors`, then `t->linked`. Builds a u32 field mask + buffer: bit0
> hash is REQUIRED (hash==0 → "Context on line <line> had hash 0!" + CG3Quit(1));
> bit1 pos (low 32 bits, plus high 32 bits when `pos & POS_64BIT`); bit2
> offset(int32); bit3 tmpl->hash; bit4 target; bit5 line; bit6 relation; bit7
> barrier; bit8 cbarrier; bit9 offset_sub(int32); bit10 flag-only if ors
> non-empty; bit11 flag-only if linked; bit12 jump_pos(int8). Writes the mask,
> then the buffer, then (after the buffer) the u32 ors count + each or's hash when
> present, then `linked->hash` when present. (See BinaryGrammar_write.md for the
> same function in full.)

