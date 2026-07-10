# src/cg-comp.cpp

> [spec:cg3:def:cg-comp.end-program-fn]
> void endProgram(char* name)

> [spec:cg3:sem:cg-comp.end-program-fn]
> Prints usage/help for `cg-comp` and terminates the process. If `name` is
> non-null, prints to stdout: `"VISL CG-3 Compiler version
> <MAJOR>.<MINOR>.<PATCH>.<REVISION>\n"` (from the CG3_VERSION_* / CG3_REVISION
> macros), then `"<basename(name)>: compile a binary grammar from a text
> file"`, then `"USAGE: <basename(name)> grammar_file output_file"` (each via
> `std::cout << ... << std::endl`; `basename` is POSIX `libgen` basename on
> non-Windows). Regardless of whether `name` was null, calls
> `exit(EXIT_FAILURE)` (does not return).

> [spec:cg3:def:cg-comp.main-fn]
> int main(int argc, char* argv[])

> [spec:cg3:sem:cg-comp.main-fn]
> Entry point for `cg-comp`: parses a textual CG grammar and writes it out as a
> binary `.cg3b` grammar. Positional-only interface (no option flags).
> Steps:
> - `status=U_ZERO_ERROR`. If `argc != 3` (i.e. not exactly `cg-comp
>   grammar_file output_file`), call `endProgram(argv[0])` (prints usage, exits
>   failure).
> - `u_init(&status)`; if `U_FAILURE(status)` and status is not
>   `U_FILE_ACCESS_ERROR`, print the ICU init error to cerr and `CG3Quit(1)`.
>   Reset `status=U_ZERO_ERROR`. `ucnv_setDefaultName("UTF-8")`;
>   `uloc_setDefault("en_US_POSIX", &status)`.
> - Construct an empty `Grammar grammar`. Open `argv[1]` with `fopen(...,"rb")`;
>   if null, print `"Error: Error opening <argv[1]> for reading!"` and
>   `CG3Quit(1)`. Read the first 4 bytes into `cbuffers[0]`; if the read count
>   is not 4, print `"Error: Error reading first 4 bytes from grammar!"` and
>   `CG3Quit(1)`. `fclose`.
> - If `is_cg3b(cbuffers[0])` (first 4 bytes are the magic `"CG3B"`), print
>   `"Binary grammar detected. Cannot re-compile binary grammars."` and
>   `CG3Quit(1)`. Otherwise create `parser = new TextualParser(grammar,
>   std::cerr)`.
> - Set `grammar.ux_stderr = &std::cerr`. Call `parser->parse_grammar(argv[1])`;
>   if it returns nonzero (failure), print `"Error: Grammar could not be parsed
>   - exiting!"` and `CG3Quit(1)`.
> - `grammar.reindex()`. Print a stats line to cerr: `"Sections: <n>, Rules:
>   <n>, Sets: <n>, Tags: <n>"` (sizes of `grammar.sections`,
>   `grammar.rule_by_number`, `grammar.sets_list`, `grammar.single_tags`). If
>   `grammar.rules_any` is set, print `"<size> rules cannot be skipped by
>   index."`. If `grammar.has_dep`, print `"Grammar has dependency rules."`.
> - Open `argv[2]` as `std::ofstream(..., std::ios::binary)`. If the stream is
>   good, construct `BinaryGrammar writer(grammar, std::cerr)` and call
>   `writer.writeBinaryGrammar(gout)`. Otherwise print `"Could not write grammar
>   to <argv[2]>"` (no quit — falls through).
> - `u_cleanup()`. `return status` (the ICU `UErrorCode`, which is
>   `U_ZERO_ERROR`==0 on success). Note: returning the raw `UErrorCode` enum as
>   the process exit code.

