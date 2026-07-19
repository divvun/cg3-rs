//! Shared support for the conformance harnesses (`golden.rs`, `apertium.rs`).
//!
//! This is the Rust-native replacement for `test/runall.pl` and the Perl/Python
//! stream filters it piped through (`scripts/cg-untrace`, `scripts/cg-sort`,
//! `scripts/cg-stabilize-relations`). The filters are applied to BOTH the
//! expected fixture and the tool output before comparing, so they only need to
//! normalise both sides consistently — they are not byte-for-byte reproductions
//! of the originals.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::{Captures, Regex};

/// `crates/cg3` -> repo root (holds `test/`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Extra per-directory flags from `args.txt` (whitespace-separated), as
/// `runall.pl` reads them.
pub fn read_args(dir: &Path) -> Vec<String> {
    match std::fs::read_to_string(dir.join("args.txt")) {
        Ok(s) => s.split_whitespace().map(|s| s.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

/// `diff -B`: equal ignoring blank-line differences.
pub fn diff_b_equal(a: &str, b: &str) -> bool {
    let na: Vec<&str> = a.lines().filter(|l| !l.trim().is_empty()).collect();
    let nb: Vec<&str> = b.lines().filter(|l| !l.trim().is_empty()).collect();
    na == nb
}

/// Run `exe args` in `cwd`, feed `input` on stdin, return `(stdout, success)`.
/// stderr is discarded (as the harness redirects it). stdin is written on a
/// thread so a large stdout can drain concurrently (no pipe deadlock).
pub fn run_capture(exe: &str, args: &[&str], cwd: &Path, input: &[u8]) -> (Vec<u8>, bool) {
    use std::io::Write;
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn {exe}: {e}"));
    let mut stdin = child.stdin.take().unwrap();
    let owned = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&owned);
        // `stdin` drops here -> EOF for the child.
    });
    let out = child
        .wait_with_output()
        .unwrap_or_else(|e| panic!("wait {exe}: {e}"));
    let _ = writer.join();
    (out.stdout, out.status.success())
}

// ---------------------------------------------------------------------------
// Ported stream filters
// ---------------------------------------------------------------------------

static UNTRACE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#" [-A-Z]+:[^"\s]+"#).unwrap());
static UNTRACE_RELN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#" (?:ADD|REM|SET)RELATIONS?\(\S+\):[^"\s]+"#).unwrap());

/// Port of `scripts/cg-untrace`: drop deleted readings and strip the `--trace`
/// tags from reading lines, protecting `ID:` / `R:` relation tags.
pub fn untrace(s: &str) -> String {
    let mut out = String::new();
    for line in s.split_inclusive('\n') {
        if line.starts_with(';') {
            continue;
        }
        let is_reading = {
            let t = line.trim_start_matches([' ', '\t']);
            t.len() != line.len() && t.starts_with('"')
        };
        if is_reading {
            let mut l = line.replace(" ID:", " xID:").replace(" R:", " xR:");
            loop {
                let n = UNTRACE_TAG.replace_all(&l, "").into_owned();
                if n == l {
                    break;
                }
                l = n;
            }
            loop {
                let n = UNTRACE_RELN.replace_all(&l, "").into_owned();
                if n == l {
                    break;
                }
                l = n;
            }
            l = l.replace(" xID:", " ID:").replace(" xR:", " R:");
            out.push_str(&l);
        } else {
            out.push_str(line);
        }
    }
    out
}

static STABILIZE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(ID:|R:[^:\s]+:)(\d+)\b").unwrap());

/// Port of `scripts/cg-stabilize-relations`: renumber `ID:` / `R:name:` targets
/// in first-seen order so relation numbering is comparable across runs.
pub fn stabilize_relations(s: &str) -> String {
    let mut id_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    STABILIZE_TAG
        .replace_all(s, |caps: &Captures| {
            let next = id_map.len() + 1;
            let id = *id_map.entry(caps[2].to_string()).or_insert(next);
            format!("{}{}", &caps[1], id)
        })
        .into_owned()
}

/// Port of `scripts/cg-sort -m <prefix>`: within each cohort, unique the
/// readings, sort each reading's mapping tags (those beginning with `prefix`),
/// then sort the readings. (Only the `--mapping` mode `runall.pl` uses is
/// implemented — no weight/reverse/first.)
pub fn cg_sort(s: &str, mapping_prefix: &str) -> String {
    let esc = regex::escape(mapping_prefix);
    let map_tag = Regex::new(&format!(r" ({esc}\S+)")).unwrap();
    let map_run = Regex::new(&format!(r"( {esc}\S+)+")).unwrap();

    let mut out = String::new();
    let mut in_cohort = false;
    let mut readings: Vec<String> = Vec::new();
    let mut seen_readings: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut seen_deleted: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut trail = String::new();

    let flush = |out: &mut String,
                 readings: &mut Vec<String>,
                 deleted: &mut Vec<String>,
                 trail: &mut String,
                 in_cohort: &mut bool| {
        if !*in_cohort {
            return;
        }
        push_sorted(out, readings, &map_tag, &map_run);
        push_sorted(out, deleted, &map_tag, &map_run);
        out.push_str(trail);
        readings.clear();
        deleted.clear();
        trail.clear();
        *in_cohort = false;
    };

    for line in s.split_inclusive('\n') {
        // Cohort header: `"<...>"`.
        if line.starts_with("\"<") {
            flush(
                &mut out,
                &mut readings,
                &mut deleted,
                &mut trail,
                &mut in_cohort,
            );
            in_cohort = true;
            seen_readings.clear();
            seen_deleted.clear();
            continue;
        }
        if in_cohort {
            let t = line.trim_start_matches([' ', '\t']);
            if t.len() != line.len() && t.starts_with('"') {
                if seen_readings.insert(line.to_string()) {
                    readings.push(line.to_string());
                }
            } else if line.starts_with(';') && t.starts_with('"') {
                if seen_deleted.insert(line.to_string()) {
                    deleted.push(line.to_string());
                }
            } else {
                trail.push_str(line);
            }
            continue;
        }
        out.push_str(line);
    }
    flush(
        &mut out,
        &mut readings,
        &mut deleted,
        &mut trail,
        &mut in_cohort,
    );
    out
}

fn push_sorted(out: &mut String, lines: &mut [String], map_tag: &Regex, map_run: &Regex) {
    for l in lines.iter_mut() {
        let mut tags: Vec<String> = map_tag.captures_iter(l).map(|c| c[1].to_string()).collect();
        if !tags.is_empty() {
            tags.sort();
            let joined = format!(" {}", tags.join(" "));
            *l = map_run.replace(l, joined.as_str()).into_owned();
        }
    }
    lines.sort();
    for l in lines.iter() {
        out.push_str(l);
    }
}
