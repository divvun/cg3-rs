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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
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

// [spec:cg3:sem:main.main-fn/test]
// The full vislcg3 main: option parsing (args.txt), textual grammar load,
// reindex, and the applicator run over test/T_Select's input, byte-checked
// against the fixture's expected.txt (runall.pl sub-test 1).
#[test]
fn vislcg3_main_runs_t_select() {
    let dir = repo_root().join("test/T_Select");
    run_vislcg3_expect(&dir, Path::new("grammar.cg3"), "vislcg3-select.txt");
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
    assert!(head.len() > 4 && &head[..4] == b"CG3B", "missing CG3B magic");
    run_vislcg3_expect(&dir, &bin, "comp-select.txt");
    let _ = std::fs::remove_file(&bin);
}

// [spec:cg3:sem:cg-comp.end-program-fn/test]
// cg-comp's endProgram: wrong argc (no args) prints the version + usage banner
// to stdout and exits EXIT_FAILURE.
#[test]
fn cg_comp_end_program_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .output()
        .expect("spawn cg-comp");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("VISL CG-3 Compiler version"), "missing banner: {stdout}");
    assert!(stdout.contains("USAGE: cg-comp grammar_file output_file"), "missing usage: {stdout}");
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
    assert_eq!(got, "\"<word>\"\n\t\"word\" N Sg\n\"<birds>\"\n\t\"bird\" N Pl\n");
}

// [spec:cg3:sem:cg-proc.main-fn/test]
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
    assert!(diff_b_equal(&want, &got), "cg-proc output differs from expected.txt");
}

// [spec:cg3:sem:cg-proc.end-program-fn/test]
// cg-proc's endProgram: with no grammar argument main falls through to the
// usage path — version + option summary on stdout, exit EXIT_FAILURE.
#[test]
fn cg_proc_end_program_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_cg-proc"))
        .output()
        .expect("spawn cg-proc");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("VISL CG-3 Disambiguator version"), "missing banner: {stdout}");
    assert!(stdout.contains("USAGE: cg-proc"), "missing usage: {stdout}");
    assert!(stdout.contains("--stream-format"), "missing option list: {stdout}");
}

// [spec:cg3:sem:cg-relabel.main-fn/test]
// [spec:cg3:sem:cg-relabel.cg3-grammar-load-fn/test]
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
    assert!(head.len() > 4 && &head[..4] == b"CG3B", "missing CG3B magic");

    run_vislcg3_expect(&dir, &bin_out, "relabel-select.txt");
    let _ = std::fs::remove_file(&bin);
    let _ = std::fs::remove_file(&bin_out);
}

// [spec:cg3:sem:cg-relabel.end-program-fn/test]
// cg-relabel's endProgram: wrong argc prints the version + usage banner to
// stdout and exits EXIT_FAILURE.
#[test]
fn cg_relabel_end_program_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_cg-relabel"))
        .output()
        .expect("spawn cg-relabel");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("VISL CG-3 Relabeller version"), "missing banner: {stdout}");
    assert!(
        stdout.contains("USAGE: cg-relabel input_grammar_file relabel_rule_file output_grammar_file"),
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
    assert!(out.status.success(), "cg-mwesplit exited with {}", out.status);
    let got = String::from_utf8_lossy(&out.stdout);
    let want = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    assert!(diff_b_equal(&want, &got), "cg-mwesplit output differs from expected.txt");
}

/// Build a profiler database the way `vislcg3 --profile` + the parser wiring
/// would: one grammar (fname + source), an AST string (interned as the
/// grammar_ast, stored under key 0), one rule + one context entry with hit
/// counts, example windows, and a rule->context link. Returns
/// `(db_path, grammar_text)`.
fn write_profile_db(name: &str, num_match: usize, with_example: bool) -> (PathBuf, String) {
    use cg3::profiler::{Key, Profiler, ET_CONTEXT, ET_RULE};

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
        let e = p.entries.get_mut(&Key { r#type: ET_RULE, id: 1 }).unwrap();
        e.num_match = num_match;
        e.num_fail = 1;
        if with_example {
            e.example_window = window;
        }
    }
    {
        let e = p.entries.get_mut(&Key { r#type: ET_CONTEXT, id: 7 }).unwrap();
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
    assert!(index.contains(r#"<a href="g2.html">profile-grammar.cg3</a>"#), "index: {index}");

    // The annotated grammar page: rule span + stats + xml-escaped source.
    let g = std::fs::read_to_string(out_dir.join("g2.html")).unwrap();
    assert!(g.contains(r#"<span class="cg-elem cgRule">"#), "g2: {g}");
    assert!(g.contains(r#"class="entry good"><span class="stats">M:3, F:1"#), "g2: {g}");
    assert!(g.contains(r#"class="entry context good"><span class="stats">M:3"#), "g2: {g}");
    assert!(g.contains("(&quot;&lt;w&gt;&quot;)"), "xml_encode missing: {g}");

    // Usage-example pages for the rule and the context, with the escaped
    // example window (file_save wrote them into rs/ and cs/).
    let rs = std::fs::read_to_string(out_dir.join("rs/1.html")).unwrap();
    assert!(rs.contains("SELECT (wanted) IF (0 (&quot;&lt;w&gt;&quot;)) ;"), "rs: {rs}");
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
#[test]
fn cg_merge_annotations_main_sums_counts() {
    use cg3::profiler::{Key, Profiler, ET_CONTEXT, ET_RULE};

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
    assert!(status.success(), "cg-merge-annotations exited with {status}");

    let mut merged = Profiler::default();
    merged.read(merged_db.to_str().unwrap()).expect("read merged db");

    let rule = merged.entries[&Key { r#type: ET_RULE, id: 1 }];
    assert_eq!(rule.num_match, 3 + 5, "rule matches not summed");
    assert_eq!(rule.num_fail, 1 + 1, "rule fails not summed");
    assert_ne!(rule.example_window, 0, "missing example window not adopted");

    let ctx = merged.entries[&Key { r#type: ET_CONTEXT, id: 7 }];
    assert_eq!(ctx.num_match, 3 + 5, "context matches not summed");

    assert_eq!(merged.rule_contexts[&(1, 7)], 3 + 5, "rule_contexts not summed");

    let _ = std::fs::remove_file(&base_db);
    let _ = std::fs::remove_file(&in_db);
    let _ = std::fs::remove_file(&merged_db);
}
