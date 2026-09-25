//! A grammar's load phases (`docs/spec/port/src/grammar_phases.md`): the
//! textual parser builds a draft and the `.cg3b` reader a numbered grammar,
//! `finish` makes either an indexed grammar, and both come out as the grammar
//! the C++ `Grammar::reindex` builds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::{GrammarCore, GrammarDraft, GrammarNumbered};
use cg3::textual_parser::TextualParser;

/// `crates/cg3` -> repo root (holds `test/`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Every fixture grammar under `test/`, with its path.
fn fixture_grammars() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(repo_root().join("test"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path().join("grammar.cg3")))
        .filter(|p| p.exists())
        .collect();
    paths.sort();
    paths
}

fn parse_text(path: &Path) -> GrammarDraft {
    let src = std::fs::read(path).unwrap();
    let mut parser = TextualParser::new(GrammarDraft::default(), false);
    parser
        .parse_grammar_named(&src, path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("{} parses: {e}", path.display()));
    parser.grammar
}

fn read_binary(blob: &[u8]) -> GrammarNumbered {
    let mut reader = BinaryGrammar::new(GrammarNumbered::default());
    reader.parse_grammar_buffer(blob).unwrap();
    reader.grammar
}

/// The fixture at `path`, parsed and finished.
fn load_text(path: &Path) -> GrammarCore {
    parse_text(path).finish().unwrap()
}

fn write(grammar: GrammarCore) -> Vec<u8> {
    let mut out = Vec::new();
    BinaryGrammar::new(grammar)
        .write_binary_grammar(&mut out)
        .unwrap();
    out
}

/// What indexing builds that the `.cg3b` does not carry, by number, so two
/// loads of one grammar compare.
#[derive(Debug, PartialEq)]
struct Indexes {
    sections: Vec<u32>,
    by_section: [Vec<u32>; 4],
    wf_rules: Vec<u32>,
    has_protect: bool,
    rules_by_set: BTreeMap<u32, Vec<u32>>,
    rules_by_tag: BTreeMap<u32, Vec<u32>>,
    sets_by_tag: BTreeMap<u32, Vec<bool>>,
    sets_any: Option<Vec<bool>>,
    rules_any: Option<Vec<u32>>,
    rule_flags: Vec<String>,
}

fn indexes(g: &GrammarCore) -> Indexes {
    let numbers = |rules: &[cg3::arena::RuleId]| -> Vec<u32> {
        rules.iter().map(|r| g.rule_by_number[r.0].number).collect()
    };
    Indexes {
        sections: g.sections.to_vec(),
        by_section: [
            numbers(&g.before_sections),
            numbers(&g.rules),
            numbers(&g.after_sections),
            numbers(&g.null_section),
        ],
        wf_rules: numbers(&g.wf_rules),
        has_protect: g.has_protect,
        rules_by_set: g
            .rules_by_set
            .iter()
            .map(|(&k, v)| (k, v.begin().collect()))
            .collect(),
        rules_by_tag: g
            .rules_by_tag
            .iter()
            .map(|(&k, v)| (k, v.begin().collect()))
            .collect(),
        sets_by_tag: g.sets_by_tag.iter().map(|(&k, v)| (k, v.clone())).collect(),
        sets_any: g.sets_any.clone(),
        rules_any: g.rules_any.as_ref().map(|v| v.begin().collect()),
        rule_flags: (0..g.rule_by_number.capacity())
            .filter_map(|i| g.rule_by_number.try_get(i))
            .map(|r| format!("{:?}", r.flags))
            .collect(),
    }
}

// A textual grammar is resolved and indexed, the `.cg3b` written from it only
// indexed: both must come out with the same indexes. Over every fixture grammar.
// [spec:cg3:req:grammar-phases.same-output/test]
// [spec:cg3:req:grammar-phases.finish/test]
#[test]
fn text_and_cg3b_index_the_same() {
    let mut compared = 0;
    for path in fixture_grammars() {
        let text = load_text(&path);
        let from_text = indexes(&text);
        let blob = write(text);
        let binary = read_binary(&blob).finish().unwrap();
        assert_eq!(
            indexes(&binary),
            from_text,
            "{}: indexes differ between the text and its .cg3b",
            path.display()
        );
        compared += 1;
    }
    assert!(compared > 50, "only {compared} fixtures compared");
}

// Each loader hands back the phase its output is in: the textual parser a
// draft, whose references are content hashes, and the `.cg3b` reader a
// numbered grammar with none of the indexes built.
// [spec:cg3:req:grammar-phases.loaders/test]
#[test]
fn loaders_return_their_phase() {
    let path = repo_root().join("test/T_SetParentChild/grammar.cg3");
    let draft: GrammarDraft = parse_text(&path);
    assert!(!draft.is_binary);
    assert!(
        !draft.sets_by_contents.is_empty(),
        "a draft refers to its sets by content hash"
    );
    let blob = write(draft.finish().unwrap());

    let numbered: GrammarNumbered = read_binary(&blob);
    assert!(numbered.is_binary);
    assert!(
        numbered.sets_by_contents.is_empty(),
        "a numbered grammar refers to its sets by number"
    );
    assert!(
        numbered.sets_by_tag.is_empty() && numbered.rules_by_tag.is_empty(),
        "reading builds no index"
    );
    assert!(!numbered.finish().unwrap().sets_by_tag.is_empty());
}

// An indexed grammar that gives its indexes up has none left, and finishing it
// again builds the same ones: nothing stale or duplicated survives. Over every
// fixture grammar, from its text and from its `.cg3b`.
// [spec:cg3:req:grammar-phases.index-rebuilds/test]
#[test]
fn refinishing_rebuilds_the_same_indexes() {
    for path in fixture_grammars() {
        let text = load_text(&path);
        let blob = write(load_text(&path));
        for (origin, grammar) in [
            ("text", text),
            (".cg3b", read_binary(&blob).finish().unwrap()),
        ] {
            let first = indexes(&grammar);
            let numbered = grammar.into_numbered();
            assert!(
                numbered.wf_rules.is_empty()
                    && numbered.sections.is_empty()
                    && numbered.rules_by_set.is_empty()
                    && numbered.rules_by_tag.is_empty()
                    && numbered.sets_by_tag.is_empty()
                    && numbered.sets_any.is_none()
                    && numbered.rules_any.is_none(),
                "{} from {origin}: indexes survive into_numbered",
                path.display()
            );
            let again = numbered.finish().unwrap();
            assert_eq!(
                indexes(&again),
                first,
                "{} from {origin}: refinishing changed the indexes",
                path.display()
            );
        }
    }
}
