//! Input that nests deep (`[spec:cg3:req:robustness.depth-bounded]`).
//!
//! Each input here used to overflow the stack, and a test binary runs on a
//! thread with a limited stack, so each of these tests used to abort. Depth
//! that is inherent in data — a sub-reading chain, a dependency chain, a set
//! built from sets, a trie, a `LINK` chain read from a `.cg3b` — is now walked
//! without recursion and must come out as it did. Depth that an author writes
//! is bounded by `MAX_NESTING`: refused while the grammar is read, or a run
//! error naming the rule while it runs, with the neighbouring input at the
//! limit still working.

use std::io::Write as _;
use std::process::{Command, Stdio};

/// `cg-conv` over `input` with `args`.
fn conv(args: &[&str], input: String) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cg-conv"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cg-conv");
    let mut stdin = child.stdin.take().expect("stdin");
    let feed = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().expect("wait cg-conv");
    feed.join().expect("feed");
    out
}

// An Apertium reading with ten thousand sub-readings, printed in every
// format that prints them whole.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-reading-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-reading-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn/test]
#[test]
fn deep_subreading_chain_prints_in_every_format() {
    let k = 10_000;
    let input = format!("^a/{}x<n>$\n", "x<n>+".repeat(k));
    for (format, expect) in [
        ("--out-apertium", "x<n>+".repeat(k)),
        ("--out-fst", "x+n#".repeat(k)),
        ("--out-jsonl", "{\"l\":\"x\",\"s\":".repeat(k)),
        ("--out-binary", String::new()),
    ] {
        let out = conv(&["--in-apertium", format], input.clone());
        assert!(out.status.success(), "{format} exited with {}", out.status);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains(&expect), "{format} printed the whole chain");
    }
}

// The CG printer indents each sub-reading a tab further, so its output grows
// with the square of the chain; a shorter chain does.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-reading-fn/test]
#[test]
fn deep_subreading_chain_prints_as_cg() {
    let k = 3000;
    let input = format!("^a/{}x<n>$\n", "x<n>+".repeat(k));
    let out = conv(&["--in-apertium", "--out-cg"], input);
    assert!(out.status.success(), "exited with {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.matches("\"x\" n").count(), k + 1);
}
