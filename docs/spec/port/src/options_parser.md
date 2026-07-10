# src/options_parser.hpp

> [spec:cg3:def:options-parser.options.parse-opts-env-fn]
> inline void parse_opts_env(const char* which, Opts& where)

> [spec:cg3:sem:options-parser.options.parse-opts-env-fn]
> Reads an environment variable and, if set, parses its value as a command
> line into the `where` UOption array. Calls `getenv(which)`; if it returns
> null (variable unset), does nothing. Otherwise copies the value into a
> `std::string env`, appends a single trailing NUL with `env.push_back(0)`
> (this extra NUL is REQUIRED: it guards `parse_opts`'s off-by-one read/write
> one byte past the last token — see that function), then calls
> `parse_opts(&env[0], where)` on the mutable buffer. Used by the tools to
> apply `CG3_DEFAULT`/`CG3_OVERRIDE` (and `CG3_CONV_DEFAULT`/
> `CG3_CONV_OVERRIDE`) into their default/override option arrays.

> [spec:cg3:def:options-parser.options.parse-opts-fn]
> inline void parse_opts(char* p, Opts& where)

> [spec:cg3:sem:options-parser.options.parse-opts-fn]
> Tokenizes a mutable, NUL-terminated C string `p` (a command line embedded in
> text, e.g. an env var or a grammar `CMDARGS` directive) into an argv vector,
> then feeds it to `u_parseArgs` to populate the `where` UOption array. The
> input buffer is MODIFIED IN PLACE (NULs written to terminate tokens).
> Steps: build `std::vector<char*> argv` sized 1 — element 0 is a placeholder
> program-name slot (a default-constructed, i.e. null, `char*`, never read by
> `u_parseArgs`). Loop while `*p`:
> - Skip leading whitespace: `while (*p && ISSPACE(*p)) ++p;` (ISSPACE is the
>   CG3 Unicode-aware single-char space test).
> - Then read one token by its first character:
>   - `'-'`: mark `n=p`, `SKIPTOWS(p)` (advance to the next whitespace; with
>     default flags this ALSO stops at an unescaped `;` and treats an unescaped
>     `#` as a to-end-of-line comment), write `*p=0`, push `n`, `++p`.
>   - `'"'`: `++p`, `n=p`, `SKIPTO(p,'"')` (advance to the next UNescaped double
>     quote, honoring backslash-escaping via ISESC), `*p=0`, push `n`, `++p`.
>   - `'\''`: same as above but delimited by an unescaped single quote.
>   - anything else: `n=p`, `SKIPTOWS(p)`, `*p=0`, push `n`, `++p` (identical to
>     the `'-'` case — the leading `-` is kept as part of the token).
> - Call `u_parseArgs(argv.size(), &argv[0], where.size(), where.data())`; the
>   return value is discarded (parse errors are silently ignored here).
> QUIRK/edge (faithfulness): when the final token runs to the end of the buffer,
> `SKIPTOWS` stops on the terminating NUL, `*p=0` rewrites that NUL, and the
> trailing `++p` advances `p` ONE BYTE PAST the terminator; the outer
> `while(*p)` then dereferences that byte. Callers must therefore pass a buffer
> with an extra trailing NUL (env callers do `push_back(0)`; grammar-cmdargs
> callers likewise append a `0`) so this lands on a second NUL and the loop
> stops cleanly. Quoted tokens include everything up to the matching quote
> (whitespace inside quotes is preserved); the closing quote is overwritten
> with NUL.

