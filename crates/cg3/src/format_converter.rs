//! Port of `src/FormatConverter.cpp` + `src/FormatConverter.hpp` — the
//! multi-format stream converter.
//!
//! ## Multi-inheritance → composition + dispatch
//! C++ `class FormatConverter : public ApertiumApplicator, BinaryApplicator,
//! FSTApplicator, JsonlApplicator, MatxinApplicator, NicelineApplicator,
//! PlaintextApplicator` — seven format applicators over ONE shared *virtual*
//! `GrammarApplicator` base. Rust has no multiple/virtual inheritance, so this
//! is modelled as a SINGLE owned [`GrammarApplicator`] base plus explicit
//! dispatch on `fmt_input`/`fmt_output`.
//!
//! The per-format applicators in this port are borrowing wrappers over the
//! single shared `GrammarApplicator` base, and the C++ virtual print dispatch
//! is the [`ConvFormat`] [`StreamFormat`] strategy: a runtime switch on
//! `fmt_output` that the drivers thread through every print.
//!
//! CG, Apertium, FST, Niceline, plaintext, JSONL, and binary are wired for both
//! input and output. `CG3SF_MATXIN` still follows the C++ converter's default
//! branch (`CG3Quit()`): upstream declares Matxin inheritance but supplies no
//! converter switch arm for it.
//!
//! ## detectFormat regex mapping (ICU uregex → `regex` crate)
//! See [`detect_format`]; every pattern's flag set and anchoring is reproduced
//! per the spec's parity notes.

use std::io::{Read, Seek, Write};

use crate::apertium_applicator::{ApertiumApplicator, ApertiumFormat};
use crate::arena::{CohortId, SwId};
use crate::fst_applicator::{FSTApplicator, FstFormat};
use crate::grammar::Grammar;
use crate::grammar_applicator::stream_format::StreamFormat;
use crate::grammar_applicator::{GrammarApplicator, cg3_sformat};
use crate::jsonl_applicator::JsonlApplicator;
use crate::niceline_applicator::NicelineApplicator;
use crate::plaintext_applicator::{PlaintextApplicator, PlaintextFormat};
use crate::streambuf::bstreambuf;
use crate::types::UStringView;

const BUF_SIZE: usize = 1000;

const STR_DUMMY: &str = "__CG3_DUMMY_STRINGBIT__";

/// `CG3Quit()` (no args) — the C++ default-branch abort. Faithful: exits(1) with
/// no diagnostic (the C++ macro passes `__FILE__`/`__LINE__` only under debug;
/// the release path aborts). Routed here for invalid/Matxin format arms.
fn cg3_quit() -> ! {
    crate::inlines::cg3_quit(1, None, 0)
}

// [spec:cg3:def:format-converter.cg3.detect-format-fn]
// [spec:cg3:sem:format-converter.cg3.detect-format-fn]
/// C++ free fn `cg3_sformat detectFormat(std::string_view buf8)`. Sniffs the
/// stream format of a UTF-8 buffer; the FIRST matching rule wins.
///
/// REGEX-CRITICAL mapping (ICU `uregex` → `regex` crate):
/// * ICU `uregex_find(rx, -1, &status)` with `startIndex == -1` is an UNANCHORED
///   whole-text search → [`Regex::is_match`] (also unanchored). Never a
///   fully-anchored match.
/// * `UREGEX_MULTILINE` → inline flag `(?m)` (so `^`/`$` match at line
///   boundaries); `UREGEX_DOTALL` → `(?s)` (so `.` spans newlines).
/// * `\S`/`\s` are Unicode-aware in both ICU and the `regex` crate.
/// * `\^`/`\$` are literal `^`/`$`.
///   The C++ converts to UTF-16 and caps the scan at [`BUF_SIZE`] (1000) UChars;
///   this port scans the (already UTF-8) prefix directly — equivalent for the
///   anchoring the patterns rely on. NEVER returns `CG3SF_MATXIN`.
pub fn detect_format(buf8: &str) -> cg3_sformat {
    use cg3_sformat::*;

    // 1. Binary sniff: first four bytes "CGBF".
    if crate::inlines::is_cg3bsf(buf8) {
        return CG3SF_BINARY;
    }

    // Cap the scanned window at BUF_SIZE chars (mirrors the UTF-16 BUF_SIZE cap).
    let buffer: String = buf8.chars().take(BUF_SIZE).collect();

    // 3. Try each regex in turn; first match wins. Patterns/flags per the spec.
    // A `.*?^` DOTALL+MULTILINE bridge between the wordform and baseform lines.
    let patterns: &[(&str, cg3_sformat)] = &[
        // `^"<[^>]+>".*?^\s+"[^"]+"` DOTALL|MULTILINE → CG
        (r#"(?sm)^"<[^>]+>".*?^\s+"[^"]+""#, CG3SF_CG),
        // `^\S+ *\t *\[\S+\]` DOTALL|MULTILINE → NICELINE
        (r"(?sm)^\S+ *\t *\[\S+\]", CG3SF_NICELINE),
        // `^\S+ *\t *"\S+"` DOTALL|MULTILINE → NICELINE
        (r#"(?sm)^\S+ *\t *"\S+""#, CG3SF_NICELINE),
        // `\^[^/]+(/[^<]+(<[^>]+>)+)+\$` DOTALL|MULTILINE → APERTIUM
        // (literal ^ / $; no leading anchor, so it matches anywhere).
        (r"(?sm)\^[^/]+(/[^<]+(<[^>]+>)+)+\$", CG3SF_APERTIUM),
        // `^\S+\t\S+(\+\S+)+$` DOTALL|MULTILINE → FST
        (r"(?sm)^\S+\t\S+(\+\S+)+$", CG3SF_FST),
        // `^\{` MULTILINE only (NO DOTALL) → JSONL
        (r"(?m)^\{", CG3SF_JSONL),
    ];

    for (pat, fmt) in patterns {
        // ICU compiles once per call and matches; `Regex::new` is the analog.
        // (The C++ never resets `status` between calls; a compile/find failure
        // there simply yields no match — mirrored by treating a build error as
        // "no match", though these literals always compile.)
        if let Ok(rx) = regex::Regex::new(pat)
            && rx.is_match(&buffer)
        {
            return *fmt;
        }
    }

    // 6. No match → PLAIN.
    CG3SF_PLAIN
}

// [spec:cg3:def:format-converter.cg3.format-converter]
/// C++ `class FormatConverter` (multi-inheritance → composition; see the module
/// header). Holds the single shared engine base plus the minimal `conv_grammar`.
pub struct FormatConverter {
    /// The single shared `GrammarApplicator` base (C++ shared virtual base).
    base: GrammarApplicator,
    /// The print vtable: the FormatConverter `print*` overrides as a
    /// [`StreamFormat`] strategy (runtime dispatch on `fmt_output`).
    fmt: ConvFormat,
    /// C++ `Grammar conv_grammar` — the minimal working grammar built by the ctor.
    pub conv_grammar: Grammar,
}

impl FormatConverter {
    // [spec:cg3:def:format-converter.cg3.format-converter.format-converter-fn]
    // [spec:cg3:sem:format-converter.cg3.format-converter.format-converter-fn]
    /// C++ `FormatConverter::FormatConverter(std::ostream& ux_err)`. Builds a
    /// minimal working grammar in `conv_grammar` (dummy set, one delimiter set
    /// holding the dummy tag, reindex) and installs it as the active grammar via
    /// `setGrammar`.
    ///
    /// DIVERGENCE: the C++ ctor also constructs all seven applicator bases over
    /// the shared virtual `GrammarApplicator`. Here a single `base` is passed in
    /// (already owning its grammar); the caller supplies it. `conv_grammar` is
    /// built here and then INSTALLED by swapping it into `base.grammar`
    /// (`setGrammar(&conv_grammar)` — the base owns the grammar by value in this
    /// port, so "install" is a move of `conv_grammar` into `base.grammar`; the
    /// previous grammar is returned into `conv_grammar`'s slot). `has_relations`
    /// etc. keep their base defaults.
    pub fn new(mut base: GrammarApplicator) -> Self {
        // Build the minimal working grammar directly in base.grammar (which is the
        // storage the C++ `conv_grammar` provides; the base owns its grammar by
        // value in this port, so building in place == `setGrammar(&conv_grammar)`).
        // The base's incoming grammar is discarded (the ctor replaces it wholesale,
        // matching the C++ where the freshly-built conv_grammar is installed).
        base.grammar = Grammar::default();
        // conv_grammar.ux_stderr = &ux_err; — Option<()> placeholder, elided.
        base.grammar.allocate_dummy_set();
        let delim = base.grammar.allocate_set();
        base.grammar.delimiters = Some(delim);
        let dummy_tag = base.grammar.allocate_tag(STR_DUMMY);
        base.grammar.add_tag_to_set(dummy_tag, delim);
        // Internal conv grammar (used_tags=false, no static sets): reindex /
        // set_grammar cannot fatal here. If they ever did, re-raise as the
        // residual `Cg3Exit` unwind so the exact exit code is preserved.
        base.grammar
            .reindex(false, false)
            .unwrap_or_else(|e| crate::error::cg3_exit(e.exit_code()));

        // setGrammar(&conv_grammar): wire begin/end/subst tags into the grammar.
        base.set_grammar()
            .unwrap_or_else(|e| crate::error::cg3_exit(e.exit_code()));

        // PlaintextApplicator's C++ constructor runs as one of
        // FormatConverter's virtual bases and enables magic readings on the
        // shared GrammarApplicator even when plaintext is not selected.
        base.allow_magic_readings = true;

        // The C++ `conv_grammar` member IS the live active grammar's storage;
        // here that storage is `base.grammar`. The member is kept for API parity
        // and holds a default placeholder (the live grammar lives in base.grammar).
        FormatConverter {
            base,
            fmt: ConvFormat::default(),
            conv_grammar: Grammar::default(),
        }
    }

    /// The shared base (`Some` outside dispatch windows).
    ///
    /// NOTE: `pub` because C++ `FormatConverter` PUBLICLY inherits
    /// `GrammarApplicator`, so callers (cg-conv / vislcg3 `main`) directly read
    /// and write base members (`fmt_input`, `fmt_output`, `unicode_tags`,
    /// `trace`, …) and call `setGrammar`/`setOptions` on the applicator object.
    /// These accessors are the minimal composition analogue of that access.
    pub fn base(&self) -> &GrammarApplicator {
        &self.base
    }
    /// Mutable access to the shared base — see [`FormatConverter::base`].
    pub fn base_mut(&mut self) -> &mut GrammarApplicator {
        &mut self.base
    }

    /// Configure plaintext input's `--add-tags` behavior.
    pub fn set_plaintext_add_tags(&mut self, add_tags: bool) {
        self.fmt.plaintext_add_tags = add_tags;
    }

    /// Configure the separator set used to parse and print FST subreadings.
    pub fn set_fst_sub_delims(&mut self, sub_delims: String) {
        self.fmt.fst.sub_delims = sub_delims;
    }

    /// Configure the FST weight-tag name.
    pub fn set_fst_wtag(&mut self, wtag: String) {
        self.fmt.fst.wtag = wtag;
    }

    /// Configure the multiplier applied to parsed FST weights.
    pub fn set_fst_wfactor(&mut self, wfactor: f64) {
        self.fmt.fst.wfactor = wfactor;
    }

    // [spec:cg3:def:format-converter.cg3.format-converter.detect-format-fn]
    // [spec:cg3:sem:format-converter.cg3.format-converter.detect-format-fn]
    /// C++ `std::unique_ptr<std::istream> FormatConverter::detectFormat(std::istream&
    /// in)`. Peeks up to [`BUF_SIZE`] bytes, records the sniffed format in the
    /// member `fmt_input`, and returns a wrapped reader ([`bstreambuf`]) that
    /// replays the peeked prefix before continuing from `in` — so downstream code
    /// sees the whole stream from the start.
    ///
    /// DIVERGENCE: the C++ leaks the heap `bstreambuf`; the Rust port owns it in
    /// the returned wrapper (no leak — a benign, memory-safe divergence).
    pub fn detect_format<R: Read>(&mut self, in_: R) -> bstreambuf<R> {
        let mut input = in_;
        let buf8 = crate::uextras::read_utf8(&mut input, BUF_SIZE);
        // The sniffer wants a &str view; read_utf8 returns UTF-8 bytes. A lossy
        // decode is safe here (only used for the anchored regex sniff).
        let buf_str = String::from_utf8_lossy(&buf8).into_owned();
        self.base_mut().fmt_input = detect_format(&buf_str);
        bstreambuf::new(input, buf8)
    }

    // [spec:cg3:def:format-converter.cg3.format-converter.run-grammar-on-text-fn]
    // [spec:cg3:sem:format-converter.cg3.format-converter.run-grammar-on-text-fn]
    /// C++ `void FormatConverter::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`. Dispatches input PARSING to the applicator matching
    /// `fmt_input`; the overridden `print*` methods emit `fmt_output`, so the two
    /// together convert. Sets `has_relations` when either side is binary.
    /// `CG3SF_MATXIN`/invalid values → `CG3Quit()`.
    pub fn run_grammar_on_text<R, W>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        R: Read + Seek,
        W: Write,
    {
        // ux_stdin = &input; ux_stdout = &output; (Option<()> placeholders, elided).
        let (fmt_input, fmt_output) = {
            let b = self.base();
            (b.fmt_input, b.fmt_output)
        };
        if fmt_output == cg3_sformat::CG3SF_BINARY || fmt_input == cg3_sformat::CG3SF_BINARY {
            self.base_mut().grammar.has_relations = true;
        }

        use cg3_sformat::*;
        match fmt_input {
            CG3SF_CG => {
                // GrammarApplicator::runGrammarOnText(input, output) — the base CG
                // stream driver, printing through the ConvFormat vtable.
                self.base
                    .run_grammar_on_text_with(&mut self.fmt, input, output)
            }
            CG3SF_APERTIUM => {
                let mut app = ApertiumApplicator::borrowing(&mut self.base);
                app.wordform_case = self.fmt.apertium.wordform_case;
                app.print_word_forms = self.fmt.apertium.print_word_forms;
                app.print_only_first = self.fmt.apertium.print_only_first;
                app.delimit_lexical_units = self.fmt.apertium.delimit_lexical_units;
                app.surface_readings = self.fmt.apertium.surface_readings;
                app.run_grammar_on_text_with(&mut self.fmt, input, output)
            }
            CG3SF_NICELINE => NicelineApplicator::new(&mut self.base).run_grammar_on_text(
                &mut self.fmt,
                input,
                output,
            ),
            CG3SF_JSONL => JsonlApplicator::new(&mut self.base).run_grammar_on_text(
                &mut self.fmt,
                input,
                output,
            ),
            // BinaryApplicator::runGrammarOnText(input, output).
            CG3SF_BINARY => crate::binary_applicator::BinaryApplicator::new(&mut self.base)
                .run_grammar_on_text(&mut self.fmt, input, output),
            CG3SF_FST => {
                let mut app = FSTApplicator::borrowing(&mut self.base);
                app.did_warn_statictags = self.fmt.fst.did_warn_statictags;
                app.wfactor = self.fmt.fst.wfactor;
                app.wtag.clone_from(&self.fmt.fst.wtag);
                app.sub_delims.clone_from(&self.fmt.fst.sub_delims);
                app.run_grammar_on_text_with(&mut self.fmt, input, output)
            }
            CG3SF_PLAIN => {
                let mut app = PlaintextApplicator::borrowing(&mut self.base);
                app.add_tags = self.fmt.plaintext_add_tags;
                app.run_grammar_on_text_with(&mut self.fmt, input, output)
            }
            // MATXIN has no C++ converter case; invalid values share its
            // default branch.
            _ => Err(crate::error::Cg3Error::fatal(1, None)),
        }
    }

    // [spec:cg3:def:format-converter.cg3.format-converter.print-cohort-fn]
    // [spec:cg3:sem:format-converter.cg3.format-converter.print-cohort-fn]
    /// C++ `void FormatConverter::printCohort(Cohort* cohort, std::ostream&
    /// output, bool profiling)`. Dispatches on `fmt_output`. `CG3SF_BINARY` is a
    /// no-op (binary emits whole windows elsewhere); `CG3SF_MATXIN`/`default` →
    /// `CG3Quit()`.
    pub fn print_cohort<W: Write>(&mut self, cohort: CohortId, output: &mut W, profiling: bool) {
        self.fmt
            .print_cohort(&mut self.base, cohort, output, profiling);
    }

    // [spec:cg3:def:format-converter.cg3.format-converter.print-single-window-fn]
    // [spec:cg3:sem:format-converter.cg3.format-converter.print-single-window-fn]
    /// C++ `void FormatConverter::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling)`. Dispatches on `fmt_output`.
    /// `CG3SF_BINARY` emits a whole-window packet; `CG3SF_MATXIN`/`default` →
    /// `CG3Quit()`.
    pub fn print_single_window<W: Write>(&mut self, window: SwId, output: &mut W, profiling: bool) {
        self.fmt
            .print_single_window(&mut self.base, window, output, profiling);
    }

    // [spec:cg3:def:format-converter.cg3.format-converter.print-stream-command-fn]
    // [spec:cg3:sem:format-converter.cg3.format-converter.print-stream-command-fn]
    /// C++ `void FormatConverter::printStreamCommand(UStringView cmd, std::ostream&
    /// output)`. JSONL/BINARY need special encoding; every other format (CG,
    /// APERTIUM, FST, NICELINE, PLAIN, default) uses the base implementation.
    pub fn print_stream_command<W: Write>(&mut self, cmd: UStringView, output: &mut W) {
        self.fmt.print_stream_command(&mut self.base, cmd, output);
    }

    // [spec:cg3:def:format-converter.cg3.format-converter.print-plain-text-line-fn]
    // [spec:cg3:sem:format-converter.cg3.format-converter.print-plain-text-line-fn]
    /// C++ `void FormatConverter::printPlainTextLine(UStringView line, std::ostream&
    /// output)`. JSONL/BINARY need special handling; every other format uses the
    /// base implementation.
    pub fn print_plain_text_line<W: Write>(&mut self, line: UStringView, output: &mut W) {
        self.fmt.print_plain_text_line(&mut self.base, line, output);
    }
}

/// The FormatConverter print vtable (wave 4): C++ `FormatConverter` overrides
/// `printCohort` / `printSingleWindow` / `printStreamCommand` /
/// `printPlainTextLine` with switches on `fmt_output`; this strategy IS those
/// switches. It owns the binary writer state (`header_done`) and the niceline
/// one-shot warn latches that must persist across print calls.
#[derive(Default)]
pub struct ConvFormat {
    /// The Apertium print vtable and its format flags.
    apertium: ApertiumFormat,
    /// The FST print vtable and CLI-configurable parsing/printing state.
    fst: FstFormat,
    /// The plaintext print vtable.
    plaintext: PlaintextFormat,
    /// Plaintext's input-only `add_tags` flag.
    plaintext_add_tags: bool,
    /// The binary print vtable (C++ `BinaryApplicator::header_done` et al).
    binary: crate::binary_applicator::BinaryFormat,
    /// `NicelineApplicator::did_warn_statictags`, persisted across prints.
    niceline_did_warn_statictags: bool,
    /// `NicelineApplicator::did_warn_subreadings`, persisted across prints.
    niceline_did_warn_subreadings: bool,
}

impl ConvFormat {
    /// Run `f` on a transient borrowing [`NicelineApplicator`], persisting the
    /// one-shot warn latches across calls.
    fn with_niceline<'b, T>(
        &mut self,
        app: &'b mut GrammarApplicator,
        f: impl FnOnce(&mut NicelineApplicator<'b>) -> T,
    ) -> T {
        let mut a = NicelineApplicator::new(app);
        a.did_warn_statictags = self.niceline_did_warn_statictags;
        a.did_warn_subreadings = self.niceline_did_warn_subreadings;
        let r = f(&mut a);
        self.niceline_did_warn_statictags = a.did_warn_statictags;
        self.niceline_did_warn_subreadings = a.did_warn_subreadings;
        r
    }
}

impl StreamFormat for ConvFormat {
    fn print_cohort<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) {
        use cg3_sformat::*;
        match app.fmt_output {
            CG3SF_CG => app.print_cohort(cohort, output, profiling),
            CG3SF_APERTIUM => self.apertium.print_cohort(app, cohort, output, profiling),
            CG3SF_FST => self.fst.print_cohort(app, cohort, output, profiling),
            CG3SF_NICELINE => {
                self.with_niceline(app, |a| a.print_cohort(cohort, output, profiling))
            }
            CG3SF_PLAIN => self.plaintext.print_cohort(app, cohort, output, profiling),
            CG3SF_JSONL => JsonlApplicator::new(app).print_cohort(cohort, output, profiling),
            CG3SF_BINARY => {}
            _ => cg3_quit(),
        }
    }

    fn print_single_window<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        use cg3_sformat::*;
        match app.fmt_output {
            CG3SF_CG => {
                app.print_single_window(window, output, profiling);
            }
            CG3SF_APERTIUM => self
                .apertium
                .print_single_window(app, window, output, profiling),
            CG3SF_FST => self.fst.print_single_window(app, window, output, profiling),
            CG3SF_NICELINE => {
                self.with_niceline(app, |a| a.print_single_window(window, output, profiling))
            }
            CG3SF_PLAIN => self
                .plaintext
                .print_single_window(app, window, output, profiling),
            CG3SF_JSONL => JsonlApplicator::new(app).print_single_window(window, output, profiling),
            // BinaryApplicator::printSingleWindow.
            CG3SF_BINARY => self
                .binary
                .print_single_window(app, window, output, profiling),
            // MATXIN has no C++ converter case; invalid values share its
            // default branch.
            _ => cg3_quit(),
        }
    }

    fn print_stream_command<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        cmd: &str,
        output: &mut W,
    ) {
        use cg3_sformat::*;
        match app.fmt_output {
            CG3SF_JSONL => JsonlApplicator::new(app).print_stream_command(cmd, output),
            // BinaryApplicator::printStreamCommand.
            CG3SF_BINARY => self.binary.bin_print_stream_command(cmd, output),
            // CG / APERTIUM / FST / NICELINE / PLAIN / default → base.
            _ => app.print_stream_command(cmd, output),
        }
    }

    fn print_plain_text_line<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        line: &str,
        output: &mut W,
    ) {
        use cg3_sformat::*;
        match app.fmt_output {
            CG3SF_JSONL => JsonlApplicator::new(app).print_plain_text_line(line, output),
            // BinaryApplicator::printPlainTextLine.
            CG3SF_BINARY => self.binary.bin_print_plain_text_line(line, output),
            // CG / APERTIUM / FST / NICELINE / PLAIN / default → base.
            _ => app.print_plain_text_line(line, output),
        }
    }
}
