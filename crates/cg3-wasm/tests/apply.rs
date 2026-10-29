//! The binding's options and runs, on the host: the same code the wasm build
//! runs, minus the value conversion at the JavaScript edge.

use cg3::binary_grammar::BinaryGrammar;
use cg3::embed::{CompiledGrammar, Format, RunOptions};
use cg3::grammar::GrammarDraft;
use cg3::textual_parser::TextualParser;
use cg3_wasm::{ApplyOptions, apply, parse_options};

fn compile(src: &str) -> CompiledGrammar {
    let mut parser = TextualParser::new(GrammarDraft::default(), false);
    parser.parse_grammar_utf8(src.as_bytes()).expect("parses");
    let mut blob = Vec::new();
    BinaryGrammar::new(parser.grammar.finish().expect("finishes"))
        .write_binary_grammar(&mut blob)
        .expect("writes");
    CompiledGrammar::from_cg3b(&blob).expect("loads")
}

fn options(json: &str) -> Result<ApplyOptions, String> {
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
    .unwrap();
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
#[test]
fn unknown_options_and_the_binary_format_are_refused() {
    assert!(
        options(r#"{"noMapings": true}"#).is_err(),
        "a misspelt option"
    );
    assert!(
        options(r#"{"no_mappings": true}"#).is_err(),
        "the Rust spelling"
    );
    assert!(
        options(r#"{"input": "binary"}"#).is_err(),
        "the binary stream"
    );
    assert!(options("{}").is_ok());
}

// [spec:cg3:req:wasm.apply+1/test]
#[test]
fn a_run_returns_output_and_its_log() {
    let grammar = compile(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#);
    let applied = apply(
        &grammar,
        "\"<word>\"\n\t\"word\" n\n\t\"word\" v\n",
        &ApplyOptions::default(),
    )
    .unwrap();
    assert_eq!(applied.output, "\"<word>\"\n\t\"word\" n\n");
    assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);

    let apertium = options(r#"{"input": "apertium", "output": "apertium"}"#).unwrap();
    let applied = apply(&grammar, "word$ ^word/word<n>$\n", &apertium).unwrap();
    assert_eq!(applied.warnings.len(), 1, "{:?}", applied.warnings);
    assert!(
        applied.warnings[0].contains("$ found without prior ^"),
        "{:?}",
        applied.warnings
    );
}

// [spec:cg3:req:wasm.errors/test]
#[test]
fn a_failed_run_leaves_the_grammar_usable() {
    let grammar = compile(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#);
    let bad = options(r#"{"depDelimit": "abc"}"#).unwrap();
    let failed = apply(&grammar, "\"<word>\"\n\t\"word\" n\n", &bad);
    assert!(failed.is_err());
    let ok = apply(
        &grammar,
        "\"<word>\"\n\t\"word\" n\n",
        &ApplyOptions::default(),
    );
    assert_eq!(ok.unwrap().output, "\"<word>\"\n\t\"word\" n\n");
}
