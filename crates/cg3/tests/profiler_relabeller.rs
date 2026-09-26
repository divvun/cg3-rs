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
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(feature = "profiler")]
use cg3::profiler::{ET_CONTEXT, ET_RULE, Entry, Key, Profiler};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cg3-profrel-{}-{}", std::process::id(), name))
}

/// `diff -B`: compare ignoring blank-line differences (same as golden.rs).
fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

fn run_ok(mut cmd: Command, what: &str) {
    let status = cmd.status().unwrap_or_else(|e| panic!("spawn {what}: {e}"));
    assert!(status.success(), "{what} exited with {status}");
}

/// The shared run.pl protocol: compile, relabel, and return the path of the
/// relabelled binary grammar. Both intermediate grammars must be non-empty
/// (run.pl's `-s grammar.cg3b && -s grammar-out.cg3b` check).
fn compile_and_relabel(fixture: &Path, stem: &str) -> PathBuf {
    let t1 = tmp(&format!("{stem}-1.cg3b"));
    let t2 = tmp(&format!("{stem}-2.cg3b"));
    let mut comp = Command::new(env!("CARGO_BIN_EXE_cg-comp"));
    comp.current_dir(fixture).arg("grammar.cg3").arg(&t1);
    run_ok(comp, "cg-comp");
    let mut relabel = Command::new(env!("CARGO_BIN_EXE_cg-relabel"));
    relabel
        .current_dir(fixture)
        .arg(&t1)
        .arg("relabel.cg3r")
        .arg(&t2);
    run_ok(relabel, "cg-relabel");
    assert!(
        std::fs::metadata(&t1).unwrap().len() > 0,
        "compiled grammar is empty"
    );
    assert!(
        std::fs::metadata(&t2).unwrap().len() > 0,
        "relabelled grammar is empty"
    );
    let _ = std::fs::remove_file(&t1);
    t2
}

/// Run vislcg3 with the relabelled grammar over the fixture's input.txt and
/// return the produced output text.
fn apply_relabelled(fixture: &Path, grammar: &Path, stem: &str) -> String {
    let out = tmp(&format!("{stem}-out.txt"));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vislcg3"));
    cmd.current_dir(fixture)
        .arg("-g")
        .arg(grammar)
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out);
    run_ok(cmd, "vislcg3");
    let got = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(grammar);
    got
}

// [spec:cg3:sem:relabeller.cg3.relabeller.relabeller-fn+1/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.relabel-fn+1/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.relabel-as-list-fn/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.transfer-tags-fn/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.add-taglists-to-set-fn/test]
// [spec:cg3:sem:relabeller.cg3.freq-sorter.freq-sorter-fn/test]
// [spec:cg3:sem:relabeller.cg3.freq-sorter.operator-fn/test]
// The test/T_RelabelList run.pl protocol. cg-relabel constructs a Relabeller
// (ctor partitions MAP rules into as-list/as-set) and runs relabel(). The
// grammar's multi-tag LISTs (e.g. `LIST DetInd = (Det Ind);` with `MAP (Ind)
// (ind)`) force relabel_as_list's cartesian expansion, transfer_tags
// re-interning, and add_taglists_to_set's freq_sorter (ctor + comparator: the
// expanded lists have >= 2 tags, so the frequency sort actually compares).
#[test]
fn relabel_list_protocol() {
    let fixture = repo_root().join("test/T_RelabelList");
    let g = compile_and_relabel(&fixture, "list");
    let got = apply_relabelled(&fixture, &g, "list");
    let want = std::fs::read_to_string(fixture.join("expected.txt")).unwrap();
    assert!(
        diff_b_equal(&want, &got),
        "T_RelabelList output differs from expected.txt:\n{got}"
    );
}

// [spec:cg3:sem:relabeller.cg3.relabeller.relabel-as-set-fn/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.copy-relabel-set-to-grammar-fn/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.add-set-to-grammar-fn/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.reindex-set-fn/test]
// [spec:cg3:sem:relabeller.cg3.trie-copy-fn/test]
// The test/T_RelabelSet run.pl protocol. `MAP (n) (N) - (Prop)` is a set-typed
// relabel target, so relabel() takes the relabel_as_set path: it copies the
// relabel set into the target grammar (copy_relabel_set_to_grammar, which
// re-interns tries via the two-arg relabeller trie_copy), registers the new
// sets (add_set_to_grammar) and reindexes the reshaped OR set (reindex_set).
// The copy has to read N and Prop out of the relabels grammar, since the same
// ids in the target name other tags.
#[test]
fn relabel_set_protocol() {
    let fixture = repo_root().join("test/T_RelabelSet");
    let g = compile_and_relabel(&fixture, "set");
    let got = apply_relabelled(&fixture, &g, "set");
    let want = std::fs::read_to_string(fixture.join("expected.txt")).unwrap();
    assert!(
        diff_b_equal(&want, &got),
        "T_RelabelSet output differs from expected.txt:\n{got}"
    );
}

// [spec:cg3:sem:relabeller.cg3.relabeller.relabel-fn+1/test]
// Every relabel fixture relabels to the same bytes in every process. Each
// cg-relabel run is its own process, so a walk in hashed order would change
// the relabelled grammar from one run to the next.
#[test]
fn relabelling_is_byte_stable() {
    for name in ["T_RelabelList", "T_RelabelList_Apertium", "T_RelabelSet"] {
        let fixture = repo_root().join("test").join(name);
        let compiled = tmp(&format!("stable-{name}.cg3b"));
        let mut comp = Command::new(env!("CARGO_BIN_EXE_cg-comp"));
        comp.current_dir(&fixture).arg("grammar.cg3").arg(&compiled);
        run_ok(comp, "cg-comp");
        let relabelled: Vec<Vec<u8>> = (0..8)
            .map(|i| {
                let out = tmp(&format!("stable-{name}-{i}.cg3b"));
                let mut relabel = Command::new(env!("CARGO_BIN_EXE_cg-relabel"));
                relabel
                    .current_dir(&fixture)
                    .arg(&compiled)
                    .arg("relabel.cg3r")
                    .arg(&out);
                run_ok(relabel, "cg-relabel");
                let bytes = std::fs::read(&out).unwrap();
                let _ = std::fs::remove_file(&out);
                bytes
            })
            .collect();
        let _ = std::fs::remove_file(&compiled);
        assert!(
            relabelled.iter().all(|b| *b == relabelled[0]),
            "{name}: cg-relabel wrote different bytes across runs"
        );
    }
}

/// The test/T_RelabelList_Apertium run.pl protocol — same compile+relabel
/// pipeline, but the relabelled grammar is applied by cg-proc over Apertium
/// stream input (`cg-proc grammar input.txt output.txt`). Supporting coverage
/// for the facets above (the Relabeller runs identically; the fixture proves
/// the relabelled binary grammar also round-trips through cg-proc).
#[test]
fn relabel_list_apertium_protocol() {
    let fixture = repo_root().join("test/T_RelabelList_Apertium");
    let g = compile_and_relabel(&fixture, "apertium");
    let out = tmp("apertium-out.txt");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cg-proc"));
    cmd.current_dir(&fixture).arg(&g).arg("input.txt").arg(&out);
    run_ok(cmd, "cg-proc");
    let got = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&g);
    let want = std::fs::read_to_string(fixture.join("expected.txt")).unwrap();
    assert!(
        diff_b_equal(&want, &got),
        "T_RelabelList_Apertium output differs from expected.txt:\n{got}"
    );
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

/// End-to-end: `vislcg3 --profile <db>` writes the profiling database (the
/// grammar AST interned as string key 0), and `cg-annotate <db> <dir>` reads it
/// back to emit the HTML report. Supporting coverage for the write/read facets
/// above, through the real binaries.
#[cfg(feature = "profiler")]
#[test]
fn profiler_via_vislcg3_and_cg_annotate() {
    let fixture = repo_root().join("test/T_RelabelList");
    let db = tmp("prof-flag.sqlite");
    let out = tmp("prof-flag-out.txt");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vislcg3"));
    cmd.current_dir(&fixture)
        .arg("--profile")
        .arg(&db)
        .arg("-g")
        .arg("grammar.cg3")
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out);
    run_ok(cmd, "vislcg3 --profile");
    let _ = std::fs::remove_file(&out);

    let bytes = std::fs::read(&db).expect("--profile did not write a database");
    assert!(
        bytes.starts_with(b"SQLite format 3\0"),
        "not a SQLite database"
    );

    // Wave 4 (w4-io-sinks-profiler): the profiler is fully wired. The parse
    // registers the grammar text (add_grammar) and every rule/context span
    // (add_rule/add_context); --profile force-enables AST building, so the AST
    // capture interned as `grammar_ast` is non-empty; and the run gathers
    // per-rule match/fail counters with example windows.
    let mut p = Profiler::default();
    p.read(db.to_str().unwrap()).expect("Profiler::read failed");
    assert!(!p.grammars.is_empty(), "parse registered the grammar text");
    // `write` stores the grammar_ast-carrying string under key 0.
    let ast_text = p
        .strings
        .iter()
        .find(|&(_, &id)| id == 0)
        .map(|(s, _)| s.clone())
        .expect("the grammar AST string (key 0) is present");
    assert!(
        ast_text.contains("<Grammar"),
        "AST capture holds the parse tree"
    );
    assert!(
        p.entries.keys().any(|k| k.r#type == ET_RULE),
        "parse registered rule entries"
    );
    assert!(
        p.entries.values().any(|e| e.num_match + e.num_fail > 0),
        "run gathered per-rule match/fail data"
    );

    // cg-annotate consumes the profile database and writes the report.
    let annot = tmp("annotate-out");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cg-annotate"));
    cmd.current_dir(&fixture).arg(&db).arg(&annot);
    run_ok(cmd, "cg-annotate");
    assert!(annot.join("index.html").is_file());
    assert!(annot.join("style.css").is_file());

    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&annot);
}
