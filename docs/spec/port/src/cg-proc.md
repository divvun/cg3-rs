# src/cg-proc.cpp

> [spec:cg3:def:cg-proc.end-program-fn]
> void endProgram(char* name)

> [spec:cg3:sem:cg-proc.end-program-fn]
> Prints usage/help for `cg-proc` and terminates. Unlike the other tools it
> always prints (no `name`-null guard on the body). Prints to stdout: `"VISL
> CG-3 Disambiguator version <MAJOR>.<MINOR>.<PATCH>.<REVISION>\n"`, then
> `"<basename(name)>: process a stream with a constraint grammar"`, then
> `"USAGE: <basename(name)> [-t] [-s] [-d] [-g] [-r rule] grammar_file
> [input_file [output_file]]"`, then `"Options:"` and a per-flag description
> block. The exact block text is chosen at compile time by `HAVE_GETOPT_LONG`:
> if defined it lists both long and short forms (`-d/--disambiguation`,
> `-s/--sections=NUM`, `-f/--stream-format=NUM` with the 0=VISL, 1=Apertium,
> 2=Matxin, 3=binary legend and "(default: 1)", `-r/--rule=NAME`,
> `-t/--trace`, `-w/--wordform-case`, `-n/--no-word-forms`, `-g/--generation`,
> `-1/--first`, `-z/--null-flush`, `-v/--version`, `-h/--help`); otherwise the
> short-only variant. Then `exit(EXIT_FAILURE)`.

> [spec:cg3:def:cg-proc.main-fn]
> int main(int argc, char* argv[])

> [spec:cg3:sem:cg-proc.main-fn]
> Entry point for `cg-proc`, the Apertium/Matxin/binary/VISL stream processor.
> Unlike the other tools it parses its OWN short flags with POSIX
> `getopt`/`getopt_long` (not `u_parseArgs`), then loads a (preferably binary)
> grammar and runs a format-specific applicator over an input stream.
> Local state and defaults: `trace=false`, `wordform_case=false`,
> `print_word_forms=true`, `delimit_lexical_units=true`, `surface_readings=false`,
> `only_first=false`, `cmd=0`, `sections=0`, `stream_format=1`, `single_rule=""`.
> Option parsing loop (`getopt_long` with optstring `"ds:f:tr:n1wvhz"` and the
> long table {disambiguation d, sections s, stream-format f(required),
> rule r, trace t, wordform-case w, no-word-forms n, generation g, version v,
> first 1, help h, null-flush z}; the non-long build uses optstring
> `"ds:f:tr:ing1wvhz"`). Cases:
> - `'d'`: if `cmd==0` set `cmd='d'`, else `endProgram(argv[0])` (repeat/conflict).
> - `'f'`: `stream_format = atoi(optarg)`.
> - `'t'`: `trace=true`.
> - `'r'`: copy `optarg` (including its NUL) into `single_rule`.
> - `'s'`: `sections = atoi(optarg)`.
> - `'n'`: `print_word_forms=false`.
> - `'g'`: `delimit_lexical_units=false`, `surface_readings=true`.
> - `'1'`: `only_first=true`.
> - `'w'`: `wordform_case=true`.
> - `'v'`: print the version banner to stdout and `exit(EXIT_SUCCESS)`.
> - `'z'`: no-op (comment: null-flush is default).
> - `'h'`/default: `endProgram(argv[0])`.
> - BUG (faithfulness): the long-option table marks `--disambiguation` and
>   `--sections` as `no_argument` (arg flag 0) even though `-s` requires one via
>   the optstring. So invoking the LONG form `--sections 5` yields `optarg==NULL`,
>   and `case 's': sections = atoi(optarg)` calls `atoi(NULL)` → undefined
>   behavior / crash. `-s 5` (short) works. Also the non-long optstring has a
>   handler-less `'i'` that falls to `default`→`endProgram`.
> Then: `u_init(&status)` (on non-file-access failure → ICU error + `CG3Quit(1)`);
> `ucnv_setDefaultName("UTF-8")`; `uloc_setDefault("en_US_POSIX", &status)`.
> Env/grammar option overlay (uses the big vislcg3 `options` array + shadow
> arrays): `parse_opts_env("CG3_DEFAULT", options_default)` and
> `parse_opts_env("CG3_OVERRIDE", options_override)`; then per-index merge (env
> default fills gaps in `options`, env override always wins). NOTE: cg-proc
> never subsequently READS the merged `options[]` to configure the applicator,
> so this overlay (and the later grammar-cmdargs overlay) is effectively inert
> here — the applicator is driven only by the getopt-parsed locals.
> Positional args (getopt leaves them at `optind`): the grammar file is
> `argv[optind]` and is REQUIRED — if `optind > argc-1`, `endProgram`. Open it
> `"rb"`; if null or `ferror`, `endProgram`; read 4 bytes into `cbuffers[0]`; if
> not 4, error + `CG3Quit(1)`; close. Optional input file `argv[optind+1]` (if
> `optind <= argc-2`): open binary `ifstream` as `ux_stdin`, else `std::cin`; on
> bad stream `endProgram`. Optional output file `argv[optind+2]` (if `optind <=
> argc-3`): open binary `ofstream` as `ux_stdout`, else `std::cout`; on bad
> stream `endProgram`.
> Parser: if `is_cg3b(cbuffers[0])`, `new BinaryGrammar`; else print a two-line
> warning ("Text grammar detected - to better process textual grammars, use
> `vislcg3'; to compile this grammar, use `cg-comp'") and use `TextualParser`.
> `grammar.ux_stderr=&std::cerr`; `parse_grammar(argv[optind])` (on failure,
> error + `CG3Quit(1)`); `grammar.reindex()`. Then the grammar-embedded
> cmdargs/cmdargs_override overlay via `parse_opts` (same inert merge as above).
> Applicator selection by `stream_format`:
> - `0`: plain `GrammarApplicator` (VISL/CG format).
> - `2`: `MatxinApplicator`; `setNullFlush(true)`; set `wordform_case`,
>   `print_word_forms`, `print_only_first(=only_first)`.
> - `3`: `BinaryApplicator`.
> - else (including default `1`): `ApertiumApplicator`; set `wordform_case`,
>   `print_word_forms`, `print_only_first`, `delimit_lexical_units`,
>   `surface_readings`.
> Then `applicator->setGrammar(&grammar)`; `setOptions()`; push sections `1..N`
> onto `applicator->sections` for `i=1..sections` (so `-s N` restricts to
> sections 1..N; `sections==0` pushes none → all sections run). Set
> `trace=trace`, `unicode_tags=true`, `unique_tags=false`.
> Single-rule (`-r`): if `single_rule` non-empty, convert it to a `UString`
> (`u_charsToUChars`, a byte→UChar widening, NOT UTF-8 decoding) and for every
> rule in `grammar->rule_by_number` whose `name` equals it, push `rule->number`
> onto `applicator->valid_rules`.
> Run: `try { switch(cmd){ case 'd': default: applicator->runGrammarOnText(
> *ux_stdin, *ux_stdout); } } catch (std::exception& e) { std::cerr << e.what();
> exit(1); }`. Finally `u_cleanup()`. No explicit return (falls off end → 0).

