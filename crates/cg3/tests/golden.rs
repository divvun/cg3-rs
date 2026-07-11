//! Golden-file regression harness — the Rust-native port of `test/runall.pl`.
//!
//! Each `test/T_*` directory holds `grammar.cg3` + `input.txt` + `expected.txt`
//! (+ optional `args.txt` with extra flags). A test passes when the `vislcg3`
//! binary reproduces `expected.txt` byte-identically (modulo blank lines, as
//! runall.pl's `diff -B`). Tests run DIR-LOCAL (cwd = the test dir) because
//! grammars use relative `INCLUDE` paths.
//!
//! Sub-test 1 (normal run) and sub-test 3 (compile to `.cg3b`, run from the
//! binary grammar) of the runall.pl protocol are covered here. Directories with
//! a custom `run.pl` are exercised by their own dedicated harnesses/tests.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    // crates/cg3 -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn read_args(dir: &Path) -> Vec<String> {
    match std::fs::read_to_string(dir.join("args.txt")) {
        Ok(s) => s.split_whitespace().map(|s| s.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

/// `diff -B`: compare ignoring blank-line differences.
fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

fn golden_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(repo_root().join("test"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_dir()
                && p.file_name().unwrap().to_string_lossy().starts_with("T_")
                && !p.join("run.pl").exists()
                && p.join("grammar.cg3").exists()
                && p.join("input.txt").exists()
                && p.join("expected.txt").exists()
        })
        .collect();
    dirs.sort();
    dirs
}

fn run_one(dir: &Path, grammar: &Path) -> Result<(), String> {
    let out = tempfile_path(dir);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vislcg3"));
    cmd.current_dir(dir)
        .args(read_args(dir))
        .arg("-g")
        .arg(grammar)
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out);
    let status = cmd.status().map_err(|e| format!("spawn vislcg3: {e}"))?;
    if !status.success() {
        return Err(format!("vislcg3 exited with {status}"));
    }
    let got = std::fs::read_to_string(&out).map_err(|e| format!("read output: {e}"))?;
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let _ = std::fs::remove_file(&out);
    if diff_b_equal(&want, &got) {
        Ok(())
    } else {
        Err("output differs from expected.txt".to_string())
    }
}

fn tempfile_path(dir: &Path) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cg3-golden-{}-{}.txt",
        dir.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ))
}

/// Sub-test 1: run each grammar textually, diff against expected.
#[test]
fn golden_textual() {
    let mut failed: Vec<String> = Vec::new();
    let dirs = golden_dirs();
    assert!(!dirs.is_empty(), "no golden test dirs found");
    for dir in &dirs {
        if let Err(e) = run_one(dir, Path::new("grammar.cg3")) {
            failed.push(format!(
                "{}: {e}",
                dir.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} golden tests failed:\n{}",
        failed.len(),
        dirs.len(),
        failed.join("\n")
    );
}

/// Sub-test 3: compile each grammar to `.cg3b` with cg-comp, run from the
/// binary grammar, diff against expected.
#[test]
fn golden_binary_grammar() {
    // Known deltas vs the textual run, faithful to the C++ (verified against
    // the C++ binaries): compile-time-only flags are not applied when running
    // from a precompiled grammar.
    const SKIP: &[&str] = &["T_CG2Compat"];
    let mut failed: Vec<String> = Vec::new();
    let dirs = golden_dirs();
    for dir in &dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let bin =
            std::env::temp_dir().join(format!("cg3-golden-{name}-{}.cg3b", std::process::id()));
        let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
            .current_dir(dir)
            .arg("grammar.cg3")
            .arg(&bin)
            .status()
            .expect("spawn cg-comp");
        if !status.success() {
            failed.push(format!("{name}: cg-comp exited with {status}"));
            continue;
        }
        if let Err(e) = run_one(dir, &bin) {
            failed.push(format!("{name}: {e}"));
        }
        let _ = std::fs::remove_file(&bin);
    }
    assert!(
        failed.is_empty(),
        "{} binary-grammar golden tests failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
}
