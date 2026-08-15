//! CLI-entry-point integration tests — one test per ported tool `main` (and its
//! helpers), driving the actual binaries over real `test/` fixtures the same way
//! `runall.pl` / the per-directory `run.pl` scripts do.
//!
//! All outputs go to `std::env::temp_dir()`; tests run dir-local (cwd = the
//! fixture dir) where relative paths matter. Nothing under `test/` is written.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// `diff -B`: compare ignoring blank-line differences (same as golden.rs).
fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cg3-tools-cli-{}-{}", std::process::id(), name))
}

/// Run `vislcg3` dir-local on `dir` with grammar `grammar`, feeding `input.txt`,
/// and assert the output diff-B-matches `expected.txt` (the runall.pl protocol).
fn run_vislcg3_expect(dir: &Path, grammar: &Path, out_name: &str) {
    let out = temp_path(out_name);
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(dir)
        .args(read_args(dir))
        .arg("-g")
        .arg(grammar)
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out)
        .status()
        .expect("spawn vislcg3");
    assert!(status.success(), "vislcg3 exited with {status}");
    let got = std::fs::read_to_string(&out).expect("read vislcg3 output");
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(
        diff_b_equal(&want, &got),
        "vislcg3 output differs from {}/expected.txt",
        dir.display()
    );
}

fn divvun_version_line(product: &str) -> String {
    format!(
        "Divvun CG-3 {product} v{} ({} {})",
        env!("CARGO_PKG_VERSION"),
        env!("CG3_BUILD_DATE"),
        env!("CG3_GIT_HASH")
    )
}

fn assert_divvun_version(binary_name: &str, binary: &str, product: &str, args: &[&str]) {
    let want = format!(
        "{}\n\
Copyright (C) 2026 UiT The Arctic University of Norway\n\
Copyright (C) 2007-2025 GrammarSoft ApS. Licensed under GPLv3+\n\
Source: {}\n",
        divvun_version_line(product),
        env!("CARGO_PKG_REPOSITORY")
    );

    for arg in args {
        let out = Command::new(binary)
            .arg(arg)
            .output()
            .unwrap_or_else(|e| panic!("spawn {binary_name}: {e}"));
        assert!(
            out.status.success(),
            "{binary_name} {arg} exited with {}",
            out.status
        );
        assert!(out.stderr.is_empty(), "{binary_name} {arg} wrote to stderr");
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            want,
            "{binary_name} {arg}"
        );
    }
}

// [spec:cg3:req:main.divvun-version-banner+2/test]
// [spec:cg3:req:tools.divvun-version-banner+2/test]
// [spec:cg3:sem:cg-proc.main-fn+1/test]
#[test]
fn all_tools_versions_identify_divvun_builds() {
    assert_divvun_version(
        "vislcg3",
        env!("CARGO_BIN_EXE_vislcg3"),
        "Disambiguator",
        &["-V", "--version"],
    );
    assert_divvun_version(
        "cg-comp",
        env!("CARGO_BIN_EXE_cg-comp"),
        "Compiler",
        &["--version"],
    );
    assert_divvun_version(
        "cg-conv",
        env!("CARGO_BIN_EXE_cg-conv"),
        "Format Converter",
        &["--version"],
    );
    assert_divvun_version(
        "cg-mwesplit",
        env!("CARGO_BIN_EXE_cg-mwesplit"),
        "MWE Splitter",
        &["--version"],
    );
    assert_divvun_version(
        "cg-proc",
        env!("CARGO_BIN_EXE_cg-proc"),
        "Disambiguator",
        &["-v", "--version"],
    );
    assert_divvun_version(
        "cg-relabel",
        env!("CARGO_BIN_EXE_cg-relabel"),
        "Relabeller",
        &["--version"],
    );

    #[cfg(feature = "profiler")]
    {
        assert_divvun_version(
            "cg-annotate",
            env!("CARGO_BIN_EXE_cg-annotate"),
            "Profiler Annotator",
            &["--version"],
        );
        assert_divvun_version(
            "cg-merge-annotations",
            env!("CARGO_BIN_EXE_cg-merge-annotations"),
            "Annotation Merger",
            &["--version"],
        );
    }
}

// [spec:cg3:sem:main.main-fn+3/test]
#[test]
fn vislcg3_help_uses_build_provenance() {
    let out = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .arg("--help")
        .output()
        .expect("spawn vislcg3");
    assert!(
        out.status.success(),
        "vislcg3 --help exited with {}",
        out.status
    );
    assert!(out.stderr.is_empty(), "vislcg3 --help wrote to stderr");
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with(&format!("{}\n", divvun_version_line("Disambiguator")))
    );
}

// [spec:cg3:sem:main.main-fn+3/test]
// The full vislcg3 main: option parsing (args.txt), textual grammar load,
// reindex, and the applicator run over test/T_Select's input, byte-checked
// against the fixture's expected.txt (runall.pl sub-test 1).
#[test]
fn vislcg3_main_runs_t_select() {
    let dir = repo_root().join("test/T_Select");
    run_vislcg3_expect(&dir, Path::new("grammar.cg3"), "vislcg3-select.txt");
}

// The `--nrules` / `--nrules-v` filters are compiled through the ICU seam, so an
// ICU-spelled filter means on the command line what the same spelling means in a
// grammar. `\Q...\E` exists only in ICU — the `regex` crate rejects it outright —
// and `[:script=Greek:]` is the reverse hazard: every Rust engine ACCEPTS it as a
// literal character set and silently matches the wrong thing, so the seam names
// it instead.
// [spec:cg3:req:tag-regex.single-seam+1/test]
#[test]
fn nrules_filters_speak_icu() {
    let dir = repo_root().join("test/T_NRules");

    // args.txt is `--nrules pick --nrules-v X`; `\Qpick\E` is the ICU spelling of
    // the same filter and must select the same rules.
    let out = temp_path("nrules-icu.txt");
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(&dir)
        .args(["--nrules", r"\Qpick\E", "--nrules-v", "X"])
        .arg("-g")
        .arg("grammar.cg3")
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out)
        .status()
        .expect("spawn vislcg3");
    assert!(status.success(), "vislcg3 exited with {status}");
    let got = std::fs::read_to_string(&out).expect("read vislcg3 output");
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(diff_b_equal(&want, &got), "ICU-spelled --nrules diverged");

    // A construct this engine would misread is refused, not quietly obeyed.
    let refused = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(&dir)
        .args(["--nrules", "[:script=Greek:]"])
        .arg("-g")
        .arg("grammar.cg3")
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(temp_path("nrules-bad.txt"))
        .output()
        .expect("spawn vislcg3");
    assert!(!refused.status.success(), "a misread pattern must not pass");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("--nrules"), "{stderr}");
    assert!(stderr.contains("in-set property"), "{stderr}");
}

// [spec:cg3:sem:cg-comp.main-fn/test]
// cg-comp main: text parse -> reindex -> binary write. Compiles
// test/T_Select/grammar.cg3 to a temp .cg3b (asserting the CG3B magic), then
// vislcg3 runs from that binary grammar and must reproduce expected.txt
// (runall.pl sub-test 3).
#[test]
fn cg_comp_main_compiles_t_select() {
    let dir = repo_root().join("test/T_Select");
    let bin = temp_path("comp-select.cg3b");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp exited with {status}");
    let head = std::fs::read(&bin).expect("read compiled grammar");
    assert!(
        head.len() > 4 && &head[..4] == b"CG3B",
        "missing CG3B magic"
    );
    run_vislcg3_expect(&dir, &bin, "comp-select.txt");
    let _ = std::fs::remove_file(&bin);
}

/// Compiling writes the grammar's source beside the `.cg3b`, with no flag asked
/// for, and the companion file describes THIS binary — so a rule number
/// resolves back to the text the author wrote, across the `INCLUDE` boundary.
// [spec:cg3:req:diagnostics.sidecar/test]
// [spec:cg3:req:diagnostics.sidecar-identity/test]
#[test]
fn cg_comp_writes_grammar_source_beside_binary() {
    let dir = repo_root().join("test/T_Include");
    let bin = temp_path("comp-include.cg3b");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp exited with {status}");

    let sidecar = cg3::grammar_sources::sidecar_path(&bin);
    assert!(
        sidecar.exists(),
        "compiling must write {} unasked",
        sidecar.display()
    );

    let sources = cg3::grammar_sources::read_sidecar(&bin).expect("a fresh sidecar is accepted");
    assert!(
        sources.sources.len() >= 2,
        "the top-level grammar and its includes, got {:?}",
        sources.sources.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    let quoted: Vec<String> = sources
        .rules
        .iter()
        .map(|&(number, _)| {
            let span = sources.locate(number).expect("every listed rule places");
            let text: Vec<char> = sources.sources[span.source].text.chars().collect();
            text[span.range].iter().collect()
        })
        .collect();
    assert!(
        quoted.iter().any(|q| q.starts_with("SELECT ASet")),
        "a rule from the top-level grammar must be quotable, got {quoted:?}"
    );
    assert!(
        quoted.iter().any(|q| q.starts_with("SELECT BSet")),
        "a rule from an INCLUDEd file must be quotable too, got {quoted:?}"
    );

    // Recompiled from a different grammar, the leftover sidecar no longer
    // describes the binary in hand and must stop being read.
    let other = repo_root().join("test/T_Select");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&other)
        .arg("grammar.cg3")
        .arg(temp_path("comp-other.cg3b"))
        .status()
        .expect("spawn cg-comp");
    assert!(status.success());
    std::fs::copy(temp_path("comp-other.cg3b"), &bin).expect("swap the binary under the sidecar");
    assert!(
        cg3::grammar_sources::read_sidecar(&bin).is_none(),
        "a sidecar that describes another grammar must be refused"
    );

    let _ = std::fs::remove_file(&sidecar);
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(temp_path("comp-other.cg3b"));
    let _ = std::fs::remove_file(cg3::grammar_sources::sidecar_path(&temp_path(
        "comp-other.cg3b",
    )));
}

// [spec:cg3:req:diagnostics.rendered/test]
// A grammar that will not compile gets a rendered report per error on stderr,
// with no flag asked for: the file it came from, the line as written, and a
// marker under the part that failed. Two bad lines, so recovery is covered too.
#[test]
fn a_bad_grammar_is_reported_against_its_source() {
    let grammar = temp_path("diagnostics.cg3");
    std::fs::write(
        &grammar,
        "DELIMITERS = \"<.>\" ;\nLIST a = \"[:script=Greek:]\"r ;\nSELECT nosuch ;\n",
    )
    .expect("write grammar");
    let out = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .arg(&grammar)
        .arg(temp_path("diagnostics.cg3b"))
        .output()
        .expect("spawn cg-comp");
    assert!(!out.status.success(), "a bad grammar must not compile");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        stderr.contains("diagnostics.cg3:2:10"),
        "the report must name the file and the place: {stderr}"
    );
    assert!(
        stderr.contains("LIST a = \"[:script=Greek:]\"r ;"),
        "the report must quote the offending line: {stderr}"
    );
    assert!(
        stderr.contains("this tag") && stderr.contains("the parse stopped here"),
        "both failures must be marked: {stderr}"
    );
    assert!(
        !stderr.contains("<utf8-memory>"),
        "a named parse must not report the in-memory placeholder: {stderr}"
    );
    let _ = std::fs::remove_file(&grammar);
}

/// A grammar whose rule asks the running stream for a tag no parser will
/// accept, with the offending rule in an `INCLUDE`d file so resolving it has to
/// cross a file boundary. `$1` captures `(x` out of the wordform, and a tag
/// cannot open with `(`.
fn runtime_failure_fixture() -> PathBuf {
    let dir = temp_path("rt-rule");
    std::fs::create_dir_all(&dir).expect("fixture dir");
    std::fs::write(
        dir.join("lists.cg3"),
        "LIST N = (n) ;\n\nSECTION\nADD (@ok) N ;\nADD (VSTR:$1) (\"<\\(.*\\)>\"r) ;\n",
    )
    .expect("write include");
    std::fs::write(
        dir.join("nb.cg3"),
        "DELIMITERS = \"<$.>\" ;\nINCLUDE lists.cg3 ;\n",
    )
    .expect("write grammar");
    std::fs::write(dir.join("input.txt"), "\"<(x>\"\n\t\"x\" n\n").expect("write input");
    dir
}

/// Run vislcg3 dir-local over the fixture with `grammar`, returning stderr.
fn runtime_failure_stderr(dir: &Path, grammar: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(dir)
        .arg("-g")
        .arg(grammar)
        .arg("-I")
        .arg("input.txt")
        .output()
        .expect("spawn vislcg3");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// `RT RULE 3467` names no file, and its line may belong to any of a dozen
/// `INCLUDE`d ones. A runtime rule failure must instead quote the rule, out of
/// the file it was actually written in — from a textual grammar, which has the
/// sources' names in hand, and equally from a `.cg3b`, which has only what the
/// companion file beside it supplies.
// [spec:cg3:req:diagnostics.runtime-placed/test]
// [spec:cg3:req:diagnostics.source-lazy/test]
#[test]
fn a_runtime_rule_failure_quotes_its_grammar() {
    let dir = runtime_failure_fixture();

    for (label, grammar) in [("textual", "nb.cg3"), ("binary", "nb.cg3b")] {
        if grammar.ends_with("b") {
            let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
                .current_dir(&dir)
                .arg("nb.cg3")
                .arg("nb.cg3b")
                .status()
                .expect("spawn cg-comp");
            assert!(status.success(), "cg-comp exited with {status}");
        }
        let stderr = runtime_failure_stderr(&dir, grammar);

        assert!(
            stderr.contains("cannot construct tag `(x`"),
            "{label}: the report must name the tag that failed: {stderr}"
        );
        assert!(
            stderr.contains("lists.cg3:5"),
            "{label}: it must name the INCLUDEd file the rule was written in: {stderr}"
        );
        assert!(
            stderr.contains("ADD (VSTR:$1)"),
            "{label}: it must quote the rule that asked: {stderr}"
        );
        assert!(
            stderr.contains("this rule asked for it"),
            "{label}: the quoted rule must be marked: {stderr}"
        );
        assert!(
            !stderr.contains("RT RULE"),
            "{label}: a placed failure must not fall back to the bare label: {stderr}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// With no companion file — a `.cg3b` the C++ compiled, one shipped without its
/// source, one whose companion is stale — there is nothing to quote, and the
/// report falls back to exactly the label and line it always gave rather than
/// guessing at a location.
// [spec:cg3:req:diagnostics.runtime-placed/test]
#[test]
fn a_runtime_failure_falls_back_unresolved() {
    let dir = runtime_failure_fixture();
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("nb.cg3")
        .arg("nb.cg3b")
        .status()
        .expect("spawn cg-comp");
    assert!(status.success());
    std::fs::remove_file(dir.join("nb.cg3b.cg3src")).expect("drop the companion file");

    let stderr = runtime_failure_stderr(&dir, "nb.cg3b");
    assert!(
        stderr.contains("RT RULE") && stderr.contains("on line 5"),
        "the fallback must be the label and line, unchanged: {stderr}"
    );
    assert!(
        stderr.contains("cannot construct tag `(x`"),
        "the tag that failed is known without any sources: {stderr}"
    );
    assert!(
        !stderr.contains("ADD (VSTR:$1)"),
        "nothing may be quoted when there is no source to quote from: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// [spec:cg3:sem:cg-comp.end-program-fn+3/test]
// cg-comp's endProgram: wrong argc (no args) prints the version + usage banner
// to stdout and exits EXIT_FAILURE.
#[test]
fn cg_comp_end_program_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .output()
        .expect("spawn cg-comp");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&divvun_version_line("Compiler")),
        "missing banner: {stdout}"
    );
    assert!(
        stdout.contains("USAGE: cg-comp grammar_file output_file"),
        "missing usage: {stdout}"
    );
}

// [spec:cg3:sem:cg-conv.main-fn/test]
// cg-conv main: option-table parsing (--in-niceline), FormatConverter setup, and
// the stdin->stdout conversion run. Niceline input is CONVERTED to the default
// CG output: the C++ niceline driver's virtual print dispatch lands on the
// FormatConverter overrides, which emit fmt_output (CG) — wave 4's ConvFormat
// strategy. (The pre-wave-4 port echoed niceline here — a fidelity bug.)
// Exact bytes asserted.
#[test]
fn cg_conv_main_converts_niceline_stream() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cg-conv"))
        .arg("--in-niceline")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cg-conv");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"word\t\"word\" N Sg\nbirds\t\"bird\" N Pl\n")
        .unwrap();
    let out = child.wait_with_output().expect("wait cg-conv");
    assert!(out.status.success(), "cg-conv exited with {}", out.status);
    let got = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        got,
        "\"<word>\"\n\t\"word\" N Sg\n\"<birds>\"\n\t\"bird\" N Pl\n"
    );
}

// [spec:cg3:sem:inlines.cg3.is-cg3bsf-fn+1/test]
// The format sniff hands the stream magic detector whatever the first read
// returned, so an empty stream used to index past the end of a zero-length
// buffer and abort the process. Asserts the reachable path: empty stdin exits
// cleanly with no output and no panic. `stderr` is captured rather than
// nulled, because a panic message is exactly what this is looking for.
#[test]
fn an_empty_stream_is_not_binary() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cg-conv"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cg-conv");
    drop(child.stdin.take().unwrap());
    let out = child.wait_with_output().expect("wait cg-conv");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !err.contains("panicked"),
        "cg-conv panicked on empty input: {err}"
    );
    assert!(out.status.success(), "cg-conv exited with {}", out.status);
    assert!(
        out.stdout.is_empty(),
        "expected no output, got {:?}",
        out.stdout
    );
}

// [spec:cg3:sem:cg-proc.main-fn+1/test]
// cg-proc main: getopt loop (-d), binary grammar load, ApertiumApplicator run
// over the Apertium stream fixture (the test/Apertium/T_Select run.pl protocol:
// cg-comp then `cg-proc -d grammar.bin input.txt output.txt`).
#[test]
fn cg_proc_main_runs_apertium_t_select() {
    let dir = repo_root().join("test/Apertium/T_Select");
    let bin = temp_path("proc-apertium-select.bin");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp exited with {status}");

    let out = temp_path("proc-apertium-select.txt");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-proc"))
        .current_dir(&dir)
        .arg("-d")
        .arg(&bin)
        .arg("input.txt")
        .arg(&out)
        .status()
        .expect("spawn cg-proc");
    assert!(status.success(), "cg-proc exited with {status}");

    let got = std::fs::read_to_string(&out).expect("read cg-proc output");
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&out);
    assert!(
        diff_b_equal(&want, &got),
        "cg-proc output differs from expected.txt"
    );
}

// [spec:cg3:sem:cg-proc.end-program-fn+3/test]
// cg-proc's endProgram: with no grammar argument main falls through to the
// usage path — version + option summary on stdout, exit EXIT_FAILURE.
#[test]
fn cg_proc_end_program_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_cg-proc"))
        .output()
        .expect("spawn cg-proc");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&divvun_version_line("Disambiguator")),
        "missing banner: {stdout}"
    );
    assert!(stdout.contains("USAGE: cg-proc"), "missing usage: {stdout}");
    assert!(
        stdout.contains("--stream-format"),
        "missing option list: {stdout}"
    );
}

// [spec:cg3:sem:cg-relabel.main-fn+1/test]
// [spec:cg3:sem:cg-relabel.cg3-grammar-load-fn+1/test]
// The test/T_RelabelList run.pl protocol: cg-comp compiles grammar.cg3,
// cg-relabel loads the BINARY grammar plus the TEXT relabel grammar (both
// branches of cg3_grammar_load), relabels, writes a new binary grammar, and
// vislcg3 run from that grammar must reproduce expected.txt.
#[test]
fn cg_relabel_main_relabels_t_relabel_list() {
    let dir = repo_root().join("test/T_RelabelList");
    let bin = temp_path("relabel-in.cg3b");
    let bin_out = temp_path("relabel-out.cg3b");

    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp exited with {status}");

    let status = Command::new(env!("CARGO_BIN_EXE_cg-relabel"))
        .current_dir(&dir)
        .arg(&bin)
        .arg("relabel.cg3r")
        .arg(&bin_out)
        .status()
        .expect("spawn cg-relabel");
    assert!(status.success(), "cg-relabel exited with {status}");
    let head = std::fs::read(&bin_out).expect("read relabelled grammar");
    assert!(
        head.len() > 4 && &head[..4] == b"CG3B",
        "missing CG3B magic"
    );

    run_vislcg3_expect(&dir, &bin_out, "relabel-select.txt");
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&bin_out);
}

// [spec:cg3:sem:cg-relabel.end-program-fn+3/test]
// cg-relabel's endProgram: wrong argc prints the version + usage banner to
// stdout and exits EXIT_FAILURE.
#[test]
fn cg_relabel_end_program_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_cg-relabel"))
        .output()
        .expect("spawn cg-relabel");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&divvun_version_line("Relabeller")),
        "missing banner: {stdout}"
    );
    assert!(
        stdout
            .contains("USAGE: cg-relabel input_grammar_file relabel_rule_file output_grammar_file"),
        "missing usage: {stdout}"
    );
}

// [spec:cg3:sem:cg-mwesplit.main-fn/test]
// cg-mwesplit main: option parsing, dummy-grammar MweSplitApplicator, and the
// stdin->stdout run over test/T_MweSplit/input.txt (that directory's run.pl
// protocol: `cg-mwesplit < input.txt`, diff -ZB vs expected.txt).
#[test]
fn cg_mwesplit_main_splits_t_mwesplit() {
    let dir = repo_root().join("test/T_MweSplit");
    let input = std::fs::read(dir.join("input.txt")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cg-mwesplit"))
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cg-mwesplit");
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let out = child.wait_with_output().expect("wait cg-mwesplit");
    assert!(
        out.status.success(),
        "cg-mwesplit exited with {}",
        out.status
    );
    let got = String::from_utf8_lossy(&out.stdout);
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    assert!(
        diff_b_equal(&want, &got),
        "cg-mwesplit output differs from expected.txt"
    );
}

/// Build a profiler database the way `vislcg3 --profile` + the parser wiring
/// would: one grammar (fname + source), an AST string (interned as the
/// grammar_ast, stored under key 0), one rule + one context entry with hit
/// counts, example windows, and a rule->context link. Returns
/// `(db_path, grammar_text)`.
#[cfg(feature = "profiler")]
fn write_profile_db(name: &str, num_match: usize, with_example: bool) -> (PathBuf, String) {
    use cg3::profiler::{ET_CONTEXT, ET_RULE, Key, Profiler};

    // Grammar text with XML metacharacters so cg-annotate must escape them.
    let grammar = "SELECT (wanted) IF (0 (\"<w>\")) ;\n".to_string();
    let rule_b = 0usize;
    let rule_e = grammar.find(';').unwrap() + 1;
    let ctx_b = grammar.find("(0").unwrap();
    let ctx_e = grammar.find("))").unwrap() + 2;

    let mut p = Profiler::default();
    let gid = p.add_grammar("profile-grammar.cg3", &grammar);
    let ast = format!(
        "<Grammar u=\"{gid}\">\n<Rule l=\"1\" b=\"{rule_b}\" e=\"{rule_e}\" u=\"1\">\
         <Context l=\"1\" b=\"{ctx_b}\" e=\"{ctx_e}\" u=\"7\"/></Rule>\n</Grammar>\n"
    );
    p.grammar_ast = p.add_string(&ast);

    p.add_rule(1, gid, rule_b, rule_e);
    p.add_context(7, gid, ctx_b, ctx_e);
    p.rule_contexts.insert((1, 7), num_match);

    let window = p.add_string("\"<word>\"\n\t\"word\" wanted\n");
    {
        let e = p
            .entries
            .get_mut(&Key {
                r#type: ET_RULE,
                id: 1,
            })
            .unwrap();
        e.num_match = num_match;
        e.num_fail = 1;
        if with_example {
            e.example_window = window;
        }
    }
    {
        let e = p
            .entries
            .get_mut(&Key {
                r#type: ET_CONTEXT,
                id: 7,
            })
            .unwrap();
        e.num_match = num_match;
        if with_example {
            e.example_window = window;
        }
    }

    let db = temp_path(name);
    p.write(db.to_str().unwrap()).expect("write profile db");
    (db, grammar)
}

// [spec:cg3:sem:cg-annotate.main-fn/test]
// [spec:cg3:sem:cg-annotate.xml-encode-fn/test]
// [spec:cg3:sem:cg-annotate.file-save-fn/test]
// cg-annotate main: reads a profiler db (built via the crate's own Profiler,
// same schema `vislcg3 --profile` writes), splits the AST per grammar, and
// emits the annotated g<N>.html / rs/<id>.html / cs/<id>.html / index.html /
// style.css report. file_save is what materialises every one of those files;
// xml_encode is verified through the escaped `("<w>")` grammar snippet
// (&quot;&lt;w&gt;&quot;) appearing in the emitted HTML.
#[cfg(feature = "profiler")]
#[test]
fn cg_annotate_main_writes_report() {
    let (db, _grammar) = write_profile_db("annotate.db", 3, true);
    let out_dir = temp_path("annotate-out");
    let _ = std::fs::remove_dir_all(&out_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_cg-annotate"))
        .arg(&db)
        .arg(&out_dir)
        .status()
        .expect("spawn cg-annotate");
    assert!(status.success(), "cg-annotate exited with {status}");

    // index.html links the grammar page (grammar string id 2: fname is 1).
    let index = std::fs::read_to_string(out_dir.join("index.html")).unwrap();
    assert!(
        index.contains(r#"<a href="g2.html">profile-grammar.cg3</a>"#),
        "index: {index}"
    );

    // The annotated grammar page: rule span + stats + xml-escaped source.
    let g = std::fs::read_to_string(out_dir.join("g2.html")).unwrap();
    assert!(g.contains(r#"<span class="cg-elem cgRule">"#), "g2: {g}");
    assert!(
        g.contains(r#"class="entry good"><span class="stats">M:3, F:1"#),
        "g2: {g}"
    );
    assert!(
        g.contains(r#"class="entry context good"><span class="stats">M:3"#),
        "g2: {g}"
    );
    assert!(
        g.contains("(&quot;&lt;w&gt;&quot;)"),
        "xml_encode missing: {g}"
    );

    // Usage-example pages for the rule and the context, with the escaped
    // example window (file_save wrote them into rs/ and cs/).
    let rs = std::fs::read_to_string(out_dir.join("rs/1.html")).unwrap();
    assert!(
        rs.contains("SELECT (wanted) IF (0 (&quot;&lt;w&gt;&quot;)) ;"),
        "rs: {rs}"
    );
    assert!(rs.contains("&quot;&lt;word&gt;&quot;"), "rs window: {rs}");
    let cs = std::fs::read_to_string(out_dir.join("cs/7.html")).unwrap();
    assert!(cs.contains("(0 (&quot;&lt;w&gt;&quot;))"), "cs: {cs}");

    let css = std::fs::read_to_string(out_dir.join("style.css")).unwrap();
    assert!(css.contains(".cg-elem"), "style.css: {css}");

    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&out_dir);
}

// [spec:cg3:sem:cg-merge-annotations.main-fn/test]
// cg-merge-annotations main: reads the base db, folds the input db into it
// (summing rule/context match counts and rule_contexts, adopting the missing
// example window), and writes the merged db — verified by reading it back with
// the crate's Profiler.
#[cfg(feature = "profiler")]
#[test]
fn cg_merge_annotations_main_sums_counts() {
    use cg3::profiler::{ET_CONTEXT, ET_RULE, Key, Profiler};

    // Base has no example window; input carries one (and different counts).
    let (base_db, _) = write_profile_db("merge-base.db", 3, false);
    let (in_db, _) = write_profile_db("merge-in.db", 5, true);
    let merged_db = temp_path("merge-out.db");

    let status = Command::new(env!("CARGO_BIN_EXE_cg-merge-annotations"))
        .arg(&merged_db)
        .arg(&base_db)
        .arg(&in_db)
        .status()
        .expect("spawn cg-merge-annotations");
    assert!(
        status.success(),
        "cg-merge-annotations exited with {status}"
    );

    let mut merged = Profiler::default();
    merged
        .read(merged_db.to_str().unwrap())
        .expect("read merged db");

    let rule = merged.entries[&Key {
        r#type: ET_RULE,
        id: 1,
    }];
    assert_eq!(rule.num_match, 3 + 5, "rule matches not summed");
    assert_eq!(rule.num_fail, 1 + 1, "rule fails not summed");
    assert_ne!(rule.example_window, 0, "missing example window not adopted");

    let ctx = merged.entries[&Key {
        r#type: ET_CONTEXT,
        id: 7,
    }];
    assert_eq!(ctx.num_match, 3 + 5, "context matches not summed");

    assert_eq!(
        merged.rule_contexts[&(1, 7)],
        3 + 5,
        "rule_contexts not summed"
    );

    let _ = std::fs::remove_file(&base_db);
    let _ = std::fs::remove_file(&in_db);
    let _ = std::fs::remove_file(&merged_db);
}
