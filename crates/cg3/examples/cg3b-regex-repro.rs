//! Reproducer for the ICU-vs-`regex` parity gap on the binary-grammar read path.
//!
//! Loads a `.cg3b` and reports whether every regex tag in it compiles. The
//! reference input is `grc-disambiguator.bin` from the `se.drb` bundle, which
//! carries the tag `"\Q$1\E.*"S$` — ICU literal quoting the `regex` crate has
//! no arm for.
//!
//! Usage: `cargo run --example cg3b-regex-repro -- <grammar.cg3b>`

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::Grammar;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: cg3b-regex-repro <grammar.cg3b>");
        std::process::exit(2);
    };

    cg3::tools::init_diagnostics();

    let mut parser = BinaryGrammar::new(Grammar::default());
    match parser.parse_grammar_filename(&path) {
        Ok(()) => println!("LOADED OK: {} tags", parser.grammar.num_tags),
        Err(e) => println!("FAILED: {e}"),
    }
}
