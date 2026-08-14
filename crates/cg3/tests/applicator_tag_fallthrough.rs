//! Characterisation of the applicator's tag fall-through.
//!
//! `parser_helpers::parse_tag` is generic over `ParseTagState`. The textual
//! parser's `error_near` diverges, so the code after each of its six error sites
//! is unreachable. The applicator's prints and RETURNS, so the same code runs —
//! with the input that just failed validation — and a tag gets built from it.
//!
//! These tests pin that behaviour so `errors-idiomatic.parser` cannot change it
//! silently. Unifying `error_near` on `Result` makes those sites `?`, which
//! aborts tag construction instead. That is very likely a fix, but it is a
//! behaviour change to a shipped engine, and
//! `[dec:cg3:parse-tag-aborts-on-invalid]` stays @tentative until these say what
//! actually depends on it.
//!
//! A FAILURE HERE IS THE POINT. When the parser conversion lands, read what
//! changed and decide: move the decision to @decided if the new behaviour is
//! right, or implement the rejected alternative (a typed recovery signal) if
//! something real depended on the old one.

use cg3::grammar::Grammar;
use cg3::grammar_applicator::GrammarApplicator;
use cg3::tag::{T_VARSTRING, TagType};
use cg3::textual_parser::TextualParser;

/// A minimal loaded applicator; `add_tag` needs a reindexed grammar.
fn applicator() -> GrammarApplicator {
    let src = "DELIMITERS = \"<.>\" ;\nLIST a = (n) ;\nSELECT a ;\n";
    let mut parser = TextualParser::new(Grammar::default(), false);
    parser
        .parse_grammar_utf8(src.as_bytes())
        .expect("fixture grammar parses");
    let mut grammar = parser.grammar;
    grammar.reindex(false, false).expect("reindex");
    GrammarApplicator::new(grammar)
}

/// What `add_tag` yields for input that fails validation inside `parse_tag`.
fn observe(txt: &str) -> String {
    let mut app = applicator();
    let id = app.add_tag(txt, T_VARSTRING);
    app.grammar.single_tags_list[id.0].tag.clone()
}

/// Empty text trips the first guard (`to0 == '\0'`) and then PANICS.
///
/// The applicator prints, falls through, and the empty text reaches
/// `inlines::is_textual`, which indexes `s[0]` unguarded — its doc says so:
/// "Panics on empty `s` (C++ front()/back() on empty is UB)". So the
/// fall-through's worst case is not a garbage tag, it is inherited undefined
/// behaviour reproduced as a crash. Converting this site to `?` removes it.
#[test]
#[should_panic(expected = "index out of bounds")]
fn empty_varstring_tag_panics_today() {
    observe("");
}

/// A tag opening with `(` trips the second guard. Today it interns verbatim.
#[test]
fn paren_leading_varstring_tag_still_interns() {
    assert_eq!(
        observe("(foo"),
        "(foo",
        "a `(`-leading varstring tag currently interns verbatim"
    );
}

/// A varstring tag whose regex will not compile trips the compile guard.
/// Today the tag is still interned, without a compiled regex.
#[test]
fn uncompilable_regex_varstring_tag_still_interns() {
    let mut app = applicator();
    let id = app.add_tag("\"[:script=Greek:]\"r", T_VARSTRING);
    let tag = &app.grammar.single_tags_list[id.0];
    assert!(
        tag.regexp.is_none(),
        "the regex did not compile, so no regex is attached"
    );
    assert!(!tag.tag.is_empty(), "but the tag itself is still interned");
}

/// The non-varstring path does NOT go through `parse_tag`, so it is unaffected
/// by the conversion. Pinned so a regression there is distinguishable.
#[test]
fn plain_tags_bypass_the_parse_tag_path() {
    let mut app = applicator();
    let id = app.add_tag("\"<word>\"", TagType::empty());
    assert_eq!(app.grammar.single_tags_list[id.0].tag, "\"<word>\"");
}
