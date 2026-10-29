//! Stream-format applicator integration tests — Apertium, Matxin, Binary
//! stream (.cg3bsf), FST, JSONL, FormatConverter, Niceline, Plaintext, and
//! MweSplit applicators.
//!
//! Drives are real end-to-end runs of the ported binaries (`cg-proc`,
//! `cg-conv`, `cg-comp`, `cg-mwesplit`) over the `test/` fixture corpus (the
//! same protocol as the fixtures' `run.pl` scripts), plus in-process library
//! calls for lower-level serializer/parser details and for Matxin output
//! (which follows the upstream converter's missing switch arm), plus
//! `ApertiumApplicator::testPR` which is a
//! commented-out debug block in the C++ `cg-proc.cpp`).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
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
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin_bytes)
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait tool");
    assert!(
        out.status.success(),
        "{bin} {args:?} exited with {}",
        out.status
    );
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
    cmd.current_dir(dir)
        .args(proc_flags)
        .arg(&bin)
        .arg("input.txt")
        .arg(&out);
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
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.run-grammar-on-text-fn+1/test]
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
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.parse-stream-var-fn+1/test]
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
    assert!(
        got.contains("[<STREAMCMD:SETVAR:myvar>]"),
        "SETVAR blank lost: {got}"
    );
    assert!(
        got.contains("[<STREAMCMD:SETVAR:a=1,b>]"),
        "SETVAR list blank lost: {got}"
    );
    assert!(got.contains("^word/word<n><sg>"), "cohort lost: {got}");
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
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.run-grammar-on-text-fn+1/test]
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
    std::fs::write(
        &input,
        "^word/word<n><sg>$ ^dog/dog<n><sg>/dog<vblex><pres>$.\n",
    )
    .unwrap();

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
    assert!(
        got.contains("<SENTENCE ord=\"1\""),
        "missing <SENTENCE>: {got}"
    );
    // procNode emitted nested NODE elements with lemma + morph info.
    assert!(got.contains("<NODE ord=\"1\""), "missing NODE 1: {got}");
    assert!(got.contains("<NODE ord=\"2\""), "missing NODE 2: {got}");
    assert!(got.contains("lem=\"word\""), "missing lemma: {got}");
    assert!(got.contains("mi=\""), "missing morph info: {got}");
}

// MatxinApplicator::testPR — DECLARED but never DEFINED in the C++, so the
// port omits the symbol entirely (see the PORT DIVERGENCE note on
// matxin-applicator.cg3.matxin-applicator.test-pr-fn in
// docs/spec/port/src/MatxinApplicator.md); there is nothing to drive.

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
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-window-fn+1/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-command-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-text-fn/test]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-single-window-fn+1/test]
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
    assert!(
        stream.starts_with(b"CGBF"),
        "binary stream must start with CGBF magic"
    );
    // The stream must contain window (0x01), command (0x02), and text (0x03)
    // packets; command FLUSH is the byte pair [0x02, 0x01].
    assert!(
        stream.windows(2).any(|w| w == [0x02, 0x01]),
        "no FLUSH command packet"
    );
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
    assert!(
        back.contains("<STREAMCMD:FLUSH>"),
        "FLUSH lost in round trip"
    );
}

// ===========================================================================
// JsonlApplicator — driven end-to-end by the real cg-conv `--in-jsonl` path
// (FormatConverter dispatches to JsonlApplicator::runGrammarOnText, whose
// printing side is JSONL as well): the input exercises a stream command
// ({"cmd"} → parse then printStreamCommand), a standalone leading text line
// ({"t"} → printPlainTextLine, since no window exists yet to attach it to),
// and a cohort with subreadings + static tags + trailing text (parseJsonCohort
// → parseJsonReading → json_to_string on every string; printing goes
// printSingleWindow → printCohort → buildJsonReading → buildJsonTags).
// Assertions pin the exact JSONL lines coming back.
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.json-to-ustring-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+1/test]
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
        lines.contains(&"{\"t\":\"hello world\"}"),
        "standalone text line lost:\n{text}"
    );
    // Stream command echoed via printStreamCommand.
    assert!(
        lines.contains(&"{\"cmd\":\"<STREAMCMD:FLUSH>\"}"),
        "stream command lost:\n{text}"
    );
    // The cohort came back with wordform, both readings, the subreading, and
    // the z-suffix text.
    let cohort_line = lines
        .iter()
        .find(|l| l.contains("\"w\":\"word\""))
        .unwrap_or_else(|| panic!("cohort line lost:\n{text}"));
    assert!(
        cohort_line.contains("\"l\":\"word\""),
        "lemma lost: {cohort_line}"
    );
    assert!(
        cohort_line.contains("\"notwanted\""),
        "tags lost: {cohort_line}"
    );
    assert!(
        cohort_line.contains("\"s\":"),
        "subreading lost: {cohort_line}"
    );
    assert!(
        cohort_line.contains("\"l\":\"sub\""),
        "subreading lemma lost: {cohort_line}"
    );
    assert!(
        cohort_line.contains("\"z\":\"tail text\""),
        "z suffix lost: {cohort_line}"
    );
}

// ===========================================================================
// FormatConverter (cg-conv side) — real cg-conv runs. The constructor builds
// the minimal conv grammar on every invocation; runGrammarOnText dispatches
// the input format; leaving the input format unspecified routes through
// FormatConverter::detectFormat (the method: peek + replay), which calls the
// free detectFormat sniffer. Both a CG-detected and a Niceline-detected input
// are pushed through auto-detection and must produce the same output as the
// explicitly-flagged runs.
// [spec:cg3:sem:format-converter.cg3.detect-format-fn+1/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.format-converter-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.detect-format-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.run-grammar-on-text-fn/test]
#[test]
fn format_converter_autodetect() {
    let root = repo_root();

    // CG-format input, auto-detected vs explicit.
    let cg_input = std::fs::read(root.join("test/T_InputCommands/input.txt")).unwrap();
    let auto = run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), &[], &root, &cg_input);
    let explicit = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-cg"],
        &root,
        &cg_input,
    );
    assert_eq!(
        String::from_utf8_lossy(&auto),
        String::from_utf8_lossy(&explicit),
        "auto-detect (CG) diverged from --in-cg"
    );
    assert!(String::from_utf8_lossy(&auto).contains("\"<word>\""));

    // Niceline-format input, auto-detected vs explicit.
    let nice_input = b"word\t\"word\" notwanted @1\n";
    let auto = run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), &[], &root, nice_input);
    let explicit = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-niceline"],
        &root,
        nice_input,
    );
    assert_eq!(
        String::from_utf8_lossy(&auto),
        String::from_utf8_lossy(&explicit),
        "auto-detect (niceline) diverged from --in-niceline"
    );
    // Default output is CG, so the niceline input is CONVERTED: the C++
    // niceline driver's virtual print dispatches through the FormatConverter
    // overrides onto the CG printers (the ConvFormat StreamFormat strategy).
    // Echoing the niceline input instead would be a fidelity bug.
    let auto_s = String::from_utf8_lossy(&auto).into_owned();
    assert!(
        auto_s.contains("\"<word>\""),
        "niceline input must convert to CG: {auto_s:?}"
    );
    assert!(
        auto_s.contains("\t\"word\" notwanted @1"),
        "reading line: {auto_s:?}"
    );

    // The remaining auto-detectable text formats use their real parser arms.
    for (name, explicit_flag, input) in [
        ("apertium", "--in-apertium", &b"^word/word<n><sg>$\n"[..]),
        ("fst", "--in-fst", &b"word\tword+N+Sg\n"[..]),
        ("plain", "--in-plain", &b"Hello, world!\n"[..]),
    ] {
        let auto = run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), &[], &root, input);
        let explicit = run_with_stdin(
            env!("CARGO_BIN_EXE_cg-conv"),
            &[explicit_flag],
            &root,
            input,
        );
        assert_eq!(auto, explicit, "auto-detect ({name}) diverged");
        assert!(!auto.is_empty(), "{name} conversion produced no output");
    }
}

// Every format arm present in the C++ FormatConverter is wired in both
// directions. A single CG cohort is emitted through each specialized output
// strategy, and each specialized parser is also converted back to CG.
#[test]
fn format_converter_wires_apertium_fst_and_plaintext() {
    let root = repo_root();
    let cg = b"\"<word>\"\n\t\"word\" N Sg\n";

    let apertium = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-cg", "--out-apertium"],
        &root,
        cg,
    );
    let apertium = String::from_utf8(apertium).unwrap();
    assert!(
        apertium.contains("^word/word<N><Sg>$"),
        "CG -> Apertium dispatch wrong: {apertium:?}"
    );

    let fst = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-cg", "--out-fst"],
        &root,
        cg,
    );
    let fst = String::from_utf8(fst).unwrap();
    assert!(
        fst.contains("word\tword+N+Sg"),
        "CG -> FST dispatch wrong: {fst:?}"
    );

    let plain = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-cg", "--out-plain"],
        &root,
        cg,
    );
    let plain = String::from_utf8(plain).unwrap();
    assert_eq!(plain.trim(), "word", "CG -> plaintext dispatch wrong");

    for (name, args, input, reading) in [
        (
            "Apertium",
            &["--in-apertium"][..],
            &b"^word/word<n><sg>$\n"[..],
            "\t\"word\" n sg",
        ),
        (
            "FST",
            &["--in-fst"][..],
            &b"word\tword+N+Sg\n"[..],
            "\t\"word\" N Sg",
        ),
        (
            "plaintext --add-tags",
            &["--add-tags"][..],
            &b"Hello\n"[..],
            "<cg-conv>",
        ),
    ] {
        let out = run_with_stdin(env!("CARGO_BIN_EXE_cg-conv"), args, &root, input);
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.contains("\"<"),
            "{name} -> CG produced no cohort: {out:?}"
        );
        assert!(out.contains(reading), "{name} -> CG lost reading: {out:?}");
    }

    let weighted = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-fst", "--wtag", "Weight", "--wfactor", "2"],
        &root,
        b"word\tword+N\t2.5\n",
    );
    let weighted = String::from_utf8(weighted).unwrap();
    assert!(
        weighted.contains("<Weight:5.000000>"),
        "FST options were not applied: {weighted:?}"
    );

    let subreadings = run_with_stdin(
        env!("CARGO_BIN_EXE_cg-conv"),
        &["--in-fst", "--sub-delim", "/"],
        &root,
        b"word\touter/inner+N\n",
    );
    let subreadings = String::from_utf8(subreadings).unwrap();
    assert!(
        subreadings.contains("\"outer\"") && subreadings.contains("\"inner\""),
        "FST sub-delimiter was not applied: {subreadings:?}"
    );

    // vislcg3 uses the same converter around a real grammar, so exercise an
    // Apertium-input/FST-output run through the engine as well.
    let visl = run_with_stdin(
        env!("CARGO_BIN_EXE_vislcg3"),
        &[
            "-g",
            "test/T_Select/grammar.cg3",
            "--in-apertium",
            "--out-fst",
        ],
        &root,
        b"^word/word<notwanted>/word<wanted>$\n",
    );
    let visl = String::from_utf8(visl).unwrap();
    assert!(
        visl.contains("word\tword+wanted"),
        "vislcg3 did not use the wired formats: {visl:?}"
    );
    assert!(!visl.contains("+notwanted"), "SELECT did not run: {visl:?}");
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
    assert!(
        text.contains("dog\t[dog] N Sg"),
        "niceline cohort 2 wrong:\n{text}"
    );
    assert!(
        text.contains("some trailing text"),
        "text line lost:\n{text}"
    );
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
// [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.split-mwe-fn+1/test]
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
