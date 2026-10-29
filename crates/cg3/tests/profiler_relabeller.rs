//! Integration tests for the Profiler (src/Profiler.cpp/.hpp port —
//! crates/cg3/src/profiler.rs) and the Relabeller (src/Relabeller.cpp/.hpp port —
//! crates/cg3/src/relabeller.rs).
//!
//! Relabeller tests reproduce the test/T_RelabelList, test/T_RelabelSet and
//! test/T_RelabelList_Apertium run.pl protocols exactly:
//!   cg-comp grammar.cg3 t1.cg3b
//!   cg-relabel t1.cg3b relabel.cg3r t2.cg3b
//!   vislcg3 -g t2.cg3b -I input.txt -O out   (cg-proc for the Apertium fixture)
//! then diff -B against expected.txt. All outputs go to std::env::temp_dir().
//!
//! Profiler tests drive the SQLite write/read round-trip in-process (the crate
//! under test links directly) and end-to-end through `vislcg3 --profile` +
//! `cg-annotate`.

#[cfg(feature = "profiler")]
use std::collections::BTreeMap;

#[cfg(feature = "profiler")]
use std::path::PathBuf;

#[cfg(feature = "profiler")]
use cg3::profiler::{ET_CONTEXT, ET_RULE, Entry, Key, Profiler};

#[cfg(feature = "profiler")]
fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cg3-profrel-{}-{}", std::process::id(), name))
}

/// A source grammar holding `det`, `ind` and `noun`, a trie over them
/// (`det ind` and `noun`), and a target grammar where the same ids name other
/// tags, so a copy that read the target at a source id would pick those up.
fn cross_grammar_trie() -> (
    cg3::grammar::GrammarDraft,
    cg3::grammar::GrammarDraft,
    cg3::tag_trie::TagTrie,
) {
    use cg3::grammar::GrammarDraft;
    use cg3::tag_trie::{TagTrie, trie_insert};

    let mut source = GrammarDraft::default();
    let det = source.allocate_tag("det").unwrap();
    let ind = source.allocate_tag("ind").unwrap();
    let noun = source.allocate_tag("noun").unwrap();
    let mut target = GrammarDraft::default();
    for other in ["x", "y", "z", "w"] {
        target.allocate_tag(other).unwrap();
    }

    let mut trie = TagTrie::new();
    assert!(trie_insert(&mut trie, &vec![det, ind], 0));
    assert!(trie_insert(&mut trie, &vec![noun], 0));
    (source, target, trie)
}

/// The text of each top-level key of `copied`, as `target` names it.
fn top_level_texts(
    copied: &cg3::tag_trie::TagTrie,
    target: &cg3::grammar::GrammarDraft,
) -> Vec<String> {
    let mut texts: Vec<String> = copied
        .keys()
        .map(|k| target.single_tags_list[k.0].tag.to_string())
        .collect();
    texts.sort();
    texts
}

// [spec:cg3:sem:relabeller.cg3.trie-copy-fn/test]
// The top level of the copy holds the source's tags, interned into the target.
// Nested levels keep the source's ids (the reproduced quirk).
#[test]
fn relabeller_trie_copy_reads_the_source_grammar() {
    use cg3::relabeller::trie_copy;

    let (source, mut target, trie) = cross_grammar_trie();
    let copied = trie_copy(&trie, &source.single_tags_list, &mut target);
    assert_eq!(top_level_texts(&copied, &target), ["det", "noun"]);
}

// [spec:cg3:sem:relabeller.cg3.trie-copy-helper-fn/test]
// The two-argument re-interning trie-copy helper is DEAD CODE in the C++ (both
// trie_copy and the helper itself recurse through TagTrie.hpp's one-argument
// helper), so no fixture can reach it; it is driven directly in-process here.
#[test]
fn relabeller_trie_copy_helper_reintern() {
    use cg3::relabeller::trie_copy_helper_reintern;

    let (source, mut target, trie) = cross_grammar_trie();
    let ind = source.single_tags_list.capacity() - 2;
    let copied = trie_copy_helper_reintern(&trie, &source.single_tags_list, &mut target);
    assert_eq!(top_level_texts(&copied, &target), ["det", "noun"]);

    let id_of = |text: &str| {
        *copied
            .keys()
            .find(|k| target.single_tags_list[k.0].tag.to_string() == text)
            .unwrap()
    };
    assert!(
        !copied[&id_of("det")].terminal,
        "inner node of det->ind must not be terminal"
    );
    assert!(
        copied[&id_of("noun")].terminal,
        "single-tag path must be terminal"
    );
    assert!(copied[&id_of("noun")].trie.is_none());

    // Nested level: copied through the ONE-arg helper — keyed by the ORIGINAL
    // TagId without re-interning (the reproduced quirk).
    let sub = copied[&id_of("det")]
        .trie
        .as_ref()
        .expect("det must keep its child level");
    assert_eq!(sub.len(), 1);
    assert!(sub.keys().all(|k| k.0 == ind));
}

// [spec:cg3:sem:profiler.cg3.profiler.add-string-fn/test]
// [spec:cg3:sem:profiler.cg3.profiler.add-grammar-fn/test]
// [spec:cg3:sem:profiler.cg3.profiler.add-rule-fn/test]
// [spec:cg3:sem:profiler.cg3.profiler.add-context-fn/test]
// [spec:cg3:sem:profiler.cg3.profiler.key.operator-fn/test]
// [spec:cg3:sem:profiler.cg3.profiler.write-fn/test]
// [spec:cg3:sem:profiler.cg3.profiler.read-fn/test]
// In-process round-trip through the whole Profiler API: intern (add_string,
// 1-based ids + dedup), add_grammar (fname-then-grammar order), add_rule /
// add_context (first-write-wins), Key's (type, id) ordering, write (PRAGMAs +
// DDL + transaction all go through the execute_batch path that replaced the
// C++ sqlite3_exec wrapper; the
// grammar_ast string is stored under key 0; subsumed contexts pruned), and
// read (merge into existing maps, key-0 quirk NOT undone).
#[cfg(feature = "profiler")]
#[test]
fn profiler_sqlite_roundtrip() {
    let mut p = Profiler::default();

    // add_string: ids are 1-based, assigned in insertion order, deduplicated.
    let ast_id = p.add_string("# the grammar AST dump");
    assert_eq!(ast_id, 1);
    assert_eq!(p.add_string("# the grammar AST dump"), 1);
    p.grammar_ast = ast_id;

    // add_grammar: interns fname first, then the grammar text (f < g).
    let g = p.add_grammar("grammar.cg3", "LIST V = V;\n");
    assert_eq!(g, 3);
    assert_eq!(p.grammars.get(&2), Some(&3));

    // add_rule / add_context: first write wins (emplace / count==0 guard).
    p.add_rule(10, g, 5, 25);
    p.add_rule(10, 999, 1, 2);
    assert_eq!(
        p.entries[&Key {
            r#type: ET_RULE,
            id: 10
        }]
            .grammar,
        g
    );
    p.add_context(4, g, 7, 19);
    p.add_context(4, 999, 1, 2);
    assert_eq!(
        p.entries[&Key {
            r#type: ET_CONTEXT,
            id: 4
        }]
            .b,
        7
    );

    // Key::operator<: ordered by type first, then id — every rule (type 0)
    // sorts before every context (type 1) regardless of id.
    assert!(
        Key {
            r#type: ET_RULE,
            id: 10
        } < Key {
            r#type: ET_CONTEXT,
            id: 4
        }
    );
    assert!(
        Key {
            r#type: ET_RULE,
            id: 2
        } < Key {
            r#type: ET_RULE,
            id: 10
        }
    );
    let keys: Vec<Key> = p.entries.keys().copied().collect();
    assert_eq!(
        keys,
        vec![
            Key {
                r#type: ET_RULE,
                id: 10
            },
            Key {
                r#type: ET_CONTEXT,
                id: 4
            }
        ]
    );

    // Two contexts sharing `e` with distinct `b` — write's fixed-10-iteration
    // prune deletes the max(b) one from the database (id 91), keeping id 90.
    p.add_context(90, g, 1, 9);
    p.add_context(91, g, 3, 9);

    p.rule_contexts.insert((10, 4), 6);

    let db = tmp("profile.sqlite");
    let db_s = db.to_str().unwrap();
    p.write(db_s).expect("Profiler::write failed");
    let bytes = std::fs::read(&db).unwrap();
    assert!(
        bytes.starts_with(b"SQLite format 3\0"),
        "not a SQLite database"
    );

    // read merges into whatever the Profiler already holds (no clearing).
    let mut q = Profiler::default();
    q.strings.insert("pre-existing".into(), 77);
    q.read(db_s).expect("Profiler::read failed");
    assert_eq!(q.strings.get("pre-existing"), Some(&77));

    // QUIRK: the grammar_ast-carrying string was written under key 0 and read
    // back verbatim — id 0 in memory, not its original id 1.
    assert_eq!(q.strings.get("# the grammar AST dump"), Some(&0));
    assert_eq!(q.strings.get("grammar.cg3"), Some(&2));
    assert_eq!(q.strings.get("LIST V = V;\n"), Some(&3));
    assert_eq!(q.grammars, p.grammars);
    assert_eq!(q.rule_contexts, p.rule_contexts);

    // Entries survive the round trip except the pruned subsumed context (91).
    let mut want_entries: BTreeMap<Key, Entry> = p.entries.clone();
    want_entries.remove(&Key {
        r#type: ET_CONTEXT,
        id: 91,
    });
    assert_eq!(q.entries, want_entries);
    assert!(q.entries.contains_key(&Key {
        r#type: ET_CONTEXT,
        id: 90
    }));

    let _ = std::fs::remove_file(&db);
}
