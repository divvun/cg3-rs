//! vislcg3 and a program loading the same `.cg3b` through `cg3::embed` see the
//! same grammar, and vislcg3 runs it through the steps the library offers.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cg3::embed::{CompiledGrammar, GrammarInfo};

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
        .join("grammar.cg3")
}

fn compiled(name: &str) -> PathBuf {
    let out = std::env::temp_dir().join(format!("cg3-embed-{name}-{}.cg3b", std::process::id()));
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .arg(corpus(name))
        .arg(&out)
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp {name}");
    out
}

/// What `vislcg3 -v` says about a grammar it has loaded.
fn verbose_summary(grammar: &Path) -> GrammarInfo {
    let out = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .args(["-v", "--grammar-only", "-g"])
        .arg(grammar)
        .stdin(Stdio::null())
        .output()
        .expect("spawn vislcg3");
    assert!(out.status.success());
    let log = String::from_utf8(out.stderr).unwrap();
    let counts = log
        .lines()
        .find_map(|l| l.split_once("Grammar has ").map(|(_, rest)| rest))
        .filter(|rest| rest.contains("sections,"))
        .expect("the summary line");
    let numbers: Vec<usize> = counts
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .collect();
    let rules_any = log
        .lines()
        .find_map(|l| l.split_once(" rules cannot be skipped by index."))
        .map_or(0, |(head, _)| {
            head.rsplit(' ').next().unwrap().parse().unwrap()
        });
    GrammarInfo {
        sections: numbers[0],
        templates: numbers[1],
        rules: numbers[2],
        sets: numbers[3],
        tags: numbers[4],
        rules_any,
        has_dependency_rules: log.contains("Grammar has dependency rules."),
        has_relation_rules: log.contains("Grammar has relation rules."),
    }
}

// [spec:cg3:req:embed.info/test]
#[test]
fn info_is_what_vislcg3_reports() {
    for name in [
        "T_Dependency",
        "T_Templates",
        "T_MultipleSections",
        "T_Relations",
    ] {
        let path = compiled(name);
        let info = CompiledGrammar::from_cg3b(&std::fs::read(&path).unwrap())
            .expect("the .cg3b loads")
            .info();
        let reported = verbose_summary(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(info, reported, "{name}");
    }
}

// [spec:cg3:req:embed.cli-runs-through+1/test]
#[test]
fn bad_option_fails_before_input_is_read() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .arg("-g")
        .arg(corpus("T_Select"))
        .args(["--num-windows", "abc"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn vislcg3");
    // Standard input stays open: a run that read it first would wait forever.
    let _stdin = child.stdin.take();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("vislcg3 waited for input before reporting the bad option");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success());
}
