//! Port of `src/cg-relabel.cpp` — relabel a binary grammar using a relabelling
//! file.
//!
//! Loads `input_grammar_file` (must be binary) and `relabel_rule_file`, runs the
//! [`crate::relabeller::Relabeller`], and writes the relabelled grammar back out
//! in binary form. LIVE flow (binary parse → relabel → binary write).

use std::fs::File;
use std::io::{Read, Write};

use crate::binary_grammar::BinaryGrammar;
use crate::grammar::{GrammarCore, GrammarDraft, GrammarNumbered};
use crate::inlines::is_cg3b;
use crate::relabeller::{RelabelRuleError, Relabeller};
use crate::textual_parser::TextualParser;

use super::{EXIT_FAILURE, basename, divvun_version_line, emit, fail};

// [spec:cg3:def:cg-relabel.end-program-fn+3]
// [spec:cg3:sem:cg-relabel.end-program-fn+3]
/// C++ `void endProgram(char* name)`.
fn end_program(name: Option<&str>) -> i32 {
    if let Some(name) = name {
        let name = basename(name);
        let usage = format!(
            "{}{name}: relabel a binary grammar using a relabelling file\n\
             USAGE: {name} input_grammar_file relabel_rule_file output_grammar_file\n",
            divvun_version_line("Relabeller"),
        );
        emit(std::io::stdout(), &usage);
    }
    EXIT_FAILURE
}

/// Why [`cg3_grammar_load`] could not produce a grammar.
///
/// The C++ answers "which step failed?" with a null `Grammar*` for three of
/// these and by terminating the process for the other two, so no caller can
/// tell them apart or choose what to do — the shape
/// `[spec:cg3:req:errors.context]` bans. Local to this tool because this
/// boundary is the only place that can produce them.
#[derive(Debug, thiserror::Error)]
enum GrammarLoadError {
    #[error("Error: Error opening {path} for reading!")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Error: Error reading first 4 bytes from grammar!")]
    Header {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Error: Text grammar detected -- to compile this grammar, use `cg-comp'")]
    RequiresBinary { path: String },
    #[error("Error: Grammar could not be parsed!")]
    Parse {
        path: String,
        #[source]
        source: crate::error::Cg3Error,
    },
    #[error("Error: Grammar {path} could not be reindexed!")]
    Reindex {
        path: String,
        #[source]
        source: crate::error::Cg3Error,
    },
}

// like libcg3's, but with a non-void grammar …
// [spec:cg3:def:cg-relabel.cg3-grammar-load-fn+1]
// [spec:cg3:sem:cg-relabel.cg3-grammar-load-fn+1]
/// C++ `cg3_grammar_load(filename, ..., bool require_binary = false)`.
///
/// DIVERGENCE: the C++ returns a null `Grammar*` for an open/read/parse failure
/// and `CG3Quit`s from inside for the other two, so the same function reports
/// failure two incompatible ways and neither carries what went wrong. Every
/// failure is now one [`GrammarLoadError`], and the caller decides — the
/// process ends at the CLI boundary, per
/// `[spec:cg3:req:errors.exit-codes-at-cli]`. BUG (leak, DIVERGENCE): the C++
/// `new Grammar` is never `delete`d on the error-return paths (a memory leak);
/// the Rust port owns the `Grammar` by value, so those paths simply drop it —
/// memory-safe, so the leak cannot be reproduced (noted).
fn cg3_grammar_load(filename: &str, require_binary: bool) -> Result<GrammarCore, GrammarLoadError> {
    // std::ifstream input(filename, std::ios::binary); if (!input) return 0;
    let mut input = File::open(filename).map_err(|source| GrammarLoadError::Open {
        path: filename.to_string(),
        source,
    })?;
    // if (!input.read(&cbuffers[0][0], 4)) { ...; return 0; }
    let mut head = [0u8; 4];
    input
        .read_exact(&mut head)
        .map_err(|source| GrammarLoadError::Header {
            path: filename.to_string(),
            source,
        })?;
    drop(input); // input.close();

    let reindex_failed = |source| GrammarLoadError::Reindex {
        path: filename.to_string(),
        source,
    };
    // Grammar* grammar = new Grammar; (owned by value here, in the phase its
    // loader builds.)
    if is_cg3b(head) {
        // parser.reset(new BinaryGrammar(*grammar, ...));
        let mut parser = BinaryGrammar::new(GrammarNumbered::default());
        parser
            .parse_grammar_filename(filename)
            .map_err(|source| GrammarLoadError::Parse {
                path: filename.to_string(),
                source,
            })?;
        parser.grammar.finish().map_err(reindex_failed)
    } else {
        if require_binary {
            return Err(GrammarLoadError::RequiresBinary {
                path: filename.to_string(),
            });
        }
        // parser.reset(new TextualParser(*grammar, ...));
        let mut parser = TextualParser::new(GrammarDraft::default(), false);
        let buffer = std::fs::read(filename).map_err(|source| GrammarLoadError::Open {
            path: filename.to_string(),
            source,
        })?;
        // [spec:cg3:req:diagnostics.source-named]
        parser
            .parse_grammar_named(&buffer, filename)
            .map_err(|source| GrammarLoadError::Parse {
                path: filename.to_string(),
                source,
            })?;
        parser.grammar.finish().map_err(reindex_failed)
    }
}

/// Report a load failure on the way out of [`main_relabel`], and derive the exit
/// code it maps to.
///
/// The headline is the C++-parity line; the causes underneath are where the
/// detail lives — an OS error, or the tag-regex diagnostics a `Cg3Error` carries
/// — so converting the loader does not cost the reader what `report_cli` used
/// to print.
fn report_load(e: &GrammarLoadError) -> i32 {
    tracing::error!("{e}");
    let mut cause = std::error::Error::source(e);
    while let Some(c) = cause {
        tracing::error!("{c}");
        cause = c.source();
    }
    EXIT_FAILURE
}

/// Why [`main_relabel`] has no relabelled grammar to write.
#[derive(Debug, thiserror::Error)]
enum RelabelFailure {
    #[error(transparent)]
    Load(#[from] GrammarLoadError),
    #[error(transparent)]
    Rule(#[from] RelabelRuleError),
    #[error(transparent)]
    Relabel(#[from] crate::error::Cg3Error),
}

/// Report a [`RelabelFailure`] on the way out of [`main_relabel`], and derive
/// the exit code it maps to.
fn report_failure(e: &RelabelFailure) -> i32 {
    match e {
        RelabelFailure::Load(e) => report_load(e),
        RelabelFailure::Rule(e) => {
            tracing::error!("{e}");
            EXIT_FAILURE
        }
        RelabelFailure::Relabel(e) => fail(e),
    }
}

/// Load the grammar at `grammar_path` and relabel it by the rules at
/// `relabel_path`.
fn relabel_grammar(grammar_path: &str, relabel_path: &str) -> Result<GrammarCore, RelabelFailure> {
    // std::unique_ptr<Grammar> grammar{ cg3_grammar_load(argv[1], ..., true) };
    // std::unique_ptr<Grammar> relabel_grammar{ cg3_grammar_load(argv[2], ...) };
    //
    // DIVERGENCE (was: BUG, null-check-missing, reproduced): the C++ checks
    // neither result before dereferencing it, so a grammar that fails to load
    // crashes the process. The loader hands the failure back as a value now, so
    // the boundary that owns the exit code reports it and returns.
    let grammar = cg3_grammar_load(grammar_path, true)?;
    let relabel_grammar = cg3_grammar_load(relabel_path, false)?;

    // Relabeller relabeller(*grammar, *relabel_grammar, std::cerr);
    // relabeller.relabel();
    Ok(Relabeller::new(grammar, &relabel_grammar, ())?.relabel()?)
}

// [spec:cg3:def:cg-relabel.main-fn+1]
// [spec:cg3:sem:cg-relabel.main-fn+1]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_relabel(args: &[String]) -> i32 {
    let status: i32 = 0;

    // if (argc != 4) endProgram(argv[0]);
    if args.len() != 4 {
        return end_program(args.first().map(|s| s.as_str()));
    }

    let grammar = match relabel_grammar(&args[1], &args[2]) {
        Ok(g) => g,
        Err(e) => return report_failure(&e),
    };

    // std::ofstream gout(argv[3], ...); if (gout) { BinaryGrammar writer; writer.writeBinaryGrammar(gout); }
    match File::create(&args[3]) {
        Ok(mut gout) => {
            let mut writer = BinaryGrammar::new(grammar);
            if let Err(e) = writer.write_binary_grammar(&mut gout) {
                return fail(&e);
            }
            let _ = gout.flush();
            drop(gout);
            // [spec:cg3:req:diagnostics.sidecar+1]
            // Relabelling rewrote tags and sets, not rule numbering, so the
            // input's companion still describes this grammar — but it is
            // stamped against the binary just superseded.
            crate::grammar_sources::carry_beside(
                std::path::Path::new(&args[1]),
                std::path::Path::new(&args[3]),
            );
        }
        Err(_) => {
            tracing::error!("Could not write grammar to {}", args[3]);
        }
    }

    status
}
