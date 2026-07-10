//! Stream-format applicator integration tests — Apertium, Matxin, Binary
//! stream (.cg3bsf), FST, JSONL, FormatConverter, Niceline, Plaintext, and
//! MweSplit applicators.
//!
//! Drives are real end-to-end runs of the ported binaries (`cg-proc`,
//! `cg-conv`, `cg-comp`, `cg-mwesplit`) over the `test/` fixture corpus (the
//! same protocol as the fixtures' `run.pl` scripts), plus in-process library
//! calls for the surfaces the current engine does not expose through any
//! binary (FST/Plaintext input arms and Matxin output CG3Quit inside
//! `FormatConverter`, and `ApertiumApplicator::testPR` which is a
//! commented-out debug block in the C++ `cg-proc.cpp`).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// `diff -B`: compare ignoring blank-line differences (same as golden.rs).
fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cg3-applicators-{}-{}", std::process::id(), name))
}

/// Run a binary with the given args/cwd, feeding `stdin_bytes`, capturing stdout.
fn run_with_stdin(bin: &str, args: &[&str], cwd: &Path, stdin_bytes: &[u8]) -> Vec<u8> {
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tool");
    child.stdin.as_mut().unwrap().write_all(stdin_bytes).expect("write stdin");
    let out = child.wait_with_output().expect("wait tool");
    assert!(out.status.success(), "{bin} {args:?} exited with {}", out.status);
    out.stdout
}

/// The run.pl protocol for an Apertium fixture dir: `cg-comp grammar.cg3 tmp.bin`
/// then `cg-proc <flags> tmp.bin input.txt tmp.out`, returning the output text.
fn cg_proc_fixture(dir: &Path, proc_flags: &[&str], name: &str) -> String {
    let bin = tmp(&format!("{name}.bin"));
    let out = tmp(&format!("{name}.out"));
    let st = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-comp");
    assert!(st.success(), "cg-comp failed for {}", dir.display());
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cg-proc"));
    cmd.current_dir(dir).args(proc_flags).arg(&bin).arg("input.txt").arg(&out);
    let st = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-proc");
    assert!(st.success(), "cg-proc failed for {}", dir.display());
    let got = std::fs::read_to_string(&out).expect("read cg-proc output");
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&out);
    got
}

/// Build the minimal "conversion" grammar + applicator base exactly the way
/// `FormatConverter::new` / cg-conv do (dummy set, one delimiter set holding
/// the dummy tag, reindex, set_grammar), for driving wrapper applicators
/// in-process.
fn conv_base() -> cg3::grammar_applicator::GrammarApplicator {
    let mut base =
        cg3::grammar_applicator::GrammarApplicator::new(cg3::grammar::Grammar::default());
    base.grammar.allocate_dummy_set();
    let delim = base.grammar.allocate_set();
    base.grammar.delimiters = Some(delim);
    let dummy_tag = base.grammar.allocate_tag("__CG3_DUMMY_STRINGBIT__");
    base.grammar.add_tag_to_set(dummy_tag, delim);
    base.grammar.reindex(false, false);
    base.set_grammar();
    base
}

// ===========================================================================
// ApertiumApplicator — driven by the real cg-comp + cg-proc pipeline over the
// test/Apertium fixture corpus (the exact run.pl protocol: compile grammar,
// `cg-proc -d grammar.bin input.txt output`, diff -B expected.txt). Parsing
// the ^word/reading<tags>$ stream drives the constructor + runGrammarOnText +
// processReading; printing the disambiguated stream drives printSingleWindow →
// printCohort → printReading; printCohort always calls mergeMappings (the
// base's split_mappings is off under cg-proc). T_SuperBlanks additionally
// covers wordbound-blank `[[...]]` handling and T_SubReadings_Apertium the
// `+`-joined subreading chains (grammar has SUBREADINGS = RTL).
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.apertium-applicator-fn/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.process-reading-fn/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-reading-fn/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-cohort-fn/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-single-window-fn/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.merge-mappings-fn/test]
#[test]
fn apertium_cg_proc_fixtures() {
    let root = repo_root();
    for (fixture, flags) in [
        ("test/Apertium/T_Append", &["-d"][..]),
        ("test/Apertium/T_SuperBlanks", &["-d"][..]),
        ("test/Apertium/T_MultiWords", &["-d"][..]),
        // Custom run.pl: cg-proc without -d (the default cmd path).
        ("test/T_SubReadings_Apertium", &[][..]),
    ] {
        let dir = root.join(fixture);
        let name = format!("ap-{}", fixture.rsplit('/').next().unwrap());
        let got = cg_proc_fixture(&dir, flags, &name);
        let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
        assert!(
            diff_b_equal(&want, &got),
            "{fixture}: cg-proc output differs from expected.txt:\n{got}"
        );
    }
}

// ApertiumApplicator::parseStreamVar — driven by a real cg-proc run over an
// Apertium stream whose superblank carries a `[<STREAMCMD:SETVAR:...>]`
// command (the only input shape that reaches parseStreamVar: a blank longer
// than 14 chars shaped `[<...>]`). Both the bare-identifier and the
// `key=value` list forms are fed; the blanks are preserved verbatim in the
// output and the cohorts still disambiguate, proving the command was consumed
// by parseStreamVar rather than the normal blank path breaking on it.
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.parse-stream-var-fn/test]
#[test]
fn apertium_stream_setvar() {
    let root = repo_root();
    let dir = root.join("test/T_SubReadings_Apertium");
    // Compile the fixture grammar fresh.
    let bin = tmp("setvar.bin");
    let st = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-comp");
    assert!(st.success());
    let input = tmp("setvar.in");
    let out = tmp("setvar.out");
    std::fs::write(
        &input,
        "[<STREAMCMD:SETVAR:myvar>]^word/word<n><sg>$ [<STREAMCMD:SETVAR:a=1,b>]^word/word<n><sg>$\n",
    )
    .unwrap();
    let st = Command::new(env!("CARGO_BIN_EXE_cg-proc"))
        .current_dir(&dir)
        .arg("-d")
        .arg(&bin)
        .arg(&input)
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-proc");
    assert!(st.success());
    let got = std::fs::read_to_string(&out).unwrap();
    for f in [&bin, &input, &out] {
        let _ = std::fs::remove_file(f);
    }
    assert!(got.contains("[<STREAMCMD:SETVAR:myvar>]"), "SETVAR blank lost: {got}");
    assert!(got.contains("[<STREAMCMD:SETVAR:a=1,b>]"), "SETVAR list blank lost: {got}");
    assert!(got.contains("^word/word<n><sg>"), "cohort lost: {got}");
}

// ApertiumApplicator::testPR — the C++ call site is a commented-out debug
// block in cg-proc.cpp ("Add a / in front to enable this test"), so it is
// unreachable from every binary; the honest drive is the in-process library
// call. The C++ block runs AFTER cg-proc loads a real grammar (testPR reads
// `grammar->single_tags[grammar->tag_any]`, and `tag_any` is only initialized
// by a grammar parse/load — the conv grammar never reaches that state), so
// the test parses a minimal grammar first, exactly like the real call site.
// It round-trips six hard-coded analysis strings through processReading +
// printReading.
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.test-pr-fn/test]
#[test]
fn apertium_test_pr_roundtrip() {
    let mut p = cg3::textual_parser::TextualParser::new(cg3::grammar::Grammar::default(), false);
    let rv = p.parse_grammar_utf8(b"DELIMITERS = \".\" ;\nSELECT (foo) ;\n");
    assert_eq!(rv, 0, "minimal grammar failed to parse");
    let mut g = p.grammar;
    g.reindex(false, false);
    let mut base =
        cg3::grammar_applicator::GrammarApplicator::new(cg3::grammar::Grammar::default());
    base.grammar = g;
    base.set_grammar();
    let mut a = cg3::apertium_applicator::ApertiumApplicator::new(base);
    let mut out: Vec<u8> = Vec::new();
    a.test_pr(&mut out);
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 6, "testPR must emit its six fixture readings:\n{text}");
    // processReading prepends the wform tag (here `*`, the tag_any tag testPR
    // passes) to every reading, and printReading skips only BASEFORM/WORDFORM
    // typed tags — so the faithful output carries an extra `<*>` after the
    // baseform, exactly as the C++ debug block would emit.
    assert_eq!(
        lines[0], "venir<*><vblex><imp><p2><sg>",
        "testPR line 1 wrong: {}",
        lines[0]
    );
    // The subreading chain fixture keeps its + joins.
    assert!(text.contains("+"), "testPR lost subreading joins:\n{text}");
    // The wordbound-space fixture keeps the escaped `# ` part.
    assert!(text.contains("be"), "testPR lost baseform text:\n{text}");
}

// ===========================================================================
// MatxinApplicator — driven end-to-end by `cg-proc -f 2` (the only wired
// entry): cg-proc constructs the applicator, calls setNullFlush(true) and
// runGrammarOnText; runGrammarOnText consults getNullFlush and routes through
// runGrammarOnTextWrapperNullFlush, which loops the real driver until EOF.
// The driver parses the ^word/lemma<tags>$ stream (processReading) and prints
// the <SENTENCE>/<NODE> XML via printSingleWindow → mergeMappings + procNode →
// printReading. Two runs over the same input assert determinism.
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.matxin-applicator-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.get-null-flush-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.set-null-flush-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-wrapper-null-flush-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.process-reading-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.print-reading-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.print-single-window-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.proc-node-fn/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.merge-mappings-fn/test]
#[test]
fn matxin_cg_proc_stream() {
    let root = repo_root();
    let dir = root.join("test/T_SubReadings_Apertium");
    let bin = tmp("matxin.bin");
    let st = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-comp");
    assert!(st.success());
    let input = tmp("matxin.in");
    std::fs::write(&input, "^word/word<n><sg>$ ^dog/dog<n><sg>/dog<vblex><pres>$.\n").unwrap();

    let mut outputs = Vec::new();
    for i in 0..2 {
        let out = tmp(&format!("matxin{i}.out"));
        let st = Command::new(env!("CARGO_BIN_EXE_cg-proc"))
            .current_dir(&dir)
            .args(["-f", "2"])
            .arg(&bin)
            .arg(&input)
            .arg(&out)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("spawn cg-proc");
        assert!(st.success());
        outputs.push(std::fs::read_to_string(&out).unwrap());
        let _ = std::fs::remove_file(&out);
    }
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&input);

    assert_eq!(outputs[0], outputs[1], "Matxin output not deterministic");
    let got = &outputs[0];
    assert!(got.contains("<corpus>"), "missing <corpus>: {got}");
    assert!(got.contains("<SENTENCE ord=\"1\""), "missing <SENTENCE>: {got}");
    // procNode emitted nested NODE elements with lemma + morph info.
    assert!(got.contains("<NODE ord=\"1\""), "missing NODE 1: {got}");
    assert!(got.contains("<NODE ord=\"2\""), "missing NODE 2: {got}");
    assert!(got.contains("lem=\"word\""), "missing lemma: {got}");
    assert!(got.contains("mi=\""), "missing morph info: {got}");
}

// MatxinApplicator::testPR — DECLARED but never DEFINED in the C++ (the port
// keeps it as a documented no-op stub); no binary can reach it, so the drive
// is the direct in-process call, asserting the stub emits nothing.
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.test-pr-fn/test]
#[test]
fn matxin_test_pr_stub() {
    let base = conv_base();
    let m = cg3::matxin_applicator::MatxinApplicator::new(base);
    let mut out: Vec<u8> = Vec::new();
    m.test_pr(&mut out);
    assert!(out.is_empty(), "MatxinApplicator::testPR must be a no-op stub");
}

// ===========================================================================
// BinaryApplicator (.cg3bsf binary stream format) — full round-trip through
// the real cg-conv binary. Leg 1 (`--in-cg --out-binary`) drives the WRITE
// side: the T_InputCommands fixture stream contains windows, plain text lines,
// and <STREAMCMD:FLUSH>/<STREAMCMD:EXIT> commands, so printSingleWindow,
// printPlainTextLine, and printStreamCommand all emit their packets (their
// ported bodies are the base-hosted bin_* writers the wrapper methods
// delegate to). Leg 2 (`--in-binary --out-cg`) drives the READ side:
// constructor, runGrammarOnText's packet loop, readPacket, readWindow,
// readCommand, and readText. The round-tripped text must be byte-identical to
// a direct `--in-cg --out-cg` pass over the same input.
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.binary-applicator-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-packet-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-window-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-command-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-text-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-single-window-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-stream-command-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-plain-text-line-fn/test]
#[test]
fn binary_stream_roundtrip() {
    let root = repo_root();
    let input = std::fs::read(root.join("test/T_InputCommands/input.txt")).unwrap();

    // Leg 1: CG text -> binary stream.
    let stream = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-cg", "--out-binary"],
        &root,
        &input,
    );
    assert!(stream.starts_with(b"CGBF"), "binary stream must start with CGBF magic");
    // The stream must contain window (0x01), command (0x02), and text (0x03)
    // packets; command FLUSH is the byte pair [0x02, 0x01].
    assert!(stream.windows(2).any(|w| w == [0x02, 0x01]), "no FLUSH command packet");
    assert!(stream.contains(&0x03u8), "no text packet");
    // printPlainTextLine wrote the fixture's literal text lines into packets.
    let hay = String::from_utf8_lossy(&stream);
    assert!(hay.contains("test2"), "text packet payload missing");

    // Leg 2: binary stream -> CG text.
    let back = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-binary", "--out-cg"],
        &root,
        &stream,
    );

    // Reference: straight CG -> CG pass.
    let reference = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-cg", "--out-cg"],
        &root,
        &input,
    );
    assert_eq!(
        String::from_utf8_lossy(&back),
        String::from_utf8_lossy(&reference),
        "cg -> binary -> cg round trip diverged from cg -> cg"
    );
    // Sanity: the round trip preserved cohorts and the stream commands.
    let back = String::from_utf8_lossy(&back);
    assert!(back.contains("\"<word>\""), "cohorts lost in round trip");
    assert!(back.contains("<STREAMCMD:FLUSH>"), "FLUSH lost in round trip");
}

// ===========================================================================
// FSTApplicator — in the current engine, FormatConverter's FST input/output
// arms route to CG3Quit (cg-conv exits), so no binary reaches the ported
// FSTApplicator; the honest drive is in-process, mirroring exactly what
// cg-conv would do once wired: conv grammar + is_conv + trace, then
// runGrammarOnText over `wordform<TAB>analysis[<TAB>weight]` lines. The
// is_conv fast path prints each finished cohort (printCohort → printReading)
// and the EOF drain prints the remaining window (printSingleWindow).
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.fst-applicator-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-reading-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-cohort-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-single-window-fn/test]
#[test]
fn fst_applicator_in_process() {
    let mut base = conv_base();
    base.is_conv = true;
    base.trace = true;
    base.verbosity_level = 0;
    let mut fst = cg3::fst_applicator::FSTApplicator::new(base);
    assert_eq!(fst.wtag, "W");
    assert_eq!(fst.sub_delims, "#");

    // Two cohorts: one plain, one with a weight (exercises the wtag path) and
    // a second ambiguous reading.
    let input = "blah\tblah+N+Sg\nblah\tblah+V+Inf\t0.5\n\ndog\tdog+N+Sg\n\n";
    let mut cursor = std::io::Cursor::new(input.as_bytes().to_vec());
    let mut out: Vec<u8> = Vec::new();
    fst.run_grammar_on_text(&mut cursor, &mut out);
    let text = String::from_utf8(out).unwrap();

    // printReading re-emits the FST `base+tags` shape per reading.
    assert!(text.contains("blah\tblah+N+Sg"), "FST reading 1 missing:\n{text}");
    assert!(text.contains("blah\tblah+V+Inf"), "FST reading 2 missing:\n{text}");
    // The weight came back as a <W:...> tag.
    assert!(text.contains("+<W:0.5"), "FST weight tag missing:\n{text}");
    assert!(text.contains("dog\tdog+N+Sg"), "FST second cohort missing:\n{text}");
}

// ===========================================================================
// JsonlApplicator — driven end-to-end by the real cg-conv `--in-jsonl` path
// (FormatConverter dispatches to JsonlApplicator::runGrammarOnText, whose
// printing side is JSONL as well): the input exercises a stream command
// ({"cmd"} → parse then printStreamCommand), a standalone leading text line
// ({"t"} → printPlainTextLine, since no window exists yet to attach it to),
// and a cohort with subreadings + static tags + trailing text (parseJsonCohort
// → parseJsonReading → jsonToUString on every string; printing goes
// printSingleWindow → printCohort → buildJsonReading → buildJsonTags →
// ustringToUtf8). Assertions pin the exact JSONL lines coming back.
// [spec:cg3:sem:jsonl-applicator.cg3.ustring-to-utf8-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.json-to-ustring-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn/test]
#[test]
fn jsonl_conv_roundtrip() {
    let root = repo_root();
    let input = concat!(
        "{\"t\":\"hello world\"}\n",
        "{\"cmd\":\"<STREAMCMD:FLUSH>\"}\n",
        "{\"w\":\"word\",\"rs\":[{\"l\":\"word\",\"ts\":[\"notwanted\",\"@1\"]},",
        "{\"l\":\"word\",\"ts\":[\"wanted\"],\"s\":{\"l\":\"sub\",\"ts\":[\"x\"]}}],",
        "\"z\":\"tail text\"}\n",
    );
    let out = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-jsonl", "--out-jsonl"],
        &root,
        input.as_bytes(),
    );
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();

    // Leading text line, before any window exists → printPlainTextLine.
    assert!(
        lines.iter().any(|l| *l == "{\"t\":\"hello world\"}"),
        "standalone text line lost:\n{text}"
    );
    // Stream command echoed via printStreamCommand.
    assert!(
        lines.iter().any(|l| *l == "{\"cmd\":\"<STREAMCMD:FLUSH>\"}"),
        "stream command lost:\n{text}"
    );
    // The cohort came back with wordform, both readings, the subreading, and
    // the z-suffix text.
    let cohort_line = lines
        .iter()
        .find(|l| l.contains("\"w\":\"word\""))
        .unwrap_or_else(|| panic!("cohort line lost:\n{text}"));
    assert!(cohort_line.contains("\"l\":\"word\""), "lemma lost: {cohort_line}");
    assert!(cohort_line.contains("\"notwanted\""), "tags lost: {cohort_line}");
    assert!(cohort_line.contains("\"s\":"), "subreading lost: {cohort_line}");
    assert!(cohort_line.contains("\"l\":\"sub\""), "subreading lemma lost: {cohort_line}");
    assert!(cohort_line.contains("\"z\":\"tail text\""), "z suffix lost: {cohort_line}");
}

// ===========================================================================
// FormatConverter (cg-conv side) — real cg-conv runs. The constructor builds
// the minimal conv grammar on every invocation; runGrammarOnText dispatches
// the input format; leaving the input format unspecified routes through
// FormatConverter::detectFormat (the method: peek + replay), which calls the
// free detectFormat sniffer. Both a CG-detected and a Niceline-detected input
// are pushed through auto-detection and must produce the same output as the
// explicitly-flagged runs.
// [spec:cg3:sem:format-converter.cg3.detect-format-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.format-converter-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.detect-format-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.run-grammar-on-text-fn/test]
#[test]
fn format_converter_autodetect() {
    let root = repo_root();

    // CG-format input, auto-detected vs explicit.
    let cg_input = std::fs::read(root.join("test/T_InputCommands/input.txt")).unwrap();
    let auto = run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), &[], &root, &cg_input);
    let explicit =
        run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), &["--in-cg"], &root, &cg_input);
    assert_eq!(
        String::from_utf8_lossy(&auto),
        String::from_utf8_lossy(&explicit),
        "auto-detect (CG) diverged from --in-cg"
    );
    assert!(String::from_utf8_lossy(&auto).contains("\"<word>\""));

    // Niceline-format input, auto-detected vs explicit.
    let nice_input = b"word\t\"word\" notwanted @1\n";
    let auto = run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), &[], &root, nice_input);
    let explicit =
        run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), &["--in-niceline"], &root, nice_input);
    assert_eq!(
        String::from_utf8_lossy(&auto),
        String::from_utf8_lossy(&explicit),
        "auto-detect (niceline) diverged from --in-niceline"
    );
    // Default output is CG, so the niceline input is CONVERTED: the C++
    // niceline driver's virtual print dispatches through the FormatConverter
    // overrides onto the CG printers (wave 4: the ConvFormat StreamFormat
    // strategy). The pre-wave-4 port echoed niceline here — a fidelity bug.
    let auto_s = String::from_utf8_lossy(&auto).into_owned();
    assert!(auto_s.contains("\"<word>\""), "niceline input must convert to CG: {auto_s:?}");
    assert!(auto_s.contains("\t\"word\" notwanted @1"), "reading line: {auto_s:?}");
}

// FormatConverter's four print dispatchers — in the current port these
// vtable-slot overrides are not reached from the tools (the base printers
// model the C++ virtual dispatch for BINARY themselves, and the wrapper
// drivers print their own formats), so the honest drive is in-process: build
// the converter exactly like cg-conv does, hand-build one window + cohort +
// reading in the shared base, and invoke each dispatcher for a live
// fmt_output arm (NICELINE for printCohort, JSONL for printSingleWindow /
// printStreamCommand / printPlainTextLine), asserting the format-specific
// encodings come out.
// [spec:cg3:sem:format-converter.cg3.format-converter.print-cohort-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.print-single-window-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.print-stream-command-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.print-plain-text-line-fn/test]
#[test]
fn format_converter_print_dispatch() {
    use cg3::grammar_applicator::cg3_sformat;

    let base = cg3::grammar_applicator::GrammarApplicator::new(cg3::grammar::Grammar::default());
    let mut fc = cg3::format_converter::FormatConverter::new(base);
    fc.base_mut().is_conv = true;
    fc.base_mut().trace = true;
    fc.base_mut().verbosity_level = 0;

    // Hand-build one window with one cohort ("<word>" with reading "word" X).
    let (sw, cohort) = {
        let b = fc.base_mut();
        let sw = b.gWindow.alloc_append_single_window(&mut b.store);
        b.init_empty_single_window(sw);
        let c = cg3::cohort::alloc_cohort(&mut b.store, Some(sw));
        let wf = b.add_tag("\"<word>\"", cg3::tag::TagType::empty());
        {
            let co = b.store.cohorts.get_mut(c.0);
            co.wordform = Some(wf);
            co.global_number = 1;
        }
        let r = cg3::reading::alloc_reading(&mut b.store, Some(c));
        b.add_tag_to_reading(r, wf);
        let bf = b.add_tag("\"word\"", cg3::tag::TagType::empty());
        b.add_tag_to_reading(r, bf);
        let t = b.add_tag("X", cg3::tag::TagType::empty());
        b.add_tag_to_reading(r, t);
        cg3::cohort::append_reading(&mut b.store, c, r);
        cg3::single_window::append_cohort(&mut b.gWindow, &mut b.store, sw, c);
        b.store.cohorts.get_mut(c.0).local_number = 1;
        (sw, c)
    };

    // printCohort → NICELINE arm.
    fc.base_mut().fmt_output = cg3_sformat::CG3SF_NICELINE;
    let mut out: Vec<u8> = Vec::new();
    fc.print_cohort(cohort, &mut out, false);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("word\t[word]"),
        "printCohort NICELINE dispatch wrong: {text}"
    );
    assert!(text.contains(" X"), "reading tag lost: {text}");

    // printSingleWindow → JSONL arm (one JSON object per cohort).
    fc.base_mut().fmt_output = cg3_sformat::CG3SF_JSONL;
    let mut out: Vec<u8> = Vec::new();
    fc.print_single_window(sw, &mut out, false);
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("\"w\":\"word\""),
        "printSingleWindow JSONL dispatch wrong: {text}"
    );
    assert!(text.contains("\"l\":\"word\""), "JSONL reading lost: {text}");

    // printStreamCommand → JSONL arm.
    let mut out: Vec<u8> = Vec::new();
    fc.print_stream_command("<STREAMCMD:FLUSH>", &mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "{\"cmd\":\"<STREAMCMD:FLUSH>\"}\n",
        "printStreamCommand JSONL dispatch wrong"
    );

    // printPlainTextLine → JSONL arm.
    let mut out: Vec<u8> = Vec::new();
    fc.print_plain_text_line("some text", &mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "{\"t\":\"some text\"}\n",
        "printPlainTextLine JSONL dispatch wrong"
    );
}

// ===========================================================================
// NicelineApplicator — driven end-to-end by the real cg-conv `--in-niceline`
// path: FormatConverter dispatches to NicelineApplicator::runGrammarOnText
// (constructor via the transient wrapper), which parses the
// `wordform<TAB>"base" tags` lines and prints them back in niceline shape
// (printSingleWindow → printCohort → printReading, with `[base]` brackets and
// TAB-indented subreadings).
// [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.niceline-applicator-fn/test]
// [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-reading-fn/test]
// [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-cohort-fn/test]
// [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-single-window-fn/test]
#[test]
fn niceline_conv() {
    let root = repo_root();
    let input = concat!(
        "word\t\"word\" notwanted @1\n",
        "dog\t\"dog\" N Sg\n",
        "some trailing text\n",
    );
    let out = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-niceline", "--out-niceline"],
        &root,
        input.as_bytes(),
    );
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("word\t[word] notwanted @1"),
        "niceline cohort 1 wrong:\n{text}"
    );
    assert!(text.contains("dog\t[dog] N Sg"), "niceline cohort 2 wrong:\n{text}");
    assert!(text.contains("some trailing text"), "text line lost:\n{text}");
}

// ===========================================================================
// PlaintextApplicator — in the current engine, FormatConverter's PLAIN input
// arm routes to CG3Quit (cg-conv exits), so no binary reaches the ported
// PlaintextApplicator; the honest drive is in-process, mirroring what cg-conv
// would do once wired: conv grammar + is_conv, then runGrammarOnText over raw
// plaintext. The driver tokenizes lines into cohorts (peeling ASCII
// punctuation) and the print side re-emits space-separated wordforms
// (printSingleWindow → printCohort).
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.plaintext-applicator-fn/test]
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-cohort-fn/test]
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-single-window-fn/test]
#[test]
fn plaintext_applicator_in_process() {
    let mut base = conv_base();
    base.is_conv = true;
    base.trace = true;
    base.verbosity_level = 0;
    let mut app = cg3::plaintext_applicator::PlaintextApplicator::new(base);
    // Constructor behavior: magic readings allowed, add_tags defaults off.
    assert!(app.base.allow_magic_readings);
    assert!(!app.add_tags);

    let input = "Hello brave world.\n";
    let mut cursor = std::io::Cursor::new(input.as_bytes().to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out);
    let text = String::from_utf8(out).unwrap();

    // Tokenized wordforms come back space-separated; the final '.' is peeled
    // into its own cohort.
    assert!(text.contains("Hello"), "token 1 lost:\n{text:?}");
    assert!(text.contains("brave"), "token 2 lost:\n{text:?}");
    assert!(text.contains("world"), "token 3 lost:\n{text:?}");
    assert!(text.contains("."), "punctuation cohort lost:\n{text:?}");
}

// ===========================================================================
// MweSplitApplicator — the exact test/T_MweSplit/run.pl protocol:
// `cg-mwesplit < input.txt`, diffed (-B) against expected.txt. cg-mwesplit
// constructs the applicator (which builds its own dummy grammar) and calls
// runGrammarOnText; every window passes through the overridden
// printSingleWindow, which calls splitMwe on each cohort; splitMwe consults
// maybeWfTag on each (sub)reading to find the `"<...>"` wordform tags that
// mark split points.
// [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.mwe-split-applicator-fn/test]
// [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.maybe-wf-tag-fn/test]
// [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.split-mwe-fn/test]
// [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.print-single-window-fn/test]
#[test]
fn mwesplit_fixture() {
    let root = repo_root();
    let dir = root.join("test/T_MweSplit");
    let input = std::fs::read(dir.join("input.txt")).unwrap();
    let out = run_with_stdin(env!("CARGO_BIN_EXE_cg-mwesplit"), &[], &dir, &input);
    let got = String::from_utf8(out).unwrap();
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    assert!(
        diff_b_equal(&want, &got),
        "cg-mwesplit output differs from expected.txt:\n{got}"
    );
}
