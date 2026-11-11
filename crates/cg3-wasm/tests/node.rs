//! The binding as JavaScript meets it, run under Node by
//! wasm-bindgen-test-runner: `cargo test -p cg3-wasm --target
//! wasm32-unknown-unknown`.
#![cfg(target_arch = "wasm32")]

use cg3::binary_grammar::write_binary_grammar;
use cg3::grammar::GrammarDraft;
use cg3::textual_parser::TextualParser;
use cg3_wasm::{ErrorCode, GlueError, read_grammar};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

fn cg3b(src: &str) -> Vec<u8> {
    let mut parser = TextualParser::new(GrammarDraft::default(), false);
    parser.parse_grammar_utf8(src.as_bytes()).expect("parses");
    let mut blob = Vec::new();
    write_binary_grammar(&parser.grammar.finish().expect("finishes"), &mut blob).expect("writes");
    blob
}

fn field(object: &JsValue, name: &str) -> JsValue {
    js_sys::Reflect::get(object, &JsValue::from_str(name)).expect("an object")
}

/// The plain object an error reaches JavaScript as.
fn thrown(error: GlueError) -> (String, String) {
    let value = JsValue::from(error);
    let text = |name| field(&value, name).as_string().expect("a string");
    (text("code"), text("message"))
}

// [spec:cg3:req:wasm.grammar-load+1/test]
// [spec:cg3:req:wasm.errors+1/test]
#[wasm_bindgen_test]
fn loading_reports_what_the_library_says() {
    let blob = cg3b(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#);
    let grammar = read_grammar(&blob, "{}").expect("loads");
    let info = grammar.info();
    assert_eq!(field(&info, "rules").as_f64(), Some(1.0));
    assert_eq!(field(&info, "hasDependencyRules").as_bool(), Some(false));
    drop(grammar);

    let truncated = read_grammar(&blob[..blob.len() / 2], "{}")
        .err()
        .expect("refused");
    let (code, message) = thrown(truncated);
    assert_eq!(code, "invalid-grammar");
    assert!(message.contains("ends early"), "{message}");
}

// [spec:cg3:req:wasm.apply+1/test]
// [spec:cg3:req:wasm.errors+1/test]
#[wasm_bindgen_test]
fn apply_takes_and_returns_javascript_values() {
    let grammar = read_grammar(&cg3b(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#), "{}").expect("loads");
    let applied = grammar
        .apply("\"<word>\"\n\t\"word\" n\n\t\"word\" v\n", "{}")
        .expect("runs");
    assert_eq!(
        field(&applied, "output").as_string().expect("a string"),
        "\"<word>\"\n\t\"word\" n\n"
    );
    assert_eq!(
        js_sys::Array::from(&field(&applied, "warnings")).length(),
        0
    );

    let refused = grammar
        .apply("", r#"{"noMapings": true}"#)
        .expect_err("a misspelt option is refused");
    assert_eq!(refused.code(), ErrorCode::InvalidArgument);
}

// [spec:cg3:req:wasm.no-process/test]
// [spec:cg3:req:wasm.errors+1/test]
#[wasm_bindgen_test]
fn an_external_rule_fails_its_run_without_trapping() {
    let grammar = read_grammar(
        &cg3b("DELIMITERS = \"<.>\" ;\nSECTION\nEXTERNAL ONCE ./external (*) ;\n"),
        "{}",
    )
    .expect("loads");
    let failed = grammar
        .apply("\"<word>\"\n\t\"word\" n\n", "{}")
        .expect_err("no process can start");
    let (code, message) = thrown(failed);
    assert_eq!(code, "run-failed");
    assert!(
        message.contains("EXTERNAL on line 3 could not be started"),
        "{message}"
    );

    let applied = read_grammar(&cg3b(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#), "{}")
        .expect("loads")
        .apply("\"<word>\"\n\t\"word\" n\n", "{}");
    assert!(applied.is_ok(), "the module still runs after a failed run");
}
