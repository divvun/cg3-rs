//! Golden-file conformance harness — the Rust-native replacement for
//! `test/runall.pl`'s per-directory checks.
//!
//! Each `test/T_*` directory holds `grammar.cg3` + `input.txt` + `expected.txt`
//! (+ optional `args.txt` extra flags, `prefix.txt` mapping prefix). For every
//! such directory this runs the four `runall.pl` sub-tests as separate nextest
//! tests, all DIR-LOCAL (cwd = the test dir, because grammars use relative
//! `INCLUDE` paths):
//!
//! 1. [`golden_textual`]          — run the textual grammar, diff `expected.txt`.
//! 2. [`golden_grammar_roundtrip`]— write the parsed grammar back out
//!    (`--grammar-out`), run from it, compare untraced output.
//! 3. [`golden_binary_grammar`]   — compile to `.cg3b`, run from the binary.
//! 4. [`golden_binary_stream`]    — round-trip the stream through the binary
//!    format (`--in-cg|--out-binary` → `--in-binary|--out-binary` →
//!    `--in-binary|--out-cg`), compare untraced/sorted/stabilised output.
//!
//! Directories needing a custom protocol (external process, relabel,
//! sub-readings) are in [`CUSTOM`] and covered by dedicated tests
//! (`tools_cli.rs`, `profiler_relabeller.rs`, `engine.rs`); the Apertium
//! `cg-proc` corpus is in `apertium.rs`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{
    cg_sort, diff_b_equal, read_args, repo_root, run_capture, stabilize_relations, untrace,
};

/// Directories whose behaviour is exercised by a dedicated test rather than the
/// generic four sub-tests here (they need cg-relabel/cg-proc/an external process
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
// Sub-test 4: round-trip the stream through the binary format.
// ---------------------------------------------------------------------------

#[test]
fn golden_binary_stream() {
    run_all("golden_binary_stream", |dir| {
        let name = name_of(dir);
        // `Include Static <grammar.cg3> ;` — an absolute path so the bsf grammar
        // can live in a temp dir while grammar.cg3 stays dir-local.
        let grammar_abs = dir.join("grammar.cg3");
        let bsf = tmp(&name, "bsf", "cg3");
        std::fs::write(&bsf, format!("Include Static {} ;\n", grammar_abs.display()))
            .map_err(|e| format!("write bsf grammar: {e}"))?;
        let bsf_s = bsf.to_str().unwrap();

        let base = read_args(dir);
        let input =
            std::fs::read(dir.join("input.txt")).map_err(|e| format!("read input: {e}"))?;

        let (b1, ok1) = run_capture(
            vislcg3(),
            &argv(&base, &["--in-cg", "--out-binary", "-g", bsf_s]),
            dir,
            &input,
        );
        let (b2, ok2) = run_capture(
            vislcg3(),
            &argv(&base, &["-g", "grammar.cg3", "--in-binary", "--out-binary"]),
            dir,
            &b1,
        );
        let (b3, ok3) = run_capture(
            vislcg3(),
            &argv(&base, &["--in-binary", "--out-cg", "-g", bsf_s]),
            dir,
            &b2,
        );
        let _ = std::fs::remove_file(&bsf);
        if !(ok1 && ok2 && ok3) {
            return Err("a binary-stream stage exited non-zero".to_string());
        }
        let prefix = mapping_prefix(dir);
        let got =
            stabilize_relations(&cg_sort(&untrace(&String::from_utf8_lossy(&b3)), &prefix));
        let want = stabilize_relations(&cg_sort(&untrace(&expected(dir)), &prefix));
        if diff_b_equal(&want, &got) {
            Ok(())
        } else {
            Err("binary-stream round-trip differs from expected".to_string())
        }
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
