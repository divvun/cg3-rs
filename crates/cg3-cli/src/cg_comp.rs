//! Port of `src/cg-comp.cpp` — compile a text grammar into a binary `.cg3b`.
//!
//! Reads `grammar_file` (text form) and writes `output_file` in binary form,
//! with its companion source file beside it, through
//! [`cg3::embed::CompiledGrammar::from_text`], `to_cg3b` and `companion`.
//! Rejects an already-binary input.

use std::fs::File;
use std::io::{Read, Write};

use cg3::embed::{CompiledGrammar, FileResolver};
use cg3::inlines::is_cg3b;

use super::{EXIT_FAILURE, basename, divvun_version_line, emit, fail};

// [spec:cg3:def:cg-comp.end-program-fn+3]
// [spec:cg3:sem:cg-comp.end-program-fn+3]
/// C++ `void endProgram(char* name)`. Prints the version + usage banner (when
/// `name` is non-null) and returns `EXIT_FAILURE` for its caller to return. In
/// the port `name` is the program-name argv[0]; the C++ `if (name)` guard is
/// always true for a real invocation, but is preserved for parity by taking
/// `Option<&str>`.
fn end_program(name: Option<&str>) -> i32 {
    if let Some(name) = name {
        let name = basename(name);
        let usage = format!(
            "{}{name}: compile a binary grammar from a text file\n\
             USAGE: {name} grammar_file output_file\n",
            divvun_version_line("Compiler"),
        );
        emit(std::io::stdout(), &usage);
    }
    // exit(EXIT_FAILURE);
    EXIT_FAILURE
}

// [spec:cg3:def:cg-comp.main-fn]
// [spec:cg3:sem:cg-comp.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_comp(args: &[String]) -> i32 {
    let status: i32 = 0;

    // if (argc != 3) endProgram(argv[0]);
    if args.len() != 3 {
        return end_program(args.first().map(|s| s.as_str()));
    }

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

    // The C++ filename overload of `parse_grammar` stat+reads the file; this
    // reads it here (the `.cg3b` magic was already sniffed off the front above)
    // and hands the parse the bytes AND the path, so a diagnostic can name the
    // file it came from.
    let buffer = match std::fs::read(&args[1]) {
        Ok(b) => b,
        Err(_) => {
            tracing::error!("Error: Error opening {} for reading!", args[1]);
            return EXIT_FAILURE;
        }
    };

    // [spec:cg3:req:embed.cli-compiles-through]
    // [spec:cg3:req:diagnostics.source-named]
    // parser->parse_grammar(argv[1]); grammar.reindex();
    let grammar = match CompiledGrammar::from_text(&buffer, &args[1], FileResolver) {
        Ok(g) => g,
        Err(e) => return fail(&e),
    };

    let info = grammar.info();
    tracing::info!(
        "Sections: {}, Rules: {}, Sets: {}, Tags: {}",
        info.sections,
        info.rules,
        info.sets,
        info.tags
    );
    if grammar.core().rules_any.is_some() {
        tracing::info!("{} rules cannot be skipped by index.", info.rules_any);
    }
    if info.has_dependency_rules {
        tracing::info!("Grammar has dependency rules.");
    }

    // std::ofstream gout(argv[2], ...); if (gout) { BinaryGrammar writer; writer.writeBinaryGrammar(gout); }
    let blob = match grammar.to_cg3b() {
        Ok(b) => b,
        Err(e) => return fail(&e),
    };
    let companion = match grammar.companion() {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    match File::create(&args[2]) {
        Ok(mut gout) => {
            if gout.write_all(&blob).is_err() {
                tracing::error!("Could not write grammar to {}", args[2]);
                return EXIT_FAILURE;
            }
            let _ = gout.flush();
            // [spec:cg3:req:diagnostics.sidecar+1]
            if let Some(companion) = companion {
                cg3::grammar_sources::write_beside(std::path::Path::new(&args[2]), &companion);
            }
        }
        Err(_) => {
            tracing::error!("Could not write grammar to {}", args[2]);
        }
    }

    status
}
