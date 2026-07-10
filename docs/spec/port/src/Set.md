# src/Set.cpp, src/Set.hpp

> [spec:cg3:def:set.cg3.set]
> class Set {
>   CG3_IMPORTS static std::ostream* dump_hashes_out;
>   uint16_t type = 0;
>   uint32_t line = 0;
>   uint32_t hash = 0;
>   uint32_t number = 0;
>   UString name;
>   trie_t trie;
>   trie_t trie_special;
>   TagSortedVector ff_tags;
>   uint32Vector set_ops;
>   uint32Vector sets;
> }

> [spec:cg3:def:set.cg3.set-set]
> typedef sorted_vector<Set*> SetSet

> [spec:cg3:def:set.cg3.set-vector]
> typedef std::vector<Set*> SetVector

> [spec:cg3:def:set.cg3.set.empty-fn]
> bool Set::empty() const

> [spec:cg3:sem:set.cg3.set.empty-fn]
> Returns true iff this Set holds nothing: the logical AND of `ff_tags.empty()`,
> `trie.empty()`, `trie_special.empty()`, and `sets.empty()`. It does NOT consult
> `set_ops` (nor `type`/`name`), so emptiness is judged purely on those four
> containers. `const`; no side effects.

> [spec:cg3:def:set.cg3.set.mark-used-fn]
> void Set::markUsed(Grammar& grammar)

> [spec:cg3:sem:set.cg3.set.mark-used-fn]
> Marks this set and everything it references as used. ORs `ST_USED` into `type`.
> Calls `trie_markused(trie)` and `trie_markused(trie_special)`, which recurse and
> call `markUsed()` on every contained `Tag`. Calls `markUsed()` on each `Tag*` in
> `ff_tags`. For each child set number `s` in `sets`, looks it up via
> `grammar.sets_by_contents.find(s)->second` (assumes present — dereferences the
> iterator with no end-check) and recursively `set->markUsed(grammar)`. No return
> value. There is no visited-guard, so it recurses the full set graph (relies on
> that graph being acyclic).

> [spec:cg3:def:set.cg3.set.rehash-fn]
> uint32_t Set::rehash()

> [spec:cg3:sem:set.cg3.set.rehash-fn]
> Recomputes and stores this set's `hash`, returning it. Uses the integer
> `hash_value(uint32_t value, uint32_t acc)` helper in the usual value-first order,
> folding each value into an accumulator `retval` that starts at 0 (the first fold
> hits `hash_value`'s `acc==0 -> CG3_HASH_SEED` remap). Does NOT memoize — always
> recomputes.
> Step 1 (unification sets): if `type` has either `ST_TAG_UNIFY` or `ST_SET_UNIFY`,
> fold in constant 5153 when `ST_TAG_UNIFY` is set, then 5171 when `ST_SET_UNIFY`
> is set. Then parse a multi-use id out of `name`: if `name[0]=='&'` and
> `u_sscanf(name.data(), "&&%u:%*S", &u) == 1` and `u != 0`, fold in `u`; else if
> `name[0]=='$'` and `u_sscanf(name.data(), "$$%u:%*S", &u) == 1` and `u != 0`,
> fold in `u`. (`%*S` is an assignment-suppressed string scan, so `== 1` requires
> that exactly the `%u` field was assigned; ICU `u_sscanf` needs the literal
> `&&`/`$$` prefix followed by digits then `:`.) This branch reads `name[0]`
> unconditionally, assuming `name` is non-empty.
> Step 2: if `sets` is empty (a LIST), fold in constant 3499 (anti-collision),
> then if `trie` is non-empty fold in `trie_rehash(trie)`, then if `trie_special`
> is non-empty fold in `trie_rehash(trie_special)`. Otherwise (a SET), fold in
> constant 2683, then fold in each set number in `sets` in order, then each op in
> `set_ops` in order.
> Stores `retval` into `hash`. If the static `Set::dump_hashes_out` stream is set,
> prints a `DEBUG: Hash <hash> for set <name> (LIST|SET)` line (LIST when `sets`
> empty, else SET). Returns `retval`.

> [spec:cg3:def:set.cg3.set.reindex-fn]
> void Set::reindex(Grammar& grammar)

> [spec:cg3:sem:set.cg3.set.reindex-fn]
> Recomputes the derived special-type flags on `type` from the trie contents and
> child sets. First clears `ST_SPECIAL` and `ST_CHILD_UNIFY` from `type`. ORs in
> `trie_reindex(trie)` and `trie_reindex(trie_special)` (each contributes
> `ST_SPECIAL`/`ST_MAPPING` per the tags they contain). For each child set number
> `s` in `sets`: look it up via `grammar.sets_by_contents.find(s)->second` (no
> presence check — dereferences the iterator, assuming `s` exists), recursively
> `set->reindex(grammar)`, then propagate: child `ST_SPECIAL` -> self `ST_SPECIAL`;
> child having any of `ST_TAG_UNIFY|ST_SET_UNIFY|ST_CHILD_UNIFY` -> self
> `ST_CHILD_UNIFY`; child `ST_MAPPING` -> self `ST_MAPPING`. Finally, if `type`
> now has any of `ST_TAG_UNIFY|ST_SET_UNIFY|ST_CHILD_UNIFY`, force both
> `ST_SPECIAL` and `ST_CHILD_UNIFY` on. Leaves all other bits (including this
> set's own `ST_TAG_UNIFY`/`ST_SET_UNIFY`, which are set by the parser) untouched.
> No return value.

> [spec:cg3:def:set.cg3.set.set-fn]
> ~Set()

> [spec:cg3:sem:set.cg3.set.set-fn]
> Destructor. Calls `trie_delete(trie)` and `trie_delete(trie_special)`, which
> recursively descend both tries and `reset()` every child sub-trie `unique_ptr`,
> freeing the nested nodes. The top-level `flat_map`s and the remaining members
> (`ff_tags`, `sets`, `set_ops`, `name`) are then torn down by their own
> destructors. No return value.

> [spec:cg3:def:set.cg3.set.set-name-fn]
> void Set::setName(uint32_t to)

> [spec:cg3:sem:set.cg3.set.set-name-fn]
> Builds this set's synthetic `name` (the `uint32_t` overload). If `to == 0`,
> replace `to` with `UI32(rand())` (a fresh pseudo-random id from libc `rand()`).
> `sprintf` into the shared global scratch buffer `cbuffers[0]` with format
> `"_G_%u_%u_"` using `line` then `to`, capturing the returned character count `n`
> (bytes written, excluding the terminating NUL). Then `name.reserve(n)` and
> `name.assign(&cbuffers[0][0], &cbuffers[0][0] + n)`, so `name` becomes
> `_G_<line>_<to>_`. Side effect: overwrites `cbuffers[0]`. (The sibling overloads
> not covered by this id — `setName(const UChar*)` and `setName(const UString&)` —
> copy a non-empty argument into `name` and otherwise call `setName()` to
> generate; `setName(const UStringView&)` forwards to the `UChar*` overload.)

> [spec:cg3:def:set.cg3.setuint32-hash-map]
> typedef std::unordered_map<uint32_t, Set*> Setuint32HashMap

> [spec:cg3:def:set.cg3.trie-reindex-fn]
> inline uint8_t trie_reindex(const trie_t& trie)

> [spec:cg3:sem:set.cg3.trie-reindex-fn]
> Free helper. Walks a `trie_t` and returns the accumulated special flags as a
> `uint8_t`, starting from 0. For each entry `kv`: if the key tag's `type` has
> `T_SPECIAL`, OR in `ST_SPECIAL`; if it has `T_MAPPING`, OR in `ST_MAPPING`; if
> the entry has a child sub-trie (`kv.second.trie` non-null), recurse into it and
> OR in the result. Returns the accumulated flags. Only `ST_SPECIAL` (2) and
> `ST_MAPPING` (32) can be set, both < 256, so the `uint8_t` return is lossless.

