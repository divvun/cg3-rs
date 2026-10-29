//! Numeric options at the edge of their range: none may overflow or expand
//! into memory the grammar can never use. The test runner kills a test after
//! 10 s, which is the assertion for the ones that used to exhaust memory.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
        .canonicalize()
        .unwrap()
}

fn vislcg3(args: &[&str]) -> Output {
    let dir = fixture("T_Select");
    Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(&dir)
        .args(["-g", "grammar.cg3", "-I", "input.txt"])
        .args(args)
        .output()
        .expect("spawn vislcg3")
}

fn assert_clean(out: &Output, what: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{what} exited with {}: {stderr}",
        out.status
    );
    assert!(!stderr.contains("panicked"), "{what} panicked: {stderr}");
}

// `(num_windows + 4) * 2 + 1` overflowed u32 for the largest values.
// [spec:cg3:req:robustness.checked-arithmetic/test]
#[test]
fn huge_num_windows_does_not_overflow() {
    for n in ["2147483644", "4294967295"] {
        assert_clean(
            &vislcg3(&["--num-windows", n]),
            &format!("--num-windows {n}"),
        );
    }
}

// A range option was expanded element by element before the run began.
// [spec:cg3:req:robustness.allocation-bounded/test]
#[test]
fn huge_rule_and_section_ranges_stay_bounded() {
    for args in [
        ["--rules", "0-4000000000"],
        ["--trace", "1-4000000000"],
        ["--sections", "4000000000"],
        ["--rules", "99999999999999999999"],
    ] {
        assert_clean(&vislcg3(&args), &args.join(" "));
    }
}

// `cg-proc -s N` pushed every section number up to N.
#[test]
fn cg_proc_huge_section_count_stays_bounded() {
    let out = Command::new(env!("CARGO_BIN_EXE_cg-proc"))
        .current_dir(fixture("T_Select"))
        .args(["-s", "2147483647", "grammar.cg3"])
        .stdin(Stdio::null())
        .output()
        .expect("spawn cg-proc");
    assert_clean(&out, "cg-proc -s 2147483647");
}

// A contextual-test offset at the top of the i32 range, which the parser
// accepts, was added to a cohort position without a check.
#[test]
fn extreme_context_offset_does_not_overflow() {
    let dir = std::env::temp_dir().join(format!("cg3-option-bounds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let grammar = dir.join("offset.cg3");
    std::fs::write(
        &grammar,
        "DELIMITERS = \"<$.>\" ;\nADD (@x) (*) (2147483647 (*)) ;\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .current_dir(fixture("T_Select"))
        .args(["-g", grammar.to_str().unwrap(), "-I", "input.txt"])
        .output()
        .expect("spawn vislcg3");
    let _ = std::fs::remove_dir_all(&dir);
    assert_clean(&out, "offset 2147483647");
}

// A range that runs past the grammar's last rule selects exactly the rules
// the same range cut off at that rule does.
#[test]
fn rule_range_past_the_grammar_selects_the_same() {
    let all = vislcg3(&["--trace"]);
    let open_ended = vislcg3(&["--trace", "--rules", "11-4000000000"]);
    let closed = vislcg3(&["--trace", "--rules", "11-12"]);
    assert_clean(&open_ended, "--rules 11-4000000000");
    assert_eq!(open_ended.stdout, closed.stdout);
    assert_ne!(
        all.stdout, closed.stdout,
        "the range must exclude the ADD on line 7"
    );
}
