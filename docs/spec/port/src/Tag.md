# src/Tag.cpp, src/Tag.hpp

> [spec:cg3:def:tag.cg3.c-ops]
> enum C_OPS : uint8_t {
>   OP_NOP;
>   OP_EQUALS;
>   OP_LESSTHAN;
>   OP_GREATERTHAN;
>   OP_LESSEQUALS;
>   OP_GREATEREQUALS;
>   OP_NOTEQUALS;
>   NUM_OPS;
> }

> [spec:cg3:def:tag.cg3.compare-tag]
> struct compare_Tag

> [spec:cg3:def:tag.cg3.compare-tag-vector]
> struct compare_TagVector

> [spec:cg3:def:tag.cg3.compare-tag-vector.operator-fn]
> inline bool operator()(const TagVector& a, const TagVector& b) const

> [spec:cg3:sem:tag.cg3.compare-tag-vector.operator-fn]
> Strict-weak ordering that lexicographically compares two `TagVector`s (each a
> `std::vector<Tag*>`) purely by their elements' `hash` fields. Loops index `i`
> from 0 while `i < a.size()` and `i < b.size()`; at the first `i` where
> `a[i]->hash != b[i]->hash`, returns `a[i]->hash < b[i]->hash`. If every
> compared element hash is equal up through the shorter length, returns
> `a.size() < b.size()` (the shorter vector sorts first). Reads only each Tag's
> `hash`; never dereferences past the shorter vector's length.

> [spec:cg3:def:tag.cg3.compare-tag.operator-fn]
> inline bool operator()(const Tag* a, const Tag* b) const

> [spec:cg3:sem:tag.cg3.compare-tag.operator-fn]
> Strict-weak ordering functor for `Tag*`: dereferences both and returns
> `a->hash < b->hash`. Compares solely on the tag's cached `hash` field; no other
> field is read. Used by `TagSortedVector`/`sorted_vector<Tag*, compare_Tag>`.

> [spec:cg3:def:tag.cg3.equal-tag]
> struct equal_Tag

> [spec:cg3:def:tag.cg3.equal-tag.operator-fn]
> inline bool operator()(const Tag* a, const Tag* b) const

> [spec:cg3:sem:tag.cg3.equal-tag.operator-fn]
> Equality functor for `Tag*`: dereferences both and returns
> `a->hash == b->hash`. Two tags are treated as equal iff their cached `hash`
> fields match; no other field participates. Pairs with `compare_Tag` for
> hashed/sorted tag containers.

> [spec:cg3:def:tag.cg3.fill-tagvector-fn]
> inline void fill_tagvector(const T& in, TagVector& tags, bool& did, bool& special)

> [spec:cg3:sem:tag.cg3.fill-tagvector-fn]
> Template helper that copies the non-numerical tags of an input container `in`
> (any iterable of `Tag*`) into `tags`, while flagging two out-params. Iterates
> `in` in order; for each `tag`: if `tag->type & T_NUMERICAL` is set, sets
> `did = true` and does NOT append the tag. Otherwise: if `tag->type & T_SPECIAL`
> is set, sets `special = true`; then `tags.push_back(tag)`. Both `did` and
> `special` are only ever assigned `true` (never reset), so the caller must
> pre-initialize them — they accumulate across calls. Result: `tags` holds the
> input's non-numerical tags in original order; `did` records whether any
> numerical tag was skipped; `special` records whether any appended tag was
> special.

> [spec:cg3:def:tag.cg3.tag]
> class Tag {
>   CG3_IMPORTS static std::ostream* dump_hashes_out;
>   C_OPS comparison_op = OP_NOP;
>   double comparison_val = 0;
>   uint32_t type = 0;
>   uint32_t comparison_hash = 0;
>   uint32_t dep_self = 0;
>   union { uint32_t dep_parent = 0; uint32_t variable_hash; uint32_t context_ref_pos; uint32_t comparison_offset; };
>   uint32_t hash = 0;
>   uint32_t plain_hash = 0;
>   uint32_t number = 0;
>   uint32_t seed = 0;
>   UString tag;
>   UString tag_raw;
>   std::unique_ptr<SetVector> vs_sets;
>   std::unique_ptr<UStringVector> vs_names;
>   mutable URegularExpression* regexp = nullptr;
> }

> [spec:cg3:def:tag.cg3.tag.allocate-vs-names-fn]
> void Tag::allocateVsNames()

> [spec:cg3:sem:tag.cg3.tag.allocate-vs-names-fn]
> Lazily allocates the `vs_names` member. If the `vs_names` unique_ptr is
> currently null, resets it to a newly heap-allocated empty `UStringVector`. If
> it is already allocated, does nothing. Idempotent; never clears an existing
> vector.

> [spec:cg3:def:tag.cg3.tag.allocate-vs-sets-fn]
> void Tag::allocateVsSets()

> [spec:cg3:sem:tag.cg3.tag.allocate-vs-sets-fn]
> Lazily allocates the `vs_sets` member. If the `vs_sets` unique_ptr is currently
> null, resets it to a newly heap-allocated empty `SetVector`
> (`std::vector<Set*>`). If already allocated, does nothing. Idempotent; leaves an
> existing vector untouched.

> [spec:cg3:def:tag.cg3.tag.mark-used-fn]
> void Tag::markUsed()

> [spec:cg3:sem:tag.cg3.tag.mark-used-fn]
> Sets the `T_USED` bit in `type` (`type |= T_USED`). No other side effects, no
> return value.

> [spec:cg3:def:tag.cg3.tag.parse-numeric-fn]
> void Tag::parseNumeric(bool trusted)

> [spec:cg3:sem:tag.cg3.tag.parse-numeric-fn]
> Attempts to interpret `tag` as a numeric comparison of the form
> `<key op value>` (e.g. `<W=5>`, `<Sem>MAX>`, or with `trusted`, a math
> expression like `<Weight=$1*2>`). On success it fills `comparison_op`,
> `comparison_val`, `comparison_hash` and ORs `T_NUMERICAL` into `type`; on any
> failure it returns leaving the tag unchanged. Steps:
> 1. If `tag.size() >= 256`, return immediately.
> 2. Declare stack buffers `tkey[256]` and `top[256]`, each zeroed at index 0.
>    Run `u_sscanf(tag.data(), "%*[<]%[^<>=:!]%[<>=:!]", &tkey, &top)`: `%*[<]`
>    matches and DISCARDS a leading run of `<`; `%[^<>=:!]` reads into `tkey` the
>    run of chars NOT in the set `<>=:!` (the key); `%[<>=:!]` reads into `top`
>    the run of chars that ARE in that set (the operator). Continue only if it
>    returned 2 (both conversions succeeded) AND `top[0] != 0`.
> 3. `tkz = u_strlen(tkey)`, `toz = u_strlen(top)`. If `tkz + toz + 1 >=
>    tag.size()`, return.
> 4. Extract the value substring `txval`: copy `tag` chars from index
>    `tkz+toz+1` up to `tag.end()-1` (excludes the final char, normally the
>    closing `>`), then NUL-terminate at index `tag.size()-tkz-toz-2`. If
>    `txval[0] == 0` (empty value), return.
> 5. `r = u_strspn(txval, "-.0123456789")` = length of the leading run of
>    digit/`.`/`-` chars in the value.
> 6. MATH branch — taken iff ALL of: `trusted` is true, `txval[r] != 0` (there is
>    a non-numeric tail after the numeric prefix), `txval` contains at least one
>    of `-+*/^%()=`, and `txval` contains NONE of
>    `"\<>[]{}!?&$¤#£@~`´';:,|_`. Then: set `comparison_offset` (union member
>    aliasing `dep_parent`) = `u_strlen(tkey)+u_strlen(top)+1`; build
>    `MathParser(NUMERIC_MIN, NUMERIC_MAX)`; take a view of `tag`, remove the
>    first `comparison_offset` chars (the `<key op` prefix) and the last char
>    (the `>`), and `eval` it. If eval throws (caught by `catch (...)`), set
>    `comparison_offset = 0` and return. On success, OR in `T_NUMERIC_MATH` and
>    FALL THROUGH (note `tval` remains 0).
> 7. Else-if `txval` is exactly "MAX" (`txval[0..3]=='M','A','X',0`), `tval =
>    NUMERIC_MAX`. Else-if exactly "MIN", `tval = NUMERIC_MIN`. Else-if
>    `txval[r] != 0` (non-numeric tail) OR `u_sscanf(txval, "%lf", &tval) != 1`,
>    return.
> 8. Clamp: if `tval < NUMERIC_MIN` set `tval = NUMERIC_MIN`; if `tval >
>    NUMERIC_MAX` set `tval = NUMERIC_MAX`. (NUMERIC_MIN = -(2^48),
>    NUMERIC_MAX = 2^48 - 1, as doubles.)
> 9. Derive `comparison_op` from `top[0]`: `<`→OP_LESSTHAN, `>`→OP_GREATERTHAN,
>    `=` or `:`→OP_EQUALS, `!`→OP_NOTEQUALS.
> 10. If `top[1]` is non-zero (two-char operator): if `top[1]` is `=` or `:`:
>     GREATERTHAN→GREATEREQUALS, LESSTHAN→LESSEQUALS, NOTEQUALS stays NOTEQUALS.
>     Else if `top[1]=='>'`: EQUALS→GREATEREQUALS, LESSTHAN→NOTEQUALS. Else if
>     `top[1]=='<'`: EQUALS→LESSEQUALS, GREATERTHAN→NOTEQUALS. (Other current
>     ops are left unchanged in each sub-case.)
> 11. `comparison_val = tval`; `comparison_hash = hash_value(tkey)`
>     (SuperFastHash over the key's UChar code units, seed CG3_HASH_SEED); OR in
>     `T_NUMERICAL`. NOTE: because the MATH branch falls through here, a math tag
>     ends up with BOTH `T_NUMERIC_MATH` and `T_NUMERICAL`, `comparison_val == 0`,
>     `comparison_op` derived from the operator, and `comparison_hash` of the key.

> [spec:cg3:def:tag.cg3.tag.parse-tag-raw-fn]
> void Tag::parseTagRaw(const UChar* to, Grammar* grammar)

> [spec:cg3:sem:tag.cg3.tag.parse-tag-raw-fn]
> Parses the raw tag text `to` (a NUL-terminated `UChar*`) into this Tag,
> deriving `type` bits and dependency/relation/numeric fields; `grammar` supplies
> the regex-tag and icase-tag tables and tag allocation. Steps:
> 1. `type = 0`. `length = u_strlen(to)`; `assert(length)` (empty input is UB in
>    release builds — no guard).
> 2. Textual/form detection: if `to[0]` is `"` or `<`, AND it is properly
>    delimited (`to[0]=='"' && to[length-1]=='"'`, or `to[0]=='<' &&
>    to[length-1]=='>'`), OR in `T_TEXTUAL`. Additionally, when double-quoted:
>    if `to[1]=='<'` AND `to[length-2]=='>'` AND `length > 4`, OR in `T_WORDFORM`;
>    otherwise OR in `T_BASEFORM`.
> 3. Store the text: `tag.assign(to, length)`.
> 4. Regex-tag scan: for each compiled `URegularExpression*` in
>    `grammar->regex_tags`, call `uregex_setText(iter, tag.data(),
>    SI32(tag.size()), &status)`; if `status == U_ZERO_ERROR`, call
>    `uregex_find(iter, -1, &status)` and if it reports a match, OR in
>    `T_TEXTUAL`. (These shared ICU regex objects are mutated in place as subject
>    text is set — inherently not thread-safe here; the Rust port must replicate
>    "does this grammar regex match the whole tag text" using each regex-tag's
>    compiled pattern/flags.)
> 5. Icase-tag scan: for each `Tag*` in `grammar->icase_tags`, if
>    `ux_strCaseCompare(tag, iter->tag)` (ICU `u_strCaseCompare` with
>    `U_FOLD_CASE_DEFAULT`, returning true on full-case-fold equality) holds, OR
>    in `T_TEXTUAL`.
> 6. Numeric: if `tag[0]=='<'` AND `tag[length-1]=='>'`, call
>    `parseNumeric(false)`.
> 7. Dependency: if `tag[0]=='#'`: try `u_sscanf(tag.data(), "#%i->%i",
>    &dep_self, &dep_parent)` — if it reads 2 fields AND `dep_self != 0`, OR in
>    `T_DEPENDENCY`. Then also try the Unicode-arrow form via `u_sscanf_u` with
>    pattern `#%i→%i` (→ = U+2192) into the same `dep_self`/`dep_parent` —
>    again requiring 2 fields and `dep_self != 0` to OR in `T_DEPENDENCY`.
>    (`dep_parent` is the active union member.)
> 8. Relation ID: if `tag[0]=='I' && tag[1]=='D' && tag[2]==':' &&
>    u_isdigit(tag[3])`, parse `u_sscanf(tag.data(), "ID:%i", &dep_self)`; if 1
>    field AND `dep_self != 0`, OR in `T_RELATION`.
> 9. Named relation: if `tag[0]=='R' && tag[1]==':'`: declare local
>    `UChar relname[256]`, set `dep_parent = UINT32_MAX`, then
>    `u_sscanf(tag.data(), "R:%[^:]:%i", &relname, &dep_parent)`; if it reads 2
>    fields AND `dep_parent != UINT32_MAX`, OR in `T_RELATION`, allocate a tag
>    for `relname` via `grammar->allocateTag(relname)`, and copy that tag's
>    `hash` into this tag's `comparison_hash`.
> 10. Finalize special: clear `T_SPECIAL`, then if `type & T_NUMERICAL` is set,
>     OR in `T_SPECIAL`. NOTE: unlike `rehash()`, this only re-derives
>     `T_SPECIAL` from `T_NUMERICAL`, not from the full `MASK_TAG_SPECIAL`.

> [spec:cg3:def:tag.cg3.tag.rehash-fn]
> uint32_t Tag::rehash()

> [spec:cg3:sem:tag.cg3.tag.rehash-fn]
> Recomputes and caches `hash` and `plain_hash` from `type`, `tag`, and `seed`;
> returns the new `hash`. Steps:
> 1. `hash = 0`, `plain_hash = 0`.
> 2. Prefix mixing, each via the `hash_value(const char*, hash)` overload
>    (SuperFastHash over the ASCII bytes of the literal with the running hash;
>    when the running hash is 0 it is internally seeded to CG3_HASH_SEED): if
>    `T_FAILFAST`, `hash = hash_value("^", hash)`; then if `T_META`
>    `hash_value("META:")`, if `T_VARIABLE` `hash_value("VAR:")`, if
>    `T_LOCAL_VARIABLE` `hash_value("LVAR:")`, if `T_SET` `hash_value("SET:")`
>    (in that order, each folding into the running `hash`).
> 3. `plain_hash = hash_value(tag)` — SuperFastHash over `tag`'s UTF-16 code
>    units (the `UChar*` overload), seeded CG3_HASH_SEED. Then: if `hash != 0`
>    (some prefix was applied), `hash = hash_value(plain_hash, hash)` using the
>    uint32 integer-mix overload (`h = c + (h<<6) + (h<<16) - h`, with results of
>    0, UINT32_MAX, or UINT32_MAX-1 remapped to CG3_HASH_SEED); else `hash =
>    plain_hash`.
> 4. Suffix mixing via the char* overload with the running hash: if
>    `T_CASE_INSENSITIVE` `hash_value("i", hash)`, if `T_REGEXP`
>    `hash_value("r", hash)`, if `T_VARSTRING` `hash_value("v", hash)`.
> 5. `hash += seed` (plain 32-bit add with wraparound; NOT remapped, so this can
>    legitimately produce `hash == 0`).
> 6. Recompute special: clear `T_SPECIAL`, then if `type & MASK_TAG_SPECIAL` is
>    non-zero, OR in `T_SPECIAL`.
> 7. If the static `dump_hashes_out` stream is non-null, print two DEBUG lines to
>    it (the `hash` and the `plain_hash`, each with `seed` and `tag`).
> 8. Return `hash`. PARITY: the prefix/suffix marker strings are hashed as
>    one-byte-per-char ASCII, while `tag` is hashed as two-byte UTF-16 code
>    units — the Rust port must feed SuperFastHash the same byte widths and the
>    same CG3_HASH_SEED seeding rules to reproduce these hashes.

> [spec:cg3:def:tag.cg3.tag.tag-fn]
> Tag::Tag(const Tag& o)

> [spec:cg3:sem:tag.cg3.tag.tag-fn]
> Copy constructor `Tag(const Tag& o)`. The member-initializer list copies these
> scalars straight from `o`: `comparison_op`, `comparison_val`, `type`,
> `comparison_hash`, `dep_self`, `dep_parent` (the active union member, which
> also fixes the aliased `variable_hash`/`context_ref_pos`/`comparison_offset`),
> `hash`, `plain_hash`, `number`, `seed`, and the `tag` UString; `regexp` is
> initialized to nullptr (NOT cloned in the list). QUIRK: `tag_raw` is NOT copied
> and is left default-empty. Body:
> 1. If `o.vs_names` is set, call `allocateVsNames()` then deep-copy the vector
>    contents (`*vs_names = *o.vs_names`).
> 2. If `o.vs_sets` is set, call `allocateVsSets()` then copy the vector
>    (`*vs_sets = *o.vs_sets`) — a shallow copy of the `Set*` pointers.
> 3. If `o.regexp` is non-null, clone it via `uregex_clone(o.regexp, &status)`
>    into `regexp` (the ICU status is ignored/unused).

> [spec:cg3:def:tag.cg3.tag.to-u-string-fn]
> UString Tag::toUString(bool escape) const

> [spec:cg3:sem:tag.cg3.tag.to-u-string-fn]
> Renders the tag back to its CG-3 source-string form. If `tag_raw` is non-empty,
> returns it verbatim (short-circuit, ignoring `type`/`escape`). Otherwise builds
> a fresh `UString str` (reserving `tag.size()`):
> 1. Emit type prefixes in this fixed order: `T_FAILFAST`→`^`; `T_META`→`META:`;
>    `T_VARIABLE`→`VAR:`; `T_LOCAL_VARIABLE`→`LVAR:`; `T_SET`→`SET:`;
>    `T_VSTR`→`VSTR:`.
> 2. If `(type & (T_CASE_INSENSITIVE | T_REGEXP))` is non-zero AND `tag` is NOT
>    textual (`is_textual` = front/back both `"` or both `<`/`>`), append an
>    opening `/`.
> 3. Body: if `escape` is true AND `tag[0] != '"'`, iterate each char `c` of
>    `tag`, emitting a `\` immediately before any of `\ ( ) ; #` or space, then
>    `c`. Otherwise append `tag` unchanged.
> 4. If `(type & (T_CASE_INSENSITIVE | T_REGEXP))` and not textual, append the
>    closing `/`.
> 5. Flag suffixes: if `T_REGEXP_LINE`, append `l`; ELSE if
>    `(type & (T_REGEXP | T_REGEXP_ANY))`, append `r`. If `T_CASE_INSENSITIVE`,
>    append `i`. If `(type & T_VARSTRING)` AND NOT `(type & T_VSTR)`, append `v`.
> 6. Return `str`.

