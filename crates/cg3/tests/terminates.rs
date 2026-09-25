//! Implementation loops that the C++ lets run forever on some inputs
//! (`[spec:cg3:req:robustness.terminates]`). The test runner kills a test
//! after 10 s, so each of these fails by timing out if its loop comes back.

use std::io::Write as _;
use std::process::{Command, Stdio};

use cg3::error::{Cg3Error, RunError};
use cg3::grammar::GrammarCore;
use cg3::grammar_applicator::GrammarApplicator;
use cg3::textual_parser::TextualParser;

fn run(grammar: &str, input: &str) -> Result<String, Cg3Error> {
    let mut parser = TextualParser::new(GrammarCore::default(), false);
    parser
        .parse_grammar_named(grammar.as_bytes(), "terminates.cg3")
        .expect("grammar parses");
    let grammar = parser.grammar.finish().expect("reindex");
    let mut app = GrammarApplicator::new(grammar.into());
    app.set_grammar().expect("applicator setup");
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(
        &mut std::io::Cursor::new(input.as_bytes().to_vec()),
        &mut out,
    )
    .map(|()| String::from_utf8(out).expect("utf-8 output"))
}

// Captured text that is itself a varstring expands into a varstring again,
// with the same capture, every time.
// [spec:cg3:req:robustness.terminates/test]
#[test]
fn self_reproducing_varstring_is_a_run_error() {
    let grammar = "DELIMITERS = \"<$.>\" ;\nSECTION\nADD (VSTR:$1) (\"<(.*)>\"r) ;\n";
    let got = run(grammar, "\"<VSTR:$1>\"\n\t\"x\" N\n");
    assert!(
        matches!(
            got,
            Err(Cg3Error::Run(RunError::VarstringLoop { line: 3, .. }))
        ),
        "expected the varstring loop to be reported, got {got:?}"
    );
}

// A varstring that expands to a plain tag once is unaffected.
#[test]
fn single_varstring_expansion_still_applies() {
    let grammar = "DELIMITERS = \"<$.>\" ;\nSECTION\nADD (VSTR:got-$1) (\"<(.*)>\"r) ;\n";
    let got = run(grammar, "\"<w>\"\n\t\"x\" N\n").expect("the run completes");
    assert!(got.contains("got-w"), "the expanded tag is added: {got}");
}

// An Apertium analysis in which no tag parses as a baseform: an escaped `<`
// lemma becomes the wordform-shaped `"<x>"`, so the C++'s tag-assignment
// rescan never consumes anything.
// [spec:cg3:req:robustness.terminates/test]
#[test]
fn apertium_analysis_without_baseform_terminates() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cg-conv"))
        .arg("--in-apertium")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cg-conv");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"^a/\\<x\\>$\n")
        .unwrap();
    let out = child.wait_with_output().expect("wait cg-conv");
    assert!(out.status.success(), "cg-conv exited with {}", out.status);
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"<a>\""));
}

// An attaching context that picks a cohort before the target made the rule
// loop resume behind the target it had just done, so COPYCOHORT copied the
// same cohort — and then its copies — without end.
// [spec:cg3:req:robustness.terminates/test]
#[test]
fn copycohort_before_its_target_applies_once() {
    let grammar = "DELIMITERS = \"<.>\" ;\nLIST X = X ;\nSECTION\n\
                   COPYCOHORT (copied) X TO AFTER (-1*A (>>>)) ;\n";
    let got = run(
        grammar,
        "\"<a>\"\n\t\"a\" N\n\"<b>\"\n\t\"b\" X\n\"<.>\"\n\t\".\" PU\n",
    )
    .expect("the run completes");
    assert_eq!(got.matches("copied").count(), 1, "one copy: {got}");
}
