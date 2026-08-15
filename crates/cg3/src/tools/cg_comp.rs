//! Port of `src/cg-comp.cpp` — compile a text grammar into a binary `.cg3b`.
//!
//! Reads `grammar_file` (text form), reindexes, and writes `output_file` in
//! binary form via [`crate::binary_grammar::BinaryGrammar::write_binary_grammar`].
//! Rejects an already-binary input. This whole flow is LIVE (no gated engine
//! paths): text parse → reindex → binary write.

use std::fs::File;
use std::io::{Read, Write};

use crate::binary_grammar::BinaryGrammar;
use crate::grammar::Grammar;
use crate::inlines::is_cg3b;
use crate::textual_parser::TextualParser;

use super::{EXIT_FAILURE, basename, fail, print_divvun_version_line};

// [spec:cg3:def:cg-comp.end-program-fn+3]
// [spec:cg3:sem:cg-comp.end-program-fn+3]
/// C++ `void endProgram(char* name)`. Prints the version + usage banner (when
/// `name` is non-null) and returns `EXIT_FAILURE` for its caller to return. In
/// the port `name` is the program-name argv[0]; the C++ `if (name)` guard is
/// always true for a real invocation, but is preserved for parity by taking
/// `Option<&str>`.
fn end_program(name: Option<&str>) -> i32 {
    if let Some(name) = name {
        print_divvun_version_line("Compiler");
        println!(
            "{}: compile a binary grammar from a text file",
            basename(name)
        );
        println!("USAGE: {} grammar_file output_file", basename(name));
    }
    // exit(EXIT_FAILURE);
    EXIT_FAILURE
}

// [spec:cg3:def:cg-comp.main-fn]
// [spec:cg3:sem:cg-comp.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_comp(args: &[String]) -> i32 {
    // UErrorCode status = U_ZERO_ERROR;
    let status: i32 = 0;

    // if (argc != 3) endProgram(argv[0]);
    if args.len() != 3 {
        return end_program(args.first().map(|s| s.as_str()));
    }

    // ICU init / codepage / locale dropped (UTF-8 port).

    // Grammar grammar; — owned by the parser in this port (moved out after parse).
    let grammar = Grammar::default();

    // FILE* input = fopen(argv[1], "rb"); read first 4 bytes; fclose(input);
    let mut input = match File::open(&args[1]) {
        Ok(f) => f,
        Err(_) => {
            tracing::error!("Error: Error opening {} for reading!", args[1]);
            return EXIT_FAILURE;
        }
    };
    let mut head = [0u8; 4];
    if input.read_exact(&mut head).is_err() {
        tracing::error!("Error: Error reading first 4 bytes from grammar!");
        return EXIT_FAILURE;
    }
    drop(input);

    if is_cg3b(head) {
        tracing::error!("Binary grammar detected. Cannot re-compile binary grammars.");
        return EXIT_FAILURE;
    }

    // parser.reset(new TextualParser(grammar, std::cerr));
    let mut parser = TextualParser::new(grammar, false);
    // grammar.ux_stderr = &std::cerr; (Option<()> placeholder, elided.)

    // if (parser->parse_grammar(argv[1])) { ... CG3Quit(1); }
    //
    // The C++ filename overload stat+reads the file; this reads it here (the
    // `.cg3b` magic was already sniffed off the front above) and hands the parse
    // the bytes AND the path, so a diagnostic can name the file it came from.
    let buffer = match std::fs::read(&args[1]) {
        Ok(b) => b,
        Err(_) => {
            tracing::error!("Error: Error opening {} for reading!", args[1]);
            return EXIT_FAILURE;
        }
    };
    // A deep parse/grammar fatal already printed its diagnostic; the variants
    // that carry theirs are surfaced by `fail`. Either way the process ends with
    // the exact code (byte-identical to the C++ CG3Quit termination).
    // [spec:cg3:req:diagnostics.source-named]
    if let Err(e) = parser.parse_grammar_named(&buffer, &args[1]) {
        return fail(&e);
    }

    // Move the built grammar out of the parser (the C++ grammar outlives the
    // parser, which is `reset()` after parsing).
    let mut grammar = parser.grammar;

    // grammar.reindex();
    if let Err(e) = grammar.reindex(false, false) {
        return fail(&e);
    }

    // Info banner to stderr (container sizes; see tools/mod.rs on Arena counts).
    tracing::info!(
        "Sections: {}, Rules: {}, Sets: {}, Tags: {}",
        grammar.sections.len(),
        grammar.rule_by_number.capacity(),
        grammar.sets_list.capacity(),
        grammar.single_tags.size()
    );

    if let Some(rules_any) = grammar.rules_any.as_ref() {
        tracing::info!("{} rules cannot be skipped by index.", rules_any.size());
    }

    if grammar.has_dep {
        tracing::info!("Grammar has dependency rules.");
    }

    // std::ofstream gout(argv[2], ...); if (gout) { BinaryGrammar writer; writer.writeBinaryGrammar(gout); }
    match File::create(&args[2]) {
        Ok(mut gout) => {
            let mut writer = BinaryGrammar::new(grammar);
            if let Err(e) = writer.write_binary_grammar(&mut gout) {
                return fail(&e);
            }
            let _ = gout.flush();
        }
        Err(_) => {
            tracing::error!("Could not write grammar to {}", args[2]);
        }
    }

    // u_cleanup dropped.
    status
}
