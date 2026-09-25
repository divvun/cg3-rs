//! The two binary protocols the engine reads from outside the process — the
//! `CGBF` input stream and the reply of an `EXTERNAL` process — fed malformed
//! input, and the binary stream writer fed windows its fields cannot hold.
//!
//! Each of these used to panic, read past a buffer, allocate what the input
//! declared, or write a corrupt stream. Now each is a run error that says what
//! was wrong and where, and valid input reads exactly as before.

use std::io::Cursor;

use cg3::binary_applicator::{BinaryCount, BinaryField, BinaryStreamFault};
use cg3::error::{Cg3Error, RunError};
use cg3::format_converter::FormatConverter;
use cg3::grammar_applicator::{GrammarApplicator, StreamFormatKind};

// ===========================================================================
// The CGBF input stream
// ===========================================================================

/// Little-endian bytes of a hand-built window body.
#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u16(mut self, v: u16) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn str(self, s: &str) -> Self {
        let mut b = self.u16(s.len() as u16);
        b.0.extend_from_slice(s.as_bytes());
        b
    }
}

/// The fields a test perturbs in an otherwise valid one-cohort window whose
/// tag table is `"<a>"`, `"a"`, `x`, `v`.
#[derive(Clone, Copy)]
struct Window {
    var_key: u16,
    wordform: u16,
    static_tag: u16,
    dep_self: u32,
    dep_parent: u32,
    rel_tag: u16,
    rel_head: u32,
    baseform: u16,
    reading_tag: u16,
}

const VALID: Window = Window {
    var_key: 3,
    wordform: 0,
    static_tag: 2,
    dep_self: 1,
    dep_parent: u32::MAX,
    rel_tag: 2,
    rel_head: 1,
    baseform: 1,
    reading_tag: 2,
};

impl Window {
    fn with(mut self, edit: impl FnOnce(&mut Window)) -> Window {
        edit(&mut self);
        self
    }

    fn body(self) -> Vec<u8> {
        Body::default()
            .u16(0)
            .u16(4)
            .str("\"<a>\"")
            .str("\"a\"")
            .str("x")
            .str("v")
            // One BFV_SETVAR variable, its value tag `x`.
            .u16(1)
            .u8(1)
            .u16(self.var_key)
            .u16(2)
            .str("")
            .str("")
            // One cohort.
            .u16(1)
            .u16(0)
            .u16(self.wordform)
            .u16(1)
            .u16(self.static_tag)
            .u32(self.dep_self)
            .u32(self.dep_parent)
            .u16(1)
            .u16(self.rel_tag)
            .u32(self.rel_head)
            .str("")
            .str("")
            // One reading.
            .u16(1)
            .u16(0)
            .u16(self.baseform)
            .u16(1)
            .u16(self.reading_tag)
            .0
    }
}

/// A CGBF stream: the header, then one window packet whose `u32` length is
/// `len` and whose body is `body`.
fn stream_with_len(len: u32, body: &[u8]) -> Vec<u8> {
    let mut s = b"CGBF".to_vec();
    s.extend_from_slice(&1u32.to_le_bytes());
    s.push(1);
    s.extend_from_slice(&len.to_le_bytes());
    s.extend_from_slice(body);
    s
}

fn stream(body: &[u8]) -> Vec<u8> {
    stream_with_len(body.len() as u32, body)
}

/// `cg-conv --in-binary --out-cg` over `input`, in process.
fn read_binary(input: Vec<u8>) -> Result<String, Cg3Error> {
    let base = GrammarApplicator::new(cg3::grammar::Grammar::default());
    let mut fc = FormatConverter::new(base).expect("conversion grammar");
    let cfg = &mut fc.base_mut().cfg;
    cfg.fmt_input = StreamFormatKind::Binary;
    cfg.fmt_output = StreamFormatKind::Cg;
    cfg.is_conv = true;
    cfg.trace = true;
    cfg.verbosity_level = 0;
    let mut out = Vec::new();
    fc.run_grammar_on_text(&mut Cursor::new(input), &mut out)?;
    Ok(String::from_utf8(out).expect("CG output is UTF-8"))
}

/// The window, offset and fault of a binary stream window error.
fn window_fault(result: Result<String, Cg3Error>) -> (u32, usize, BinaryStreamFault) {
    match result {
        Err(Cg3Error::Run(RunError::BinaryStreamWindow {
            window,
            offset,
            fault,
        })) => (window, offset, fault),
        other => panic!("expected a binary stream window error, got {other:?}"),
    }
}

/// The hand-built window is valid, so the failures below are the fields they
/// perturb and not the builder.
#[test]
fn binary_valid_window_still_reads() {
    let out = read_binary(stream(&VALID.body())).expect("a valid window reads");
    assert!(out.contains("\"<a>\""), "{out}");
    assert!(out.contains("\"a\" x"), "{out}");
}

/// A packet cut short anywhere — inside its length, inside its body, or with a
/// string running past its body — is a run error naming the window, where it
/// used to index past the buffer.
// [spec:cg3:req:robustness.binary-stream-validated/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-window-fn+1/test]
#[test]
fn binary_window_cut_short_is_refused() {
    let mut lone_type_byte = stream(&[]);
    lone_type_byte.truncate(9);
    let fault = window_fault(read_binary(lone_type_byte));
    let length = BinaryField::Length;
    assert_eq!(
        fault,
        (1, 0, BinaryStreamFault::Truncated { field: length })
    );

    let fault = window_fault(read_binary(stream(&[])));
    let flags = BinaryField::WindowFlags;
    assert_eq!(fault, (1, 0, BinaryStreamFault::Truncated { field: flags }));

    // A tag whose length says 200 bytes, in a body that holds 3 of them.
    let mut body = Body::default().u16(0).u16(1).u16(200).0;
    body.extend_from_slice(b"abc");
    let fault = window_fault(read_binary(stream(&body)));
    let table = BinaryField::TagTable;
    assert_eq!(fault, (1, 6, BinaryStreamFault::Truncated { field: table }));

    // Every proper prefix of a valid body, declared at its own length.
    let valid = VALID.body();
    for n in 0..valid.len() {
        let (window, _, fault) = window_fault(read_binary(stream(&valid[..n])));
        assert_eq!(window, 1, "prefix {n}");
        assert!(
            matches!(fault, BinaryStreamFault::Truncated { .. }),
            "prefix {n}: {fault:?}"
        );
    }
}

/// A body declared far longer than the stream is a truncated packet, found by
/// reading what the stream holds rather than by allocating what it declares.
// [spec:cg3:req:robustness.allocation-bounded/test]
#[test]
fn binary_declared_length_is_not_trusted() {
    let fault = window_fault(read_binary(stream_with_len(0xFFFF_FFF0, &VALID.body())));
    let body = BinaryField::Body;
    assert_eq!(fault, (1, 0, BinaryStreamFault::Truncated { field: body }));
}

/// Every tag reference is checked against the window's tag table.
// [spec:cg3:req:robustness.binary-stream-validated/test]
#[test]
fn binary_tag_index_past_table_is_refused() {
    let cases = [
        (BinaryField::Variable, VALID.with(|w| w.var_key = 9)),
        (BinaryField::Wordform, VALID.with(|w| w.wordform = 9)),
        (BinaryField::StaticTag, VALID.with(|w| w.static_tag = 9)),
        (BinaryField::Relation, VALID.with(|w| w.rel_tag = 9)),
        (BinaryField::Baseform, VALID.with(|w| w.baseform = 9)),
        (BinaryField::ReadingTag, VALID.with(|w| w.reading_tag = 9)),
    ];
    for (field, window) in cases {
        let (number, _, fault) = window_fault(read_binary(stream(&window.body())));
        assert_eq!(number, 1);
        let want = BinaryStreamFault::TagIndex {
            field,
            index: 9,
            count: 4,
        };
        assert_eq!(fault, want);
    }
}

/// The dependency and relation numbers the flat hash containers reserve are
/// refused where the stream is read, before a container sees them; a parent
/// of `DEP_NO_PARENT` keeps its meaning.
// [spec:cg3:req:robustness.reserved-keys/test]
#[test]
fn binary_reserved_link_numbers_are_refused() {
    let (dep, rel, max) = (BinaryField::Dependency, BinaryField::Relation, u32::MAX);
    let cases = [
        (dep, max, VALID.with(|w| w.dep_self = max)),
        (dep, max - 1, VALID.with(|w| w.dep_self = max - 1)),
        (dep, max - 1, VALID.with(|w| w.dep_parent = max - 1)),
        (rel, max, VALID.with(|w| w.rel_head = max)),
        (rel, max - 1, VALID.with(|w| w.rel_head = max - 1)),
    ];
    for (field, value, window) in cases {
        let (_, _, fault) = window_fault(read_binary(stream(&window.body())));
        assert_eq!(fault, BinaryStreamFault::ReservedNumber { field, value });
    }
}

// ===========================================================================
// The binary stream writer
// ===========================================================================

/// `cg-conv --in-plain --out-binary` over `text`, in process: the run's result
/// and everything it wrote.
fn write_binary(text: String) -> (Result<(), Cg3Error>, Vec<u8>) {
    let base = GrammarApplicator::new(cg3::grammar::Grammar::default());
    let mut fc = FormatConverter::new(base).expect("conversion grammar");
    let cfg = &mut fc.base_mut().cfg;
    cfg.fmt_input = StreamFormatKind::Plain;
    cfg.fmt_output = StreamFormatKind::Binary;
    cfg.is_conv = true;
    cfg.trace = true;
    cfg.verbosity_level = 0;
    let mut out = Vec::new();
    let result = fc.run_grammar_on_text(&mut Cursor::new(text.into_bytes()), &mut out);
    (result, out)
}

/// What a refused window overflowed, and by how much.
fn overflow(result: Result<(), Cg3Error>) -> (BinaryCount, usize, usize) {
    match result {
        Err(Cg3Error::Run(RunError::BinaryStreamOverflow {
            window: 1,
            what,
            count,
            max,
        })) => (what, count, max),
        other => panic!("expected a binary stream overflow, got {other:?}"),
    }
}

/// Plain text never splits a window, so a long enough line is one window with
/// more cohorts, or more distinct tags, than the format's 16-bit counts hold.
/// The writer refuses it and writes nothing, where it used to wrap the count.
// [spec:cg3:req:robustness.checked-arithmetic/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-single-window-fn+1/test]
#[test]
fn binary_writer_refuses_oversized_counts() {
    let (result, out) = write_binary(vec!["w"; 66_000].join(" "));
    assert_eq!(overflow(result), (BinaryCount::Cohorts, 66_000, 65_535));
    assert!(out.is_empty(), "a refused window writes nothing");

    let words: Vec<String> = (0..66_000).map(|i| format!("w{i}")).collect();
    let (result, out) = write_binary(words.join(" "));
    assert_eq!(overflow(result), (BinaryCount::Tags, 65_537, 65_535));
    assert!(out.is_empty(), "a refused window writes nothing");
}

/// A string longer than its 16-bit length prefix can say is refused rather
/// than truncated.
// [spec:cg3:req:robustness.checked-arithmetic/test]
#[test]
fn binary_writer_refuses_overlong_string() {
    let (result, out) = write_binary("a".repeat(70_000));
    let (what, count, max) = overflow(result);
    assert_eq!((what, max), (BinaryCount::StringBytes, 65_535));
    assert!(count > 70_000, "the wordform with its quotes: {count}");
    assert!(out.is_empty(), "a refused window writes nothing");
}

// ===========================================================================
// EXTERNAL replies
// ===========================================================================

#[cfg(unix)]
mod external {
    use std::io::Cursor;
    use std::os::unix::fs::PermissionsExt;

    use cg3::error::{Cg3Error, RunError};
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::grammar_applicator::external::{ExternalFault, ExternalField};

    /// Host-order bytes of a hand-built reply, the protocol's byte order.
    #[derive(Default)]
    struct Reply(Vec<u8>);

    impl Reply {
        fn u32(mut self, v: u32) -> Self {
            self.0.extend_from_slice(&v.to_ne_bytes());
            self
        }
        fn str(mut self, s: &str) -> Self {
            self.0.extend_from_slice(&(s.len() as u16).to_ne_bytes());
            self.0.extend_from_slice(s.as_bytes());
            self
        }
        /// A reply to window 1 declaring `cohorts` cohorts, then the header of
        /// cohort 1 with `flags`, its wordform unchanged.
        fn cohort(cohorts: u32, flags: u32) -> Self {
            Reply::default()
                .u32(1)
                .u32(1)
                .u32(cohorts)
                .u32(0)
                .u32(1)
                .u32(flags)
        }
        /// A reading packet declaring `len` bytes and holding `flags`.
        fn reading(self, len: u32, flags: u32) -> Self {
            self.u32(len).u32(flags)
        }
    }

    /// Run a one-cohort window through `EXTERNAL ONCE` a child that reads the
    /// protocol handshake, replies with `reply`, and exits.
    fn run_external(name: &str, reply: &Reply) -> Result<(), Cg3Error> {
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("cg3-external-reply-{pid}-{name}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let reply_path = dir.join("reply.bin");
        std::fs::write(&reply_path, &reply.0).expect("write reply");
        let child = dir.join("child.sh");
        let script = format!(
            "#!/bin/sh\ndd bs=1 count=4 of=/dev/null 2>/dev/null\ncat '{}'\n",
            reply_path.display()
        );
        std::fs::write(&child, script).expect("write child");
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o755)).unwrap();

        let src = format!(
            "DELIMITERS = \"<$.>\" ;\nSECTION\nEXTERNAL ONCE {} (*) ;\n",
            child.display()
        );
        let mut parser =
            cg3::textual_parser::TextualParser::new(cg3::grammar::GrammarCore::default(), false);
        parser.parse_grammar_utf8(src.as_bytes()).expect("parses");
        let mut grammar = parser.grammar;
        let _ = grammar.reindex(false, false).unwrap();
        let mut app = GrammarApplicator::new(grammar.into());
        app.set_grammar().unwrap();
        let input = b"\"<a>\"\n\t\"a\" x\n".to_vec();
        let result = app.run_grammar_on_text(&mut Cursor::new(input), &mut Vec::new());
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    fn reply_fault(result: Result<(), Cg3Error>) -> ExternalFault {
        match result {
            Err(Cg3Error::Run(RunError::ExternalReply { window: 1, fault })) => fault,
            other => panic!("expected an EXTERNAL reply error, got {other:?}"),
        }
    }

    /// A well-formed reply leaving the reading unmodified is accepted, so the
    /// failures below are the replies and not the harness.
    #[test]
    fn external_valid_reply_is_accepted() {
        let reply = Reply::cohort(1, 0).str("\"<a>\"").u32(1).reading(4, 0);
        run_external("valid", &reply).expect("a valid reply is accepted");
    }

    /// A reply claiming more cohorts than the window sent used to index past
    /// the window.
    // [spec:cg3:req:robustness.external-validated/test]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-single-window-fn+1/test]
    #[test]
    fn external_reply_with_extra_cohorts_is_refused() {
        let reply = Reply::cohort(2, 0).str("\"<a>\"").u32(0);
        let fault = reply_fault(run_external("cohorts", &reply));
        assert!(
            matches!(fault, ExternalFault::Cohorts { sent: 1, got: 2 }),
            "{fault:?}"
        );
    }

    /// A reply claiming more readings than the cohort sent used to index past
    /// the cohort's readings.
    // [spec:cg3:req:robustness.external-validated/test]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-cohort-fn+1/test]
    #[test]
    fn external_reply_with_extra_readings_is_refused() {
        let reply = Reply::cohort(1, 0)
            .str("\"<a>\"")
            .u32(2)
            .reading(4, 0)
            .reading(4, 0);
        let fault = reply_fault(run_external("readings", &reply));
        assert!(
            matches!(
                fault,
                ExternalFault::Readings {
                    cohort: 1,
                    sent: 1,
                    got: 2,
                }
            ),
            "{fault:?}"
        );
    }

    /// A reading packet declaring 4 GiB, then ending, is a reply that breaks
    /// off — read as it arrives, not allocated at the length it declares.
    // [spec:cg3:req:robustness.external-validated/test]
    // [spec:cg3:req:robustness.allocation-bounded/test]
    #[test]
    fn external_reading_length_is_not_trusted() {
        let reply = Reply::cohort(1, 0)
            .str("\"<a>\"")
            .u32(1)
            .reading(0xFFFF_FFF0, 1);
        let fault = reply_fault(run_external("length", &reply));
        assert!(
            matches!(
                fault,
                ExternalFault::Truncated {
                    field: ExternalField::Reading,
                    ..
                }
            ),
            "{fault:?}"
        );
    }

    /// A modified reading whose fields run past its packet is an overrun,
    /// where the fields past the end used to read as zeros.
    // [spec:cg3:req:robustness.external-validated/test]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.pipe-in-reading-fn+1/test]
    #[test]
    fn external_reading_overrunning_its_packet_is_refused() {
        let reply = Reply::cohort(1, 0).str("\"<a>\"").u32(1).reading(4, 1);
        let fault = reply_fault(run_external("overrun", &reply));
        assert!(
            matches!(
                fault,
                ExternalFault::ReadingOverrun {
                    cohort: 1,
                    len: 4,
                    field: ExternalField::Tags,
                }
            ),
            "{fault:?}"
        );
    }

    /// A process that exits without replying used to leave the window
    /// silently unchanged.
    // [spec:cg3:req:robustness.external-validated/test]
    #[test]
    fn external_reply_cut_short_is_refused() {
        let fault = reply_fault(run_external("empty", &Reply::default()));
        assert!(
            matches!(
                fault,
                ExternalFault::Truncated {
                    field: ExternalField::WindowHeader,
                    ..
                }
            ),
            "{fault:?}"
        );
    }

    /// The parent number the flat hash containers reserve is refused where the
    /// reply is read.
    // [spec:cg3:req:robustness.reserved-keys/test]
    #[test]
    fn external_reply_reserved_parent_is_refused() {
        let reply = Reply::cohort(1, 1 << 1).u32(u32::MAX - 1);
        let fault = reply_fault(run_external("parent", &reply));
        assert!(
            matches!(
                fault,
                ExternalFault::ReservedParent {
                    cohort: 1,
                    parent: 0xFFFF_FFFE,
                }
            ),
            "{fault:?}"
        );
    }
}
