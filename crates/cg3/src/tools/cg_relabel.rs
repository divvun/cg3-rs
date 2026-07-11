//! Port of `src/cg-relabel.cpp` — relabel a binary grammar using a relabelling
//! file.
//!
//! Loads `input_grammar_file` (must be binary) and `relabel_rule_file`, runs the
//! [`crate::relabeller::Relabeller`], and writes the relabelled grammar back out
//! in binary form. LIVE flow (binary parse → relabel → binary write).

use std::fs::File;
use std::io::{Read, Write};

use crate::binary_grammar::BinaryGrammar;
use crate::grammar::Grammar;
use crate::inlines::{cg3_quit, is_cg3b};
use crate::relabeller::Relabeller;
use crate::textual_parser::TextualParser;

use super::{
    basename, CG3_REVISION, CG3_VERSION_MAJOR, CG3_VERSION_MINOR, CG3_VERSION_PATCH,
};

// [spec:cg3:def:cg-relabel.end-program-fn]
// [spec:cg3:sem:cg-relabel.end-program-fn]
/// C++ `void endProgram(char* name)`.
fn end_program(name: Option<&str>) -> ! {
    if let Some(name) = name {
        println!(
            "VISL CG-3 Relabeller version {}.{}.{}.{}",
            CG3_VERSION_MAJOR, CG3_VERSION_MINOR, CG3_VERSION_PATCH, CG3_REVISION
        );
        println!("{}: relabel a binary grammar using a relabelling file", basename(name));
        println!(
            "USAGE: {} input_grammar_file relabel_rule_file output_grammar_file",
            basename(name)
        );
    }
    crate::error::cg3_exit(1)
}

// like libcg3's, but with a non-void grammar …
// [spec:cg3:def:cg-relabel.cg3-grammar-load-fn]
// [spec:cg3:sem:cg-relabel.cg3-grammar-load-fn]
/// C++ `Grammar* cg3_grammar_load(const char* filename, std::ostream& ux_stdout,
/// std::ostream& ux_stderr, bool require_binary = false)`.
///
/// Returns `None` on error (the C++ `return 0;` — a null `Grammar*`). BUG
/// (reproduced): the C++ caller (`main`) never null-checks the returned pointer;
/// it dereferences it unconditionally — see [`main_relabel`]. BUG (leak,
/// DIVERGENCE): the C++ `new Grammar` is never `delete`d on the error-return
/// paths (a memory leak); the Rust port owns the `Grammar` by value, so those
/// paths simply drop it — memory-safe, so the leak cannot be reproduced (noted).
fn cg3_grammar_load(filename: &str, require_binary: bool) -> Option<Grammar> {
    // std::ifstream input(filename, std::ios::binary); if (!input) return 0;
    let mut input = match File::open(filename) {
        Ok(f) => f,
        Err(_) => {
            tracing::error!("Error: Error opening {} for reading!", filename);
            return None;
        }
    };
    // if (!input.read(&cbuffers[0][0], 4)) { ...; return 0; }
    let mut head = [0u8; 4];
    if input.read_exact(&mut head).is_err() {
        tracing::error!("Error: Error reading first 4 bytes from grammar!");
        return None;
    }
    drop(input); // input.close();

    // Grammar* grammar = new Grammar; (owned by value here.)
    let grammar = Grammar::default();
    // grammar->ux_stderr / ux_stdout = ...; (Option<()> placeholders, elided.)

    if is_cg3b(head) {
        // parser.reset(new BinaryGrammar(*grammar, ux_stderr));
        let mut parser = BinaryGrammar::binary_grammar(grammar);
        if parser.parse_grammar_filename(filename) != 0 {
            tracing::error!("Error: Grammar could not be parsed!");
            return None;
        }
        let mut grammar = parser.grammar;
        grammar.reindex(false, false);
        Some(grammar)
    } else {
        if require_binary {
            tracing::error!("Error: Text grammar detected -- to compile this grammar, use `cg-comp'");
            cg3_quit(1, None, 0);
        }
        // parser.reset(new TextualParser(*grammar, ux_stderr));
        let mut parser = TextualParser::new(grammar, false);
        let buffer = match std::fs::read(filename) {
            Ok(b) => b,
            Err(_) => {
                tracing::error!("Error: Error opening {} for reading!", filename);
                return None;
            }
        };
        if parser.parse_grammar_utf8(&buffer) != 0 {
            tracing::error!("Error: Grammar could not be parsed!");
            return None;
        }
        let mut grammar = parser.grammar;
        grammar.reindex(false, false);
        Some(grammar)
    }
}

// [spec:cg3:def:cg-relabel.main-fn]
// [spec:cg3:sem:cg-relabel.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_relabel(args: &[String]) -> i32 {
    // UErrorCode status = U_ZERO_ERROR;
    let status: i32 = 0;

    // if (argc != 4) endProgram(argv[0]);
    if args.len() != 4 {
        end_program(args.first().map(|s| s.as_str()));
    }

    // ICU init / codepage / locale dropped (UTF-8 port).

    // std::unique_ptr<Grammar> grammar{ cg3_grammar_load(argv[1], ..., true) };
    // std::unique_ptr<Grammar> relabel_grammar{ cg3_grammar_load(argv[2], ...) };
    //
    // BUG (null-check-missing, reproduced): C++ does NOT check either result for
    // null before dereferencing below. The faithful analogue is an unchecked
    // `.expect(...)` (panics on the None the loader returns on error — the direct
    // stand-in for the C++ null-pointer dereference / UB).
    let mut grammar = cg3_grammar_load(&args[1], true)
        .expect("cg-relabel: null grammar dereference (input grammar failed to load)");
    let relabel_grammar = cg3_grammar_load(&args[2], false)
        .expect("cg-relabel: null grammar dereference (relabel grammar failed to load)");

    // Relabeller relabeller(*grammar, *relabel_grammar, std::cerr);
    // relabeller.relabel();
    {
        let mut relabeller = Relabeller::new(&mut grammar, &relabel_grammar, ());
        relabeller.relabel();
    }

    // std::ofstream gout(argv[3], ...); if (gout) { BinaryGrammar writer; writer.writeBinaryGrammar(gout); }
    match File::create(&args[3]) {
        Ok(mut gout) => {
            let mut writer = BinaryGrammar::binary_grammar(grammar);
            writer.write_binary_grammar(&mut gout);
            let _ = gout.flush();
        }
        Err(_) => {
            tracing::error!("Could not write grammar to {}", args[3]);
        }
    }

    // u_cleanup dropped.
    status
}
