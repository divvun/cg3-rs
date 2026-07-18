//! Port of `src/cg-mwesplit.cpp` — the MWE (multi-word expression) splitter.
//!
//! Reads a CG stream on stdin, splits multi-word cohorts into their component
//! words via [`crate::mwesplit_applicator::MweSplitApplicator`], and writes the
//! result to stdout. No grammar file: the applicator builds its own minimal
//! dummy grammar in its constructor.

use crate::icu_uoptions::u_parseArgs;
use crate::options::{UOPT_NO_ARG, UOption};

use super::to_uargv;

// [spec:cg3:def:cg-mwesplit.options-mwe.options]
/// C++ `OptionsMWE::OPTIONS` — the tiny option enum for cg-mwesplit (help only).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OPTIONS {
    HELP1,
    HELP2,
    NUM_OPTIONS_MWE,
}

/// Local `UOption` aggregate-init helper (the crate's `UOption::new` is private
/// to `crate::options`, and we may not edit that module).
fn uo(long: &'static str, short: char, has_arg: u8, desc: &'static str) -> UOption {
    UOption {
        long_name: Some(long),
        short_name: short,
        has_arg,
        description: desc.to_string(),
        does_occur: false,
        value: String::new(),
    }
}

/// C++ `OptionsMWE::options_mwe[]` — the two help aliases. Built as owned local
/// state (the C++ global array is mutated in place by `u_parseArgs`); indexed by
/// [`OPTIONS`].
fn options_mwe() -> [UOption; OPTIONS::NUM_OPTIONS_MWE as usize] {
    [
        uo("help", 'h', UOPT_NO_ARG, "shows this help"),
        uo("?", '?', UOPT_NO_ARG, "shows this help"),
    ]
}

// [spec:cg3:def:cg-mwesplit.main-fn]
// [spec:cg3:sem:cg-mwesplit.main-fn]
/// C++ `int main(int argc, char** argv)`.
// faithful port: `for (i=0; i<NUM_OPTIONS_MWE; ++i)` walks the enum-sized option
// table by index (bound is the enum constant, not `.len()`), mirroring the C++.
#[allow(clippy::needless_range_loop)]
pub fn main_mwesplit(args: &[String]) -> i32 {
    // UErrorCode status = U_ZERO_ERROR;
    let status: i32 = 0;

    // ICU init dropped (UTF-8 port); see tools/mod.rs.

    let mut options_mwe = options_mwe();
    let mut argv = to_uargv(args);
    let argc = u_parseArgs(
        argv.len() as i32,
        &mut argv,
        OPTIONS::NUM_OPTIONS_MWE as i32,
        &mut options_mwe,
    );

    let occ = |o: OPTIONS| options_mwe[o as usize].does_occur;
    if argc < 0 || occ(OPTIONS::HELP1) || occ(OPTIONS::HELP2) {
        // out = (argc < 0) ? stderr : stdout;
        let mut out = String::new();
        out.push_str("Usage: cg-mwesplit [OPTIONS]\n");
        out.push('\n');
        out.push_str("Options:\n");

        let mut longest = 0usize;
        for i in 0..OPTIONS::NUM_OPTIONS_MWE as usize {
            if !options_mwe[i].description.is_empty() {
                longest = longest.max(options_mwe[i].long_name.map_or(0, |s| s.len()));
            }
        }
        for i in 0..OPTIONS::NUM_OPTIONS_MWE as usize {
            let desc = &options_mwe[i].description;
            if !desc.is_empty() && !desc.starts_with('!') {
                out.push(' ');
                if options_mwe[i].short_name != '\0' {
                    out.push_str(&format!("-{},", options_mwe[i].short_name));
                } else {
                    out.push_str("   ");
                }
                let ln = options_mwe[i].long_name.unwrap_or("");
                out.push_str(&format!(" --{}", ln));
                let mut ldiff = longest - ln.len();
                while ldiff > 0 {
                    out.push(' ');
                    ldiff -= 1;
                }
                out.push_str(&format!("  {}\n", desc));
            }
        }

        if argc < 0 {
            eprint!("{}", out);
            // U_ILLEGAL_ARGUMENT_ERROR
            return crate::tools::U_ILLEGAL_ARGUMENT_ERROR;
        } else {
            print!("{}", out);
            return 0; // U_ZERO_ERROR
        }
    }

    // ucnv_setDefaultName / uloc_setDefault dropped (UTF-8 port).

    // MweSplitApplicator applicator(std::cerr);
    // The port's applicator OWNS its GrammarApplicator base (which owns a fresh
    // Grammar); the ctor builds+installs the minimal dummy grammar.
    let base =
        crate::grammar_applicator::GrammarApplicator::new(crate::grammar::Grammar::default());
    let mut applicator = crate::mwesplit_applicator::MweSplitApplicator::new(base);

    // applicator.verbosity_level = 0;
    applicator.base.cfg.verbosity_level = 0;

    // applicator.runGrammarOnText(std::cin, std::cout);
    //
    // The ported driver needs `R: Read + Seek`; stdin is not seekable, so the
    // whole stream is buffered into a Cursor first (faithful for the
    // line-by-line CG state machine the driver runs).
    let mut input_bytes = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut input_bytes);
    let mut cursor = std::io::Cursor::new(input_bytes);
    let mut stdout = std::io::stdout();
    if let Err(e) = applicator.run_grammar_on_text(&mut cursor, &mut stdout) {
        crate::error::cg3_exit(e.exit_code());
    }

    // u_cleanup dropped. C++ main falls off the end → returns 0 (status unused
    // by the return; kept for parity with the initialised value).
    status
}
