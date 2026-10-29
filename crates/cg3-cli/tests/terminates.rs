//! Implementation loops that the C++ lets run forever on some inputs
//! (`[spec:cg3:req:robustness.terminates]`). The test runner kills a test
//! after 10 s, so each of these fails by timing out if its loop comes back.

use std::io::Write as _;
use std::process::{Command, Stdio};

// An Apertium analysis in which no tag parses as a baseform: an escaped `<`
// lemma becomes the wordform-shaped `"<x>"`, so the C++'s tag-assignment
// rescan never consumes anything.
// [spec:cg3:req:robustness.terminates/test]
#[test]
fn apertium_analysis_without_baseform_terminates() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cg-conv"))
        .arg("--in-apertium")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cg-conv");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"^a/\\<x\\>$\n")
        .unwrap();
    let out = child.wait_with_output().expect("wait cg-conv");
    assert!(out.status.success(), "cg-conv exited with {}", out.status);
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"<a>\""));
}
