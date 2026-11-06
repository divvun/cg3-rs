//! Golden-file conformance harness — the Rust-native replacement for
//! `test/runall.pl`'s per-directory checks.
//!
//! Each `test/T_*` directory holds `grammar.cg3` + `input.txt` + `expected.txt`
//! (+ optional `args.txt` extra flags, `prefix.txt` mapping prefix). For every
//! such directory this runs the five `runall.pl` sub-tests as separate nextest
//! tests, all DIR-LOCAL (cwd = the test dir, because the grammar and input are
//! passed as bare names — `-g grammar.cg3`, `-I input.txt` — and `expected.txt`
//! is read from the dir. NOT because of `INCLUDE`: relative includes resolve
//! against the including file's own directory, so T_Include works from
//! anywhere):
//!
//! 1. [`golden_textual`]          — run the textual grammar, diff `expected.txt`.
//! 2. [`golden_grammar_roundtrip`]— write the parsed grammar back out
//!    (`--grammar-out`), run from it, compare untraced output.
//! 3. [`golden_binary_grammar`]   — compile to `.cg3b`, run from the binary.
//! 4. [`golden_binary_stream`]    — round-trip the stream through the binary
//!    format (`--in-cg|--out-binary` → `--in-binary|--out-binary` →
//!    `--in-binary|--out-cg`), compare untraced/sorted/stabilised output.
//! 5. [`golden_jsonl_stream`]     — the same round trip through JSONL, except
//!    in a dir holding `skip-jsonl.txt`.
//!
//! Directories needing a custom protocol (external process, relabel,
//! sub-readings) are in [`CUSTOM`] and covered by dedicated tests
//! (`tools_cli.rs`, `profiler_relabeller.rs`, `engine.rs`); the Apertium
//! `cg-proc` corpus is in `apertium.rs`.

mod common;
#[path = "common/filters.rs"]
mod filters;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{diff_b_equal, repo_root, run_capture};
use filters::{cg_sort, read_args, stabilize_relations, untrace};

/// Directories whose behaviour is exercised by a dedicated test rather than the
/// generic five sub-tests here (they need cg-relabel/cg-proc/an external process
/// or a dummy grammar).
const CUSTOM: &[&str] = &[
    "T_External",
    "T_MweSplit",
    "T_RelabelList",
    "T_RelabelList_Apertium",
    "T_RelabelSet",
    "T_SubReadings_Apertium",
];

fn vislcg3() -> &'static str {
    env!("CARGO_BIN_EXE_vislcg3")
}

fn golden_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(repo_root().join("test"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            p.is_dir()
                && name.starts_with("T_")
                && !CUSTOM.contains(&name.as_ref())
                && p.join("grammar.cg3").exists()
                && p.join("input.txt").exists()
                && p.join("expected.txt").exists()
        })
        .collect();
    dirs.sort();
    dirs
}

fn name_of(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().into_owned()
}

fn tmp(name: &str, tag: &str, ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cg3-{name}-{tag}-{}.{ext}", std::process::id()))
}

/// Copy the files directly in `from` into a fresh `to`, so a grammar there
/// resolves its relative includes (T_Include's sets.cg3, rules.cg3) as it
/// would in the fixture dir.
fn copy_fixture(from: &Path, to: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let path = entry?.path();
        if path.is_file() {
            std::fs::copy(&path, to.join(path.file_name().unwrap()))?;
        }
    }
    Ok(())
}

fn expected(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("expected.txt")).unwrap()
}

fn mapping_prefix(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("prefix.txt"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "@".to_string())
}

/// `args.txt` flags + `extra`, as a borrowed view for [`run_capture`].
fn argv<'a>(base: &'a [String], extra: &[&'a str]) -> Vec<&'a str> {
    base.iter()
        .map(|s| s.as_str())
        .chain(extra.iter().copied())
        .collect()
}

// ---------------------------------------------------------------------------
// Sub-test 1: normal textual run.
// ---------------------------------------------------------------------------

fn run_textual(dir: &Path, grammar: &Path) -> Result<(), String> {
    let out = tmp(&name_of(dir), "run", "txt");
    let status = Command::new(vislcg3())
        .current_dir(dir)
        .args(read_args(dir))
        .arg("-g")
        .arg(grammar)
        .arg("-I")
        .arg("input.txt")
        .arg("-O")
        .arg(&out)
        .status()
        .map_err(|e| format!("spawn vislcg3: {e}"))?;
    if !status.success() {
        return Err(format!("vislcg3 exited with {status}"));
    }
    let got = std::fs::read_to_string(&out).map_err(|e| format!("read output: {e}"))?;
    let _ = std::fs::remove_file(&out);
    if diff_b_equal(&expected(dir), &got) {
        Ok(())
    } else {
        Err("output differs from expected.txt".to_string())
    }
}

// [spec:cg3:req:crate-split.tools-unchanged/test]
#[test]
fn golden_textual() {
    run_all("golden_textual", |dir| {
        run_textual(dir, Path::new("grammar.cg3"))
    });
}

// ---------------------------------------------------------------------------
// Sub-test 2: write the parsed grammar back out, run from it.
// ---------------------------------------------------------------------------

#[test]
fn golden_grammar_roundtrip() {
    run_all("golden_grammar_roundtrip", |dir| {
        let name = name_of(dir);
        let gout = tmp(&name, "gout", "cg3");
        let out = tmp(&name, "gout", "txt");
        let write = Command::new(vislcg3())
            .current_dir(dir)
            .args(read_args(dir))
            .arg("-g")
            .arg("grammar.cg3")
            .arg("--grammar-only")
            .arg("--grammar-out")
            .arg(&gout)
            .status()
            .map_err(|e| format!("spawn (grammar-out): {e}"))?;
        if !write.success() {
            return Err(format!("--grammar-out exited with {write}"));
        }
        let run = Command::new(vislcg3())
            .current_dir(dir)
            .args(read_args(dir))
            .arg("-g")
            .arg(&gout)
            .arg("-I")
            .arg("input.txt")
            .arg("-O")
            .arg(&out)
            .status()
            .map_err(|e| format!("spawn (run from grammar-out): {e}"))?;
        let got = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&gout);
        let _ = std::fs::remove_file(&out);
        if !run.success() {
            return Err(format!("run from grammar-out exited with {run}"));
        }
        if diff_b_equal(&untrace(&expected(dir)), &untrace(&got)) {
            Ok(())
        } else {
            Err("round-tripped grammar output differs".to_string())
        }
    });
}

// ---------------------------------------------------------------------------
// Sub-test 3: compile to `.cg3b`, run from the binary grammar.
// ---------------------------------------------------------------------------

#[test]
fn golden_binary_grammar() {
    // Compile-time-only flags are not re-applied when running from a precompiled
    // grammar, faithful to the C++ (verified against the C++ binaries).
    const SKIP: &[&str] = &["T_CG2Compat"];
    run_all("golden_binary_grammar", |dir| {
        let name = name_of(dir);
        if SKIP.contains(&name.as_str()) {
            return Ok(());
        }
        let bin = tmp(&name, "bin", "cg3b");
        let compile = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
            .current_dir(dir)
            .arg("grammar.cg3")
            .arg(&bin)
            .status()
            .map_err(|e| format!("spawn cg-comp: {e}"))?;
        if !compile.success() {
            return Err(format!("cg-comp exited with {compile}"));
        }
        let r = run_textual(dir, &bin);
        let _ = std::fs::remove_file(&bin);
        r
    });
}

// ---------------------------------------------------------------------------
// Sub-tests 4 and 5: round-trip the stream through the binary or JSONL format.
// ---------------------------------------------------------------------------

/// `format` names the stream format as its `--in-*`/`--out-*` flags spell it.
/// It also names the scratch copy of the fixture, so the two legs never share
/// one when `cargo test` runs them as threads of a single process.
fn stream_round_trip(dir: &Path, format: &str) -> Result<(), String> {
    let name = name_of(dir);
    // runall.pl writes `Include Static grammar.cg3 ;` beside grammar.cg3.
    // The wrapper goes beside a copy instead, so the corpus is never
    // written to, and the include stays relative: an INCLUDE path ends at
    // the first whitespace and cannot be quoted, so an absolute path breaks
    // in any checkout whose path has a space.
    let bsf_dir = tmp(&name, format, "d");
    copy_fixture(dir, &bsf_dir).map_err(|e| format!("copy fixture: {e}"))?;
    let bsf = bsf_dir.join("grammar.bsf.cg3");
    std::fs::write(&bsf, "Include Static grammar.cg3 ;\n")
        .map_err(|e| format!("write bsf grammar: {e}"))?;
    let bsf_s = bsf.to_str().unwrap();
    let in_flag = format!("--in-{format}");
    let out_flag = format!("--out-{format}");

    let base = read_args(dir);
    let input = std::fs::read(dir.join("input.txt")).map_err(|e| format!("read input: {e}"))?;

    let (s1, ok1) = run_capture(
        vislcg3(),
        &argv(&base, &["--in-cg", &out_flag, "-g", bsf_s]),
        dir,
        &input,
    );
    let (s2, ok2) = run_capture(
        vislcg3(),
        &argv(&base, &["-g", "grammar.cg3", &in_flag, &out_flag]),
        dir,
        &s1,
    );
    let (s3, ok3) = run_capture(
        vislcg3(),
        &argv(&base, &[&in_flag, "--out-cg", "-g", bsf_s]),
        dir,
        &s2,
    );
    let _ = std::fs::remove_dir_all(&bsf_dir);
    if !(ok1 && ok2 && ok3) {
        return Err(format!("a {format}-stream stage exited non-zero"));
    }
    let prefix = mapping_prefix(dir);
    let got = stabilize_relations(&cg_sort(&untrace(&String::from_utf8_lossy(&s3)), &prefix));
    let want = stabilize_relations(&cg_sort(&untrace(&expected(dir)), &prefix));
    if diff_b_equal(&want, &got) {
        Ok(())
    } else {
        Err(format!("{format}-stream round-trip differs from expected"))
    }
}

#[test]
fn golden_binary_stream() {
    run_all("golden_binary_stream", |dir| {
        stream_round_trip(dir, "binary")
    });
}

#[test]
fn golden_jsonl_stream() {
    run_all("golden_jsonl_stream", |dir| {
        // runall.pl skips this leg, and only this leg, where the dir carries a
        // skip-jsonl.txt; the file says why the test cannot round-trip.
        if dir.join("skip-jsonl.txt").exists() {
            return Ok(());
        }
        stream_round_trip(dir, "jsonl")
    });
}

// ---------------------------------------------------------------------------

/// Run `f` over every golden dir, aggregating failures into one assertion.
fn run_all(label: &str, f: impl Fn(&Path) -> Result<(), String>) {
    let dirs = golden_dirs();
    assert!(!dirs.is_empty(), "no golden test dirs found");
    let mut failed: Vec<String> = Vec::new();
    for dir in &dirs {
        if let Err(e) = f(dir) {
            failed.push(format!("{}: {e}", name_of(dir)));
        }
    }
    assert!(
        failed.is_empty(),
        "{label}: {} of {} dirs failed:\n{}",
        failed.len(),
        dirs.len(),
        failed.join("\n")
    );
}
