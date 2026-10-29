//! The binding as JavaScript meets it, run under Node by
//! wasm-bindgen-test-runner: `cargo test -p cg3-wasm --target
//! wasm32-unknown-unknown`.
#![cfg(target_arch = "wasm32")]

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::GrammarDraft;
use cg3::textual_parser::TextualParser;
use cg3_wasm::Grammar;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

fn cg3b(src: &str) -> Vec<u8> {
    let mut parser = TextualParser::new(GrammarDraft::default(), false);
    parser.parse_grammar_utf8(src.as_bytes()).expect("parses");
    let mut blob = Vec::new();
    BinaryGrammar::new(parser.grammar.finish().expect("finishes"))
        .write_binary_grammar(&mut blob)
        .expect("writes");
    blob
}

fn message(e: wasm_bindgen::JsError) -> String {
    let value = JsValue::from(e);
    let error: js_sys::Error = value.dyn_into().expect("an Error");
    String::from(error.message())
}

fn field(object: &JsValue, name: &str) -> JsValue {
    js_sys::Reflect::get(object, &JsValue::from_str(name)).unwrap()
}

// [spec:cg3:req:wasm.grammar-load/test]
// [spec:cg3:req:wasm.errors/test]
#[wasm_bindgen_test]
fn loading_reports_what_the_library_says() {
    let blob = cg3b(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#);
    let grammar = Grammar::from_bytes(&blob).expect("loads");
    let info = grammar.info().unwrap();
    assert_eq!(field(&info, "rules").as_f64(), Some(1.0));
    assert_eq!(field(&info, "hasDependencyRules").as_bool(), Some(false));
    drop(grammar);

    let truncated = Grammar::from_bytes(&blob[..blob.len() / 2])
        .err()
        .expect("refused");
    assert!(message(truncated).contains("ends early"));
}

// [spec:cg3:req:wasm.apply+1/test]
#[wasm_bindgen_test]
fn apply_takes_and_returns_javascript_values() {
    let grammar = Grammar::from_bytes(&cg3b(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#)).unwrap();
    let applied = grammar
        .apply("\"<word>\"\n\t\"word\" n\n\t\"word\" v\n", None)
        .unwrap();
    assert_eq!(
        field(&applied, "output").as_string().unwrap(),
        "\"<word>\"\n\t\"word\" n\n"
    );
    assert_eq!(
        js_sys::Array::from(&field(&applied, "warnings")).length(),
        0
    );

    let options = js_sys::JSON::parse(r#"{"noMapings": true}"#).unwrap();
    assert!(
        grammar.apply("", Some(options.unchecked_into())).is_err(),
        "a misspelt option is refused"
    );
}

// [spec:cg3:req:wasm.no-process/test]
// [spec:cg3:req:wasm.errors/test]
#[wasm_bindgen_test]
fn an_external_rule_fails_its_run_without_trapping() {
    let grammar = Grammar::from_bytes(&cg3b(
        "DELIMITERS = \"<.>\" ;\nSECTION\nEXTERNAL ONCE ./external (*) ;\n",
    ))
    .unwrap();
    let failed = grammar
        .apply("\"<word>\"\n\t\"word\" n\n", None)
        .expect_err("no process can start");
    let message = message(failed);
    assert!(
        message.contains("EXTERNAL on line 3 could not be started"),
        "{message}"
    );

    let applied = Grammar::from_bytes(&cg3b(r#"DELIMITERS = "<.>" ; SELECT (n) ;"#))
        .unwrap()
        .apply("\"<word>\"\n\t\"word\" n\n", None);
    assert!(applied.is_ok(), "the module still runs after a failed run");
}
