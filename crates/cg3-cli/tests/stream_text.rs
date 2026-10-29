//! The stream readers take any text (`[spec:cg3:req:robustness.stream-text]`,
//! `[spec:cg3:req:robustness.stream-invalid-utf8]`,
//! `[spec:cg3:req:robustness.empty-tag]`).
//!
//! Each test feeds a reader an input it must take without panicking, hanging
//! or misreading: bytes that are not UTF-8, a U+FFFF, a non-ASCII tag, a long
//! line, a last line with no newline, an empty tag. Most run a reader
//! in-process through the converter `cg-conv` builds; the ones that need a
//! real grammar, the format sniff or `cg-mwesplit` run the binaries.

use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cg3::error::Cg3Error;
use cg3::format_converter::FormatConverter;
use cg3::grammar::Grammar;
use cg3::grammar_applicator::{GrammarApplicator, StreamFormatKind};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Converts `input` from `fmt` to CG in-process, configured as `cg-conv`
/// configures it, with the input called `in.txt`.
fn convert(fmt: StreamFormatKind, input: &[u8]) -> Result<String, Cg3Error> {
    let base = GrammarApplicator::new(Grammar::default());
    let mut fc = FormatConverter::new(base).unwrap();
    let cfg = &mut fc.base_mut().cfg;
    cfg.fmt_input = fmt;
    cfg.fmt_output = StreamFormatKind::Cg;
    cfg.is_conv = true;
    cfg.trace = true;
    cfg.verbosity_level = 0;
    cfg.input_name = "in.txt".to_string();
    let mut out = Vec::new();
    fc.run_grammar_on_text(&mut Cursor::new(input.to_vec()), &mut out)?;
    Ok(String::from_utf8(out).expect("the writers write UTF-8"))
}

/// Runs `bin` with `args` over `input` on stdin, returning its exit success,
/// stdout and stderr.
fn run_tool(bin: &str, args: &[&str], input: &[u8]) -> (bool, String, String) {
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(repo_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tool");
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().expect("wait for tool");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

// Format detection only sniffs: bytes that are not UTF-8 in the block it peeks
// at, even ones that end it mid-sequence, are left for the reader it picks,
// which refuses them with the line. `cg-conv` exits with that error, not a
// panic.
// [spec:cg3:req:robustness.stream-invalid-utf8/test]
// [spec:cg3:sem:uextras.read-utf8-fn+1/test]
#[test]
fn cg_conv_sniff_leaves_bad_bytes_to_reader() {
    for input in [&b"caf\xE9"[..], b"abc\xE0\x80\x80\x80", b"\xF0"] {
        let (ok, _, err) = run_tool(env!("CARGO_BIN_EXE_cg-conv"), &[], input);
        assert!(!ok, "a stream that is not UTF-8 must fail: {err}");
        assert!(!err.contains("panicked"), "{err}");
        assert!(
            err.contains("<stdin>: invalid UTF-8") && err.contains("on line 1"),
            "{err}"
        );
    }
}

// The CLI names the file it opened, and the line.
// [spec:cg3:req:robustness.stream-invalid-utf8/test]
#[test]
fn vislcg3_names_file_and_line_of_bad_bytes() {
    let path = std::env::temp_dir().join(format!("cg3-stream-text-{}.txt", std::process::id()));
    std::fs::write(&path, b"\"<a>\"\n\t\"a\" N\n\"<b\x80>\"\n").unwrap();
    let path_arg = path.to_string_lossy().into_owned();
    let (ok, _, err) = run_tool(
        env!("CARGO_BIN_EXE_vislcg3"),
        &["-g", "test/T_Select/grammar.cg3", "-I", &path_arg],
        b"",
    );
    let _ = std::fs::remove_file(&path);
    assert!(!ok, "{err}");
    assert!(!err.contains("panicked"), "{err}");
    assert!(
        err.contains(&format!("{path_arg}: invalid UTF-8 byte 0x80 on line 3")),
        "{err}"
    );
}

// A niceline reading steps through its tags by character, so a non-ASCII tag
// after the first is read whole — the format sniff picks niceline for this.
// [spec:cg3:req:robustness.stream-text/test]
#[test]
fn niceline_reads_non_ascii_tags_after_first() {
    let input = "ord\t\"a\" N @\u{2192}N Ø\n";
    let out = convert(StreamFormatKind::Niceline, input.as_bytes()).unwrap();
    assert!(out.contains("\"a\" N Ø @\u{2192}N\n"), "{out}");

    let (ok, out, err) = run_tool(env!("CARGO_BIN_EXE_cg-conv"), &[], input.as_bytes());
    assert!(ok, "{err}");
    assert!(out.contains("@\u{2192}N"), "{out}");
}

// With no window span, a delimiter can shuffle the only window out before the
// grammar runs; there is then no window to run on, and nothing runs.
#[test]
fn jsonl_num_windows_zero_survives_a_delimiter() {
    let input = b"{\"w\":\"a\"}\n{\"w\":\"$.\"}\n{\"w\":\"b\"}\n";
    let (ok, out, err) = run_tool(
        env!("CARGO_BIN_EXE_vislcg3"),
        &[
            "-g",
            "test/T_Select/grammar.cg3",
            "--in-jsonl",
            "--num-windows",
            "0",
        ],
        input,
    );
    assert!(ok, "{err}");
    for wf in ["\"<a>\"", "\"<$.>\"", "\"<b>\""] {
        assert!(out.contains(wf), "{wf} lost:\n{out}");
    }
}

// A sub-reading's wordform tag that is blank inside leaves no word to split
// out, so the cohort is printed whole rather than trimmed past its end.
// [spec:cg3:req:robustness.stream-text/test]
// [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.split-mwe-fn+1/test]
#[test]
fn mwesplit_leaves_blank_inner_wordform_unsplit() {
    let input = b"\"<a b>\"\n\t\"b\" N \"< >\"\n\t\t\"a\" N \"<a>\"\n";
    let (ok, out, err) = run_tool(env!("CARGO_BIN_EXE_cg-mwesplit"), &[], input);
    assert!(ok, "{err}");
    assert!(out.contains("\"<a b>\""), "cohort split or lost:\n{out}");
    assert!(!err.contains("panicked"), "{err}");
}
