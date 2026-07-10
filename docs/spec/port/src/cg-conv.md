# src/cg-conv.cpp

> [spec:cg3:def:cg-conv.main-fn]
> int main(int argc, char* argv[])

> [spec:cg3:sem:cg-conv.main-fn]
> Entry point for `cg-conv`, the stream format converter. It reads stdin,
> converts between CG-family formats via a `FormatConverter` applicator, and
> writes stdout. Uses the `options_conv` UOption array (enum in
> `options-conv.options-conv.options`), NOT the big vislcg3 options set.
> Steps:
> - `u_init(&status)`; on non-`U_FILE_ACCESS_ERROR` failure, print ICU error to
>   cerr and `CG3Quit(1)`.
> - `argc = u_parseArgs(argc, argv, options_conv.size(), options_conv.data())`.
> - Env overlay: `parse_opts_env("CG3_CONV_DEFAULT", options_default)` and
>   `parse_opts_env("CG3_CONV_OVERRIDE", options_override)`. Then for each index
>   `i`: if `options_default[i]` occurred and `options_conv[i]` did not, copy
>   the default into `options_conv[i]`; if `options_override[i]` occurred, copy
>   it into `options_conv[i]` unconditionally (override wins).
> - Help/error: if `argc < 0` OR HELP1 (`--help`/`-h`) OR HELP2 (`--?`/`-?`):
>   choose `out=stderr` if `argc<0` else `stdout`; print `"Usage: cg-conv
>   [OPTIONS]"`, the two env-var descriptions (`CG3_CONV_DEFAULT`,
>   `CG3_CONV_OVERRIDE`), then the aligned option list (same layout as the other
>   tools: skip options with empty description or whose description starts with
>   `'!'`; pad long names to the longest), then a fixed "Keys for JSONL format"
>   legend block. Return `argc<0 ? U_ILLEGAL_ARGUMENT_ERROR : U_ZERO_ERROR`.
> - Aliases: if IN_CG2 (`-v`) occurred, force IN_CG on; if OUT_CG2 (`-V`)
>   occurred, force OUT_CG on.
> - `ucnv_setDefaultName("UTF-8")`; save `codepage_default = ucnv_getDefaultName()`
>   (now "UTF-8"); `uloc_setDefault("en_US_POSIX", &status)`.
> - Construct `FormatConverter applicator(std::cerr)`; take a reference to its
>   internal `conv_grammar` as `grammar`. If ORDERED (`-o`) occurred, set
>   `grammar.ordered = true`.
> - `ux_stripBOM(std::cin)` (consume a leading UTF-8 BOM if present). Default
>   `instream = &std::cin`.
> - Input format selection into `fmt` (default `CG3SF_INVALID`): if ADD_TAGS
>   (`--add-tags`) occurred, force IN_PLAIN on and set the (Plaintext) applicator
>   `add_tags=true`. Then, in order, the first of IN_CG(`-c`)→CG3SF_CG,
>   IN_NICELINE(`-n`)→NICELINE, IN_APERTIUM(`-a`)→APERTIUM, IN_FST(`-f`)→FST,
>   IN_PLAIN(`-x`)→PLAIN, IN_JSONL(`-j`)→JSONL, IN_BINARY(`-z`)→BINARY sets
>   `fmt`. If IN_AUTO (`-u`) occurred OR `fmt==CG3SF_INVALID`, call
>   `applicator.detectFormat(std::cin)` (returns a wrapping istream, used as
>   `instream`) and take `fmt = applicator.fmt_input`. Finally
>   `applicator.fmt_input = fmt`.
> - If SUB_LTR (`-l`) occurred, set `grammar.sub_readings_ltr = true` (SUB_RTL
>   `-r` is the default and has no effect here).
> - If MAPPING_PREFIX (`-p`) occurred: convert its value from the default
>   codepage to UTF-16 via an ICU converter and set `grammar.mapping_prefix` to
>   the FIRST resulting UChar (`buf[0]`).
> - If SUB_DELIMITER (`-S`) occurred: convert its value to UTF-16 into
>   `applicator.sub_delims`, resize to the converted length, then append a `'+'`.
> - If FST_WTAG (`--wtag`) occurred: convert its value to UTF-16 into
>   `applicator.wtag`.
> - If FST_WFACTOR (`-W`) occurred: `applicator.wfactor = std::stod(value)`.
> - Output format: default `applicator.fmt_output = CG3SF_CG`; then the first of
>   OUT_APERTIUM(`-A`)→APERTIUM (also sets `unicode_tags=true`),
>   OUT_FST(`-F`)→FST, OUT_NICELINE(`-N`)→NICELINE, OUT_PLAIN(`-X`)→PLAIN,
>   OUT_JSONL(`-J`)→JSONL, OUT_BINARY(`-Z`)→BINARY. (OUT_MATXIN `-M` is present
>   in the option table but NOT handled here, so `-M` silently leaves output as
>   CG — a quirk.)
> - If UNICODE_TAGS occurred, set `unicode_tags=true`; if PIPE_DELETED
>   (`--deleted`) occurred, set `pipe_deleted=true`; if NO_BREAK (`-B`) occurred,
>   set `add_spacing=false`.
> - If PARSE_DEP (`-D`) occurred, set `parse_dep=true` and `has_dep=true`. If
>   DEP_DELIMIT occurred: if it has a value, `dep_delimit = stoul(value)`, else
>   `dep_delimit = 10`; and set `parse_dep=true`.
> - Set `applicator.is_conv=true`, `applicator.trace=true`,
>   `applicator.verbosity_level=0`, then
>   `applicator.runGrammarOnText(*instream, std::cout)`. `u_cleanup()`. No
>   explicit return (falls off end → 0).

