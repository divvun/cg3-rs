//! Binary-grammar (de)serialization + grammar-writer round-trip tests.
//!
//! Covers `src/BinaryGrammar.cpp` / `BinaryGrammar_read.cpp` /
//! `BinaryGrammar_write.cpp` (the `.cg3b` reader/writer, incl. contextual-test
//! records and the legacy-10043 rejection stubs) and `src/GrammarWriter.cpp`
//! (textual grammar re-serialization), following the runall.pl sub-test 2
//! (`--grammar-out` round-trip) and sub-test 3 (`.cg3b` round-trip) protocols.
//!
//! Fixtures: `test/T_Templates` (TEMPLATE + `[...]` alternation → `tmpl`/`ors`
//! context records), `test/T_Dependency` (dep rules → `dep_target`/`dep_tests`
//! context hash lists), `test/T_ContextTest` (NOT/careful/LINK chains →
//! `linked` records; also ships a real pre-10298 `grammar.cg3b.10043`).
//! All outputs go to std::env::temp_dir(); fixture dirs are read-only.

use std::path::{Path, PathBuf};
use std::process::Command;

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::Grammar;
use cg3::textual_parser::TextualParser;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn test_dir(name: &str) -> PathBuf {
    repo_root().join("test").join(name)
}

fn read_args(dir: &Path) -> Vec<String> {
    match std::fs::read_to_string(dir.join("args.txt")) {
        Ok(s) => s.split_whitespace().map(|s| s.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

/// `diff -B`: compare ignoring blank-line differences (as golden.rs/runall.pl).
fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

fn temp_path(stem: &str, ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cg3-binser-{stem}-{}.{ext}", std::process::id()))
}

/// Run vislcg3 in `dir` with the fixture's args + the given grammar, feeding
/// `input.txt`, and assert the output diff-B-matches `expected.txt`.
fn run_and_check(dir: &Path, grammar: &Path, label: &str) {
    let out = temp_path(label, "txt");
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(dir)
        .args(read_args(dir))
        .arg("-g")
        .arg(grammar)
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out)
        .status()
        .expect("spawn vislcg3");
    assert!(status.success(), "{label}: vislcg3 exited with {status}");
    let got = std::fs::read_to_string(&out).expect("read output");
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(
        diff_b_equal(&want, &got),
        "{label}: output differs from expected.txt"
    );
}

// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.parse-grammar-fn/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.write-binary-grammar-fn/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.write-contextual-test-fn/test]
// cg-comp compiles T_Templates (TEMPLATE defs + [..] alternation → tmpl/ors
// context records) via BinaryGrammar::write_binary_grammar/write_contextual_test;
// vislcg3 then loads the .cg3b by filename (the BinaryGrammar.cpp parse_grammar
// file entry point) and the run must still match expected.txt.
#[test]
fn binary_roundtrip_templates() {
    let dir = test_dir("T_Templates");
    let bin = temp_path("templates", "cg3b");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp exited with {status}");
    run_and_check(&dir, &bin, "templates-bin");
    let _ = std::fs::remove_file(&bin);
}

// [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn/test]
// T_Dependency's SETPARENT/SETCHILD rules carry dep_target/dep_tests contexts;
// writing its .cg3b exercises BinaryGrammar_write.cpp's writeContextualTest
// dependency-first recursion + per-rule dep hash lists, and the binary run
// must still match expected.txt.
#[test]
fn binary_roundtrip_dependency() {
    let dir = test_dir("T_Dependency");
    let bin = temp_path("dependency", "cg3b");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp exited with {status}");
    run_and_check(&dir, &bin, "dependency-bin");
    let _ = std::fs::remove_file(&bin);
}

// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-fn/test]
// runall.pl sub-test "bin": vislcg3 --grammar-only --grammar-bin writes the
// .cg3b, then a second vislcg3 reads it back — T_ContextTest's NOT/careful/
// LINK-chained contexts land in readContextualTest's linked/pos/offset fields
// and the re-run must match expected.txt.
#[test]
fn grammar_bin_flag_roundtrip_contexttest() {
    let dir = test_dir("T_ContextTest");
    let bin = temp_path("contexttest", "cg3b");
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(&dir)
        .arg("--grammar-only")
        .arg("-g")
        .arg("grammar.cg3")
        .arg("--grammar-bin")
        .arg(&bin)
        .status()
        .expect("spawn vislcg3 --grammar-bin");
    assert!(
        status.success(),
        "vislcg3 --grammar-bin exited with {status}"
    );
    run_and_check(&dir, &bin, "contexttest-bin");
    let _ = std::fs::remove_file(&bin);
}

// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.binary-grammar-fn/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-compatible-fn/test]
// [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-binary-grammar-fn/test]
// [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn/test]
// [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn/test]
// In-process round-trip: TextualParser parses T_Templates, the BinaryGrammar
// ctor wraps it (set_compatible is the documented no-op), write_binary_grammar
// serializes to a byte buffer, and a FRESH BinaryGrammar's istream-style
// parse_grammar/read_contextual_test rebuild it; tag/set/rule/context counts
// must survive the trip and the reread grammar is flagged binary.
#[test]
fn inprocess_binary_roundtrip() {
    let dir = test_dir("T_Templates");
    let src = std::fs::read(dir.join("grammar.cg3")).unwrap();
    let mut parser = TextualParser::new(Grammar::default(), false);
    assert_eq!(parser.parse_grammar_utf8(&src), 0, "textual parse failed");
    let mut grammar = parser.grammar;
    grammar.reindex(false, false);

    let num_tags = grammar.num_tags;
    let num_sets = grammar.sets_list_order.len();
    let num_rules = grammar.rule_by_number.capacity();
    let num_contexts = grammar.contexts.len();
    assert!(
        num_contexts > 0,
        "T_Templates must produce contextual tests"
    );

    let mut writer = BinaryGrammar::binary_grammar(grammar);
    writer.set_compatible(true); // C++ setCompatible: empty body, flag discarded
    let mut blob: Vec<u8> = Vec::new();
    assert_eq!(writer.write_binary_grammar(&mut blob), 0);
    assert_eq!(&blob[..4], b"CG3B", "magic bytes");

    let mut reader = BinaryGrammar::binary_grammar(Grammar::default());
    assert_eq!(
        reader.parse_grammar_buffer(&blob),
        0,
        "binary reread failed"
    );
    assert!(reader.grammar.is_binary);
    assert_eq!(reader.grammar.num_tags, num_tags, "tag count");
    assert_eq!(reader.grammar.sets_list_order.len(), num_sets, "set count");
    assert_eq!(
        reader.grammar.rule_by_number.capacity(),
        num_rules,
        "rule count"
    );
    assert_eq!(reader.grammar.contexts.len(), num_contexts, "context count");
}

// [spec:cg3:sem:grammar-writer.cg3.grammar-writer.grammar-writer-fn/test]
// [spec:cg3:sem:grammar-writer.cg3.grammar-writer.write-grammar-fn/test]
// [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-set-fn/test]
// [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-rule-fn/test]
// [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-contextual-test-fn/test]
// [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-tag-fn/test]
// runall.pl sub-test "out": vislcg3 --grammar-out re-serializes T_Templates via
// GrammarWriter (ctor + write_grammar walking printSet/printTag/printRule/
// printContextualTest over its LIST/SET defs, ADD/SELECT rules and template
// contexts); the written text must itself parse and reproduce expected.txt.
#[test]
fn grammar_out_roundtrip_templates() {
    let dir = test_dir("T_Templates");
    let out_cg3 = temp_path("templates-out", "cg3");
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(&dir)
        .arg("--grammar-only")
        .arg("-g")
        .arg("grammar.cg3")
        .arg("--grammar-out")
        .arg(&out_cg3)
        .status()
        .expect("spawn vislcg3 --grammar-out");
    assert!(
        status.success(),
        "vislcg3 --grammar-out exited with {status}"
    );
    let text = std::fs::read_to_string(&out_cg3).expect("read written grammar");
    assert!(
        text.contains("DELIMITERS"),
        "written grammar has DELIMITERS"
    );
    assert!(text.contains("SELECT"), "written grammar has rules");
    run_and_check(&dir, &out_cg3, "templates-out");
    let _ = std::fs::remove_file(&out_cg3);
}

// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-verbosity-fn/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-10043-fn/test]
// The legacy pre-10298 reader is OUT OF SCOPE for the port (rev 13898 only):
// readBinaryGrammar_10043 is an ERRORING STUB. Feed the real checked-in
// test/T_ContextTest/grammar.cg3b.10043 (rev 10043 <= BIN_REV_ANCIENT, read
// only) and assert the documented rejection (return 1, nothing parsed).
// set_verbosity(1) stores the verbosity that gates this path's legacy-revision
// warning. readContextualTest_10043 is UNREACHABLE by design — its only
// caller errors out first — so its facet sits here on the test proving that
// rejection happens before any contextual test could be read.
#[test]
fn legacy_10043_rejected() {
    let legacy = test_dir("T_ContextTest").join("grammar.cg3b.10043");
    let blob = std::fs::read(&legacy).expect("read legacy fixture");
    assert_eq!(&blob[..4], b"CG3B");
    let rev = u32::from_be_bytes([blob[4], blob[5], blob[6], blob[7]]);
    assert_eq!(rev, 10043, "fixture is the legacy revision");

    let mut reader = BinaryGrammar::binary_grammar(Grammar::default());
    reader.set_verbosity(1); // enables the legacy-revision warning branch
    assert_eq!(
        reader.parse_grammar_buffer(&blob),
        1,
        "legacy 10043 grammar must be rejected by the stub"
    );
    // Nothing was parsed: the grammar stays empty and is never marked binary.
    assert!(!reader.grammar.is_binary);
    assert_eq!(reader.grammar.num_tags, 0);
    assert_eq!(reader.grammar.contexts.len(), 0);
}
