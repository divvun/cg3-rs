//! What an EMBEDDER sees on stderr — the path that does not go through
//! `run_cli`.
//!
//! The parser ports the C++ `throw int` parse-error recovery as
//! `panic_any(ParseError)`, caught once per directive so parsing can continue.
//! The default panic hook prints for every one of those, several frames before
//! the catch, so a library consumer got `thread 'main' panicked ... Box<dyn
//! Any>` for an ordinary bad grammar — reading like a crash next to the `Err`
//! it also returned.
//!
//! Asserting this needs a real process: panic output is a property of the
//! process's hook, not of a return value. The test re-executes its own binary
//! and reads the child's stderr.

use std::process::Command;

const CHILD: &str = "CG3_EMBED_DIAGNOSTICS_CHILD";

/// Two bad regex tags plus the rule that references the first set, so the
/// parser recovers more than once and would print more than one panic.
const BAD_GRAMMAR: &str = "DELIMITERS = \"<.>\" ;\n\
                           LIST a = \"[:script=Greek:]\"r ;\n\
                           LIST b = \"[:script=Latin:]\"r ;\n\
                           SELECT a ;\n";

fn child_loads_a_bad_grammar() {
    cg3::tools::init_diagnostics();
    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let err = parser
        .parse_grammar_utf8(BAD_GRAMMAR.as_bytes())
        .expect_err("this grammar must not parse");
    // Print through the same channel an embedder would, so the parent can
    // confirm the child really reached the failure rather than exiting early.
    eprintln!("embedder saw: {err}");
}

// [spec:cg3:req:errors.control-flow-quiet/test]
#[test]
fn embedder_sees_no_panic_noise() {
    if std::env::var(CHILD).is_ok() {
        child_loads_a_bad_grammar();
        return;
    }

    let exe = std::env::current_exe().expect("path to this test binary");
    let out = Command::new(exe)
        .env(CHILD, "1")
        .args(["--exact", "embedder_sees_no_panic_noise", "--nocapture"])
        .output()
        .expect("re-exec the test binary");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "child failed:\n{stderr}");
    // The child must actually have hit the recovery path — otherwise the
    // absence of panic noise below would prove nothing.
    assert!(
        stderr.contains("on line 2 near"),
        "child never reached the parse errors:\n{stderr}"
    );
    assert!(
        stderr.contains("embedder saw: grammar could not be parsed"),
        "child did not return the error to its caller:\n{stderr}"
    );

    assert!(
        !stderr.contains("panicked at"),
        "recoverable parse errors leaked Rust panic noise to an embedder:\n{stderr}"
    );
    assert!(
        !stderr.contains("Box<dyn Any>"),
        "control-flow payload reached stderr:\n{stderr}"
    );
}

/// Errors are layered by boundary: a grammar that will not load surfaces as a
/// `GrammarError`, never as a stream variant, and the collection of failures
/// stays legible rather than collapsing to a count.
// [spec:cg3:req:errors.layered/test]
#[test]
fn load_failures_are_grammar_errors() {
    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let err = parser
        .parse_grammar_utf8(BAD_GRAMMAR.as_bytes())
        .expect_err("this grammar must not parse");

    let cg3::error::Cg3Error::Grammar(g) = &err else {
        panic!("a load failure must be a GrammarError, got {err:?}");
    };
    assert!(
        matches!(g, cg3::error::GrammarError::Parse { .. }),
        "got {g:?}"
    );
}

/// A tag whose regex will not compile must reach the caller naming the tag —
/// the diagnostic travels in the value, not only in the log.
// [spec:cg3:req:errors.layered/test]
#[test]
fn tag_regex_failures_name_the_tag() {
    let src = "DELIMITERS = \"<.>\" ;\nLIST a = \"x\"r ;\n";
    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    parser.parse_grammar_utf8(src.as_bytes()).expect("parses");
    let mut grammar = parser.grammar;
    grammar.reindex(false, false).unwrap();

    let mut applicator = cg3::grammar_applicator::GrammarApplicator::new(grammar);
    let err = applicator
        .set_text_delimiter("[:script=Greek:]".to_string())
        .expect_err("an ICU in-set property must be rejected");

    let rendered = err.to_string();
    assert!(rendered.contains("in-set property"), "{rendered}");
    assert!(
        !err.tag_regex_errors().is_empty(),
        "the diagnostic must be reachable structurally, not only as text"
    );
}

/// A bad grammar reports EVERY recoverable error in one pass, not just the
/// first — the property the old catch_unwind provided as a side effect and the
/// accumulating loop now provides on purpose.
// [spec:cg3:req:errors.parse-reports-all/test]
#[test]
fn every_recoverable_error_is_reported() {
    // Three independent failures on three separate lines.
    let src = "DELIMITERS = \"<.>\" ;\n\
               LIST a = \"[:script=Greek:]\"r ;\n\
               LIST b = \"[:script=Latin:]\"r ;\n\
               SELECT a ;\n";
    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let err = parser
        .parse_grammar_utf8(src.as_bytes())
        .expect_err("must not parse");

    let cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::Parse { count }) = &err else {
        panic!("expected a parse failure, got {err:?}");
    };
    assert!(
        *count >= 3,
        "all three failures must be reported, got {count}"
    );
}
