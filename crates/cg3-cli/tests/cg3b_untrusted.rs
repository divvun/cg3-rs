//! A `.cg3b` is untrusted bytes, and the tools treat it so: vislcg3 reports a
//! damaged one and exits nonzero, and loads an undamaged one as it runs the
//! grammar's source. The library's own `cg3b_untrusted` tests damage the
//! records one at a time.

use cg3::binary_grammar::BinaryGrammar;
use cg3::grammar::GrammarDraft;
use cg3::textual_parser::TextualParser;

// ---------------------------------------------------------------------------
// Fixtures and harness.
// ---------------------------------------------------------------------------

/// A grammar with one of nearly everything the format stores: varstring,
/// variable and context-reference tags, parentheses, preferred targets,
/// reopen-mappings, anchors, set operators, a template, OR'd and LINKed
/// tests, a relation, barriers, a dependency target and a WITH block.
const GRAMMAR: &str = r#"
DELIMITERS = "<.>" ;
SOFT-DELIMITERS = "<,>" ;
PREFERRED-TARGETS = n ;
PARENTHESES = ("<(>" "<)>") ;
REOPEN-MAPPINGS = @x ;
LIST N = n ;
LIST V = v ;
LIST A = a ;
LIST CC = cc ;
LIST R = "<r.*>"r ;
SET NV = N OR V ;
SET NA = N - A ;
SET NR = N + R ;
TEMPLATE nn = (1 N) ;
TEMPLATE anywhere = (? N) ;
BEFORE-SECTIONS
ADD (@x) N ;
SECTION
SELECT NV IF (T:nn) ;
SELECT V IF ((1 N) OR (-1 NA)) ;
REMOVE A IF (1 N LINK 1* V BARRIER CC) (-1* N CBARRIER NR) ;
ADD:named (@v) V (0 (VAR:x=y)) ;
ADD (@r) (*) (r:rel (n)) ;
ADD (VSTR:@$1{N}) V (0 ("<(.*)>"r)) ;
SUBSTITUTE (n) (n q) N ;
SETPARENT V TO (1 N) ;
REMOVE V IF (-1* T:anywhere) ;
ANCHOR here ;
WITH V IF (1 N) {
  MAP (@m) _C1_ ;
} ;
"#;

fn compile(src: &str) -> Vec<u8> {
    let mut parser = TextualParser::new(GrammarDraft::default(), false);
    parser
        .parse_grammar_utf8(src.as_bytes())
        .expect("fixture grammar compiles");
    let grammar = parser.grammar.finish().expect("fixture reindexes");
    let mut blob = Vec::new();
    BinaryGrammar::new(grammar)
        .write_binary_grammar(&mut blob)
        .expect("fixture writes");
    blob
}

/// A grammar with more sections than 1023 has as many rules, and loads from
/// the `.cg3b` cg-comp writes for it, running as it does from its source.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
// [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn+3/test]
#[test]
fn many_sections_load_from_their_cg3b() {
    let dir = std::env::temp_dir().join(format!("cg3-sections-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("sections.cg3");
    let mut grammar = String::from("DELIMITERS = \"<$.>\" ;\n");
    for i in 0..1100 {
        grammar.push_str(&format!("SECTION\nREMOVE (x{i}) ;\n"));
    }
    std::fs::write(&source, grammar).unwrap();
    let compiled = dir.join("sections.cg3b");
    let comp = std::process::Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .arg(&source)
        .arg(&compiled)
        .output()
        .expect("spawn cg-comp");
    assert!(
        comp.status.success(),
        "{}",
        String::from_utf8_lossy(&comp.stderr)
    );
    let run = |grammar: &std::path::Path| {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_vislcg3"))
            .arg("-g")
            .arg(grammar)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn vislcg3");
        let mut stdin = child.stdin.take().unwrap();
        std::io::Write::write_all(&mut stdin, b"\"<a>\"\n\t\"a\" N\n").unwrap();
        drop(stdin);
        child.wait_with_output().expect("wait for vislcg3")
    };
    let (from_source, from_cg3b) = (run(&source), run(&compiled));
    let _ = std::fs::remove_dir_all(&dir);
    let stderr = String::from_utf8_lossy(&from_cg3b.stderr);
    assert!(from_cg3b.status.success(), "{stderr}");
    assert_eq!(from_source.stdout, from_cg3b.stdout);
}

/// The command line reports a truncated `.cg3b` and exits nonzero, without
/// a panic.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn vislcg3_reports_a_truncated_grammar() {
    let blob = compile(GRAMMAR);
    let path = std::env::temp_dir().join(format!("cg3-untrusted-{}.cg3b", std::process::id()));
    std::fs::write(&path, &blob[..blob.len() / 2]).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vislcg3"))
        .arg("-g")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn vislcg3");
    let _ = std::fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("ends early"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}
