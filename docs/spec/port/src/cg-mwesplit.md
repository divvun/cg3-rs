# src/cg-mwesplit.cpp

> [spec:cg3:def:cg-mwesplit.main-fn]
> int main(int argc, char** argv)

> [spec:cg3:sem:cg-mwesplit.main-fn]
> Entry point for the `cg-mwesplit` tool, which splits multi-word-expression
> cohorts on a CG stream. It loads NO grammar; it just runs a
> `MweSplitApplicator` over stdin→stdout.
> Steps:
> - `status=U_ZERO_ERROR`; `u_init(&status)`; if `U_FAILURE(status)` and status
>   is not `U_FILE_ACCESS_ERROR`, print `"Error: Cannot initialize ICU. Status =
>   <name>"` to cerr and `CG3Quit(1)`.
> - `argc = u_parseArgs(argc, argv, NUM_OPTIONS_MWE, options_mwe)` where
>   `options_mwe` has exactly two NO_ARG entries: `--help`/`-h` (HELP1) and
>   `--?`/`-?` (HELP2). (`NUM_OPTIONS_MWE` is 2; note the enum has 3 members but
>   only 2 UOptions are defined.)
> - If `argc < 0` (parse error) OR HELP1 OR HELP2 occurred: pick `out = stderr`
>   when `argc<0` else `stdout`. Print `"Usage: cg-mwesplit [OPTIONS]\n\n"`,
>   `"Options:\n"`, then an aligned option list: compute `longest` = max
>   `strlen(longName)` over options with a non-empty description; for each
>   option whose description is non-empty and does not begin with `'!'`, print
>   `" "`, then `"-<short>,"` if a shortName exists else three spaces, then
>   `" --<longName>"`, then `(longest - strlen(longName))` padding spaces, then
>   `"  <description>\n"`. Return `argc<0 ? U_ILLEGAL_ARGUMENT_ERROR :
>   U_ZERO_ERROR`.
> - Otherwise: `ucnv_setDefaultName("UTF-8")`; `uloc_setDefault("en_US_POSIX",
>   &status)`. Construct `MweSplitApplicator applicator(std::cerr)`; set
>   `applicator.verbosity_level = 0`; call `applicator.runGrammarOnText(std::cin,
>   std::cout)`; then `u_cleanup()`. There is no explicit `return` on this path,
>   so `main` falls off the end and returns 0. Non-option/positional args are
>   ignored (input is always stdin).

> [spec:cg3:def:cg-mwesplit.options-mwe.options]
> enum OPTIONS {
>   HELP1;
>   HELP2;
>   NUM_OPTIONS_MWE;
> }

