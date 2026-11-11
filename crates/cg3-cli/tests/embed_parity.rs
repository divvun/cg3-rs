//! vislcg3 and a program loading the same `.cg3b` through `cg3::embed` see the
//! same grammar, vislcg3 runs it through the steps the library offers, and
//! cg-comp and vislcg3 write the bytes the library's writers return.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cg3::embed::{CompiledGrammar, FileResolver, GrammarInfo};

#[expect(
    dead_code,
    reason = "the conversions compare bytes exactly, so diff_b_equal and run_capture go unused here"
)]
mod common;

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

/// Every corpus grammar's source, by its path.
fn corpus_grammars() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test");
    let mut grammars: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("grammar.cg3"))
        .filter(|path| path.is_file() && path.to_string_lossy().contains("/T_"))
        .collect();
    grammars.sort();
    assert!(
        grammars.len() > 50,
        "only {} corpus grammars",
        grammars.len()
    );
    grammars
}

fn scratch(grammar: &Path, what: &str) -> PathBuf {
    let dir = grammar
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    std::env::temp_dir().join(format!("cg3-writers-{dir}-{what}-{}", std::process::id()))
}

/// The file at `path` and its companion, removed once read.
fn take_written(path: &Path) -> (Vec<u8>, Option<Vec<u8>>) {
    let companion = cg3::grammar_sources::sidecar_path(path);
    let written = (std::fs::read(path).unwrap(), std::fs::read(&companion).ok());
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(companion);
    written
}

/// `vislcg3 --grammar-only` loading `grammar`, writing it to `text` and `binary`.
fn vislcg3_writes(grammar: &Path, text: &Path, binary: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .args(["--grammar-only", "-g"])
        .arg(grammar)
        .arg("--grammar-out")
        .arg(text)
        .arg("--grammar-bin")
        .arg(binary)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn vislcg3");
    assert!(status.success(), "vislcg3 {}", grammar.display());
}

/// cg-comp writes, for every corpus grammar, the `.cg3b` and companion file
/// the library's writers return for the grammar `from_text` loads.
// [spec:cg3:req:embed.write-binary/test]
// [spec:cg3:req:embed.cli-compiles-through/test]
#[test]
fn cg_comp_writes_what_to_cg3b_returns() {
    for grammar in corpus_grammars() {
        let out = scratch(&grammar, "comp.cg3b");
        let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
            .arg(&grammar)
            .arg(&out)
            .stderr(Stdio::null())
            .status()
            .expect("spawn cg-comp");
        assert!(status.success(), "cg-comp {}", grammar.display());
        let loaded = CompiledGrammar::from_text(
            &std::fs::read(&grammar).unwrap(),
            grammar.to_str().unwrap(),
            FileResolver,
        )
        .unwrap();
        let expected = (loaded.to_cg3b().unwrap(), loaded.companion().unwrap());
        assert!(take_written(&out) == expected, "{}", grammar.display());
    }
}

/// vislcg3 writes `--grammar-out` and `--grammar-bin` as `to_text`, `to_cg3b`
/// and `companion` return them, for a grammar it loads from text and for one
/// it loads from a `.cg3b`, which has no companion.
// [spec:cg3:req:embed.write-binary/test]
// [spec:cg3:req:embed.write-text/test]
// [spec:cg3:req:embed.cli-compiles-through/test]
#[test]
fn vislcg3_writes_what_the_library_writers_return() {
    for grammar in corpus_grammars() {
        let text = CompiledGrammar::from_text(
            &std::fs::read(&grammar).unwrap(),
            grammar.to_str().unwrap(),
            FileResolver,
        )
        .unwrap();
        let cg3b = scratch(&grammar, "in.cg3b");
        std::fs::write(&cg3b, text.to_cg3b().unwrap()).unwrap();
        let binary = CompiledGrammar::from_cg3b(&text.to_cg3b().unwrap()).unwrap();
        for (path, loaded) in [(grammar.as_path(), &text), (cg3b.as_path(), &binary)] {
            let (text_out, binary_out) =
                (scratch(&grammar, "out.cg3"), scratch(&grammar, "out.cg3b"));
            vislcg3_writes(path, &text_out, &binary_out);
            let written = take_written(&text_out);
            assert!(
                written == (loaded.to_text().into_bytes(), None),
                "{}",
                path.display()
            );
            let expected = (loaded.to_cg3b().unwrap(), loaded.companion().unwrap());
            assert!(take_written(&binary_out) == expected, "{}", path.display());
        }
        let _ = std::fs::remove_file(cg3b);
    }
}

/// cg-conv writes what `cg3::convert` returns for the same input and options,
/// for inputs in each format and the options that change its output.
// [spec:cg3:req:embed.convert/test]
#[test]
fn cg_conv_writes_what_convert_returns() {
    use cg3::embed::Format;
    use cg3::{ConvertOptions, convert};
    let o = ConvertOptions::default;
    let cases: Vec<(&[&str], ConvertOptions)> = vec![
        (&[], o()),
        (
            &["--in-cg", "--out-apertium"],
            ConvertOptions {
                input: Some(Format::Cg),
                output: Some(Format::Apertium),
                ..o()
            },
        ),
        (
            &["--out-fst", "-W", "2", "--wtag", "X", "-S", "#"],
            ConvertOptions {
                output: Some(Format::Fst),
                wfactor: Some(2.0),
                wtag: Some("X".into()),
                sub_delim: Some("#".into()),
                ..o()
            },
        ),
        (
            &["--out-niceline", "--deleted"],
            ConvertOptions {
                output: Some(Format::Niceline),
                deleted: true,
                ..o()
            },
        ),
        (
            &["--out-plain", "--no-break"],
            ConvertOptions {
                output: Some(Format::Plain),
                no_break: true,
                ..o()
            },
        ),
        (
            &["--out-jsonl", "--unicode-tags"],
            ConvertOptions {
                output: Some(Format::Jsonl),
                unicode_tags: true,
                ..o()
            },
        ),
        (
            &["--out-binary"],
            ConvertOptions {
                output: Some(Format::Binary),
                ..o()
            },
        ),
        (
            &["--add-tags"],
            ConvertOptions {
                add_tags: true,
                ..o()
            },
        ),
        (
            &["--add-tags", "--in-auto"],
            ConvertOptions {
                add_tags: true,
                in_auto: true,
                ..o()
            },
        ),
        (
            &["--in-plain", "--in-jsonl"],
            ConvertOptions {
                input: Some(Format::Plain),
                ..o()
            },
        ),
        (
            &["--out-apertium", "--ordered", "--ltr", "--prefix", "%"],
            ConvertOptions {
                output: Some(Format::Apertium),
                ordered: true,
                ltr: true,
                prefix: Some('%'),
                ..o()
            },
        ),
        (
            &["--parse-dep"],
            ConvertOptions {
                parse_dep: true,
                ..o()
            },
        ),
        (
            &["--dep-delimit", "3"],
            ConvertOptions {
                dep_delimit: Some(3),
                ..o()
            },
        ),
        (
            &["--in-apertium"],
            ConvertOptions {
                input: Some(Format::Apertium),
                ..o()
            },
        ),
    ];
    let root = common::repo_root().join("test");
    for input in [
        "T_Jsonl/input.txt",
        "T_Jsonl/input-rules.jsonl",
        "T_Dependency/input.txt",
        "Apertium/T_MultiWords/input.txt",
    ] {
        let bytes = std::fs::read(root.join(input)).unwrap();
        for (args, options) in &cases {
            let out = common::run_output(
                env!("CARGO_BIN_EXE_cg-conv"),
                args,
                &root,
                &bytes,
                Stdio::null(),
            );
            let expected = convert(&bytes, options).map_err(|e| e.to_string());
            assert_eq!(out.status.success(), expected.is_ok(), "{input} {args:?}");
            assert!(
                expected.is_err() || out.stdout == expected.unwrap(),
                "{input} {args:?}"
            );
        }
    }
}

/// T_Include compiled from its files held in memory gives the `.cg3b` and
/// companion file cg-comp writes for it run beside them, where the companion
/// names each source as the grammar's includes do.
// [spec:cg3:req:embed.include-resolver/test]
#[test]
fn memory_includes_compile_as_cg_comp_does() {
    let dir = common::repo_root().join("test/T_Include");
    let files: cg3::embed::MemoryResolver = ["sets.cg3", "rules.cg3"]
        .into_iter()
        .map(|name| (name, std::fs::read(dir.join(name)).unwrap()))
        .collect();
    let source = std::fs::read(dir.join("grammar.cg3")).unwrap();
    let grammar = CompiledGrammar::from_text(&source, "grammar.cg3", files).unwrap();

    let out = scratch(&dir.join("grammar.cg3"), "memory.cg3b");
    let status = Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .current_dir(&dir)
        .arg("grammar.cg3")
        .arg(&out)
        .stderr(Stdio::null())
        .status()
        .expect("spawn cg-comp");
    assert!(status.success());
    let expected = (grammar.to_cg3b().unwrap(), grammar.companion().unwrap());
    assert!(take_written(&out) == expected);
}
