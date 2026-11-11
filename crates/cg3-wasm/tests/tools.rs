//! convert, mwesplit and proc on the host: the options JavaScript passes, read
//! into the library's, and the strings the library's tools write.

use std::path::{Path, PathBuf};

use cg3::embed::{CompiledGrammar, Format, LoadOptions, MemoryResolver, ProcFormat};
use cg3_wasm::{
    ErrorCode, GlueError, convert, mwesplit, parse_convert_options, parse_proc_options,
    read_grammar,
};

fn corpus(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(path)
}

fn text(path: &str) -> String {
    std::fs::read_to_string(corpus(path)).expect("a corpus file")
}

fn cg3b(path: &str) -> Vec<u8> {
    CompiledGrammar::from_text(
        text(path).as_bytes(),
        path,
        MemoryResolver::default(),
        &LoadOptions::default(),
    )
    .and_then(|g| g.to_cg3b())
    .expect("compiles")
}

// [spec:cg3:req:wasm.tools/test]
#[test]
fn convert_options_are_cg_conv_long_names() {
    let o = parse_convert_options(
        r#"{"input": "apertium", "inAuto": true, "addTags": true, "output": "fst",
            "prefix": "%", "wfactor": 2.5, "wtag": "X", "subDelim": "+", "rtl": true,
            "ltr": true, "ordered": true, "parseDep": true, "depDelimit": 3,
            "unicodeTags": true, "deleted": true, "noBreak": true}"#,
    )
    .expect("the options parse");
    assert_eq!(o.input, Some(Format::Apertium));
    assert_eq!(o.output, Some(Format::Fst));
    assert!(o.in_auto && o.add_tags && o.ltr && o.ordered && o.parse_dep);
    assert!(o.unicode_tags && o.deleted && o.no_break);
    assert_eq!(o.prefix, Some('%'));
    assert_eq!(o.wfactor, Some(2.5));
    assert_eq!(o.wtag.as_deref(), Some("X"));
    assert_eq!(o.sub_delim.as_deref(), Some("+"));
    assert_eq!(o.dep_delimit, Some(3));

    let flag = |json| parse_convert_options(json).expect("parses").dep_delimit;
    assert_eq!(
        flag(r#"{"depDelimit": true}"#),
        Some(10),
        "cg-conv's default"
    );
    assert_eq!(flag(r#"{"depDelimit": false}"#), None);
    let o = parse_convert_options("{}").expect("parses");
    assert_eq!((o.input, o.output, o.prefix), (None, None, None));
}

// [spec:cg3:req:wasm.tools/test]
#[test]
fn proc_options_are_cg_proc_long_names() {
    let o = parse_proc_options(
        r#"{"streamFormat": "matxin", "sections": 2, "rule": "r1", "trace": true,
            "wordformCase": true, "noWordForms": true, "generation": true, "first": true,
            "disambiguation": true, "nullFlush": true}"#,
    )
    .expect("the options parse");
    assert_eq!(o.stream_format, ProcFormat::Matxin);
    assert_eq!(o.sections, Some(2));
    assert_eq!(o.rule.as_deref(), Some("r1"));
    assert!(o.trace && o.wordform_case && o.no_word_forms && o.generation && o.first);
    let format = |json| parse_proc_options(json).expect("parses").stream_format;
    assert_eq!(format(r#"{"streamFormat": "cg"}"#), ProcFormat::Cg);
    assert_eq!(format("{}"), ProcFormat::Apertium, "cg-proc's default");
}

// [spec:cg3:req:wasm.tools/test]
// [spec:cg3:req:wasm.errors+1/test]
#[test]
fn unknown_options_and_binary_streams_are_invalid() {
    let code = |refused: Result<_, GlueError>| refused.err().map(|e| e.code());
    for json in [
        r#"{"inApertium": true}"#,
        r#"{"sub_delim": "+"}"#,
        r#"{"input": "binary"}"#,
        r#"{"output": "binary"}"#,
        r#"{"prefix": "%%"}"#,
        r#"{"prefix": ""}"#,
        r#"{"depDelimit": -1}"#,
        r#"{"depDelimit": "3"}"#,
    ] {
        let refused = code(parse_convert_options(json).map(drop));
        assert_eq!(refused, Some(ErrorCode::InvalidArgument), "convert {json}");
    }
    for json in [
        r#"{"streamFormat": "binary"}"#,
        r#"{"streamFormat": 3}"#,
        r#"{"wordFormCase": true}"#,
        r#"{"sections": -1}"#,
    ] {
        let refused = code(parse_proc_options(json).map(drop));
        assert_eq!(refused, Some(ErrorCode::InvalidArgument), "proc {json}");
    }
    let refused = convert("x", r#"{"noBreaks": true}"#).expect_err("refused");
    assert!(
        refused.message().contains("noBreaks"),
        "{}",
        refused.message()
    );
}

// [spec:cg3:req:wasm.tools/test]
#[test]
fn the_tools_write_what_the_library_writes() {
    let apertium = "^w/a<n>+b<v>$\n";
    assert_eq!(
        convert(apertium, r#"{"input": "apertium"}"#).expect("converts"),
        "\"<w>\"\n\t\"b\" v\n\t\t\"a\" n\n\n"
    );
    assert_eq!(
        convert(apertium, r#"{"input": "apertium", "ltr": true}"#).expect("converts"),
        "\"<w>\"\n\t\"a\" n\n\t\t\"b\" v\n\n"
    );
    let input = text("T_Jsonl/input.txt");
    for json in [
        "{}",
        r#"{"output": "jsonl", "unicodeTags": true}"#,
        r#"{"output": "apertium", "ordered": true, "ltr": true, "prefix": "%"}"#,
        r##"{"output": "fst", "wfactor": 2, "wtag": "X", "subDelim": "#"}"##,
        r#"{"addTags": true, "inAuto": true}"#,
        r#"{"depDelimit": true}"#,
    ] {
        let options = parse_convert_options(json).expect("parses");
        let expected = cg3::convert(input.as_bytes(), &options).expect("the library converts");
        assert_eq!(
            convert(&input, json).expect("converts").as_bytes(),
            expected,
            "{json}"
        );
    }

    let split = text("T_MweSplit/input.txt");
    assert_eq!(
        mwesplit(&split).expect("splits").as_bytes(),
        cg3::mwesplit(split.as_bytes()).expect("the library splits"),
    );

    let bytes = cg3b("Apertium/T_Select/grammar.cg3");
    let grammar = read_grammar(&bytes, "{}").expect("loads");
    let input = text("Apertium/T_Select/input.txt");
    for json in [
        "{}",
        r#"{"trace": true, "wordformCase": true}"#,
        r#"{"noWordForms": true, "first": true}"#,
        r#"{"generation": true}"#,
        r#"{"streamFormat": "matxin"}"#,
    ] {
        let options = parse_proc_options(json).expect("parses");
        let expected = grammar
            .compiled()
            .proc(input.as_bytes(), &options)
            .expect("runs");
        assert_eq!(
            grammar.proc(&input, json).expect("runs").as_bytes(),
            expected,
            "{json}"
        );
    }
    assert_eq!(
        grammar.proc(&input, "{}").expect("runs"),
        text("Apertium/T_Select/expected.txt")
    );
}

// [spec:cg3:req:wasm.tools/test]
// [spec:cg3:req:wasm.errors+1/test]
#[test]
fn failures_are_run_failed_and_grammars_stay_usable() {
    let reserved =
        "{\"w\": \"a\", \"rs\": [{\"l\": \"a\", \"ts\": [\"n\"]}], \"ds\": 4294967295}\n";
    let failed = convert(reserved, r#"{"input": "jsonl"}"#).expect_err("a reserved number");
    assert_eq!(failed.code(), ErrorCode::RunFailed);
    assert!(!failed.message().is_empty());

    let source = "DELIMITERS = \"<.>\" ;\nSECTION\nEXTERNAL ONCE /nonexistent/cg3-external (*) ;\n";
    let bytes = CompiledGrammar::from_text(
        source.as_bytes(),
        "external.cg3",
        MemoryResolver::default(),
        &LoadOptions::default(),
    )
    .and_then(|g| g.to_cg3b())
    .expect("compiles");
    let external = read_grammar(&bytes, "{}").expect("loads");
    let input = "\"<word>\"\n\t\"word\" n\n";
    let failed = external
        .proc(input, r#"{"streamFormat": "cg"}"#)
        .expect_err("the EXTERNAL rule fails");
    assert_eq!(failed.code(), ErrorCode::RunFailed);
    assert!(!failed.message().is_empty());
    assert_eq!(
        external
            .proc(input, r#"{"streamFormat": "cg"}"#)
            .map_err(|e| e.code()),
        Err(ErrorCode::RunFailed),
        "the grammar fails again, as it did"
    );
}
