# src/main.cpp

> [spec:cg3:def:main.main-fn]
> int main(int argc, char* argv[])

> [spec:cg3:sem:main.main-fn]
> Entry point for the `vislcg3` disambiguator binary. Parses CLI + env +
> grammar-embedded options into the global `options` UOption array (enum in
> `options.options.options`; option table in `src/options.cpp`), loads a
> textual or binary grammar, optionally runs it over an input stream via a
> `FormatConverter` applicator, and optionally writes grammars/profile out.
> Startup: `main_timer = clock()`; `status=U_ZERO_ERROR`; `srand(time(0))` (seed
> the C RNG used by random rule selection). `argc = u_parseArgs(argc, argv,
> options.size(), options.data())`; default `out = stderr`.
> Env overlay: `parse_opts_env("CG3_DEFAULT", options_default)` and
> `parse_opts_env("CG3_OVERRIDE", options_override)`. Then per-index merge: if
> `options_default[i]` occurred and `options[i]` did not, copy the default in;
> if `options_override[i]` occurred, overwrite `options[i]` (override always
> wins).
> Early exits:
> - If VERSION_TOO_OLD (`--min-binary-revision`): print `CG3_TOO_OLD` to cout,
>   `return 0`.
> - If VERSION (`-V`/`--version`) OR HELP1 (`-h`/`--help`) OR HELP2 (`--?`): set
>   `out=stdout` and print `"VISL CG-3 Disambiguator version M.m.p.r\n"` (this
>   version line is emitted for version AND help; the comment notes that
>   `vislcg3 --version | grep -Eo '[0-9]+$'` must keep yielding just the
>   revision).
> - If `argc < 0` (u_parseArgs error): `fprintf(stderr, "%s: error in command
>   line argument \"%s\"\n", argv[0], argv[-argc])` — `argv[-argc]` indexes the
>   offending argument (u_parseArgs returned the negative failing index). Then
>   `return argc` (a negative exit code).
> - If VERSION: print `CG3_COPYRIGHT_STRING` to `out`; `return U_ZERO_ERROR`.
> - If NOT GRAMMAR (`-g`) and not HELP1/HELP2: print `"Error: No grammar
>   specified - cannot continue!\n"` to stderr and set `argc = -argc` (forces the
>   error/help branch below).
> - If `argc < 0` OR HELP1 OR HELP2: print full usage: `"Usage: vislcg3
>   [OPTIONS]"`, the `CG3_DEFAULT`/`CG3_OVERRIDE` env-var descriptions, then
>   `"Options:"` and the aligned list of every option with a non-empty
>   description (compute the longest `longName`; print `"-<short>,"` or three
>   spaces, `" --<longName>"`, padding spaces, `"  <description>"`). Return
>   `argc<0 ? U_ILLEGAL_ARGUMENT_ERROR : U_ZERO_ERROR`.
> - `fflush(out); fflush(stderr)`.
> Option implications:
> - If SHOW_UNUSED_SETS or SHOW_SET_HASHES or DUMP_AST occurred → force
>   GRAMMAR_ONLY on.
> - If GRAMMAR_ONLY → if VERBOSE not set, force VERBOSE on.
> - If QUIET → force VERBOSE off.
> - If VERBOSE occurred with value exactly `"0"` → force VERBOSE off.
> ICU/codepage: `u_init` (on non-file-access failure → error + `CG3Quit(1)`);
> reset status. Save `codepage_cli = ucnv_getDefaultName()` BEFORE
> `ucnv_setDefaultName("UTF-8")`. If any of CODEPAGE_GLOBAL (`-C`),
> CODEPAGE_INPUT, CODEPAGE_OUTPUT, CODEPAGE_GRAMMAR occurred, warn `"The -C and
> --codepage-* option are deprecated and now default to UTF-8"`.
> `uloc_setDefault("en_US_POSIX", &status)`. `conv = ucnv_open(codepage_cli,
> &status)` — a converter for the ORIGINAL CLI codepage, used to widen option
> values (nrules regexes, mapping prefix) into UTF-16.
> Streams (each: if the corresponding option is set, open a binary
> file-stream, and on `bad()` print a "Failed to open the ... stream" error and
> `CG3Quit(1)`; otherwise use the standard stream):
> - `ux_stdout` ← STDOUT (`-O`) or `std::cout`.
> - `ux_stderr` ← STDERR (`-E`) or `std::cerr`.
> - `ux_stdin`  ← STDIN (`-I`) or `std::cin`; for STDIN it first `stat()`s the
>   path and, on stat error, prints `"Cannot stat <path> due to error <n>!"` and
>   `CG3Quit(1)` before opening.
> Grammar setup: construct `Grammar grammar`. If SHOW_TAG_HASHES →
> `Tag::dump_hashes_out = ux_stderr`; if SHOW_SET_HASHES → `Set::dump_hashes_out
> = ux_stderr`. Open `options[GRAMMAR].value` `"rb"`; on null → error + quit;
> read 4 bytes into `cbuffers[0]`; if not 4 → error + quit; close.
> - If `is_cg3b(cbuffers[0])` (binary grammar): if VERBOSE print `"Info: Binary
>   grammar detected."`; if DUMP_AST → error `"--dump-ast is for textual
>   grammars only!"` + quit; if PROFILING → error `"--profile is for textual
>   grammars only!"` + quit; else `parser = new BinaryGrammar(...)`.
> - Else `parser = new TextualParser(grammar, *ux_stderr, dump_ast_flag)` where
>   `dump_ast_flag = DUMP_AST.doesOccur`.
> Verbosity: if VERBOSE, and its value is non-empty, `verbosity_level =
> stoul(value)` and apply to both `parser->setVerbosity` and
> `grammar.verbosity_level`; else use level 1. Set `grammar.ux_stderr/ux_stdout`;
> `parser->setCompatible(VISLCGCOMPAT (`-2`))`. If VERBOSE, print `"Initialization
> took <t> seconds."` and reset `main_timer`.
> Rule-name regex filters (ICU `uregex`, the Rust port must reproduce with the
> `regex` crate): if NRULES (`--nrules`) set, `ucnv_reset(conv)`, widen its value
> to UTF-16, `parser->nrules = uregex_open(...)`; if the resulting `status !=
> U_ZERO_ERROR`, print `"Error: uregex_open returned <name> trying to parse
> --nrules <pattern>"` and `CG3Quit(1)`. Same for NRULES_INV (`--nrules-v`) into
> `parser->nrules_inv` (error message says `--nrules-v`). These select which
> rule NAMES are parsed/run (nrules = allow, nrules_inv = deny).
> Profiling: if PROFILING (`--profile`) set, `profiler = new Profiler` and assign
> it to the TextualParser's `profiler` pointer (valid only for textual grammars,
> enforced above).
> Parse: `parser->parse_grammar(options[GRAMMAR].value)`; on failure → error
> `"Grammar could not be parsed - exiting!"` + quit.
> Grammar-embedded cmdargs: if `grammar.cmdargs` is non-empty, copy it, append a
> `0`, and `parse_opts(...)` into `grammar_options_default`; likewise
> `grammar.cmdargs_override` into `grammar_options_override`. Merge: for each `i`,
> if `grammar_options_default[i]` occurred and `options[i]` did not → copy it in;
> if `grammar_options_override[i]` occurred AND `options_override[i]` did NOT →
> copy it in (grammar-level override only applies where the env override didn't).
> Post-parse: if DUMP_AST → `parser->print_ast(*ux_stdout)`. If PROFILING → clear
> `profiler->buf`, `print_ast(buf)`, `profiler->grammar_ast =
> profiler->addString(buf.str())` (this AST string later gets id 0 in the DB).
> Mapping prefix (`-p`): if MAPPING_PREFIX set, widen its value to UTF-16; if the
> grammar is binary and its existing `mapping_prefix != buf[0]`, error `"Mapping
> prefix must match the one used for compiling the binary grammar!"` + quit;
> then set `grammar.mapping_prefix = buf[0]` (the first UChar only).
> Reindex: if VERBOSE print `"Reindexing grammar..."`; call
> `grammar.reindex(SHOW_UNUSED_SETS==1, SHOW_TAGS==1)`. `parser.reset()` (free
> the parser). If VERBOSE print parse timing and grammar stats (sections,
> templates, rules, sets, tags; `rules_any` count; `has_dep`; `has_relations`).
> If PROFILING and GRAMMAR_ONLY → error `"Cannot gather profiling data with no
> input to run grammar on."` + quit.
> Run (only if NOT GRAMMAR_ONLY): construct `FormatConverter applicator(*ux_stderr)`.
> Input format defaults to `CG3SF_CG`, overridden by the first set of IN_CG,
> IN_NICELINE, IN_APERTIUM, IN_FST, IN_PLAIN, IN_JSONL, IN_BINARY.
> `applicator.setGrammar(&grammar)`; `applicator.setOptions(conv)` (this
> transfers the FULL remaining option set — sections, rules, trace/trace-*,
> dry-run, limits, dependency, num-windows, etc. — into the applicator; see
> `GrammarApplicator::setOptions`). Output format defaults to `CG3SF_CG`,
> overridden by the first set of OUT_APERTIUM (also sets `unicode_tags=true`),
> OUT_FST, OUT_NICELINE, OUT_PLAIN, OUT_JSONL, OUT_BINARY. If PROFILING, assign
> `applicator.profiler`. `applicator.runGrammarOnText(*ux_stdin, *ux_stdout)`. If
> VERBOSE, print apply timing.
> Grammar output: if GRAMMAR_OUT (`--grammar-out`) set, open the value binary; if
> good, `GrammarWriter(grammar,*ux_stderr).writeGrammar(gout)` (VERBOSE timing);
> else `"Could not write grammar to <path>"`. If GRAMMAR_BIN (`--grammar-bin`)
> set, same with `BinaryGrammar(...).writeBinaryGrammar`.
> Profile output: if PROFILING, `profiler->write(options[PROFILING].value)`
> (SQLite DB).
> Teardown: `ucnv_close(conv)`; `u_cleanup()`; if VERBOSE print cleanup timing;
> `return status` (`U_ZERO_ERROR`==0 on success).

