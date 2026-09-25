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

use cg3::error::{Cg3Error, RunError};
use cg3::format_converter::FormatConverter;
use cg3::grammar::{Grammar, GrammarCore};
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

/// Runs the Matxin reader in-process over `input` with a grammar that does
/// nothing, the input called `in.txt`.
fn matxin(input: &[u8]) -> Result<String, Cg3Error> {
    let mut grammar = GrammarCore::default();
    grammar.allocate_dummy_set();
    let delim = grammar.allocate_set();
    grammar.delimiters = Some(delim);
    let dummy = grammar.allocate_tag("__CG3_DUMMY_STRINGBIT__").unwrap();
    grammar.add_tag_to_set(dummy, delim);
    let _ = grammar.reindex(false, false).unwrap();
    let mut base = GrammarApplicator::new(grammar.into());
    base.set_grammar().unwrap();
    base.cfg.input_name = "in.txt".to_string();
    let mut app = cg3::matxin_applicator::MatxinApplicator::new(base);
    let mut out = Vec::new();
    app.run_grammar_on_text(&mut Cursor::new(input.to_vec()), &mut out)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The invalid UTF-8 a run was refused for: `(input, line, bytes)`.
fn invalid_utf8(result: Result<String, Cg3Error>) -> (String, u32, Vec<u8>) {
    match result {
        Err(Cg3Error::Run(RunError::InvalidUtf8 {
            input,
            line,
            source,
        })) => (input, line, source.bytes),
        other => panic!("expected an invalid UTF-8 run error, got {other:?}"),
    }
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

// A truncated sequence on the second line of the input, in every text format,
// is a run error naming the input and line 2 — not a panic, and not U+FFFD.
// [spec:cg3:req:robustness.stream-invalid-utf8/test]
#[test]
fn invalid_utf8_refused_in_every_text_format() {
    let cases: [(StreamFormatKind, &[u8]); 6] = [
        (StreamFormatKind::Cg, b"ok\ncaf\xE9\n"),
        (StreamFormatKind::Niceline, b"ok\ncaf\xE9\n"),
        (StreamFormatKind::Plain, b"ok\ncaf\xE9\n"),
        (StreamFormatKind::Fst, b"ok\tok+N\ncaf\xE9\tx+N\n"),
        (StreamFormatKind::Apertium, b"^ok/ok<n>$\n^caf\xE9/x<n>$\n"),
        (
            StreamFormatKind::Jsonl,
            b"{\"w\":\"ok\"}\n{\"w\":\"caf\xE9\"}\n",
        ),
    ];
    for (fmt, input) in cases {
        let (name, line, bytes) = invalid_utf8(convert(fmt, input));
        assert_eq!(
            (name.as_str(), line, bytes.as_slice()),
            ("in.txt", 2, &b"\xE9"[..]),
            "{fmt:?}"
        );
    }

    // Each kind of bad sequence, on the first line.
    for bad in [
        &b"\x80"[..],
        b"\xC0\xAF",
        b"\xED\xA0\x80",
        b"\xF4\x90\x80\x80",
    ] {
        let (_, line, bytes) = invalid_utf8(convert(StreamFormatKind::Cg, bad));
        assert_eq!((line, bytes.as_slice()), (1, bad));
    }

    let e = convert(StreamFormatKind::Cg, b"caf\xE9\n").unwrap_err();
    assert_eq!(e.to_string(), "in.txt: invalid UTF-8 byte 0xE9 on line 1");
}

// Matxin reads a character at a time: bad bytes are a run error with their
// line, and input that ends inside a cohort ends the cohort instead of being
// read forever.
// [spec:cg3:req:robustness.stream-invalid-utf8/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-fn+1/test]
#[test]
fn matxin_reader_errors_and_ends_inside_cohorts() {
    let (name, line, bytes) = invalid_utf8(matxin(b"^a/a<n>$\n^caf\xE9/x<n>$\n"));
    assert_eq!(
        (name.as_str(), line, bytes.as_slice()),
        ("in.txt", 2, &b"\xE9"[..])
    );

    for truncated in [&b"^abc"[..], b"^abc/x<n", b"^abc<st", b"^a\\"] {
        let out = matxin(truncated).expect("a truncated cohort still ends");
        assert!(out.contains("</corpus>"), "{out}");
    }
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

// U+FFFF in the text is a character like any other: the input goes on past it.
// [spec:cg3:req:robustness.stream-text/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.run-grammar-on-text-fn+1/test]
#[test]
fn u_ffff_is_text_not_end_of_stream() {
    let cg = "\"<a\u{FFFF}b>\"\n\t\"x\" N\n\"<c>\"\n\t\"c\" N\n";
    let out = convert(StreamFormatKind::Cg, cg.as_bytes()).unwrap();
    assert!(out.contains("\"<a\u{FFFF}b>\""), "{out}");
    assert!(out.contains("\"<c>\""), "input after U+FFFF lost:\n{out}");

    let apertium = "^a\u{FFFF}/x<n>$ ^c/c<n>$\n";
    let out = convert(StreamFormatKind::Apertium, apertium.as_bytes()).unwrap();
    assert!(out.contains("\"<a\u{FFFF}>\""), "{out}");
    assert!(out.contains("\"<c>\""), "input after U+FFFF lost:\n{out}");

    let out = matxin("^a\u{FFFF}/x<n>$ ^c/c<n>$\n".as_bytes()).unwrap();
    assert!(out.contains("form=\"a\u{FFFF}\""), "{out}");
    assert!(
        out.contains("form=\"c\""),
        "input after U+FFFF lost:\n{out}"
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

// A line is read whole whatever its length: a long FST line does not overrun
// the reader's buffer, and a long line that is mostly whitespace is one line,
// not two.
// [spec:cg3:req:robustness.stream-text/test]
// [spec:cg3:sem:uextras.cg3.get-line-clean-fn+1/test]
#[test]
fn long_lines_are_read_whole() {
    let long = "a".repeat(1023);
    let out = convert(StreamFormatKind::Fst, format!("{long}\n").as_bytes()).unwrap();
    assert!(out.contains(&long), "long FST line lost");

    let spaced = format!("blah{}\tblah+N+Sg\n", " ".repeat(3000));
    let out = convert(StreamFormatKind::Fst, spaced.as_bytes()).unwrap();
    assert!(out.contains("\"<blah>\"\n\t\"blah\" N Sg"), "{out}");

    let cg = format!("\"<w>\"\n\t\"w\" N{}Sg\n", " ".repeat(3000));
    let out = convert(StreamFormatKind::Cg, cg.as_bytes()).unwrap();
    assert!(out.contains("\t\"w\" N Sg\n"), "the line was split:\n{out}");
}

// The last line of the input, with no newline after it, is read as itself: no
// character of the longer line before it is left over at its end.
// [spec:cg3:req:robustness.stream-text/test]
// [spec:cg3:sem:uextras.u-fgets-fn+1/test]
#[test]
fn last_line_without_newline_reads_only_itself() {
    let out = convert(StreamFormatKind::Cg, b"\"<abcdefgh>\"\n\t\"x\" N").unwrap();
    assert!(out.contains("\t\"x\" N\n"), "{out}");
    assert!(!out.contains("Ne"), "stale character read:\n{out}");

    let out = convert(StreamFormatKind::Fst, b"abcdefgh\tabcdefgh+N\nx\tx+V").unwrap();
    assert!(out.contains("\"x\" V\n"), "stale character read:\n{out}");
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

// No empty tag is interned: an empty `<>` in Apertium, which the C++ interned,
// is a run error naming the input and the line; so is empty text handed to
// the interner directly.
// [spec:cg3:req:robustness.empty-tag/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-fn+1/test]
#[test]
fn empty_stream_tags_are_errors_not_interned() {
    for input in ["^a/b<>$\n", "^a<>/b$\n", "^x/x<n>$\n^a/b<n><>$\n"] {
        let line = input.lines().count() as u32;
        match convert(StreamFormatKind::Apertium, input.as_bytes()) {
            Err(Cg3Error::Run(RunError::EmptyTag { file, line: at })) => {
                assert_eq!((file.as_str(), at), ("in.txt", line), "{input}");
            }
            other => panic!("{input}: expected an empty-tag error, got {other:?}"),
        }
    }

    let mut app = GrammarApplicator::new(Grammar::default());
    let e = app
        .add_tag("", cg3::tag::TagType::empty())
        .expect_err("empty text interns no tag");
    assert!(matches!(e, RunError::EmptyTag { .. }), "{e:?}");
    assert_eq!(e.to_string(), "<stdin>: empty tag on line 1");
}

// A variable command with an empty name sets or removes nothing, in each
// format that carries one; an empty JSONL value beside `=` is `*`, as the CG
// reader reads the same command.
// [spec:cg3:req:robustness.empty-tag/test]
// [spec:cg3:sem:grammar-applicator-run-grammar.cg3.grammar-applicator.run-grammar-on-text-fn+1/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.parse-stream-var-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+1/test]
#[test]
fn empty_variable_names_are_skipped() {
    let cohort = "\"<a>\"\n\t\"a\" N\n";
    for cmd in [
        "<STREAMCMD:SETVAR:>",
        "<STREAMCMD:SETVAR:",
        "<STREAMCMD:SETVAR:x,>",
        "<STREAMCMD:REMVAR:>",
    ] {
        let out = convert(StreamFormatKind::Cg, format!("{cmd}\n{cohort}").as_bytes()).unwrap();
        assert!(out.contains("\"<a>\""), "{cmd}:\n{out}");
        assert!(!out.contains("SETVAR:>"), "{cmd}:\n{out}");
    }
    let out = convert(
        StreamFormatKind::Cg,
        format!("<STREAMCMD:SETVAR:x,>\n{cohort}").as_bytes(),
    )
    .unwrap();
    assert!(out.contains("<STREAMCMD:SETVAR:x>"), "{out}");

    let out = convert(
        StreamFormatKind::Apertium,
        b"[<STREAMCMD:SETVAR:>]^a/b<n>$\n",
    )
    .unwrap();
    assert!(out.contains("\"<a>\""), "{out}");

    // The key defaults to the grammar's `*`, which `cg-conv`'s grammar spells
    // differently; what matters is that it is not empty.
    for (cmd, want) in [
        ("<STREAMCMD:SETVAR:>", None),
        ("<STREAMCMD:REMVAR:>", None),
        ("<STREAMCMD:SETVAR:x=>", Some("<STREAMCMD:SETVAR:x>")),
        ("<STREAMCMD:SETVAR:=v>", Some("=v>")),
    ] {
        let input = format!("{{\"cmd\":\"{cmd}\"}}\n{{\"w\":\"a\"}}\n");
        let out = convert(StreamFormatKind::Jsonl, input.as_bytes()).unwrap();
        assert!(out.contains("\"<a>\""), "{cmd}:\n{out}");
        assert!(
            !out.contains("VAR:>") && !out.contains("VAR:="),
            "{cmd}:\n{out}"
        );
        match want {
            Some(want) => assert!(out.contains(want), "{cmd}:\n{out}"),
            None => assert!(!out.contains("VAR:"), "{cmd}:\n{out}"),
        }
    }
}
