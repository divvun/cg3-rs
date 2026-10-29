//! The stream readers take any text (`[spec:cg3:req:robustness.stream-text]`,
//! `[spec:cg3:req:robustness.stream-invalid-utf8]`,
//! `[spec:cg3:req:robustness.empty-tag]`).
//!
//! Each test feeds a reader an input it must take without panicking, hanging
//! or misreading: bytes that are not UTF-8, a U+FFFF, a non-ASCII tag, a long
//! line, a last line with no newline, an empty tag. Most run a reader
//! in-process through the converter `cg-conv` builds; the ones that need a
//! real grammar, the format sniff or `cg-mwesplit` run the binaries.

use std::io::Cursor;

use cg3::error::{Cg3Error, RunError};
use cg3::format_converter::FormatConverter;
use cg3::grammar::{Grammar, GrammarCore};
use cg3::grammar_applicator::{GrammarApplicator, StreamFormatKind};

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
    let grammar = grammar.finish().unwrap();
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
// [spec:cg3:req:robustness.terminates/test]
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
