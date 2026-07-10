# src/ContextualTest.cpp, src/ContextualTest.hpp

> [spec:cg3:def:contextual-test.cg3.context-list]
> typedef std::list<ContextualTest*> ContextList

> [spec:cg3:def:contextual-test.cg3.context-vector]
> typedef std::vector<ContextualTest*> ContextVector

> [spec:cg3:def:contextual-test.cg3.contextual-test]
> class ContextualTest {
>   bool is_used = false;
>   int32_t offset = 0;
>   int32_t offset_sub = 0;
>   uint32_t line = 0;
>   uint32_t hash = 0;
>   uint32_t seed = 0;
>   uint64_t pos = 0;
>   uint32_t target = 0;
>   uint32_t relation = 0;
>   uint32_t barrier = 0;
>   uint32_t cbarrier = 0;
>   int8_t jump_pos = JUMP_MARK;
>   ContextualTest* tmpl = nullptr;
>   ContextualTest* linked = nullptr;
>   ContextVector ors;
> }

> [spec:cg3:def:contextual-test.cg3.contextual-test.contextual-test-fn]
> ContextualTest() = default

> [spec:cg3:sem:contextual-test.cg3.contextual-test.contextual-test-fn]
> Defaulted default constructor: leaves every member at its in-class initializer —
> `is_used=false`, `offset=0`, `offset_sub=0`, `line=0`, `hash=0`, `seed=0`,
> `pos=0`, `target=0`, `relation=0`, `barrier=0`, `cbarrier=0`,
> `jump_pos=JUMP_MARK` (0), `tmpl=nullptr`, `linked=nullptr`, and an empty `ors`
> vector. No other effects.

> [spec:cg3:def:contextual-test.cg3.contextual-test.mark-used-fn]
> void ContextualTest::markUsed(Grammar& grammar)

> [spec:cg3:sem:contextual-test.cg3.contextual-test.mark-used-fn]
> Recursively marks this test and its referenced sets/tests as used, guarded
> against re-entry: if `is_used` is already true, return immediately; otherwise set
> `is_used = true`. If `target != 0`, call `grammar.getSet(target)->markUsed(grammar)`.
> Likewise for `barrier` and `cbarrier` when non-zero. If `tmpl` is set, call
> `tmpl->markUsed(grammar)`. For each test in `ors` (in order),
> `idts->markUsed(grammar)`. If `linked` is set, `linked->markUsed(grammar)`.
> (A single reused local `Set* s` holds the three `getSet` results but has no
> lasting effect.) No return value.

> [spec:cg3:def:contextual-test.cg3.contextual-test.operator-fn]
> bool ContextualTest::operator==(const ContextualTest& other) const

> [spec:cg3:sem:contextual-test.cg3.contextual-test.operator-fn]
> Structural equality (`const`). Returns false at the first mismatch, comparing in
> order: `hash`, `pos`, `jump_pos`, `target`, `barrier`, `cbarrier`, `relation`,
> `offset`, `offset_sub`. Then `linked`: if the two `linked` pointers are not
> identical, they still count as equal ONLY when both are non-null AND
> `linked->hash == other.linked->hash` (compared by hash, not deep); any other case
> (one null, or hashes differ) returns false. `tmpl` is compared by pointer
> identity (must be the same object). Finally `ors` is compared with
> `std::vector::operator==` (same length and element-pointer equality, in order).
> Returns true if all checks pass. Note: because `hash` is compared first, two
> tests with differing cached hashes are always unequal even if otherwise
> identical.

> [spec:cg3:def:contextual-test.cg3.contextual-test.rehash-fn]
> uint32_t ContextualTest::rehash()

> [spec:cg3:sem:contextual-test.cg3.contextual-test.rehash-fn]
> Computes and caches `hash`, returning it. Memoized: if `hash != 0`, return it
> immediately without recomputing. Otherwise build it with the integer
> `hash_value` helpers. IMPORTANT argument-order quirk: except for the first line,
> every fold is `hash = hash_value(hash, <field>)`, where the CURRENT hash is the
> first (`c`, the "value") argument and `<field>` is the second (`h`, the
> accumulator/seed) argument — the REVERSE of the value-then-accumulator order used
> by `Set::rehash`, so here the field plays the seed role. Steps in sequence:
> `hash = hash_value(pos)` (the `uint64_t` overload: SuperFastHash over the 8 raw
> bytes of `pos`, with no prior hash mixed in); then fold with `jump_pos`, then
> `target`, `barrier`, `cbarrier`, `relation`; then `abs(offset)` and, if
> `offset < 0`, additionally fold constant 5000; then `abs(offset_sub)` and, if
> `offset_sub < 0`, fold constant 5000. If `linked` is set, fold in
> `linked->rehash()`. If `tmpl` is set, fold in
> `UI32(reinterpret_cast<uintptr_t>(tmpl))` — the low 32 bits of the pointer
> ADDRESS, which is non-deterministic across runs (flagged). For each test in
> `ors` (in order), fold in `iter->rehash()`. Finally `hash += seed`. Returns
> `hash`. Notes: `jump_pos` is `int8_t`, so negatives (`JUMP_ATTACH=-1`,
> `JUMP_TARGET=-2`) sign-extend to large `uint32_t` values when passed as the seed,
> and `jump_pos == 0` (the default) triggers `hash_value`'s internal
> `h == 0 -> CG3_HASH_SEED` remap.

> [spec:cg3:def:contextual-test.cg3.copy-cntx-fn]
> inline void copy_cntx(const ContextualTest* src, ContextualTest* trg)

> [spec:cg3:sem:contextual-test.cg3.copy-cntx-fn]
> Free helper that shallow-copies most fields from `src` into `trg`: `offset`,
> `offset_sub`, `line`, `hash`, `seed`, `pos`, `target`, `relation`, `barrier`,
> `cbarrier`, `jump_pos`, and the raw pointers `tmpl` and `linked` (pointer copy,
> not deep). It does NOT copy `is_used` or the `ors` vector — those keep whatever
> `trg` already had (flagged: `ors` and `is_used` are intentionally left
> untouched). No return value.

> [spec:cg3:def:contextual-test.cg3.gsr-specials]
> enum GSR_SPECIALS {
>   GSR_ANY = 32767;
> }

> [spec:cg3:def:contextual-test.cg3.pos-jump-pos]
> enum POS_JUMP_POS : int8_t {
>   JUMP_TARGET = -2;
>   JUMP_ATTACH = -1;
>   JUMP_MARK = 0;
> }

