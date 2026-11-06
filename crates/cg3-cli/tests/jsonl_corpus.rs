//! `test/T_Jsonl`, driven case for case as its upstream `run.pl` drives it:
//! each case feeds one file on stdin to `cg-conv` or `vislcg3`, run in the test
//! dir, and compares stdout with `expected.<case>.txt` byte for byte, and
//! stderr with `expected-stderr.<case>.txt` where that file exists.

#[expect(
    dead_code,
    reason = "run.pl diffs exactly and keeps stderr, so diff_b_equal and run_capture go unused here"
)]
mod common;

use std::process::Stdio;

use common::{repo_root, run_output};

const CG_CONV: &str = env!("CARGO_BIN_EXE_cg-conv");
const VISLCG3: &str = env!("CARGO_BIN_EXE_vislcg3");

/// The CLI reports through tracing, which starts each line with the level,
/// right-aligned in five columns, and a space; the C++ prints the message alone.
const LEVELS: [&str; 5] = ["TRACE ", "DEBUG ", " INFO ", " WARN ", "ERROR "];

fn without_levels(stderr: &str) -> String {
    stderr
        .split_inclusive('\n')
        .map(|line| {
            LEVELS
                .iter()
                .find_map(|level| line.strip_prefix(level))
                .unwrap_or(line)
        })
        .collect()
}

fn first_difference(want: &str, got: &str) -> String {
    let want: Vec<&str> = want.split_inclusive('\n').collect();
    let got: Vec<&str> = got.split_inclusive('\n').collect();
    let at = want
        .iter()
        .zip(&got)
        .position(|(w, g)| w != g)
        .unwrap_or(want.len().min(got.len()));
    format!(
        "line {}: expected {:?}, got {:?}",
        at + 1,
        want.get(at),
        got.get(at)
    )
}

fn case(name: &str, exe: &str, args: &[&str], stdin: &str) {
    let dir = repo_root().join("test/T_Jsonl");
    let input = std::fs::read(dir.join(stdin)).unwrap_or_else(|e| panic!("read {stdin}: {e}"));
    let out = run_output(exe, args, &dir, &input, Stdio::piped());
    let mut failures = Vec::new();

    // run.pl ignores the exit status, but the C++ exits 0 in every case, and a
    // crash after the last line of output would otherwise pass.
    if !out.status.success() {
        failures.push(format!("exited with {}", out.status));
    }

    let expected = format!("expected.{name}.txt");
    let want =
        std::fs::read(dir.join(&expected)).unwrap_or_else(|e| panic!("read {expected}: {e}"));
    if out.stdout != want {
        failures.push(format!(
            "stdout differs from {expected}, {}",
            first_difference(
                &String::from_utf8_lossy(&want),
                &String::from_utf8_lossy(&out.stdout)
            )
        ));
    }

    let expected_stderr = format!("expected-stderr.{name}.txt");
    if dir.join(&expected_stderr).exists() {
        let want = std::fs::read_to_string(dir.join(&expected_stderr))
            .unwrap_or_else(|e| panic!("read {expected_stderr}: {e}"));
        let got = without_levels(&String::from_utf8(out.stderr).expect("stderr is UTF-8"));
        if got != want {
            failures.push(format!(
                "stderr differs from {expected_stderr}, {}",
                first_difference(&want, &got)
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "T_Jsonl {name}:\n{}",
        failures.join("\n")
    );
}

// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-variables-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn+1/test]
#[test]
fn cg_converts_to_jsonl() {
    case(
        "cg-to-jsonl",
        CG_CONV,
        &["-c", "-D", "--deleted", "-J"],
        "input.txt",
    );
}

// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+2/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+2/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn+1/test]
#[test]
fn jsonl_converts_back_to_cg() {
    case(
        "jsonl-to-cg",
        CG_CONV,
        &["-j", "-D", "--deleted", "-C"],
        "expected.cg-to-jsonl.txt",
    );
}

#[test]
fn jsonl_round_trips_through_jsonl() {
    case(
        "jsonl-to-jsonl",
        CG_CONV,
        &["-j", "-D", "--deleted", "-J"],
        "expected.cg-to-jsonl.txt",
    );
}

// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+2/test]
#[test]
fn jsonl_without_deleted_ignores_deleted_readings() {
    case(
        "no-deleted",
        CG_CONV,
        &["-j", "-D", "-C"],
        "expected.cg-to-jsonl.txt",
    );
}

// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+2/test]
#[test]
fn rules_run_on_jsonl_input() {
    case(
        "rules",
        VISLCG3,
        &["-g", "grammar.cg3", "--in-jsonl", "--out-jsonl"],
        "input-rules.jsonl",
    );
}

// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn+1/test]
#[test]
fn traced_rules_write_jsonl() {
    case(
        "rules-trace",
        VISLCG3,
        &["-g", "grammar.cg3", "--in-jsonl", "--out-jsonl", "--trace"],
        "input-rules.jsonl",
    );
}

#[test]
fn traced_rules_write_cg_from_jsonl() {
    case(
        "rules-trace-cg",
        VISLCG3,
        &["-g", "grammar.cg3", "--in-jsonl", "--out-cg", "--trace"],
        "input-rules.jsonl",
    );
}

// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+2/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+2/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn+1/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.add-single-mapping-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.get-json-string-fn/test]
#[test]
fn malformed_jsonl_lines_warn_and_are_skipped() {
    case("errors", CG_CONV, &["-j", "-J"], "input-errors.jsonl");
}
