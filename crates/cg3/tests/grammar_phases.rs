//! A grammar's load phases (`docs/spec/port/src/grammar_phases.md`): a
//! textual grammar is resolved and indexed, a `.cg3b` only indexed, and both
//! come out as the grammar the C++ `Grammar::reindex` builds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::GrammarCore;
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

/// The fixture at `path`, parsed and reindexed.
fn load_text(path: &PathBuf) -> GrammarCore {
    let src = std::fs::read(path).unwrap();
    let mut parser = TextualParser::new(GrammarCore::default(), false);
    parser
        .parse_grammar_named(&src, path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("{} parses: {e}", path.display()));
    let mut grammar = parser.grammar;
    let _ = grammar.reindex(false, false).unwrap();
    grammar
}

/// `blob`, read as a `.cg3b` and reindexed.
fn load_binary(blob: &[u8]) -> GrammarCore {
    let mut reader = BinaryGrammar::new(GrammarCore::default());
    reader.parse_grammar_buffer(blob).unwrap();
    let mut grammar = reader.grammar;
    let _ = grammar.reindex(false, false).unwrap();
    grammar
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
#[test]
fn text_and_cg3b_index_the_same() {
    let mut compared = 0;
    for path in fixture_grammars() {
        let text = load_text(&path);
        let from_text = indexes(&text);
        let blob = write(text);
        let binary = load_binary(&blob);
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
