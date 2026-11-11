//! The compile module's compiles, writers and relabelling on the host: the
//! same code the wasm build runs, minus the value conversion at the
//! JavaScript edge.

use std::path::{Path, PathBuf};

use cg3::embed::{CompiledGrammar, FileResolver, LoadOptions, MemoryResolver};
use cg3_wasm_compile::{CompileError, compile, relabel, to_text};
use cg3_wasm_shared::ErrorCode;

fn corpus(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(path)
        .canonicalize()
        .expect("a corpus path")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("a corpus file")
}

fn library(path: &Path) -> CompiledGrammar {
    let name = path.to_str().expect("a UTF-8 path");
    CompiledGrammar::from_text(&read(path), name, FileResolver, &LoadOptions::default())
        .expect("the library compiles it")
}

fn failed(source: &str, files: MemoryResolver) -> CompileError {
    compile(source, "broken.cg3", files, &LoadOptions::default()).expect_err("it fails")
}

/// T_Include, its included files given from memory under the names its
/// `INCLUDE`s resolve to, compiles to the `.cg3b` and companion the library
/// writes for it read from disk, which are cg-comp's.
// [spec:cg3:req:wasm.compile/test]
// [spec:cg3:req:wasm.compile-includes/test]
#[test]
fn includes_from_memory_compile_as_from_disk() {
    let grammar = corpus("T_Include/grammar.cg3");
    let files: MemoryResolver = ["sets.cg3", "rules.cg3"]
        .map(|name| {
            let path = corpus(&format!("T_Include/{name}"));
            (path.to_string_lossy().into_owned(), read(&path))
        })
        .into_iter()
        .collect();
    let source = String::from_utf8(read(&grammar)).expect("UTF-8");
    let name = grammar.to_str().expect("a UTF-8 path");
    let compiled = compile(&source, name, files, &LoadOptions::default()).expect("compiles");
    let expected = library(&grammar);
    assert_eq!(compiled.cg3b, expected.to_cg3b().expect("writes"));
    assert_eq!(compiled.companion, expected.companion().expect("writes"));
    assert!(compiled.companion.is_some(), "a grammar from text has one");
    assert!(compiled.warnings.is_empty(), "{:?}", compiled.warnings);
}

// [spec:cg3:req:wasm.compile/test]
#[test]
fn warnings_come_back_in_order() {
    let source = "DELIMITERS = \"<.>\" ;\nSELECT: (a) ;\nREMOVE: (b) ;\n";
    let compiled = compile(
        source,
        "warns.cg3",
        MemoryResolver::default(),
        &LoadOptions::default(),
    )
    .expect("compiles");
    assert_eq!(compiled.warnings.len(), 2, "{:?}", compiled.warnings);
    for warning in &compiled.warnings {
        assert!(warning.contains("Rule had : but no name"), "{warning}");
    }
    let prefixed = LoadOptions {
        prefix: Some('%'),
        ..LoadOptions::default()
    };
    let mapped = "DELIMITERS = \"<.>\" ;\nMAP (%x) (a) ;\n";
    let compiled = compile(mapped, "m.cg3", MemoryResolver::default(), &prefixed)
        .expect("compiles under the prefix");
    let loaded = CompiledGrammar::from_cg3b(&compiled.cg3b, &prefixed);
    assert!(
        loaded.is_ok(),
        "the .cg3b loads under the prefix it was compiled with"
    );
}

/// A failed parse reports every error it found, each with its source, line,
/// rendered text without colour, and span in UTF-16 code units.
// [spec:cg3:req:wasm.compile-errors/test]
#[test]
fn a_failed_parse_carries_every_error() {
    let source = "DELIMITERS = \"<.>\" ;\n# \u{1F600}\nSELECT (a ;\nLIST X = ;\n";
    let error = failed(source, MemoryResolver::default());
    assert_eq!(error.code(), Some(ErrorCode::CompileFailed));
    let diagnostics = error.diagnostics();
    assert!(diagnostics.len() >= 2, "{diagnostics:?}");
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.source, "broken.cg3");
        assert!(
            !diagnostic.text.contains('\u{1b}'),
            "no colour: {diagnostic:?}"
        );
        assert!(
            diagnostic.text.contains(&diagnostic.message),
            "{diagnostic:?}"
        );
        assert!(
            error
                .message()
                .is_some_and(|m| m.contains(&diagnostic.message)),
            "the message lists it"
        );
    }
    let first = &diagnostics[0];
    assert_eq!(first.line, 3);
    let span = first.span.clone().expect("placed");
    let units: Vec<u16> = source.encode_utf16().collect();
    let marked = String::from_utf16(&units[span]).expect("whole characters");
    assert!(
        source
            .lines()
            .nth(2)
            .is_some_and(|line| line.contains(marked.trim()))
    );
}

// [spec:cg3:req:wasm.compile-includes/test]
// [spec:cg3:req:wasm.compile-errors/test]
#[test]
fn an_include_not_given_fails_naming_the_file() {
    let grammar = corpus("T_Include/grammar.cg3");
    let source = String::from_utf8(read(&grammar)).expect("UTF-8");
    let name = grammar.to_str().expect("a UTF-8 path");
    let given: MemoryResolver = [(corpus("T_Include/sets.cg3"), b"LIST ASet = a ;\n")]
        .into_iter()
        .map(|(path, bytes)| (path.to_string_lossy().into_owned(), bytes.to_vec()))
        .collect();
    let error = compile(&source, name, given, &LoadOptions::default())
        .expect_err("rules.cg3 is on disk but was not given");
    assert_eq!(error.code(), Some(ErrorCode::CompileFailed));
    let message = error.message().unwrap_or_default();
    assert!(message.contains("rules.cg3"), "{message}");
    assert!(
        error
            .diagnostics()
            .iter()
            .any(|d| d.text.contains("rules.cg3")),
        "{:?}",
        error.diagnostics()
    );
}

// [spec:cg3:req:wasm.compile/test]
#[test]
fn to_text_is_what_the_library_writes() {
    for name in ["T_Select", "T_Templates", "T_Dependency"] {
        let grammar = library(&corpus(&format!("{name}/grammar.cg3")));
        let cg3b = grammar.to_cg3b().expect("writes");
        let loaded = CompiledGrammar::from_cg3b(&cg3b, &LoadOptions::default()).expect("loads");
        assert_eq!(
            to_text(&cg3b).expect("writes text"),
            loaded.to_text(),
            "{name}"
        );
    }
}

/// Relabel rules read as cg-relabel reads its rule file: the same rules as
/// text or as a `.cg3b` relabel a grammar to the library's bytes.
// [spec:cg3:req:wasm.compile/test]
#[test]
fn relabel_takes_its_rules_as_text_or_cg3b() {
    let grammar = library(&corpus("T_RelabelList/grammar.cg3"));
    let rules = library(&corpus("T_RelabelList/relabel.cg3r"));
    let cg3b = grammar.to_cg3b().expect("writes");
    let read = CompiledGrammar::from_cg3b(&cg3b, &LoadOptions::default()).expect("loads");
    let expected = read
        .relabel(&rules)
        .and_then(|g| g.to_cg3b())
        .expect("relabels");
    let text = std::fs::read(corpus("T_RelabelList/relabel.cg3r")).expect("the rules");
    let as_text = relabel(&cg3b, &text).map_err(|e| e.message().map(str::to_owned));
    assert_eq!(as_text.as_ref(), Ok(&expected));
    let rules_cg3b = rules.to_cg3b().expect("writes");
    let as_cg3b = relabel(&cg3b, &rules_cg3b).map_err(|e| e.message().map(str::to_owned));
    assert_eq!(as_cg3b, Ok(expected));
}

// [spec:cg3:req:wasm.compile-errors/test]
#[test]
fn other_failures_follow_the_package_codes() {
    let code = |error: CompileError| error.code();
    assert_eq!(
        to_text(&[1, 2, 3]).map_err(|e| e.code()),
        Err(ErrorCode::InvalidGrammar)
    );
    let cg3b = library(&corpus("T_Select/grammar.cg3"))
        .to_cg3b()
        .expect("writes");
    assert_eq!(
        relabel(b"not a cg3b", &cg3b).map_err(code),
        Err(Some(ErrorCode::InvalidGrammar))
    );
    assert_eq!(
        relabel(&cg3b, b"MAP (a ;").map_err(code),
        Err(Some(ErrorCode::CompileFailed)),
        "rules that do not parse are a failed compile"
    );
    assert_eq!(
        relabel(&cg3b, b"CG3B\x01\x02").map_err(code),
        Err(Some(ErrorCode::InvalidGrammar))
    );
}
