# src/icu_uoptions.cpp

> [spec:cg3:def:icu-uoptions.u-parse-args-fn]
> int u_parseArgs(int argc, char* argv[],

> [spec:cg3:sem:icu-uoptions.u-parse-args-fn]
> A getopt-style in-place command-line parser (ICU-derived). It scans
> `argv[1..argc)`, matching option tokens against the `options[0..optionCount)`
> array, recording each match in the matched UOption (`doesOccur=1`, and
> `value` set to the argument string when one is consumed), and compacting all
> non-option arguments to the front of `argv`. `argv[0]` (program name) is
> never read or modified.
> Algorithm: maintain `i=1` (read cursor), `remaining=1` (write cursor for
> kept non-options), and `stopOptions=0`. Loop while `i < argc`, taking
> `arg = argv[i]`:
> - If NOT stopOptions AND `arg[0]=='-'` AND `arg[1]` (call it `c`) is nonzero,
>   the token is an option. Advance `arg += 2` (past "-X").
>   - If `c=='-'` this is a long option. If `*arg==0` (the token was exactly
>     "--"), set `stopOptions=1` (everything after is treated as non-options)
>     and fall through. Otherwise linear-search `options` for the first entry
>     whose `longName` equals `arg` (via `uprv_strcmp`, exact match). If none
>     matches, return `-i` (negative index of the offending arg). Set
>     `option->doesOccur=1`. If `option->hasArg != UOPT_NO_ARG`: if there is a
>     next arg (`i+1 < argc`) that is NOT itself an option token (the test is
>     `!(argv[i+1][0]=='-' && argv[i+1][1]!=0)`, so a bare "-" IS accepted as a
>     value), consume it: `option->value = argv[++i]`. Else if
>     `hasArg==UOPT_REQUIRES_ARG` (required but missing), return `-i`.
>     (UOPT_OPTIONAL_ARG with no available value leaves `value` empty.)
>   - Else (short option(s)): loop over the letters remaining in this token.
>     Linear-search `options` for `shortName==c`; if none, return `-i`; set
>     `doesOccur=1`. If `hasArg != UOPT_NO_ARG`: if `*arg != 0` (characters
>     follow in the same argv entry), the rest of the token is the argument:
>     `option->value = arg`, and break (do not treat those chars as further
>     option letters). Else if a usable next arg exists (same non-option test
>     as above), `option->value = argv[++i]`, break. Else if
>     `UOPT_REQUIRES_ARG`, return `-i`. If the option takes no argument (or an
>     optional one that was absent), advance to the next letter: `option=NULL;
>     c = *arg++;` and continue while `c != 0`.
>   - After resolving the option, if `option` is non-null and it has a non-null
>     `optionFn` callback, call `option->optionFn(option->context, option)`; if
>     that returns `< 0`, return `-i`.
>   - `++i` to move to the next argv entry.
> - Otherwise the token is a non-option (a normal argument, a lone "-", or
>   anything after "--"): `argv[remaining++] = arg; ++i;` (compact it forward).
> Return `remaining` (count of surviving non-option strings, including
> `argv[0]`); a negative return is `-index` of the argv entry where parsing
> failed. Repeated options keep the last value (later occurrences overwrite).
> NOTE (faithfulness): this translation unit (`src/icu_uoptions.cpp`) is NOT in
> the CMake build and includes a non-existent `icu_uoptions.hpp`; it also reads
> `option->optionFn`/`option->context`, which are not members of the current
> `UOption` struct, so it would not even compile against the live header. The
> ACTUAL parser used by every binary is the identical inline `u_parseArgs` in
> `include/uoptions.hpp`, which is byte-for-byte the same logic MINUS the
> `optionFn` callback block and uses `strcmp`/`size_t optionCount`. Port the
> live (header) behavior; the callback path here is dead.

