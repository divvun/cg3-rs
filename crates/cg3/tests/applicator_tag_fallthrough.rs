//! Characterisation of the applicator's tag fall-through.
//!
//! `parser_helpers::parse_tag` is generic over `ParseTagState`. The textual
//! parser's `error_near` diverges, so the code after each of its six error sites
//! is unreachable. The applicator's prints and RETURNS, so the same code runs —
//! with the input that just failed validation — and a tag gets built from it.
//!
//! These tests pinned that behaviour across the conversion, and now record what
//! replaced it: every site is a `?`, so tag construction stops instead of
//! continuing. The golden and Apertium suites passed unchanged either side of
//! the change, which is the evidence that closed
//! `[dec:cg3:parse-tag-aborts-on-invalid]`.
//!
//! The failure used to stop at `add_tag`, which logged it and interned an empty
//! tag in its place. It now reaches the caller as a `RunError`, so what these
//! assert is the error rather than the substitute tag it used to leave behind.

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

/// What `add_tag` yields for input that fails validation inside `parse_tag`:
/// the rendered failure, or the tag text if one was built after all.
fn observe(txt: &str) -> Result<String, String> {
    let mut app = applicator();
    match app.add_tag(txt, T_VARSTRING) {
        Ok(id) => Ok(app.grammar.single_tags_list[id.0].tag.clone()),
        Err(e) => Err(e.to_string()),
    }
}

/// Empty text used to fall through into `inlines::is_textual`, which indexes
/// `s[0]` unguarded and panicked — inherited C++ UB ("Panics on empty `s`").
/// Stopping at the guard removes it: the tag is simply not built.
#[test]
fn empty_varstring_tag_no_longer_panics() {
    let e = observe("").expect_err("empty text builds no tag");
    assert!(e.contains("could not be constructed"), "{e}");
}

/// A tag opening with `(` trips the second guard. It used to intern verbatim;
/// construction now stops there.
#[test]
fn paren_leading_varstring_tag_is_not_interned() {
    let e = observe("(foo").expect_err("a `(`-leading varstring tag is not built");
    assert!(
        e.contains("`(foo`"),
        "the failure names the offending tag: {e}"
    );
}

/// A varstring tag whose regex will not compile used to be interned anyway,
/// without a compiled regex — a tag that could never match what it named.
/// Construction now stops instead.
#[test]
fn uncompilable_regex_varstring_tag_is_not_interned() {
    let e = observe("\"[:script=Greek:]\"r").expect_err("no tag built from a bad pattern");
    assert!(e.contains("[:script=Greek:]"), "{e}");
}

/// The non-varstring path does NOT go through `parse_tag`, so it is unaffected
/// by the conversion. Pinned so a regression there is distinguishable.
#[test]
fn plain_tags_bypass_the_parse_tag_path() {
    let mut app = applicator();
    let id = app
        .add_tag("\"<word>\"", TagType::empty())
        .expect("a plain tag never goes near parse_tag");
    assert_eq!(app.grammar.single_tags_list[id.0].tag, "\"<word>\"");
}
