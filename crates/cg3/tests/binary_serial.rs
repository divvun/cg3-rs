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

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::{GrammarCore, GrammarNumbered};
use cg3::igrammar_parser::IGrammarParser;
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

// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.binary-grammar-fn/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-compatible-fn/test]
// [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-binary-grammar-fn/test]
// [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn+3/test]
// [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn+1/test]
// In-process round-trip: TextualParser parses T_Templates, the BinaryGrammar
// ctor wraps it (set_compatible is the documented no-op), write_binary_grammar
// serializes to a byte buffer, and a FRESH BinaryGrammar's istream-style
// parse_grammar/read_contextual_test rebuild it; tag/set/rule/context counts
// must survive the trip and the reread grammar is flagged binary.
#[test]
fn inprocess_binary_roundtrip() {
    let dir = test_dir("T_Templates");
    let src = std::fs::read(dir.join("grammar.cg3")).unwrap();
    let mut parser = TextualParser::new(GrammarCore::default(), false);
    parser
        .parse_grammar_utf8(&src)
        .expect("textual parse failed");
    let grammar = parser.grammar.finish().unwrap();

    let num_tags = grammar.num_tags;
    let num_sets = grammar.sets_list_order.len();
    let num_rules = grammar.rule_by_number.capacity();
    let num_contexts = grammar.contexts.len();
    assert!(
        num_contexts > 0,
        "T_Templates must produce contextual tests"
    );

    let mut writer = BinaryGrammar::new(grammar);
    let mut blob: Vec<u8> = Vec::new();
    writer.write_binary_grammar(&mut blob).unwrap();
    assert_eq!(&blob[..4], b"CG3B", "magic bytes");

    let mut reader = BinaryGrammar::new(GrammarNumbered::default());
    reader.set_compatible(true); // C++ setCompatible: empty body, flag discarded
    reader
        .parse_grammar_buffer(&blob)
        .expect("binary reread failed");
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

// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-verbosity-fn/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn/test]
// The legacy pre-10298 reader is OUT OF SCOPE for the port (rev 13898 only):
// readBinaryGrammar_10043 is an ERRORING STUB. Feed the real checked-in
// test/T_ContextTest/grammar.cg3b.10043 (rev 10043 <= BIN_REV_ANCIENT, read
// only) and assert the documented rejection (return 1, nothing parsed).
// set_verbosity(1) stores the verbosity that gates this path's legacy-revision
// warning. This is also what proves readContextualTest_10043 is unreachable —
// the rejection lands before any contextual test could be read, which is why
// that rule is retired rather than carried as a stub.
#[test]
fn legacy_10043_rejected() {
    let legacy = test_dir("T_ContextTest").join("grammar.cg3b.10043");
    let blob = std::fs::read(&legacy).expect("read legacy fixture");
    assert_eq!(&blob[..4], b"CG3B");
    let rev = u32::from_be_bytes([blob[4], blob[5], blob[6], blob[7]]);
    assert_eq!(rev, 10043, "fixture is the legacy revision");

    let mut reader = BinaryGrammar::new(GrammarCore::default());
    reader.set_verbosity(1); // enables the legacy-revision warning branch
    // [spec:cg3:req:errors.parse-result/test]
    let err = reader
        .parse_grammar_buffer(&blob)
        .expect_err("legacy 10043 grammar must be rejected by the stub");
    assert!(
        matches!(
            err,
            cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::LegacyRevision {
                found: 10043
            })
        ),
        "the rejection must name the revision it refused, got {err:?}"
    );
    // Nothing was parsed: the grammar stays empty and is never marked binary.
    assert!(!reader.grammar.is_binary);
    assert_eq!(reader.grammar.num_tags, 0);
    assert_eq!(reader.grammar.contexts.len(), 0);
}

/// Rule provenance is in-memory state, and the `.cg3b` is frozen at revision
/// 13898: neither the section list nor the rule record is length-prefixed, so a
/// field leaking into the stream would not degrade for a C++ reader but
/// desynchronise it. Proven by writing the same grammar twice — once as parsed,
/// once with every rule's provenance stripped — and demanding the same bytes.
// [spec:cg3:req:diagnostics.wire-frozen/test]
#[test]
fn provenance_never_reaches_the_wire() {
    let src = std::fs::read(test_dir("T_Templates").join("grammar.cg3")).expect("read fixture");

    let compile = |strip: bool| -> Vec<u8> {
        let mut parser = TextualParser::new(GrammarCore::default(), false);
        parser
            .parse_grammar_named(&src, "grammar.cg3")
            .expect("fixture parses");
        let mut grammar = parser.grammar.finish().expect("reindex");
        if strip {
            for i in 0..grammar.rule_by_number.capacity() {
                if grammar.rule_by_number.try_get(i).is_some() {
                    grammar.rule_by_number.get_mut(i).provenance = None;
                }
            }
        } else {
            assert!(
                (0..grammar.rule_by_number.capacity())
                    .filter_map(|i| grammar.rule_by_number.try_get(i))
                    .any(|r| r.provenance.is_some()),
                "the fixture must actually carry provenance for this to prove anything"
            );
        }
        let mut out = Vec::new();
        let mut writer = BinaryGrammar::new(grammar);
        writer
            .write_binary_grammar(&mut out)
            .expect("write the binary grammar");
        out
    };

    assert_eq!(
        compile(false),
        compile(true),
        "the .cg3b must not depend on rule provenance"
    );
}
