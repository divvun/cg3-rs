//! Grammar text the parser must refuse rather than crash or hang on.
//!
//! Each grammar here used to panic, overflow the stack, or loop forever — in
//! the port, and in the C++, where most of them are undefined behaviour. Each
//! is now a `ParseError` of a kind that says what was wrong, placed at the
//! text that was wrong, and each has a neighbour that must still load, so a
//! fix that refuses too much shows up as well as one that refuses too little.

use cg3::error::{Cg3Error, GrammarError, ParseError, ParseErrorKind as K, ParseSource, RunError};
use cg3::grammar::GrammarCore;
use cg3::textual_parser::TextualParser;

/// The errors a grammar is refused with, and the sources their spans index.
fn refuse_named(src: &str, name: &str) -> (Vec<ParseError>, Vec<ParseSource>) {
    let mut p = TextualParser::new(GrammarCore::default(), false);
    match p.parse_grammar_named(src.as_bytes(), name) {
        Ok(()) => panic!("grammar loaded, and must not:\n{src}"),
        Err(Cg3Error::Grammar(GrammarError::Parse { errors, sources })) => (errors, sources),
        Err(e) => panic!("expected a parse failure, got {e:?}"),
    }
}

/// The one error `src` is refused with, and the text its span selects.
fn refusal(src: &str) -> (ParseError, String) {
    let (mut errors, sources) = refuse_named(src, "x.cg3");
    assert_eq!(
        errors.len(),
        1,
        "exactly one error for:\n{src}\n{errors:#?}"
    );
    let e = errors.remove(0);
    let marked = marked_text(&e, &sources);
    (e, marked)
}

/// The text a span selects out of its source.
fn marked_text(e: &ParseError, sources: &[ParseSource]) -> String {
    let span = e.span.as_ref().expect("the error has a place");
    let text: Vec<char> = sources[span.source].text.chars().collect();
    text[span.range.clone()].iter().collect()
}

/// Parse a grammar that must load.
fn loads(src: &str) -> TextualParser {
    let mut p = TextualParser::new(GrammarCore::default(), false);
    if let Err(e) = p.parse_grammar_named(src.as_bytes(), "x.cg3") {
        panic!("grammar must load:\n{src}\n{e:?}");
    }
    p
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-list-fn+1/test]
#[test]
fn empty_tag_list_is_refused() {
    let (e, marked) = refusal("LIST A = a ;\nLIST X = a\n  () b ;\n");
    assert!(matches!(e.kind, K::EmptyTagList), "{e:?}");
    assert_eq!(e.line, 3);
    assert!(marked.starts_with("()"), "{marked}");
    for src in ["LIST X = () ;\n", "DELIMITERS = () ;\n", "LIST X = ( ) ;\n"] {
        let (e, _) = refusal(src);
        assert!(matches!(e.kind, K::EmptyTagList), "{src}: {e:?}");
    }
    loads("LIST X = a (b c) ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:parser-helpers.cg3.parse-tag-fn+1/test]
#[test]
fn slash_tags_without_body_are_refused() {
    for tag in ["/r", "/i", "/l", "/ri"] {
        let (e, marked) = refusal(&format!("LIST X = a {tag} ;\n"));
        assert!(
            matches!(&e.kind, K::TagWithoutBody { tag: t } if t == tag),
            "{tag}: {e:?}"
        );
        assert!(marked.starts_with(tag), "{marked}");
    }
    loads("LIST X = /a/r /b/i //r ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:parser-helpers.cg3.parse-tag-fn+1/test]
#[test]
fn bare_failfast_marker_is_refused() {
    for src in ["LIST X = ^ ;\n", "LIST X = a ^^ ;\n"] {
        let (e, marked) = refusal(src);
        assert!(matches!(e.kind, K::FailFastWithoutTag), "{src}: {e:?}");
        assert!(marked.starts_with('^'), "{marked}");
    }
    loads("LIST X = ^a ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-fn+1/test]
#[test]
fn empty_context_list_item_is_refused() {
    for item in ["[A,]", "[A,,]", "[A, ]"] {
        let src = format!("LIST A = a ;\nSELECT A IF ({item}) ;\n");
        let (e, marked) = refusal(&src);
        assert!(matches!(e.kind, K::EmptyListItem), "{item}: {e:?}");
        assert_eq!(e.line, 2);
        assert!(
            marked.starts_with(']') || marked.starts_with(','),
            "{marked}"
        );
    }
    loads("LIST A = a ;\nLIST B = b ;\nSELECT A IF ([A, B]) ([B , A]) ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:tag.cg3.tag.parse-numeric-fn+1/test]
#[test]
fn math_variable_outside_a_to_z_is_refused() {
    for tag in ["<x=a+1>", "<x=b=1+1>", "<x=-a>"] {
        let (e, marked) = refusal(&format!("LIST X = {tag} ;\n"));
        assert!(
            matches!(&e.kind, K::NumericTag { tag: t, .. } if &**t == tag),
            "{tag}: {e:?}"
        );
        assert!(marked.starts_with(tag), "{marked}");
    }
    let p = loads("LIST X = <x=A+1> <x=MAX-1> <x=ab+1> ;\n");
    drop(p);
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+1/test]
#[test]
fn numeric_branch_on_template_reference_is_refused() {
    for pos in ["f", "-1f", "1*f"] {
        let src = format!("LIST A = a ;\nTEMPLATE x = (1 A) ;\nSELECT A IF ({pos} T:x) ;\n");
        let (e, marked) = refusal(&src);
        assert!(
            matches!(e.kind, K::NumericBranchWithoutTarget),
            "{pos}: {e:?}"
        );
        assert_eq!(e.line, 3);
        assert!(marked.starts_with("T:x"), "{marked}");
    }
    loads("LIST A = a ;\nSELECT A IF (-1*f (A <W>30>)) ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+1/test]
#[test]
fn unclosed_inline_template_is_refused() {
    // Past the buffer's 40 NULs of padding, where the cursor used to walk off
    // the end, and well short of it.
    for depth in [2, 45, 200] {
        let src = format!("LIST A = a ;\nSELECT A IF {}-1 A", "(".repeat(depth));
        let (e, marked) = refusal(&src);
        assert!(matches!(e.kind, K::UnclosedParenthesis), "{depth}: {e:?}");
        assert_eq!(marked, "(-1 A", "the innermost `(` is marked");
    }
    loads("LIST A = a ;\nSELECT A IF (((-1 A) OR (1 A))) ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:req:robustness.checked-arithmetic/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn+1/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-flags-fn+1/test]
#[test]
fn oversized_numbers_are_refused() {
    let cases = [
        ("SELECT A IF (99999999999 A) ;", "99999999999"),
        ("SELECT A IF (-2147483648 A) ;", "2147483648"),
        ("SELECT A IF (1/99999999999 A) ;", "99999999999"),
        (
            "SELECT SUB:99999999999999999999 A ;",
            "99999999999999999999",
        ),
        ("SELECT SUB:2147483648 A ;", "2147483648"),
    ];
    for (rule, number) in cases {
        let (e, marked) = refusal(&format!("LIST A = a ;\n{rule}\n"));
        assert!(
            matches!(&e.kind, K::NumberOutOfRange { text } if text == number),
            "{rule}: {e:?}"
        );
        assert_eq!(marked, number);
        assert_eq!(e.line, 2);
    }
    loads("LIST A = a ;\nSELECT A IF (-2147483647 A) (1/-3 A) ;\nSELECT SUB:-2147483648 A ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn+1/test]
#[test]
fn unclosed_varstring_brace_is_refused() {
    let (e, marked) = refusal("LIST A = a ;\nADD (\"x{\"v) A ;\n");
    assert!(
        matches!(&e.kind, K::UnclosedVarstringBrace { tag } if tag == "\"x{\"v"),
        "{e:?}"
    );
    assert_eq!(e.line, 2);
    assert_eq!(marked, "\"x{\"v");
    // A varstring built while parsing another tag, which the directive's own
    // checks never see.
    let (e, marked) = refusal("LIST A = a ;\n\nLIST X = VAR:\"x{\"v ;\n");
    assert!(matches!(e.kind, K::UnclosedVarstringBrace { .. }), "{e:?}");
    assert_eq!(e.line, 3);
    assert_eq!(marked, "\"x{\"v");
    loads("LIST A = a ;\nADD (\"x{A}\"v) A ;\n");
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn+1/test]
#[test]
fn unknown_position_without_override_is_refused() {
    let head = "LIST N = n ;\nTEMPLATE q = ? (*) ;\nTEMPLATE qs = (? N) OR (1 N) ;\n";
    let cases = [
        ("ADD (@x) N (? (*)) ;", "? (*)", 4),
        ("ADD (@x) N (T:q) ;", "? (*)", 2),
        ("ADD (@x) N (? T:q) ;", "? T:q", 4),
        ("ADD (@x) N ((? N) OR (1 N)) ;", "? N", 4),
        ("ADD (@x) N (T:qs) ;", "? N", 3),
        ("SETPARENT N TO (? (*)) ;", "? (*)", 4),
    ];
    for (rule, test, line) in cases {
        let (e, marked) = refusal(&format!("{head}{rule}\n"));
        assert!(
            matches!(e.kind, K::PositionWithoutOverride { rule_line: 4 }),
            "{rule}: {e:?}"
        );
        assert_eq!(e.line, line, "{rule}");
        assert_eq!(marked, test, "{rule}");
    }
    // A LINK runs with its own position, override or not.
    let (e, marked) = refusal("LIST N = n ;\nTEMPLATE l = 1 N LINK ? N ;\nADD (@x) N (-1 T:l) ;\n");
    assert!(
        matches!(e.kind, K::PositionWithoutOverride { rule_line: 3 }),
        "{e:?}"
    );
    assert_eq!(marked, "? N");
    // An override replaces the `?`, and carries through nested templates and
    // OR alternatives.
    loads(&format!(
        "{head}TEMPLATE nest = -1 T:q ;\n\
         ADD (@x) N (-1 T:q) (1* T:qs) (NEGATE 2 T:nest) (T:nest) ;\n"
    ));
}

/// A `?` that only a compiled grammar can carry into a run — the parser
/// refuses it in source — is a run error naming the test's line.
// [spec:cg3:req:robustness.grammar-text-errors/test]
#[test]
fn unknown_position_at_run_time_is_error() {
    use cg3::contextual_test::POS_TMPL_OVERRIDE;
    use cg3::grammar_applicator::GrammarApplicator;

    let src = "DELIMITERS = \"<$.>\" ;\nLIST N = n ;\nTEMPLATE q = ? (*) ;\nSECTION\n\
               ADD (@x) N (-1 T:q) ;\n";
    let mut grammar = loads(src).grammar;
    let _ = grammar.reindex(false, false).expect("reindex");
    // Take the override away, as a hand-built `.cg3b` could.
    let rule = (0..grammar.rule_by_number.capacity())
        .find_map(|i| grammar.rule_by_number.try_get(i))
        .expect("one rule");
    let test = *rule.tests.front().expect("one test");
    grammar.contexts_arena[test.0].pos &= !POS_TMPL_OVERRIDE;

    let mut app = GrammarApplicator::new(grammar.into());
    app.set_grammar().expect("applicator setup");
    let mut input = std::io::Cursor::new(b"\"<a>\"\n\t\"a\" n\n\"<b>\"\n\t\"b\" n\n".to_vec());
    let mut out: Vec<u8> = Vec::new();
    let err = app
        .run_grammar_on_text(&mut input, &mut out)
        .expect_err("a `?` with no override cannot run");
    assert!(
        matches!(
            err,
            Cg3Error::Run(RunError::PositionWithoutOverride { line: 3 })
        ),
        "{err:?}"
    );
}

// [spec:cg3:req:robustness.cycles+1/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn+1/test]
#[test]
fn template_cycles_are_refused_by_name() {
    let cases: [(&str, &[&str], &str); 6] = [
        ("TEMPLATE ta = T:ta ;\n", &["ta", "ta"], "T:ta"),
        (
            "TEMPLATE a = T:b ;\nTEMPLATE b = T:a ;\n",
            &["a", "b", "a"],
            "T:b",
        ),
        ("TEMPLATE a = -1 T:a ;\n", &["a", "a"], "-1 T:a"),
        (
            "TEMPLATE a = (T:a) OR (1 A) ;\n",
            &["a", "a"],
            "(T:a) OR (1 A)",
        ),
        (
            "TEMPLATE a = T:b LINK 1 A ;\nTEMPLATE b = NEGATE T:a ;\n",
            &["a", "b", "a"],
            "T:b LINK 1 A",
        ),
        // Used before it is defined: still placed at the definition.
        (
            "SELECT A IF (T:ta) ;\nTEMPLATE ta = T:ta ;\n",
            &["ta", "ta"],
            "T:ta",
        ),
    ];
    for (templates, cycle, at) in cases {
        let src = format!("LIST A = a ;\n{templates}SELECT A IF (T:{}) ;\n", cycle[0]);
        let (errors, sources) = refuse_named(&src, "x.cg3");
        assert_eq!(errors.len(), 1, "{templates}: {errors:#?}");
        let K::TemplateCycle { cycle: named } = &errors[0].kind else {
            panic!("{templates}: {errors:#?}");
        };
        assert_eq!(named, cycle, "{templates}");
        let def_line = 2 + u32::from(templates.starts_with("SELECT"));
        assert_eq!(errors[0].line, def_line, "{templates}");
        assert_eq!(marked_text(&errors[0], &sources), at, "{templates}");
    }
    // Recursion behind a LINK or a later alternative depends on the input, and
    // the C++ test corpus (`test/T_Templates`) has one.
    loads(
        "LIST A = a ;\nLIST B = b ;\n\
         TEMPLATE run = (1 A) OR (1 B LINK T:run) ;\n\
         TEMPLATE alts = (1 A) OR (T:alts) ;\n\
         SELECT A IF (T:run) (T:alts) ;\n",
    );
}

// [spec:cg3:req:robustness.cycles+1/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-from-u-char-fn+1/test]
#[test]
fn include_cycles_are_refused_by_name() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("include-cycles");
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
    write("self.cg3", "LIST A = a ;\nINCLUDE self.cg3 ;\n");
    write("a.cg3", "LIST A = a ;\nINCLUDE b.cg3 ;\n");
    write("b.cg3", "LIST B = b ;\nINCLUDE a.cg3 ;\n");
    write("top.cg3", "INCLUDE a.cg3 ;\nSELECT A ;\n");

    let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let refuse_file = |name: &str| {
        let text = std::fs::read(dir.join(name)).unwrap();
        let src = String::from_utf8(text).unwrap();
        refuse_named(&src, &path(name))
    };

    let (errors, sources) = refuse_file("self.cg3");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        matches!(&errors[0].kind, K::IncludeCycle { cycle } if *cycle == [path("self.cg3"), path("self.cg3")]),
        "{errors:#?}"
    );
    assert_eq!(errors[0].line, 2);
    assert!(marked_text(&errors[0], &sources).starts_with("self.cg3"));

    // Through another file, and entered from a third that is not on the cycle.
    let (errors, sources) = refuse_file("top.cg3");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        matches!(&errors[0].kind, K::IncludeCycle { cycle } if *cycle == [path("a.cg3"), path("b.cg3"), path("a.cg3")]),
        "{errors:#?}"
    );
    assert_eq!(
        errors[0].file, "b.cg3",
        "placed in the file that closes the cycle"
    );
    assert!(marked_text(&errors[0], &sources).starts_with("a.cg3"));
}

// [spec:cg3:req:robustness.grammar-text-errors/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn+1/test]
#[test]
fn jump_through_set_of_sets_is_checked() {
    let sets = "LIST A = (a b) (c d) ;\nLIST B = e ;\nSET C = A OR B ;\n";
    let (errors, _) = refuse_named(&format!("{sets}ANCHOR foo ;\nJUMP C A ;\n"), "x.cg3");
    assert_eq!(
        errors.len(),
        1,
        "the missing anchor is reported: {errors:#?}"
    );
    loads("LIST A = foo ;\nLIST B = bar ;\nSET C = A OR B ;\nANCHOR foo ;\nJUMP C A ;\n");
}
