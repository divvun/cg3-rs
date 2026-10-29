//! A program loads a `.cg3b` from bytes and runs it as vislcg3 does: the
//! corpus grammars, compiled in memory, give each test's expected output
//! through `CompiledGrammar::run`, with the typed options its `args.txt` names.

use std::path::{Path, PathBuf};

use cg3::binary_grammar::BinaryGrammar;
use cg3::embed::{CompiledGrammar, Format, RunContext, RunOptions};
use cg3::grammar::GrammarDraft;
use cg3::textual_parser::TextualParser;

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
}

/// The `.cg3b` cg-comp writes for a corpus grammar.
fn cg3b(name: &str) -> Vec<u8> {
    let path = corpus(name).join("grammar.cg3");
    let text = std::fs::read(&path).unwrap();
    let mut parser = TextualParser::new(GrammarDraft::default(), false);
    parser
        .parse_grammar_named(&text, path.to_str().unwrap())
        .expect("corpus grammar parses");
    let mut blob = Vec::new();
    BinaryGrammar::new(parser.grammar.finish().expect("corpus grammar finishes"))
        .write_binary_grammar(&mut blob)
        .expect("corpus grammar writes");
    blob
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
