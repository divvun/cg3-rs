//! The tools against command lines and output streams they cannot use: each
//! is refused with a message and a nonzero exit, or is ended by `SIGPIPE` as
//! the C++ tools are, and none of them panics.
//!
//! Every test drives the real binaries. Outputs go to `std::env::temp_dir()`.
#![cfg(unix)]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

const VISLCG3: &str = env!("CARGO_BIN_EXE_vislcg3");
const CG_PROC: &str = env!("CARGO_BIN_EXE_cg-proc");
const CG_COMP: &str = env!("CARGO_BIN_EXE_cg-comp");
const CG_CONV: &str = env!("CARGO_BIN_EXE_cg-conv");
const CG_MWESPLIT: &str = env!("CARGO_BIN_EXE_cg-mwesplit");
const CG_RELABEL: &str = env!("CARGO_BIN_EXE_cg-relabel");
#[cfg(feature = "profiler")]
const CG_ANNOTATE: &str = env!("CARGO_BIN_EXE_cg-annotate");
#[cfg(feature = "profiler")]
const CG_MERGE: &str = env!("CARGO_BIN_EXE_cg-merge-annotations");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cg3-cli-robust-{}-{}", std::process::id(), name))
}

/// Every tool binary this build has.
fn all_tools() -> Vec<&'static str> {
    [
        &[VISLCG3, CG_PROC, CG_COMP, CG_CONV, CG_MWESPLIT, CG_RELABEL][..],
        #[cfg(feature = "profiler")]
        &[CG_ANNOTATE, CG_MERGE],
    ]
    .concat()
}

/// Assert `out` is a refusal: exit status 1, no panic, and a message on stderr
/// containing `says`.
fn assert_refused(what: &str, out: &Output, says: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{what} panicked: {stderr}");
    assert_eq!(out.status.code(), Some(1), "{what}: {stderr}");
    assert!(
        stderr.contains(says),
        "{what} should say {says:?}: {stderr}"
    );
}

/// Which standard stream the tool finds closed.
#[derive(Clone, Copy, Debug)]
enum Closed {
    Stdout,
    Stderr,
}

/// The signal a write into a pipe with no reader raises.
const SIGPIPE: i32 = 13;

/// Assert `bin args` ended as the C++ tool does when its reader has gone:
/// killed by `SIGPIPE`, not exiting (a panic exits 101).
fn assert_sigpipe(status: ExitStatus, bin: &str, args: &[&str], closed: Closed) {
    assert_eq!(
        status.signal(),
        Some(SIGPIPE),
        "{bin} {args:?} into a closed {closed:?} ended with {status}"
    );
}

/// Run `bin args` with its `closed` stream a pipe whose reader is already gone,
/// so the first write to it raises `SIGPIPE`, and return how it ended.
fn exit_into_closed_pipe(bin: &str, args: &[&str], closed: Closed) -> ExitStatus {
    run_into_closed_pipe(bin, args, b"", closed)
}

/// [`exit_into_closed_pipe`], with `input` on the tool's stdin.
fn run_into_closed_pipe(bin: &str, args: &[&str], input: &[u8], closed: Closed) -> ExitStatus {
    let (reader, writer) = std::io::pipe().expect("pipe");
    drop(reader);
    let mut cmd = Command::new(bin);
    cmd.args(args).stdin(Stdio::piped());
    match closed {
        Closed::Stdout => cmd.stdout(writer).stderr(Stdio::piped()),
        Closed::Stderr => cmd.stderr(writer).stdout(Stdio::null()),
    };
    let mut child = cmd.spawn().expect("spawn");
    let mut stdin = child.stdin.take().expect("stdin");
    // A tool that ends without reading its input closes this pipe too.
    let _ = std::io::Write::write_all(&mut stdin, input);
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.is_empty(),
        "{bin} {args:?} into a closed {closed:?} said: {stderr}"
    );
    out.status
}

// [spec:cg3:req:robustness.cli-output+1/test]
// Usage, help and version text written into a pipe nobody reads: the tool is
// ended by the signal, and says nothing about the pipe.
#[test]
fn help_into_a_closed_pipe_raises_sigpipe() {
    let cases: &[(&str, &[&str])] = &[
        (VISLCG3, &["--help"]),
        (VISLCG3, &["-V"]),
        (VISLCG3, &["--min-binary-revision"]),
        (CG_PROC, &["-h"]),
        (CG_PROC, &["-v"]),
        (CG_PROC, &[]),
        (CG_COMP, &[]),
        (CG_RELABEL, &[]),
        (CG_CONV, &["--help"]),
        (CG_MWESPLIT, &["--help"]),
    ];
    for &(bin, args) in cases {
        let status = exit_into_closed_pipe(bin, args, Closed::Stdout);
        assert_sigpipe(status, bin, args, Closed::Stdout);
    }
    for bin in all_tools() {
        let status = exit_into_closed_pipe(bin, &["--version"], Closed::Stdout);
        assert_sigpipe(status, bin, &["--version"], Closed::Stdout);
    }
}

// [spec:cg3:req:robustness.cli-output+1/test]
// A run whose reader has gone ends as the help does.
#[test]
fn a_run_into_a_closed_pipe_raises_sigpipe() {
    let select = repo_root().join("test/T_Select");
    let grammar = select.join("grammar.cg3");
    let input = std::fs::read(select.join("input.txt")).unwrap();
    let cases: [(&str, &[&str]); 3] = [
        (VISLCG3, &["-g", grammar.to_str().unwrap()]),
        (CG_CONV, &[]),
        (CG_MWESPLIT, &[]),
    ];
    for (bin, args) in cases {
        let status = run_into_closed_pipe(bin, args, &input, Closed::Stdout);
        assert_sigpipe(status, bin, args, Closed::Stdout);
    }
}

// [spec:cg3:req:robustness.cli-output+1/test]
// A bad flag's usage goes to stderr; with stderr closed the signal ends the
// tool there, as it does with stdout.
#[test]
fn usage_into_a_closed_stderr_raises_sigpipe() {
    for bin in [CG_CONV, CG_MWESPLIT] {
        let status = exit_into_closed_pipe(bin, &["--no-such-flag"], Closed::Stderr);
        assert_sigpipe(status, bin, &["--no-such-flag"], Closed::Stderr);
    }
}

// [spec:cg3:req:robustness.cli-arguments/test]
// An argument that is not UTF-8 is named and refused, by every tool, before the
// tool starts.
#[test]
fn every_tool_refuses_a_non_utf8_argument() {
    for bin in all_tools() {
        let out = Command::new(bin)
            .arg("-g")
            .arg(OsStr::from_bytes(b"\xff.cg3"))
            .output()
            .expect("spawn");
        assert_refused(bin, &out, "\"\u{fffd}.cg3\": not valid UTF-8");
    }
}

// [spec:cg3:req:robustness.cli-arguments/test]
// The option variables are held to the same rule as the command line.
#[test]
fn option_variables_must_be_utf8() {
    for (bin, var) in [
        (VISLCG3, "CG3_DEFAULT"),
        (CG_PROC, "CG3_OVERRIDE"),
        (CG_CONV, "CG3_CONV_DEFAULT"),
    ] {
        let out = Command::new(bin)
            .env(var, OsStr::from_bytes(b"-W \xff"))
            .stdin(Stdio::null())
            .output()
            .expect("spawn");
        assert_refused(bin, &out, &format!("{var}: not valid UTF-8"));
    }
}

fn cg_conv(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut child = Command::new(CG_CONV)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cg-conv");
    let mut stdin = child.stdin.take().unwrap();
    std::io::Write::write_all(&mut stdin, b"\"<a>\"\n\t\"a\" N\n").unwrap();
    drop(stdin);
    child.wait_with_output().expect("wait cg-conv")
}

/// A cg-conv command line, the option variables it runs with, and what its
/// refusal says.
type ConvCase = (
    &'static [&'static str],
    &'static [(&'static str, &'static str)],
    &'static str,
);

// [spec:cg3:req:robustness.cli-arguments/test]
// [spec:cg3:sem:cg-conv.main-fn+1/test]
// `-W` and `--dep-delimit` values that are not numbers the option can hold —
// on the command line or from `CG3_CONV_DEFAULT` — are refused by name.
#[test]
fn cg_conv_refuses_values_that_are_not_numbers() {
    let cases: &[ConvCase] = &[
        (
            &["-W", "abc"],
            &[],
            "--wfactor expects a number, not \"abc\"",
        ),
        (&["-Wabc"], &[], "--wfactor expects"),
        (&["-W", "0,5"], &[], "--wfactor expects"),
        (&[], &[("CG3_CONV_DEFAULT", "-W x")], "--wfactor expects"),
        (&["--dep-delimit", "abc"], &[], "--dep-delimit expects"),
        (
            &["--dep-delimit", "99999999999"],
            &[],
            "--dep-delimit expects",
        ),
    ];
    for &(args, env, says) in cases {
        let out = cg_conv(args, env);
        assert_refused(&format!("cg-conv {args:?} {env:?}"), &out, says);
        assert!(out.stdout.is_empty(), "cg-conv {args:?} wrote output");
    }
}

// vislcg3's numeric options (read in GrammarApplicator::set_options, so the
// command line, CG3_DEFAULT and a grammar's CMDARGS alike) and cg-proc's `-f`
// / `-s` used to read a value that is no number as 0.
// [spec:cg3:req:robustness.cli-arguments/test]
#[test]
fn numeric_options_refuse_values_that_are_not_numbers() {
    let dir = repo_root().join("test/T_Select");
    for args in [
        &["--dep-delimit", "abc"][..],
        &["--num-windows", "x"],
        &["--soft-limit", "99999999999"],
        &["--hard-limit", "12abc"],
    ] {
        let out = Command::new(VISLCG3)
            .current_dir(&dir)
            .args(["-g", "grammar.cg3", "-I", "input.txt"])
            .args(args)
            .output()
            .expect("spawn vislcg3");
        assert_refused(&format!("vislcg3 {args:?}"), &out, "expects a whole number");
    }
    for args in [&["-s", "abc"][..], &["-f", "x"]] {
        let out = Command::new(CG_PROC)
            .current_dir(&dir)
            .args(args)
            .arg("grammar.cg3")
            .stdin(Stdio::null())
            .output()
            .expect("spawn cg-proc");
        assert_refused(&format!("cg-proc {args:?}"), &out, "expects a whole number");
    }
    let out = Command::new(VISLCG3)
        .current_dir(&dir)
        .args(["-g", "grammar.cg3", "-I", "input.txt", "--dep-delimit", "7"])
        .output()
        .expect("spawn vislcg3");
    assert!(out.status.success(), "a real number still goes through");
}

// [spec:cg3:sem:cg-conv.main-fn+1/test]
// The numbers those options do name still go through, bare `--dep-delimit`
// included.
#[test]
fn cg_conv_still_takes_numeric_option_values() {
    for args in [
        &["-W", "0.5"][..],
        &["-W", " 2"],
        &["--dep-delimit", "5"],
        &["--dep-delimit"],
    ] {
        let out = cg_conv(args, &[]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "cg-conv {args:?}: {stderr}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "\"<a>\"\n\t\"a\" N\n");
    }
}

// [spec:cg3:req:robustness.cli-arguments/test]
// [spec:cg3:sem:relabeller.cg3.relabeller.relabeller-fn+1/test]
// A relabel file holding a rule with no tag list to relabel from is refused,
// naming the rule, and no grammar is written.
#[test]
fn cg_relabel_refuses_a_rule_without_maplist() {
    let bin = temp_path("relabel-in.cg3b");
    let rules = temp_path("relabel-select.cg3r");
    let out_bin = temp_path("relabel-out.cg3b");
    let _ = std::fs::remove_file(&out_bin);
    let status = Command::new(CG_COMP)
        .arg(repo_root().join("test/T_RelabelList/grammar.cg3"))
        .arg(&bin)
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp exited with {status}");
    std::fs::write(&rules, "LIST A = a ;\nSELECT A ;\n").unwrap();

    let out = Command::new(CG_RELABEL)
        .arg(&bin)
        .arg(&rules)
        .arg(&out_bin)
        .output()
        .expect("spawn cg-relabel");
    assert_refused("cg-relabel", &out, "line 2 is a SELECT rule");
    assert!(!out_bin.exists(), "a refused relabel wrote {out_bin:?}");
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&rules);
}

/// Write a profile database with one grammar, `ast` as its grammar AST, and
/// one rule matched `num_match` times.
#[cfg(feature = "profiler")]
fn profile_db(name: &str, ast: &str, num_match: usize) -> PathBuf {
    use cg3::profiler::{ET_RULE, Key, Profiler};
    let mut p = Profiler::default();
    let gid = p.add_grammar("robust.cg3", "SELECT (a) ;\n");
    p.grammar_ast = p.add_string(ast);
    p.add_rule(1, gid, 0, 12);
    let key = Key {
        r#type: ET_RULE,
        id: 1,
    };
    p.entries.get_mut(&key).unwrap().num_match = num_match;
    let db = temp_path(name);
    p.write(db.to_str().unwrap()).expect("write profile db");
    db
}

#[cfg(feature = "profiler")]
fn run(bin: &str, args: &[&OsStr]) -> Output {
    Command::new(bin).args(args).output().expect("spawn")
}

#[cfg(feature = "profiler")]
const GOOD_AST: &str = "<Grammar u=\"2\">\n<Rule l=\"1\" b=\"0\" e=\"12\" u=\"1\"/>\n</Grammar>\n";

// [spec:cg3:req:robustness.cli-arguments/test]
// [spec:cg3:sem:cg-annotate.main-fn+1/test]
// [spec:cg3:sem:cg-merge-annotations.main-fn+1/test]
// Too few arguments to either profile tool: a usage message, not an index
// past `argv`.
#[cfg(feature = "profiler")]
#[test]
fn profile_tools_refuse_missing_arguments() {
    let a = OsStr::new("a.db");
    let cases: [(&str, &[&OsStr]); 4] = [
        (CG_ANNOTATE, &[]),
        (CG_ANNOTATE, &[a]),
        (CG_MERGE, &[]),
        (CG_MERGE, &[a]),
    ];
    for (bin, args) in cases {
        assert_refused(bin, &run(bin, args), "USAGE:");
    }
}

// [spec:cg3:req:robustness.cli-arguments/test]
// [spec:cg3:sem:cg-annotate.main-fn+1/test]
// A database that cannot be read, and an output folder that is a file or sits
// under one, are refused by cg-annotate with a message.
#[cfg(feature = "profiler")]
#[test]
fn cg_annotate_refuses_unusable_database_or_folder() {
    let db = profile_db("annotate-folder.db", GOOD_AST, 1);
    let file = temp_path("annotate-folder-file");
    std::fs::write(&file, "not a folder").unwrap();
    let under = file.join("sub");
    let missing = temp_path("annotate-no-such.db");
    let out_dir = temp_path("annotate-folder-out");
    let cases: [(&Path, &Path, &str); 4] = [
        (&missing, &out_dir, "cannot read profile database"),
        (&file, &out_dir, "cannot read profile database"),
        (&db, &file, "cannot enter output folder"),
        (&db, &under, "could not be created"),
    ];
    for (db, folder, says) in cases {
        let out = run(CG_ANNOTATE, &[db.as_os_str(), folder.as_os_str()]);
        assert_refused(&format!("cg-annotate {db:?} {folder:?}"), &out, says);
    }
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&out_dir);
}

// [spec:cg3:req:robustness.cli-arguments/test]
// [spec:cg3:sem:cg-annotate.main-fn+1/test]
// A grammar AST the report cannot be built from is refused; a multi-byte
// character after `</Grammar>` is taken whole instead of split.
#[cfg(feature = "profiler")]
#[test]
fn cg_annotate_checks_the_grammar_ast() {
    for (ast, says) in [
        ("<Grammar u=\"2\">\n", "no well-formed </Grammar>"),
        ("<Grammar u=\"x\">\n</Grammar>\n", "no well-formed u=\""),
        (
            "<Grammar u=\"2\">\n<Rule l=\"1\" b=\"0\"/>\n</Grammar>\n",
            "no well-formed e=\"",
        ),
    ] {
        let db = profile_db("annotate-ast.db", ast, 1);
        let out_dir = temp_path("annotate-ast-out");
        let out = run(CG_ANNOTATE, &[db.as_os_str(), out_dir.as_os_str()]);
        assert_refused(&format!("cg-annotate on {ast:?}"), &out, says);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    let ast = GOOD_AST.replace("</Grammar>\n", "</Grammar>é");
    let db = profile_db("annotate-utf8.db", &ast, 1);
    let out_dir = temp_path("annotate-utf8-out");
    let out = run(CG_ANNOTATE, &[db.as_os_str(), out_dir.as_os_str()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "cg-annotate: {stderr}");
    assert!(out_dir.join("g2.html").exists(), "no grammar page written");
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_dir_all(&out_dir);
}

// [spec:cg3:req:robustness.cli-arguments/test]
// [spec:cg3:sem:cg-merge-annotations.main-fn+1/test]
// Databases from different grammars, or whose counts overflow when summed,
// are refused, and nothing is written.
#[cfg(feature = "profiler")]
#[test]
fn cg_merge_refuses_databases_it_cannot_merge() {
    let base = profile_db("merge-base.db", GOOD_AST, 3);
    let other = profile_db("merge-other.db", "<Grammar u=\"3\"/>", 3);
    let huge = profile_db("merge-huge.db", GOOD_AST, usize::MAX);
    let merged = temp_path("merge-refused.db");
    let _ = std::fs::remove_file(&merged);
    for (input, says) in [
        (&other, "different grammars"),
        (&huge, "overflow when merged"),
    ] {
        let args = [merged.as_os_str(), base.as_os_str(), input.as_os_str()];
        let out = run(CG_MERGE, &args);
        assert_refused(&format!("cg-merge-annotations {input:?}"), &out, says);
        assert!(!merged.exists(), "a refused merge wrote {merged:?}");
    }
    for db in [base, other, huge] {
        let _ = std::fs::remove_file(db);
    }
}
