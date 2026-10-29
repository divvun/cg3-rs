//! Wave-3 engine-phase `/test` facets for the CORE DATA MODEL symbols:
//! Cohort, CohortIterator (+ subclasses), Reading, Set, Rule, Tag,
//! ContextualTest, SingleWindow, Window.
//!
//! Strategy: DIRECT library API tests against the arena/store model (a
//! `RuntimeStore` owns the dynamic cohort/reading/single-window arenas; a
//! `Grammar` owns the static tag/set/rule/context arenas; the C++ `Window` is
//! dissolved Stage-B into the engine-layer `WindowStream`/`CohortRegistry`/
//! `DepBookkeeping` views threaded explicitly). One id
//! (`ContextualTest::markUsed`, ported as the PRIVATE `Grammar::context_mark_used`)
//! is driven through a fixture run of the `vislcg3` binary instead, because the
//! Rust port exposes no public entry point short of a full grammar load.

// Test builders set a few fields on a `T::default()` incrementally, which reads
// clearer here than a struct literal with a long `..Default::default()` tail.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `diff -B`: compare ignoring blank-line differences (same as golden.rs).
fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

fn repo_test_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
}

// ContextualTest::markUsed is ported as the PRIVATE `Grammar::context_mark_used`
// (grammar.rs), reachable only through `Grammar::reindex` during a real grammar
// load. Driven by the T_ContextTest fixture: its grammar is full of contextual
// tests, so loading it in vislcg3 runs reindex -> context_mark_used over every
// rule's tests (marking their target/barrier sets used); the run then applies
// those rules, so a correct expected-output diff depends on the marking.
// [spec:cg3:sem:contextual-test.cg3.contextual-test.mark-used-fn/test]
#[test]
fn contextual_test_mark_used_via_fixture() {
    let dir = repo_test_dir("T_ContextTest");
    let out = std::env::temp_dir().join(format!("cg3-model-core-ctx-{}.txt", std::process::id()));
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(&dir)
        .arg("-g")
        .arg("grammar.cg3")
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out)
        .status()
        .expect("spawn vislcg3");
    assert!(status.success(), "vislcg3 exited with {status}");
    let got = std::fs::read_to_string(&out).unwrap();
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(diff_b_equal(&want, &got), "T_ContextTest output differs");
}
