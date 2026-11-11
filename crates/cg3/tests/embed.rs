//! A program loads a `.cg3b` from bytes and runs it as vislcg3 does: the
//! corpus grammars, compiled in memory, give each test's expected output
//! through `CompiledGrammar::run`, with the typed options its `args.txt` names.
//! A textual grammar loaded through `CompiledGrammar::from_text` runs as the
//! `.cg3b` compiled from it does.

use std::path::{Path, PathBuf};

use cg3::embed::{CompiledGrammar, FileResolver, Format, RunContext, RunOptions};

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
}

/// The `.cg3b` cg-comp writes for a corpus grammar.
fn cg3b(name: &str) -> Vec<u8> {
    let path = corpus(name).join("grammar.cg3");
    let text = std::fs::read(&path).unwrap();
    CompiledGrammar::from_text(&text, path.to_str().unwrap(), FileResolver)
        .expect("corpus grammar loads")
        .to_cg3b()
        .expect("corpus grammar writes")
}

fn input(name: &str) -> Vec<u8> {
    std::fs::read(corpus(name).join("input.txt")).unwrap()
}

/// The harness compares as `diff -b` does, ignoring blank lines.
fn same_output(expected: &str, got: &str) -> bool {
    let lines = |s: &str| -> Vec<String> {
        s.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    };
    lines(expected) == lines(got)
}

fn assert_runs_as_expected(name: &str, options: &RunOptions) {
    let grammar = CompiledGrammar::from_cg3b(&cg3b(name)).expect("the .cg3b loads");
    let got = grammar
        .run(&input(name), options)
        .expect("the grammar runs");
    let expected = std::fs::read_to_string(corpus(name).join("expected.txt")).unwrap();
    assert!(
        same_output(&expected, &String::from_utf8(got).unwrap()),
        "{name}: output differs from expected.txt"
    );
}

// [spec:cg3:req:embed.load/test]
// [spec:cg3:req:embed.run/test]
#[test]
fn corpus_grammars_run_from_their_cg3b_bytes() {
    for name in [
        "T_Active",
        "T_Barrier",
        "T_ContextTest",
        "T_Iff",
        "T_MultipleSections",
        "T_SetOps",
        "T_Templates",
        "T_Variables",
    ] {
        assert_runs_as_expected(name, &RunOptions::default());
    }
}

// [spec:cg3:req:embed.run-options/test]
#[test]
fn typed_options_act_as_their_command_line_flags() {
    let cases: [(&str, RunOptions); 11] = [
        (
            "T_Select",
            RunOptions {
                show_end_tags: true,
                ..Default::default()
            },
        ),
        (
            "T_Sections",
            RunOptions {
                sections: Some("4".into()),
                ..Default::default()
            },
        ),
        (
            "T_SectionRanges",
            RunOptions {
                sections: Some("1,4-6,3".into()),
                ..Default::default()
            },
        ),
        (
            "T_SoftDelimiters",
            RunOptions {
                soft_limit: Some(4),
                hard_limit: Some(6),
                ..Default::default()
            },
        ),
        (
            "T_Trace",
            RunOptions {
                trace: Some(String::new()),
                ..Default::default()
            },
        ),
        (
            "T_Unification",
            RunOptions {
                trace_name_only: true,
                ..Default::default()
            },
        ),
        (
            "T_Parentheses",
            RunOptions {
                trace_encl: true,
                ..Default::default()
            },
        ),
        (
            "T_OriginPassing",
            RunOptions {
                no_pass_origin: true,
                ..Default::default()
            },
        ),
        (
            "T_Dependency_Loops",
            RunOptions {
                dep_no_crossing: true,
                ..Default::default()
            },
        ),
        (
            "T_AnyMinusSome",
            RunOptions {
                unique_tags: true,
                ..Default::default()
            },
        ),
        (
            "T_MapAdd_Different",
            RunOptions {
                trace: Some(String::new()),
                split_mappings: true,
                ..Default::default()
            },
        ),
    ];
    for (name, options) in &cases {
        assert_runs_as_expected(name, options);
    }
}

// [spec:cg3:req:embed.grammar-options/test]
#[test]
fn a_grammars_cmdargs_reach_its_run() {
    // T_CmdArgs's grammar sets `CMDARGS += --num-windows 4`, and its expected
    // output depends on it.
    assert_runs_as_expected("T_CmdArgs", &RunOptions::default());

    let grammar = CompiledGrammar::from_cg3b(&cg3b("T_CmdArgs")).unwrap();
    let unmerged = RunOptions::default().to_table();
    let mut without = Vec::new();
    grammar
        .prepare(&unmerged, RunContext::default())
        .unwrap()
        .run(&input("T_CmdArgs"), &mut without)
        .unwrap();
    let with = grammar
        .run(&input("T_CmdArgs"), &RunOptions::default())
        .unwrap();
    assert_ne!(with, without, "the CMDARGS window count changes the output");

    // CMDARGS give a value only to an option the caller left unset.
    let explicit = RunOptions {
        num_windows: Some(2),
        ..Default::default()
    };
    let mut table = explicit.to_table();
    cg3::options_parser::merge_grammar_options(&mut table, &grammar.core().cmdargs, "", None);
    assert_eq!(table[cg3::options::Opt::NumWindows as usize].value, "2");
}

// [spec:cg3:req:embed.runs-independent/test]
#[test]
fn runs_share_nothing_but_the_grammar() {
    let grammar = CompiledGrammar::from_cg3b(&cg3b("T_Variables")).unwrap();
    let options = RunOptions::default();
    let first = grammar.run(&input("T_Variables"), &options).unwrap();
    let other = grammar.run(&input("T_Barrier"), &options).unwrap();
    assert_eq!(grammar.run(&input("T_Variables"), &options).unwrap(), first);

    let threads: Vec<_> = (0..4)
        .map(|i| {
            let grammar = grammar.clone();
            let options = options.clone();
            std::thread::spawn(move || {
                let name = if i % 2 == 0 {
                    "T_Variables"
                } else {
                    "T_Barrier"
                };
                (i, grammar.run(&input(name), &options).unwrap())
            })
        })
        .collect();
    for t in threads {
        let (i, out) = t.join().unwrap();
        assert_eq!(&out, if i % 2 == 0 { &first } else { &other });
    }
}

// [spec:cg3:req:embed.load/test]
#[test]
fn partial_or_foreign_bytes_fail_to_load() {
    let blob = cg3b("T_Barrier");
    let truncated = CompiledGrammar::from_cg3b(&blob[..blob.len() / 2]);
    let message = truncated
        .err()
        .expect("half a .cg3b is refused")
        .to_string();
    assert!(message.contains("ends early"), "{message}");
    assert!(CompiledGrammar::from_cg3b(b"CG3B").is_err());
    assert!(CompiledGrammar::from_cg3b(b"not a grammar").is_err());
}

// [spec:cg3:req:embed.run-options/test]
#[test]
fn formats_name_their_stream_options() {
    use cg3::options::Opt;
    let table = RunOptions {
        input: Some(Format::Apertium),
        output: Some(Format::Jsonl),
        ..Default::default()
    }
    .to_table();
    assert!(table[Opt::InApertium as usize].does_occur);
    assert!(table[Opt::OutJsonl as usize].does_occur);
    // vislcg3 writes CG when no `--out-*` is given; there is no `--out-cg`.
    let cg = RunOptions {
        output: Some(Format::Cg),
        ..Default::default()
    }
    .to_table();
    assert_eq!(
        cg.iter().filter(|o| o.does_occur).count(),
        0,
        "asking for CG output sets nothing"
    );
}

/// Every corpus grammar's path and source, but those whose `EXTERNAL` rules
/// would start a process.
fn textual_corpus() -> Vec<(String, PathBuf, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test");
    let mut grammars: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|dir| {
            dir.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("T_"))
                && dir.join("grammar.cg3").is_file()
                && dir.join("input.txt").is_file()
        })
        .map(|dir| {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            let path = dir.join("grammar.cg3");
            let source = std::fs::read(&path).unwrap();
            (name, path, source)
        })
        .filter(|(_, _, source)| {
            !String::from_utf8_lossy(source)
                .lines()
                .any(|l| l.trim_start().starts_with("EXTERNAL"))
        })
        .collect();
    grammars.sort();
    grammars
}

// [spec:cg3:req:embed.load-text/test]
#[test]
fn a_textual_grammar_runs_as_its_cg3b_does() {
    let corpus = textual_corpus();
    assert!(corpus.len() > 50, "only {} corpus grammars", corpus.len());
    let options = RunOptions::default();
    for (name, path, source) in &corpus {
        let text = CompiledGrammar::from_text(source, path.to_str().unwrap(), FileResolver)
            .unwrap_or_else(|e| panic!("{name}: the textual grammar loads: {e}"));
        let binary = CompiledGrammar::from_cg3b(&cg3b(name)).expect("the .cg3b loads");
        let input = input(name);
        match (text.run(&input, &options), binary.run(&input, &options)) {
            (Ok(a), Ok(b)) => assert!(a == b, "{name}: the outputs differ"),
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string(), "{name}"),
            (a, b) => panic!("{name}: text gives {:?}, .cg3b {:?}", a.err(), b.err()),
        }
    }
}

// [spec:cg3:req:embed.load-text/test]
// [spec:cg3:req:embed.include-resolver/test]
#[test]
fn a_relative_include_resolves_against_the_named_file() {
    let path = corpus("T_Include").join("grammar.cg3");
    let source = std::fs::read(&path).unwrap();
    let grammar = CompiledGrammar::from_text(&source, path.to_str().unwrap(), FileResolver)
        .expect("T_Include's includes are found beside it");
    let got = grammar
        .run(&input("T_Include"), &RunOptions::default())
        .unwrap();
    let expected = std::fs::read_to_string(corpus("T_Include").join("expected.txt")).unwrap();
    assert!(same_output(&expected, &String::from_utf8(got).unwrap()));

    let elsewhere = corpus("T_Active").join("grammar.cg3");
    let refused = CompiledGrammar::from_text(&source, elsewhere.to_str().unwrap(), FileResolver)
        .err()
        .expect("T_Active's directory has no sets.cg3");
    let cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::Parse { errors, .. }) = refused
    else {
        panic!("a parse failure, not {refused}");
    };
    assert!(errors[0].to_string().contains("sets.cg3"), "{errors:?}");
}

// [spec:cg3:req:embed.load-text/test]
#[test]
fn a_failed_parse_carries_its_errors_and_source() {
    use cg3::error::{Cg3Error, GrammarError};
    let source = b"DELIMITERS = \"<.>\" ;\nSELECT NoSuchSet ;\nREMOVE AlsoMissing ;\n";
    let error = CompiledGrammar::from_text(source, "broken.cg3", FileResolver)
        .err()
        .expect("undefined sets are refused");
    let Cg3Error::Grammar(GrammarError::Parse { errors, sources }) = error else {
        panic!("a parse failure, not {error}");
    };
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "broken.cg3");
    assert_eq!(sources[0].text.as_bytes(), source);
}

/// The writers read a grammar and leave it as it was: every corpus grammar,
/// loaded from text or from its `.cg3b`, writes the same bytes twice, and runs
/// after the writes as it ran before them.
// [spec:cg3:req:embed.write-binary/test]
// [spec:cg3:req:embed.write-text/test]
#[test]
fn writing_a_grammar_leaves_it_unchanged() {
    let options = RunOptions::default();
    for (name, path, source) in &textual_corpus() {
        let text =
            CompiledGrammar::from_text(source, path.to_str().unwrap(), FileResolver).unwrap();
        let cg3b = text.to_cg3b().unwrap();
        let binary = CompiledGrammar::from_cg3b(&cg3b).unwrap();
        assert_eq!(
            binary.companion().unwrap(),
            None,
            "{name}: a .cg3b has no sources"
        );
        let input = input(name);
        for grammar in [&text, &binary] {
            let before = grammar.run(&input, &options).map_err(|e| e.to_string());
            let written = (
                grammar.to_cg3b().unwrap(),
                grammar.companion().unwrap(),
                grammar.to_text(),
            );
            let again = (
                grammar.to_cg3b().unwrap(),
                grammar.companion().unwrap(),
                grammar.to_text(),
            );
            assert!(written == again, "{name}: a second write differs");
            let after = grammar.run(&input, &options).map_err(|e| e.to_string());
            assert_eq!(before, after, "{name}: a run after the writes differs");
        }
        assert!(
            text.companion().unwrap().is_some(),
            "{name}: the sources were kept"
        );
    }
}

/// `cg3::convert` converts T_Jsonl's stream to JSONL and back as its upstream
/// expectations say cg-conv does, detects the format when none is named, and
/// reads plain text for `add_tags`.
// [spec:cg3:req:embed.convert/test]
#[test]
fn convert_gives_cg_convs_expected_streams() {
    use cg3::{ConvertOptions, convert};
    let dir = corpus("T_Jsonl");
    let read = |name: &str| std::fs::read(dir.join(name)).unwrap();
    let options = |input, output| ConvertOptions {
        input,
        output: Some(output),
        parse_dep: true,
        deleted: true,
        ..Default::default()
    };
    let jsonl = convert(
        &read("input.txt"),
        &options(Some(Format::Cg), Format::Jsonl),
    )
    .unwrap();
    assert!(jsonl == read("expected.cg-to-jsonl.txt"), "CG to JSONL");
    for named in [Some(Format::Jsonl), None] {
        let cg = convert(&jsonl, &options(named, Format::Cg)).unwrap();
        assert!(
            cg == read("expected.jsonl-to-cg.txt"),
            "JSONL to CG, {named:?}"
        );
    }

    let tagged = ConvertOptions {
        add_tags: true,
        ..Default::default()
    };
    let words = String::from_utf8(convert(b"Two words\n", &tagged).unwrap()).unwrap();
    assert!(words.contains("\"<Two>\""), "{words}");
    assert!(words.contains("\t\"two\""), "{words}");
}

/// T_Include's files, held in memory under the names its grammar gives them.
fn include_files() -> cg3::embed::MemoryResolver {
    ["grammar.cg3", "sets.cg3", "rules.cg3"]
        .into_iter()
        .map(|name| (name, std::fs::read(corpus("T_Include").join(name)).unwrap()))
        .collect()
}

/// A grammar whose includes are held in memory compiles and runs as it does
/// from its files, with each include named as vislcg3 names it beside the
/// grammar; an include the files do not hold fails the parse naming it.
// [spec:cg3:req:embed.include-resolver/test]
#[test]
fn includes_resolve_from_files_held_in_memory() {
    let path = corpus("T_Include").join("grammar.cg3");
    let source = std::fs::read(&path).unwrap();
    let from_memory = CompiledGrammar::from_text(&source, "grammar.cg3", include_files()).unwrap();
    let from_disk =
        CompiledGrammar::from_text(&source, path.to_str().unwrap(), FileResolver).unwrap();
    assert!(from_memory.to_cg3b().unwrap() == from_disk.to_cg3b().unwrap());
    let options = RunOptions::default();
    let input = input("T_Include");
    assert_eq!(
        from_memory.run(&input, &options).unwrap(),
        from_disk.run(&input, &options).unwrap()
    );
    let names: Vec<String> = from_memory
        .core()
        .sources
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(names, ["grammar.cg3", "./sets.cg3", "./rules.cg3"]);

    let only_sets: cg3::embed::MemoryResolver = [(
        "sets.cg3",
        std::fs::read(corpus("T_Include").join("sets.cg3")).unwrap(),
    )]
    .into_iter()
    .collect();
    let refused = CompiledGrammar::from_text(&source, "grammar.cg3", only_sets)
        .err()
        .expect("rules.cg3 is not among the files");
    let cg3::error::Cg3Error::Grammar(cg3::error::GrammarError::Parse { errors, .. }) = refused
    else {
        panic!("a parse failure, not {refused}");
    };
    assert!(
        matches!(&errors[0].kind, cg3::error::ParseErrorKind::IncludeUnreadable { path, .. } if path == "./rules.cg3"),
        "{errors:?}"
    );
}

/// A grammar loaded from text quotes its rule when a run fails in it, out of
/// the text it parsed: here there is no file to read, since its include came
/// from memory.
// [spec:cg3:req:embed.include-resolver/test]
#[test]
fn a_runtime_failure_quotes_the_kept_sources() {
    let lists = "LIST N = (n) ;\n\nSECTION\nADD (@ok) N ;\nADD (VSTR:$1) (\"<\\(.*\\)>\"r) ;\n";
    let files: cg3::embed::MemoryResolver = [("lists.cg3", lists)].into_iter().collect();
    let grammar = CompiledGrammar::from_text(
        b"DELIMITERS = \"<$.>\" ;\nINCLUDE lists.cg3 ;\n",
        "nowhere/nb.cg3",
        files,
    )
    .unwrap();
    let error = grammar
        .run(b"\"<(x>\"\n\t\"x\" n\n", &RunOptions::default())
        .expect_err("the tag cannot be built");
    let cg3::error::Cg3Error::Run(cg3::error::RunError::TagConstruction {
        source, sources, ..
    }) = error
    else {
        panic!("a tag construction failure, not {error}");
    };
    assert_eq!(source.file, "lists.cg3", "{source:?}");
    assert!(
        sources
            .iter()
            .any(|s| s.name == "lists.cg3" && s.text == lists)
    );
}
