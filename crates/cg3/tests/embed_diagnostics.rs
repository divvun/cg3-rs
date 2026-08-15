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
/// accumulating loop now provides on purpose. Each one arrives whole, not as a
/// tally: the failure the caller has to explain is the error, not the count.
// [spec:cg3:req:errors.parse-reports-all/test]
// [spec:cg3:req:diagnostics.errors-carried/test]
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

    let cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::Parse { errors, sources }) = &err
    else {
        panic!("expected a parse failure, got {err:?}");
    };
    assert!(
        errors.len() >= 3,
        "all three failures must be reported, got {}",
        errors.len()
    );
    assert_eq!(sources.len(), 1, "one buffer, one source");
    assert_eq!(
        sources[0].text, src,
        "the retained source must be the text as written, padding stripped"
    );
}

/// The span an error carries must select the offending grammar text out of the
/// retained source — the whole point of keeping both.
// [spec:cg3:req:diagnostics.span/test]
// [spec:cg3:req:diagnostics.source-retained/test]
#[test]
fn spans_select_the_offending_text() {
    let src = "DELIMITERS = \"<.>\" ;\nLIST a = \"[:script=Greek:]\"r ;\nSELECT a ;\n";
    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let err = parser
        .parse_grammar_utf8(src.as_bytes())
        .expect_err("must not parse");

    let cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::Parse { errors, sources }) = &err
    else {
        panic!("expected a parse failure, got {err:?}");
    };
    let span = errors[0].span.as_ref().expect("a syntax error has a place");
    let text: Vec<char> = sources[span.source].text.chars().collect();
    let quoted: String = text[span.range.clone()].iter().collect();
    assert!(
        quoted.starts_with("\"[:script=Greek:]\""),
        "the span must cover the tag that failed, got {quoted:?}"
    );
    assert!(
        !quoted.contains('\n'),
        "a span stays on one line so a renderer underlines one line, got {quoted:?}"
    );
}

/// `INCLUDE` makes one parse cover several files, so a span has to name WHICH
/// one — and two failures in two files must land on two different sources.
// [spec:cg3:req:diagnostics.source-identity/test]
#[test]
fn included_files_are_separate_sources() {
    let dir = std::env::temp_dir().join(format!("cg3-diag-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let included = dir.join("lists.cg3");
    std::fs::write(&included, "LIST a = \"[:script=Greek:]\"r ;\n").expect("write include");
    let top = format!(
        "DELIMITERS = \"<.>\" ;\nINCLUDE {} ;\nSELECT nosuch ;\n",
        included.display()
    );

    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let err = parser
        .parse_grammar_named(top.as_bytes(), "top.cg3")
        .expect_err("must not parse");

    let cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::Parse { errors, sources }) = &err
    else {
        panic!("expected a parse failure, got {err:?}");
    };
    assert_eq!(sources.len(), 2, "the top-level grammar and its include");
    let places: Vec<usize> = errors
        .iter()
        .filter_map(|e| e.span.as_ref().map(|s| s.source))
        .collect();
    assert!(
        places.contains(&0) && places.contains(&1),
        "one failure per file, each naming its own source, got {places:?}"
    );
    let _ = std::fs::remove_file(&included);
}

/// Every rule a parse keeps must know where it was written — which source, and
/// the span of that source its own text occupies — including the rules that came
/// out of an `INCLUDE`d file. The span is checked by slicing it back out of the
/// file it names: anything less proves only that a number was stored.
// [spec:cg3:req:diagnostics.rule-provenance/test]
#[test]
fn rules_know_where_they_were_written() {
    let dir = std::env::temp_dir().join(format!("cg3-prov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let included = dir.join("rules.cg3");
    let top = dir.join("top.cg3");
    std::fs::write(&included, "SELECT b ;\n").expect("write include");
    std::fs::write(
        &top,
        "DELIMITERS = \"<.>\" ;\nLIST a = x ;\nLIST b = y ;\n\
         SECTION\nSELECT a ;\nINCLUDE rules.cg3 ;\n",
    )
    .expect("write top");

    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let bytes = std::fs::read(&top).expect("read top");
    parser
        .parse_grammar_named(&bytes, &top.to_string_lossy())
        .expect("parses");
    let grammar = parser.grammar;

    assert_eq!(
        grammar.source_names.len(),
        2,
        "the top-level grammar and its include, got {:?}",
        grammar.source_names
    );

    let mut quoted: Vec<String> = Vec::new();
    for i in 0..grammar.rule_by_number.capacity() {
        let Some(rule) = grammar.rule_by_number.try_get(i) else {
            continue;
        };
        let p = rule.provenance.expect("a parsed rule knows its place");
        let text = std::fs::read_to_string(&grammar.source_names[p.source as usize])
            .expect("the source names a readable file");
        let chars: Vec<char> = text.chars().collect();
        quoted.push(chars[p.begin as usize..p.end as usize].iter().collect());
    }
    quoted.sort();
    assert_eq!(
        quoted,
        vec!["SELECT a ".to_string(), "SELECT b ".to_string()],
        "each rule's span must slice its own text out of its own source"
    );

    let _ = std::fs::remove_file(&included);
    let _ = std::fs::remove_file(&top);
}

/// A parse told its file name heads its reports with it, instead of the
/// `<utf8-memory>` placeholder a caller that only had bytes gets.
// [spec:cg3:req:diagnostics.source-named/test]
#[test]
fn a_named_parse_reports_its_file_name() {
    let src = "DELIMITERS = \"<.>\" ;\nLIST a = \"[:script=Greek:]\"r ;\n";
    let mut parser =
        cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let err = parser
        .parse_grammar_named(src.as_bytes(), "grammars/nb.cg3")
        .expect_err("must not parse");

    let cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::Parse { errors, sources }) = &err
    else {
        panic!("expected a parse failure, got {err:?}");
    };
    assert_eq!(sources[0].name, "grammars/nb.cg3");
    assert_eq!(errors[0].file, "nb.cg3");
}
