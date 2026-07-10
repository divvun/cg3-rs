# src/cg-relabel.cpp

> [spec:cg3:def:cg-relabel.cg3-grammar-load-fn]
> Grammar* cg3_grammar_load(const char* filename, std::ostream& ux_stdout, std::ostream& ux_stderr, bool require_binary = false)

> [spec:cg3:sem:cg-relabel.cg3-grammar-load-fn]
> Loads a grammar file (binary or textual) into a freshly-allocated `Grammar`
> and returns a raw owning pointer, or `0` (null) on error. Like libcg3's
> loader but returns a non-void `Grammar*`. `require_binary` defaults to false.
> Steps:
> - Open `filename` as `std::ifstream(..., std::ios::binary)`. If the stream is
>   falsy, `u_fprintf(ux_stderr, "Error: Error opening %s for reading!\n",
>   filename)` and return `0`.
> - Read the first 4 bytes into `cbuffers[0]`. If the read fails,
>   `u_fprintf(... "Error: Error reading first 4 bytes from grammar!\n")` and
>   return `0`. Close the stream.
> - Allocate `Grammar* grammar = new Grammar`; set `grammar->ux_stderr =
>   &ux_stderr` and `grammar->ux_stdout = &ux_stdout`.
> - If `is_cg3b(cbuffers[0])` (magic `"CG3B"`), `parser = new
>   BinaryGrammar(*grammar, ux_stderr)`. Otherwise, if `require_binary` is true,
>   print `"Error: Text grammar detected -- to compile this grammar, use
>   `cg-comp'\n"` and `CG3Quit(1)`; else `parser = new TextualParser(*grammar,
>   ux_stderr)`.
> - Call `parser->parse_grammar(filename)`; if nonzero (failure), `u_fprintf(...
>   "Error: Grammar could not be parsed!\n")` and return `0` (NOTE: on this path
>   the `new Grammar` is leaked — parser is a unique_ptr but the raw grammar is
>   not freed).
> - `grammar->reindex()`; return `grammar`.

> [spec:cg3:def:cg-relabel.end-program-fn]
> void endProgram(char* name)

> [spec:cg3:sem:cg-relabel.end-program-fn]
> Prints usage/help for `cg-relabel` and terminates. If `name` is non-null,
> prints to stdout: `"VISL CG-3 Relabeller version
> <MAJOR>.<MINOR>.<PATCH>.<REVISION>\n"`, then `"<basename(name)>: relabel a
> binary grammar using a relabelling file"`, then `"USAGE: <basename(name)>
> input_grammar_file relabel_rule_file output_grammar_file"`. Regardless of
> `name`, calls `exit(EXIT_FAILURE)`.

> [spec:cg3:def:cg-relabel.main-fn]
> int main(int argc, char* argv[])

> [spec:cg3:sem:cg-relabel.main-fn]
> Entry point for `cg-relabel`: loads a binary grammar, applies a relabelling
> grammar to it, and writes the result as a new binary grammar. Positional-only
> (no flags): `cg-relabel input_grammar_file relabel_rule_file
> output_grammar_file`.
> Steps:
> - `status=U_ZERO_ERROR`. If `argc != 4`, `endProgram(argv[0])`.
> - `u_init(&status)`; on non-`U_FILE_ACCESS_ERROR` failure, print ICU error and
>   `CG3Quit(1)`; reset status. `ucnv_setDefaultName("UTF-8")`;
>   `uloc_setDefault("en_US_POSIX", &status)`.
> - `grammar = cg3_grammar_load(argv[1], std::cout, std::cerr, true)` — the input
>   grammar MUST be binary (`require_binary=true`; a text grammar triggers the
>   "use `cg-comp'" error and quit inside the loader).
> - `relabel_grammar = cg3_grammar_load(argv[2], std::cout, std::cerr)` — the
>   relabel rule file may be text or binary. Both are held in `std::unique_ptr`.
> - Construct `Relabeller relabeller(*grammar, *relabel_grammar, std::cerr)` and
>   call `relabeller.relabel()` to mutate `grammar` per the relabel rules.
> - Open `argv[3]` as `std::ofstream(..., std::ios::binary)`. If good, construct
>   `BinaryGrammar writer(*grammar, std::cerr)` and
>   `writer.writeBinaryGrammar(gout)`; else print `"Could not write grammar to
>   <argv[3]>"`.
> - `u_cleanup()`; `return status` (0 on success).
> - EDGE (faithfulness): there is NO null check on the loader results — if
>   `cg3_grammar_load` returns `0`, the `unique_ptr` wraps null and `*grammar` /
>   `*relabel_grammar` dereferences null, crashing.

