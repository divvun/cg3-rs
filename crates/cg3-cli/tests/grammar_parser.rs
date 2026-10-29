//! Wave-3 test facets for the grammar/parser area: `src/Grammar.cpp`,
//! `src/Grammar.hpp`, `src/TextualParser.cpp`, `src/TextualParser.hpp`,
//! `src/IGrammarParser.hpp`.
//!
//! Strategy: drive the library IN-PROCESS. `TextualParser::new(...)` parses
//! real fixture grammars from `test/T_*` plus a few small crafted grammar
//! strings that hit paths the fixtures don't; `Grammar` methods that no parse
//! path reaches (destroy_*, remove_numeric_tags, ...) are called directly and
//! their effects asserted. The binary side (`trie_unserialize`) is driven by
//! compiling a fixture with the real `cg-comp` binary and loading the `.cg3b`
//! in-process through `BinaryGrammar`.

use std::path::PathBuf;

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::GrammarNumbered;

fn repo_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

// trie_unserialize (Grammar.hpp): compile a fixture grammar to .cg3b with the
// real cg-comp binary, then load it in-process through BinaryGrammar — every
// set's trie/trie_special in the binary image is rebuilt via trie_unserialize.
// [spec:cg3:sem:grammar.cg3.trie-unserialize-fn+1/test]
#[test]
fn binary_grammar_roundtrip_unserializes_tries() {
    let grammar = repo_root().join("test/T_SetOps/grammar.cg3");
    let bin = std::env::temp_dir().join(format!("cg3-grammar-parser-{}.cg3b", std::process::id()));
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .arg(&grammar)
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp failed");

    let mut bg = BinaryGrammar::new(GrammarNumbered::default());
    let rv = bg.parse_grammar_filename(bin.to_str().unwrap());
    let _ = std::fs::remove_file(&bin);
    rv.expect("binary grammar failed to load");

    let g = &bg.grammar;
    assert!(g.is_binary);
    assert!(g.num_tags > 0, "tags unserialized");
    let with_trie = (0..g.sets_list.capacity())
        .filter(|&i| {
            g.sets_list
                .try_get(i)
                .is_some_and(|s| !s.trie.is_empty() || !s.trie_special.is_empty())
        })
        .count();
    assert!(with_trie > 0, "trie_unserialize rebuilt set tries");
}
