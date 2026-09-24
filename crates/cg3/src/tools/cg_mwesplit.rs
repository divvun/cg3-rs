//! Port of `src/cg-mwesplit.cpp` — the MWE (multi-word expression) splitter.
//!
//! Reads a CG stream on stdin, splits multi-word cohorts into their component
//! words via [`crate::mwesplit_applicator::MweSplitApplicator`], and writes the
//! result to stdout. No grammar file: the applicator builds its own minimal
//! dummy grammar in its constructor.

use crate::arg_parser::parse_args;
use crate::options::{ArgOption, HasArg};

use super::{fail, to_argv};

// [spec:cg3:def:cg-mwesplit.options-mwe.options]
/// C++ `OptionsMWE::OPTIONS` — the tiny option enum for cg-mwesplit (help only).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    Help1,
    Help2,
    NumOptionsMwe,
}

/// C++ `OptionsMWE::options_mwe[]` — the two help aliases. Built as owned local
/// state (the C++ global array is mutated in place by the argument parser);
/// indexed by [`Opt`].
fn options_mwe() -> [ArgOption; Opt::NumOptionsMwe as usize] {
    [
        ArgOption::new("help", 'h', HasArg::No, "shows this help"),
        ArgOption::new("?", '?', HasArg::No, "shows this help"),
    ]
}

// [spec:cg3:def:cg-mwesplit.main-fn]
// [spec:cg3:sem:cg-mwesplit.main-fn]
/// C++ `int main(int argc, char** argv)`.
// faithful port: the C++ `for (i=0; i<NUM_OPTIONS_MWE; ++i)` scans cover the
// whole table — its length IS the enum constant (`[ArgOption; NumOptionsMwe]`).
pub fn main_mwesplit(args: &[String]) -> i32 {
    let status: i32 = 0;

    let mut options_mwe = options_mwe();
    let mut argv = to_argv(args);
    let argc = parse_args(
        argv.len() as i32,
        &mut argv,
        Opt::NumOptionsMwe as i32,
        &mut options_mwe,
    );

    let occ = |o: Opt| options_mwe[o as usize].does_occur;
    if argc < 0 || occ(Opt::Help1) || occ(Opt::Help2) {
        // out = (argc < 0) ? stderr : stdout;
        let mut out = String::new();
        out.push_str("Usage: cg-mwesplit [OPTIONS]\n");
        out.push('\n');
        out.push_str("Options:\n");

        let mut longest = 0usize;
        for o in options_mwe.iter() {
            if !o.description.is_empty() {
                longest = longest.max(o.long_name.map_or(0, |s| s.len()));
            }
        }
        for o in options_mwe.iter() {
            let desc = &o.description;
            if !desc.is_empty() && !desc.starts_with('!') {
                out.push(' ');
                if o.short_name != '\0' {
                    out.push_str(&format!("-{},", o.short_name));
                } else {
                    out.push_str("   ");
                }
                let ln = o.long_name.unwrap_or("");
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
            return crate::tools::EXIT_FAILURE;
        } else {
            print!("{}", out);
            return crate::tools::EXIT_SUCCESS;
        }
    }

    // MweSplitApplicator applicator(std::cerr);
    // The port's applicator OWNS its GrammarApplicator base (which owns a fresh
    // Grammar); the ctor builds+installs the minimal dummy grammar.
    let base =
        crate::grammar_applicator::GrammarApplicator::new(crate::grammar::Grammar::default());
    let mut applicator = match crate::mwesplit_applicator::MweSplitApplicator::new(base) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };

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
        return fail(&e);
    }

    // C++ main falls off the end → returns 0 (status unused by the return;
    // kept for parity with the initialised value).
    status
}
