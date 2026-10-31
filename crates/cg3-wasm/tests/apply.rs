//! The binding's loads, options and runs, on the host: the same code the wasm
//! build runs, minus the value conversion at the JavaScript edge.

use cg3::binary_grammar::BinaryGrammar;
use cg3::embed::{Format, RunOptions};
use cg3::grammar::GrammarDraft;
use cg3::textual_parser::TextualParser;
use cg3_wasm::{ApplyOptions, ErrorCode, GlueError, Grammar, apply, parse_options, read_grammar};

fn cg3b(src: &str) -> Vec<u8> {
    let mut parser = TextualParser::new(GrammarDraft::default(), false);
    parser.parse_grammar_utf8(src.as_bytes()).expect("parses");
    let mut blob = Vec::new();
    BinaryGrammar::new(parser.grammar.finish().expect("finishes"))
        .write_binary_grammar(&mut blob)
        .expect("writes");
    blob
}

fn compile(src: &str) -> Grammar {
    read_grammar(&cg3b(src)).expect("loads")
}

fn options(json: &str) -> Result<ApplyOptions, GlueError> {
    parse_options(json)
}

// [spec:cg3:req:wasm.apply+1/test]
#[test]
fn options_are_the_long_names_in_camel_case() {
    let o = options(
        r#"{"input": "apertium", "output": "jsonl", "unsafe": true, "traceNameOnly": true,
            "trace": true, "textDelimit": "/x/", "depDelimit": false, "numWindows": 3,
            "noMagicReadings": true, "depNoCrossing": true}"#,
    )
    .expect("the options parse");
    let run = RunOptions::from(&o);
    assert_eq!(run.input, Some(Format::Apertium));
    assert_eq!(run.output, Some(Format::Jsonl));
    assert!(run.unsafe_mode && run.trace_name_only && run.no_magic_readings && run.dep_no_crossing);
    assert_eq!(run.trace.as_deref(), Some(""));
    assert_eq!(run.text_delimit.as_deref(), Some("/x/"));
    assert_eq!(run.dep_delimit, None);
    assert_eq!(run.num_windows, Some(3));
}

// [spec:cg3:req:wasm.apply+1/test]
// [spec:cg3:req:wasm.errors+1/test]
#[test]
fn unknown_options_and_binary_streams_are_invalid_arguments() {
    for (json, what) in [
        (r#"{"noMapings": true}"#, "a misspelt option"),
        (r#"{"no_mappings": true}"#, "the Rust spelling"),
        (r#"{"input": "binary"}"#, "the binary stream"),
        (r#"{"numWindows": -1}"#, "a negative number"),
    ] {
        let refused = options(json).expect_err(what);
        assert_eq!(refused.code(), ErrorCode::InvalidArgument, "{what}");
    }
    assert!(options("{}").is_ok());
}

// [spec:cg3:req:wasm.grammar-load+1/test]
// [spec:cg3:req:wasm.errors+1/test]
#[test]
fn bytes_that_are_no_grammar_are_invalid_grammars() {
    let blob = cg3b(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#);
    let grammar = read_grammar(&blob).expect("loads");
    assert_eq!(grammar.compiled().info().rules, 1);
    for bytes in [&blob[..blob.len() / 2], &[1, 2, 3][..], &[][..]] {
        let refused = read_grammar(bytes).err().expect("refused");
        assert_eq!(refused.code(), ErrorCode::InvalidGrammar);
        assert!(!refused.message().is_empty());
    }
    let truncated = read_grammar(&blob[..blob.len() / 2])
        .err()
        .expect("refused");
    assert!(
        truncated.message().contains("ends early"),
        "{}",
        truncated.message()
    );
}

// [spec:cg3:req:wasm.apply+1/test]
#[test]
fn a_run_returns_output_and_its_log() {
    let grammar = compile(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#);
    let applied = apply(
        grammar.compiled(),
        "\"<word>\"\n\t\"word\" n\n\t\"word\" v\n",
        &ApplyOptions::default(),
    )
    .expect("runs");
    assert_eq!(applied.output, "\"<word>\"\n\t\"word\" n\n");
    assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);

    let apertium = options(r#"{"input": "apertium", "output": "apertium"}"#).expect("parses");
    let applied = apply(grammar.compiled(), "word$ ^word/word<n>$\n", &apertium).expect("runs");
    assert_eq!(applied.warnings.len(), 1, "{:?}", applied.warnings);
    assert!(
        applied.warnings[0].contains("$ found without prior ^"),
        "{:?}",
        applied.warnings
    );
}

// [spec:cg3:req:wasm.errors+1/test]
#[test]
fn a_failed_run_keeps_the_grammar_usable() {
    let grammar = compile(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#);
    let bad = options(r#"{"depDelimit": "abc"}"#).expect("parses");
    let failed =
        apply(grammar.compiled(), "\"<word>\"\n\t\"word\" n\n", &bad).expect_err("the run fails");
    assert_eq!(failed.code(), ErrorCode::RunFailed);
    let ok = apply(
        grammar.compiled(),
        "\"<word>\"\n\t\"word\" n\n",
        &ApplyOptions::default(),
    );
    assert_eq!(
        ok.expect("the grammar runs again").output,
        "\"<word>\"\n\t\"word\" n\n"
    );
}
