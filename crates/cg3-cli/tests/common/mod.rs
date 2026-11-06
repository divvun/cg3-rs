//! Shared support for the conformance harnesses (`golden.rs`, `apertium.rs`,
//! `jsonl_corpus.rs`): path resolution, process capture, and blank-insensitive
//! comparison. The golden-only `runall.pl` stream filters live in
//! `filters.rs`, which only `golden.rs` includes, so the other harnesses don't
//! compile them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// `crates/cg3` -> repo root (holds `test/`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// `diff -B`: equal ignoring blank-line differences.
pub fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

/// Run `exe args` in `cwd`, feed `input` on stdin, return `(stdout, success)`.
/// stderr is discarded (as the harness redirects it).
pub fn run_capture(exe: &str, args: &[&str], cwd: &Path, input: &[u8]) -> (Vec<u8>, bool) {
    let out = run_output(exe, args, cwd, input, Stdio::null());
    (out.stdout, out.status.success())
}

/// stdin is written on a thread so a large stdout can drain concurrently (no
/// pipe deadlock).
pub fn run_output(exe: &str, args: &[&str], cwd: &Path, input: &[u8], stderr: Stdio) -> Output {
    use std::io::Write;
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .unwrap_or_else(|e| panic!("spawn {exe}: {e}"));
    let mut stdin = child.stdin.take().unwrap();
    let owned = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&owned);
        // `stdin` drops here -> EOF for the child.
    });
    let out = child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("wait {exe}: {e}"));
    let _ = writer.join();
    out
}
