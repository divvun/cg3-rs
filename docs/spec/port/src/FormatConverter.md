# src/FormatConverter.cpp, src/FormatConverter.hpp

> [spec:cg3:def:format-converter.cg3.detect-format-fn]
> cg3_sformat detectFormat(std::string_view buf8)

> [spec:cg3:sem:format-converter.cg3.detect-format-fn]
> Free function that sniffs the stream format of a UTF-8 buffer `buf8` and
> returns a `cg3_sformat`. Order matters; the first match wins.
>
> 1. If `is_cg3bsf(buf8)` (first four bytes are `C`,`G`,`B`,`F`), return
>    `CG3SF_BINARY` immediately.
> 2. Convert `buf8` to UTF-16 into a `UString buffer` of capacity `BUF_SIZE`
>    (1000 UChars) via `u_strFromUTF8`; if `U_FAILURE(status)`, throw
>    `std::runtime_error("UTF-8 to UTF-16 conversion failed")`. Resize `buffer`
>    to the produced length `nr`. (`status` is NOT reset between the following
>    regex calls.)
> 3. Try each regex in turn with `uregex_openC`, `uregex_setText(buffer)`, and
>    `uregex_find(rx, -1, &status)`. `startIndex == -1` makes `uregex_find` an
>    UNANCHORED search over the whole text (finds a match anywhere). On a match,
>    set `fmt` and `break`; on no match, `uregex_close(rx)` and try the next.
>    The patterns and results, in order:
>    - `^"<[^>]+>".*?^\s+"[^"]+"` with flags `UREGEX_DOTALL | UREGEX_MULTILINE`
>      → `CG3SF_CG` (a `"<wordform>"` line followed later by an indented
>      `"baseform"` line).
>    - `^\S+ *\t *\[\S+\]` with `DOTALL | MULTILINE` → `CG3SF_NICELINE`.
>    - `^\S+ *\t *"\S+"` with `DOTALL | MULTILINE` → `CG3SF_NICELINE`.
>    - `\^[^/]+(/[^<]+(<[^>]+>)+)+\$` with `DOTALL | MULTILINE` →
>      `CG3SF_APERTIUM` (a `^...$` cohort with `/lemma<tag>...` readings; note
>      this pattern has no leading `^` anchor so it matches anywhere).
>    - `^\S+\t\S+(\+\S+)+$` with `DOTALL | MULTILINE` → `CG3SF_FST`.
>    - `^\{` with `MULTILINE` only (NO DOTALL) → `CG3SF_JSONL`.
>    - If none match, `fmt = CG3SF_PLAIN`.
> 4. After the loop, `uregex_close(rx)` once (closing whichever regex was last
>    opened — the matching one, or the JSONL one on the PLAIN fallthrough, since
>    that last branch is not closed inside the loop) and return `fmt`.
>
> Regex-parity notes for the Rust `regex` crate: `^`/`$` here are multiline
> (`(?m)`), matching at line boundaries; `UREGEX_DOTALL` = `(?s)` so `.` spans
> newlines; the searches are UNANCHORED (use `is_match`/`find`, never a
> fully-anchored match). `\S`/`\s` are Unicode-aware in ICU and also in the
> `regex` crate, so classes should agree. `\^` and `\$` are literal `^`/`$`.
> The CG pattern relies on lazy `.*?` spanning newlines (DOTALL) between the two
> anchored lines. This function NEVER returns `CG3SF_MATXIN` — Matxin input is
> not auto-detected.

> [spec:cg3:def:format-converter.cg3.format-converter]
> class FormatConverter : public ApertiumApplicator, public BinaryApplicator, public FSTApplicator, public JsonlApplicator, public MatxinApplicator, public Nic... {
>   Grammar conv_grammar;
> }

> [spec:cg3:def:format-converter.cg3.format-converter.detect-format-fn]
> std::unique_ptr<std::istream> FormatConverter::detectFormat(std::istream& in)

> [spec:cg3:sem:format-converter.cg3.format-converter.detect-format-fn]
> Member method that peeks at the head of an input stream to detect its format
> and returns a wrapped stream that replays those peeked bytes. Steps: read up
> to `BUF_SIZE` (1000) bytes into `std::string buf8` via `read_utf8(in,
> BUF_SIZE)` (which reads ~996 bytes plus enough trailing bytes to complete any
> partial UTF-8 sequence). Call the free `CG3::detectFormat(buf8)` and store the
> result in the member `fmt_input`. Construct a new `std::istream` whose
> streambuf is `new bstreambuf(in, std::move(buf8))` — the `bstreambuf` first
> serves the already-consumed `buf8` prefix and then continues reading from the
> underlying `in`, so downstream code sees the full stream from the beginning.
> Return that `std::unique_ptr<std::istream>`. Note the heap-allocated
> `bstreambuf` is not owned/deleted by the `std::istream`, so it is leaked
> (faithful to the original).

> [spec:cg3:def:format-converter.cg3.format-converter.format-converter-fn]
> FormatConverter::FormatConverter(std::ostream& ux_err)

> [spec:cg3:sem:format-converter.cg3.format-converter.format-converter-fn]
> Constructor. Initializes the (virtual) `GrammarApplicator(ux_err)` base plus
> all seven format-applicator bases — `ApertiumApplicator`, `BinaryApplicator`,
> `FSTApplicator`, `JsonlApplicator`, `MatxinApplicator`, `NicelineApplicator`,
> `PlaintextApplicator` — each passed `ux_err`. Because `GrammarApplicator` is a
> shared virtual base, it is constructed exactly once (by the most-derived
> class) despite each applicator naming it. Body: build a minimal working
> grammar in the member `conv_grammar` — set `conv_grammar.ux_stderr =
> &ux_err`, call `allocateDummySet()`, set `conv_grammar.delimiters =
> allocateSet()`, add a single dummy tag (`allocateTag(STR_DUMMY)`, i.e.
> `__CG3_DUMMY_STRINGBIT__`) to that delimiter set via `addTagToSet`, then
> `conv_grammar.reindex()`. Finally call `setGrammar(&conv_grammar)` to install
> it as the active grammar so the per-format applicators have a valid grammar
> with a delimiter to run against.

> [spec:cg3:def:format-converter.cg3.format-converter.print-cohort-fn]
> void FormatConverter::printCohort(Cohort* cohort, std::ostream& output, bool profiling)

> [spec:cg3:sem:format-converter.cg3.format-converter.print-cohort-fn]
> Dispatches cohort printing to the applicator matching the member
> `fmt_output`. `switch (fmt_output)`: `CG3SF_CG` →
> `GrammarApplicator::printCohort`; `CG3SF_APERTIUM` →
> `ApertiumApplicator::printCohort`; `CG3SF_FST` → `FSTApplicator::printCohort`;
> `CG3SF_NICELINE` → `NicelineApplicator::printCohort`; `CG3SF_PLAIN` →
> `PlaintextApplicator::printCohort`; `CG3SF_JSONL` →
> `JsonlApplicator::printCohort`; `CG3SF_BINARY` → do nothing (empty case, the
> binary path emits whole windows elsewhere); `default` → `CG3Quit()`. Each call
> forwards `cohort`, `output`, and `profiling`. `CG3SF_MATXIN` has no case and
> therefore hits `default` → `CG3Quit()`.

> [spec:cg3:def:format-converter.cg3.format-converter.print-plain-text-line-fn]
> void FormatConverter::printPlainTextLine(UStringView line, std::ostream& output)

> [spec:cg3:sem:format-converter.cg3.format-converter.print-plain-text-line-fn]
> Dispatches printing of a plain-text (non-cohort) line based on `fmt_output`.
> `switch (fmt_output)`: `CG3SF_JSONL` → `JsonlApplicator::printPlainTextLine`;
> `CG3SF_BINARY` → `BinaryApplicator::printPlainTextLine`; all other cases
> (`CG3SF_CG`, `CG3SF_APERTIUM`, `CG3SF_FST`, `CG3SF_NICELINE`, `CG3SF_PLAIN`,
> and `default`) → `GrammarApplicator::printPlainTextLine`. Forwards `line` and
> `output`. Only the JSONL and binary formats need special handling; every
> other format uses the base implementation.

> [spec:cg3:def:format-converter.cg3.format-converter.print-single-window-fn]
> void FormatConverter::printSingleWindow(SingleWindow* window, std::ostream& output, bool profiling)

> [spec:cg3:sem:format-converter.cg3.format-converter.print-single-window-fn]
> Dispatches whole-window printing based on `fmt_output`. `switch (fmt_output)`:
> `CG3SF_CG` → `GrammarApplicator::printSingleWindow`; `CG3SF_APERTIUM` →
> `ApertiumApplicator::printSingleWindow`; `CG3SF_FST` →
> `FSTApplicator::printSingleWindow`; `CG3SF_NICELINE` →
> `NicelineApplicator::printSingleWindow`; `CG3SF_PLAIN` →
> `PlaintextApplicator::printSingleWindow`; `CG3SF_JSONL` →
> `JsonlApplicator::printSingleWindow`; `CG3SF_BINARY` →
> `BinaryApplicator::printSingleWindow` (binary emits at window granularity);
> `default` → `CG3Quit()`. Forwards `window`, `output`, `profiling`.
> `CG3SF_MATXIN` has no case and falls to `default` → `CG3Quit()`.

> [spec:cg3:def:format-converter.cg3.format-converter.print-stream-command-fn]
> void FormatConverter::printStreamCommand(UStringView cmd, std::ostream& output)

> [spec:cg3:sem:format-converter.cg3.format-converter.print-stream-command-fn]
> Dispatches printing of a stream command (e.g. a `<STREAMCMD:...>` directive)
> based on `fmt_output`. `switch (fmt_output)`: `CG3SF_JSONL` →
> `JsonlApplicator::printStreamCommand`; `CG3SF_BINARY` →
> `BinaryApplicator::printStreamCommand`; all other cases (`CG3SF_CG`,
> `CG3SF_APERTIUM`, `CG3SF_FST`, `CG3SF_NICELINE`, `CG3SF_PLAIN`, and `default`)
> → `GrammarApplicator::printStreamCommand`. Forwards `cmd` and `output`. Only
> JSONL and binary need format-specific encoding; the rest use the base
> implementation.

> [spec:cg3:def:format-converter.cg3.format-converter.run-grammar-on-text-fn]
> void FormatConverter::runGrammarOnText(std::istream& input, std::ostream& output)

> [spec:cg3:sem:format-converter.cg3.format-converter.run-grammar-on-text-fn]
> Runs the conversion by dispatching input parsing to the applicator matching
> the detected `fmt_input`. Set `ux_stdin = &input`, `ux_stdout = &output`. If
> either `fmt_output == CG3SF_BINARY` or `fmt_input == CG3SF_BINARY`, set
> `grammar->has_relations = true` (binary streams carry relations). Then
> `switch (fmt_input)`: `CG3SF_CG` → `GrammarApplicator::runGrammarOnText`;
> `CG3SF_APERTIUM` → `ApertiumApplicator::runGrammarOnText`; `CG3SF_NICELINE` →
> `NicelineApplicator::runGrammarOnText`; `CG3SF_PLAIN` →
> `PlaintextApplicator::runGrammarOnText`; `CG3SF_FST` →
> `FSTApplicator::runGrammarOnText`; `CG3SF_JSONL` →
> `JsonlApplicator::runGrammarOnText`; `CG3SF_BINARY` →
> `BinaryApplicator::runGrammarOnText`; `default` → `CG3Quit()`. Each is passed
> `input, output`. The chosen parser reads the input format while the overridden
> `print*` methods emit the `fmt_output` format, so the two together perform the
> conversion. `CG3SF_MATXIN` has no case and hits `default` → `CG3Quit()`, so
> Matxin is not accepted as an input format here.

