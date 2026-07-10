# src/Rule.cpp, src/Rule.hpp

> [spec:cg3:def:rule.cg3.init-flag-excls-fn]
> constexpr auto init_flag_excls(rule_flags_t v)

> [spec:cg3:sem:rule.cg3.init-flag-excls-fn]
> `constexpr` helper mapping a flag bit-index `v` (0..FLAGS_COUNT-1) to the
> mutual-exclusion mask it belongs to. Iterates the static `flag_excls[]` table
> (each entry is an OR of flags that are mutually exclusive, e.g. `RF_NEAREST |
> RF_ALLOWLOOP`, `RF_DELAYED | RF_IMMEDIATE | RF_IGNORED`, ...). For each `excl`,
> if `excl & (static_cast<rule_flags_t>(1) << v)` is non-zero (i.e. the flag
> whose bit index is `v` is a member of that group), returns `excl` immediately.
> If `v` belongs to no group, returns `static_cast<rule_flags_t>(0)`. Consumed by
> `make_array<FLAGS_COUNT>(init_flag_excls)` to precompute `_flags_excls[v]` =
> the mask of flags mutually exclusive with (and including) flag `v`.

> [spec:cg3:def:rule.cg3.rule]
> class Rule {
>   UString name;
>   Tag* wordform = nullptr;
>   uint32_t target = 0;
>   uint32_t childset1 = 0, childset2 = 0;
>   uint32_t line = 0, number = 0;
>   uint32_t varname = 0, varvalue = 0;
>   uint64_t flags = 0;
>   int32_t section = 0;
>   int32_t sub_reading = 0;
>   KEYWORDS type = K_IGNORE;
>   Set* maplist = nullptr;
>   Set* sublist = nullptr;
>   RuleVector sub_rules;
>   mutable ContextList tests;
>   mutable ContextList dep_tests;
>   mutable ContextualTest* dep_target = nullptr;
> }

> [spec:cg3:def:rule.cg3.rule-by-line-hash-map]
> typedef std::unordered_map<uint32_t, Rule*> RuleByLineHashMap

> [spec:cg3:def:rule.cg3.rule-by-line-map]
> typedef std::map<uint32_t, Rule*> RuleByLineMap

> [spec:cg3:def:rule.cg3.rule-flags]
> enum RULE_FLAGS : uint64_t {
>   RF_NEAREST = (1 << 0);
>   RF_ALLOWLOOP = (1 << 1);
>   RF_DELAYED = (1 << 2);
>   RF_IMMEDIATE = (1 << 3);
>   RF_LOOKDELETED = (1 << 4);
>   RF_LOOKDELAYED = (1 << 5);
>   RF_UNSAFE = (1 << 6);
>   RF_SAFE = (1 << 7);
>   RF_REMEMBERX = (1 << 8);
>   RF_RESETX = (1 << 9);
>   RF_KEEPORDER = (1 << 10);
>   RF_VARYORDER = (1 << 11);
>   RF_ENCL_INNER = (1 << 12);
>   RF_ENCL_OUTER = (1 << 13);
>   RF_ENCL_FINAL = (1 << 14);
>   RF_ENCL_ANY = (1 << 15);
>   RF_ALLOWCROSS = (1 << 16);
>   RF_WITHCHILD = (1 << 17);
>   RF_NOCHILD = (1 << 18);
>   RF_ITERATE = (1 << 19);
>   RF_NOITERATE = (1 << 20);
>   RF_UNMAPLAST = (1 << 21);
>   RF_REVERSE = (1 << 22);
>   RF_SUB = (1 << 23);
>   RF_OUTPUT = (1 << 24);
>   RF_CAPTURE_UNIF = (1 << 25);
>   RF_REPEAT = (1 << 26);
>   RF_BEFORE = (1 << 27);
>   RF_AFTER = (1 << 28);
>   RF_IGNORED = (1 << 29);
>   RF_LOOKIGNORED = (1 << 30);
>   RF_NOMAPPED = (1ull << 31);
>   RF_NOPARENT = (1ull << 32);
>   RF_DETACH = (1ull << 33);
> }

> [spec:cg3:def:rule.cg3.rule-vector]
> typedef std::vector<Rule*> RuleVector

> [spec:cg3:def:rule.cg3.rule.add-contextual-test-fn]
> void Rule::addContextualTest(ContextualTest* to, ContextList& head)

> [spec:cg3:sem:rule.cg3.rule.add-contextual-test-fn]
> Prepends the `ContextualTest*` `to` onto the FRONT of the passed-in
> `ContextList& head` (a `std::list<ContextualTest*>`) via `head.push_front(to)`.
> Operates on whatever list reference is supplied (e.g. `tests` or `dep_tests`),
> not implicitly `this->tests`. No return value. Because tests are pushed to the
> front, list order is reverse of insertion order (see
> `reverseContextualTests`).

> [spec:cg3:def:rule.cg3.rule.reverse-contextual-tests-fn]
> void Rule::reverseContextualTests()

> [spec:cg3:sem:rule.cg3.rule.reverse-contextual-tests-fn]
> Reverses, in place, both of the rule's contextual-test lists: calls
> `tests.reverse()` then `dep_tests.reverse()` (`std::list::reverse`). No return
> value. Typically used to restore source order after tests were accumulated via
> front-insertion.

> [spec:cg3:def:rule.cg3.rule.rule-fn]
> Rule() = default

> [spec:cg3:sem:rule.cg3.rule.rule-fn]
> Compiler-defaulted constructor (`Rule() = default`): no custom logic. Every
> member takes its in-class initializer — `name` empty; `wordform = nullptr`;
> `target = 0`; `childset1 = childset2 = 0`; `line = number = 0`; `varname =
> varvalue = 0`; `flags = 0`; `section = 0`; `sub_reading = 0`; `type =
> K_IGNORE`; `maplist = sublist = nullptr`; `sub_rules`, `tests`, `dep_tests`
> default-empty; `dep_target = nullptr`.

> [spec:cg3:def:rule.cg3.rule.set-name-fn]
> void Rule::setName(const UChar* to)

> [spec:cg3:sem:rule.cg3.rule.set-name-fn]
> Sets the rule's `name`. First `name.clear()`; then if `to` is non-null,
> `name = to` (assign/copy from the NUL-terminated `UChar*`). If `to` is null,
> `name` is left empty.

