//! What an EMBEDDER gets back from a failed load: the shape of the error value,
//! not a diagnostic it has to scrape out of a log.
//!
//! The process-level half of this file is gone with the mechanism it watched.
//! The parser used to port the C++ `throw int` recovery as a caught panic, so a
//! library consumer saw `thread 'main' panicked ... Box<dyn Any>` for an
//! ordinary bad grammar, and a process-global hook existed to hide it. Nothing
//! unwinds any more, so there is nothing to suppress and nothing to assert
//! about a child process's stderr.

/// Two bad regex tags plus the rule that references the first set, so the
/// parser recovers more than once.
const BAD_GRAMMAR: &str = "DELIMITERS = \"<.>\" ;\n\
                           LIST a = \"[:script=Greek:]\"r ;\n\
                           LIST b = \"[:script=Latin:]\"r ;\n\
                           SELECT a ;\n";

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
    let _ = grammar.reindex(false, false).unwrap();

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
/// first — the property the old caught unwind provided as a side effect and the
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
