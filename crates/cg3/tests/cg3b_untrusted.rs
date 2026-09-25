//! A `.cg3b` is untrusted bytes, and the reader treats it so.
//!
//! Each test compiles a grammar the ordinary way, decodes the `.cg3b` into
//! its records with the small codec below, damages one thing, re-encodes it
//! and loads the result. What loads must survive reindexing, both writers and
//! a run; what cannot must be refused with an error naming the fault and the
//! byte it sits at — never a panic, a hang or an allocation the file chose.

use std::io::Cursor;

use cg3::binary_grammar::BinaryGrammar;
use cg3::error::{BinaryFault, Cg3Error, GrammarError};
use cg3::grammar::GrammarCore;
use cg3::grammar_applicator::GrammarApplicator;
use cg3::grammar_writer::GrammarWriter;
use cg3::textual_parser::TextualParser;

#[path = "cg3b_untrusted/codec.rs"]
mod codec;

use codec::{BINF_PREFIX, BINF_TAGS, Cg3b, Entry, Field, Rec, Wr};

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

const INPUT: &[u8] = b"\"<a>\"\n\t\"a\" n\n\"<b>\"\n\t\"b\" v\n\t\"b\" a\n\"<,>\"\n\t\",\" cc\n\"<r>\"\n\t\"r\" n\n\"<.>\"\n\t\".\" punct\n";

fn compile(src: &str) -> Vec<u8> {
    let mut parser = TextualParser::new(GrammarCore::default(), false);
    parser
        .parse_grammar_utf8(src.as_bytes())
        .expect("fixture grammar compiles");
    let mut grammar = parser.grammar;
    let _ = grammar.reindex(false, false).expect("fixture reindexes");
    let mut blob = Vec::new();
    BinaryGrammar::new(grammar)
        .write_binary_grammar(&mut blob)
        .expect("fixture writes");
    blob
}

fn base() -> Cg3b {
    let blob = compile(GRAMMAR);
    let model = Cg3b::decode(&blob);
    assert_eq!(model.encode(), blob, "the test codec round-trips");
    model
}

fn load(blob: &[u8]) -> Result<GrammarCore, Cg3Error> {
    let mut parser = BinaryGrammar::new(GrammarCore::default());
    parser.parse_grammar_buffer(blob)?;
    Ok(parser.grammar)
}

fn loaded(blob: &[u8]) -> GrammarCore {
    let mut g = load(blob).unwrap_or_else(|e| panic!("a well-formed grammar loads: {e}"));
    let _ = g.reindex(false, false).expect("a loaded grammar reindexes");
    g
}

/// Load, reindex, write both ways, and run: everything a loaded grammar
/// must survive.
fn exercise(blob: &[u8]) {
    let mut g = loaded(blob);
    let mut text = Vec::new();
    assert_eq!(GrammarWriter::new(&g).write_grammar(&mut g, &mut text), 0);
    let mut bin = Vec::new();
    BinaryGrammar::new(loaded(blob))
        .write_binary_grammar(&mut bin)
        .expect("a loaded grammar writes");
    let mut app = GrammarApplicator::new(loaded(blob).into());
    app.set_grammar().expect("applicator setup");
    let mut out = Vec::new();
    app.run_grammar_on_text(&mut Cursor::new(INPUT.to_vec()), &mut out)
        .expect("a loaded grammar runs");
    assert!(!out.is_empty());
}

/// One way of damaging a decoded grammar or record.
type Damage<T> = Box<dyn Fn(&mut T)>;

/// The grammar error a `.cg3b` is refused with.
fn refusal(blob: &[u8]) -> GrammarError {
    match load(blob) {
        Err(Cg3Error::Grammar(e)) => e,
        Err(e) => panic!("refused with a non-grammar error: {e}"),
        Ok(_) => panic!("a damaged grammar loaded"),
    }
}

/// The fault a `.cg3b` is refused for; the offset must lie inside the file.
fn fault(model: &Cg3b) -> BinaryFault {
    let blob = model.encode();
    match refusal(&blob) {
        GrammarError::BinaryMalformed { offset, fault } => {
            assert!(offset < blob.len(), "offset {offset} past the end");
            fault
        }
        e => panic!("expected a malformed-grammar error, got {e}"),
    }
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

/// The fixture itself loads and survives everything a run does to it.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn well_formed_grammar_loads_and_runs() {
    exercise(&base().encode());
}

/// Every strict prefix of a real `.cg3b` is refused as truncated, rather
/// than read as zeros into a grammar that panics once it runs.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
// [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn+1/test]
#[test]
fn every_truncation_is_refused_as_truncated() {
    // Without the regex tags, whose compilation would dominate thousands of
    // loads.
    let plain = GRAMMAR
        .replace(r#""<r.*>"r"#, r#""<r>""#)
        .replace(r#"("<(.*)>"r)"#, r#"("<b>")"#);
    let blob = compile(&plain);
    for cut in 0..blob.len() {
        match refusal(&blob[..cut]) {
            GrammarError::TruncatedHeader => assert!(cut < 4, "cut {cut}"),
            GrammarError::Truncated { offset, .. } | GrammarError::CountPastEnd { offset, .. } => {
                assert!(offset <= cut, "cut {cut}: offset {offset}")
            }
            e => panic!("cut {cut}: {e}"),
        }
    }
}

/// A length or count is checked against the bytes left before anything is
/// sized by it: a 20-byte file cannot ask for 4 GiB.
// [spec:cg3:req:robustness.allocation-bounded/test]
#[test]
fn huge_length_and_count_allocate_nothing() {
    let mut head = b"CG3B".to_vec();
    head.extend_from_slice(&13898u32.to_be_bytes());
    let mut prefix = head.clone();
    prefix.extend_from_slice(&BINF_PREFIX.to_be_bytes());
    prefix.extend_from_slice(&u32::MAX.to_be_bytes());
    match refusal(&prefix) {
        GrammarError::Truncated {
            needed, remaining, ..
        } => {
            assert_eq!((needed, remaining), (u64::from(u32::MAX), 0))
        }
        e => panic!("{e}"),
    }
    let mut tags = head;
    tags.extend_from_slice(&BINF_TAGS.to_be_bytes());
    tags.extend_from_slice(&[0; 8]); // empty CMDARGS and CMDARGS-OVERRIDE
    tags.extend_from_slice(&u32::MAX.to_be_bytes());
    match refusal(&tags) {
        GrammarError::CountPastEnd { count, .. } => assert_eq!(count, u32::MAX),
        e => panic!("{e}"),
    }
}

// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn tag_numbers_are_in_range_and_unique() {
    let mut m = base();
    let n = m.tags.len() as u32;
    m.tags[1].set(0, Field::U32(n));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "tag number",
            ..
        }
    ));

    // Two records claiming one number; the first carries varstring sets and
    // the second, which resolving them would reach, has none.
    let mut m = base();
    let vs = Cg3b::with(&m.tags, 10);
    let other = (vs + 1) % m.tags.len();
    let number = m.tags[vs].u32(0);
    m.tags[other].set(0, Field::U32(number));
    assert!(matches!(
        fault(&m),
        BinaryFault::Duplicate {
            what: "tag number",
            ..
        }
    ));
}

// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn tag_hashes_are_unique_and_unreserved() {
    let mut m = base();
    let hash = m.tags[0].u32(1);
    m.tags[1].set(1, Field::U32(hash));
    assert!(matches!(
        fault(&m),
        BinaryFault::Duplicate {
            what: "tag hash",
            ..
        }
    ));

    for reserved in [u32::MAX, u32::MAX - 1] {
        let mut m = base();
        m.tags[2].set(1, Field::U32(reserved));
        assert!(
            matches!(fault(&m), BinaryFault::ReservedHash { .. }),
            "{reserved:#x}"
        );
    }
}

/// The two union roles a tag record can store must be the role its type
/// gives it: the engine reads a role through the type and refuses a value
/// stored for another.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn tag_role_bits_must_match_type() {
    let base = base();
    let var = Cg3b::with(&base.tags, 13);
    let ctx = Cg3b::with(&base.tags, 14);
    let plain = base.tag("n");
    let value = Field::U32(base.tags[var].u32(13));
    let (v1, v2) = (value.clone(), value);
    let cases: Vec<Damage<Cg3b>> = vec![
        // A variable value on a tag that is not a variable.
        Box::new(move |m| m.tags[plain].set(13, v1.clone())),
        // A context position on a tag that is not a context reference.
        Box::new(move |m| m.tags[plain].set(14, Field::U32(1))),
        // A context reference with no position, or position 0.
        Box::new(move |m| m.tags[ctx].clear(14)),
        Box::new(move |m| m.tags[ctx].set(14, Field::U32(0))),
        // Both roles at once.
        Box::new(move |m| m.tags[ctx].set(13, v2.clone())),
    ];
    for (i, damage) in cases.iter().enumerate() {
        let mut m = base.clone();
        damage(&mut m);
        assert!(matches!(fault(&m), BinaryFault::TagRole { .. }), "case {i}");
    }

    // A variable value must name a tag.
    let mut m = base.clone();
    m.tags[var].set(13, Field::U32(0x1234_5678));
    assert!(matches!(
        fault(&m),
        BinaryFault::UnknownTag {
            what: "tag variable value",
            ..
        }
    ));
}

/// Tables of tag hashes must name tags: reindexing and the grammar writer
/// look every one of them up.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn tag_hash_tables_must_name_tags() {
    let unknown = 0x0bad_cafe;
    let cases: [fn(&mut Cg3b, u32); 5] = [
        |m, h| m.preferred[0] = h,
        |m, h| m.parens[0].0 = h,
        |m, h| m.parens[0].1 = h,
        |m, h| m.reopen[0] = h,
        |m, h| m.anchors[0].0 = h,
    ];
    for (i, damage) in cases.iter().enumerate() {
        let mut m = base();
        damage(&mut m, unknown);
        assert!(
            matches!(fault(&m), BinaryFault::UnknownTag { .. }),
            "case {i}"
        );
    }
    let mut m = base();
    m.anchors[0].1 = m.rules.len() as u32 + 1;
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "anchor position",
            ..
        }
    ));
}

// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn set_numbers_are_in_range_and_unique() {
    let mut m = base();
    let n = m.sets.len() as u32;
    m.sets[1].set(0, Field::U32(n));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "set number",
            ..
        }
    ));

    let mut m = base();
    let number = m.sets[1].u32(0);
    m.sets[2].set(0, Field::U32(number));
    assert!(matches!(
        fault(&m),
        BinaryFault::Duplicate {
            what: "set number",
            ..
        }
    ));

    let mut m = base();
    let c = m.composite_set();
    m.sets[c].set(5, Field::List(vec![1, n]));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "member set",
            ..
        }
    ));

    let mut m = base();
    m.delims[0] = Some(n);
    assert!(matches!(fault(&m), BinaryFault::OutOfRange { .. }));

    let mut m = base();
    let vs = Cg3b::with(&m.tags, 10);
    m.tags[vs].set(10, Field::List(vec![n]));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "varstring set",
            ..
        }
    ));
}

/// Set operators must be ones the matcher and the writer implement, one
/// between each pair of member sets.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn set_operators_are_checked_against_range() {
    let mut m = base();
    let c = m.composite_set();
    m.sets[c].set(4, Field::List(vec![200]));
    assert!(matches!(
        fault(&m),
        BinaryFault::SetOperator { op: 200, .. }
    ));

    let mut m = base();
    let c = m.composite_set();
    m.sets[c].set(4, Field::List(vec![]));
    assert!(matches!(
        fault(&m),
        BinaryFault::SetOperatorCount { ops: 0, .. }
    ));

    // A unified set unifies over its first member; a LIST has none.
    let mut m = base();
    let list = Cg3b::with(&m.sets, 3);
    m.sets[list].clear(2);
    m.sets[list].set(1, Field::U32(u32::from(cg3::set::ST_SET_UNIFY.bits())));
    assert!(matches!(fault(&m), BinaryFault::EmptyUnifiedSet { .. }));
}

// [spec:cg3:req:robustness.binary-grammar-validated/test]
// [spec:cg3:sem:grammar.cg3.trie-unserialize-fn+1/test]
#[test]
fn trie_tag_numbers_are_checked() {
    let mut m = base();
    let n = m.tags.len() as u32;
    let list = Cg3b::with(&m.sets, 3);
    m.sets[list].set(3, Field::Tries(vec![Entry(n, 1, vec![])], vec![]));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "trie tag",
            ..
        }
    ));
}

/// A trie nested far deeper than any call stack is read, and freed,
/// without recursing; cut short inside, it is refused the same way.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
// [spec:cg3:sem:grammar.cg3.trie-unserialize-fn+1/test]
#[test]
fn deep_trie_neither_overflows_nor_leaks() {
    const DEPTH: u32 = 100_000;
    let mut tries = Wr::default();
    tries.u32(1);
    for level in 0..=DEPTH {
        let last = level == DEPTH;
        tries.u32(0); // tag 0
        tries.0.push(u8::from(last));
        tries.u32(u32::from(!last));
    }
    tries.u32(0); // no special trie
    let mut m = base();
    let list = Cg3b::with(&m.sets, 3);
    m.sets[list].set(3, Field::Raw(tries.0));
    let blob = m.encode();
    drop(load(&blob).expect("a deep trie loads"));
    let cut = blob.len() / 2;
    match refusal(&blob[..cut]) {
        GrammarError::Truncated { .. } | GrammarError::CountPastEnd { .. } => {}
        e => panic!("{e}"),
    }
}

/// Every set number a contextual test holds names a set, and its relation
/// names a tag.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn context_references_are_checked() {
    let n = base().sets.len() as u32;
    for bit in [4, 7, 8] {
        let mut m = base();
        let i = Cg3b::with(&m.contexts, 4);
        m.contexts[i].set(bit, Field::U32(n));
        assert!(
            matches!(fault(&m), BinaryFault::OutOfRange { .. }),
            "bit {bit}"
        );
    }
    let mut m = base();
    let i = Cg3b::with(&m.contexts, 6);
    m.contexts[i].set(6, Field::U32(0x0bad_cafe));
    assert!(matches!(
        fault(&m),
        BinaryFault::UnknownTag {
            what: "test relation",
            ..
        }
    ));

    let mut m = base();
    m.contexts[0].clear(0);
    assert_eq!(fault(&m), BinaryFault::ContextWithoutHash);

    let mut m = base();
    let hash = m.contexts[0].u32(0);
    m.contexts[1].set(0, Field::U32(hash));
    assert!(matches!(
        fault(&m),
        BinaryFault::Duplicate {
            what: "contextual test hash",
            ..
        }
    ));
}

/// Context hashes — a rule's tests and dependency target, a test's template,
/// OR'd tests and LINK — must name a test the file defines.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
// [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn+1/test]
// [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-fn+1/test]
#[test]
fn context_hashes_must_name_tests() {
    let unknown = 0x0bad_cafe;
    let cases: [fn(&mut Cg3b, u32); 6] = [
        |m, h| m.rules[0].tests.push(h),
        |m, h| m.rules[0].dep_tests.push(h),
        |m, h| m.rules.iter_mut().find(|r| r.dep != 0).unwrap().dep = h,
        |m, h| {
            let i = Cg3b::with(&m.contexts, 3);
            m.contexts[i].set(3, Field::U32(h))
        },
        |m, h| {
            let i = Cg3b::with(&m.contexts, 10);
            m.contexts[i].set(10, Field::List(vec![h]))
        },
        |m, h| {
            let i = Cg3b::with(&m.contexts, 11);
            m.contexts[i].set(11, Field::U32(h))
        },
    ];
    for (i, damage) in cases.iter().enumerate() {
        let mut m = base();
        damage(&mut m, unknown);
        assert!(
            matches!(fault(&m), BinaryFault::UnknownContext { .. }),
            "case {i}"
        );
    }
}

// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn rule_numbers_and_references_are_checked() {
    let n_rules = base().rules.len() as u32;
    let n_sets = base().sets.len() as u32;
    let n_tags = base().tags.len() as u32;
    let cases: Vec<(u32, u32, &str)> = vec![
        (14, n_rules, "rule number"),
        (5, n_sets, "rule target set"),
        (10, n_sets, "rule child set"),
        (11, n_sets, "rule child set"),
        (12, n_sets, "rule tag list set"),
        (13, n_sets, "rule sub-list set"),
        (6, n_tags, "rule wordform tag"),
    ];
    for (bit, value, what) in cases {
        let mut m = base();
        m.rules[1].head.set(bit, Field::U32(value));
        match fault(&m) {
            BinaryFault::OutOfRange { what: w, .. } => assert_eq!(w, what),
            f => panic!("bit {bit}: {f}"),
        }
    }

    let mut m = base();
    let number = m.rules[1].head.u32(14);
    m.rules[2].head.set(14, Field::U32(number));
    assert!(matches!(
        fault(&m),
        BinaryFault::Duplicate {
            what: "rule number",
            ..
        }
    ));

    let mut m = base();
    let with = m.rule_with(15);
    m.rules[with].head.set(15, Field::List(vec![n_rules]));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "sub-rule",
            ..
        }
    ));
}

/// Rule types, comparison operators and sections are checked against their
/// ranges; the highest section sizes what reindexing and every run do.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn keyword_and_section_ranges_are_checked() {
    let mut m = base();
    m.rules[0].head.set(1, Field::U32(9999));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "rule type",
            ..
        }
    ));

    for section in [-4i32, 0x7fff_ffff] {
        let mut m = base();
        m.rules[0].head.set(0, Field::U32(section as u32));
        assert!(
            matches!(fault(&m), BinaryFault::Section { .. }),
            "{section}"
        );
    }

    let mut m = base();
    m.tags[0].set(6, Field::U32(99));
    assert!(matches!(
        fault(&m),
        BinaryFault::OutOfRange {
            what: "comparison operator",
            ..
        }
    ));
}

/// What the grammar writer and the engine dereference without asking: the
/// tag list SUBSTITUTE works on, and the tag an EXTERNAL command names.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn rules_carry_what_their_type_needs() {
    let substitute = cg3::strings::Keywords::KSubstitute as u32;
    let mut m = base();
    let i = m
        .rules
        .iter()
        .position(|r| r.head.u32(1) == substitute)
        .unwrap();
    m.rules[i].head.clear(13);
    assert!(matches!(fault(&m), BinaryFault::MissingSublist { .. }));

    let mut m = base();
    m.rules[0]
        .head
        .set(1, Field::U32(cg3::strings::Keywords::KExternalOnce as u32));
    assert!(matches!(
        fault(&m),
        BinaryFault::UnknownTag {
            what: "rule variable name",
            ..
        }
    ));
}

/// Cycles grammar source cannot write: a set among its own members, tests
/// that OR or LINK back to themselves, a rule among its own WITH sub-rules.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn structural_cycles_are_refused() {
    let mut m = base();
    let c = m.composite_set();
    let own = m.sets[c].u32(0);
    m.sets[c].set(5, Field::List(vec![own, own]));
    assert!(matches!(fault(&m), BinaryFault::SetCycle { .. }));

    let mut m = base();
    let i = Cg3b::with(&m.contexts, 10);
    let own = m.contexts[i].u32(0);
    m.contexts[i].set(10, Field::List(vec![own]));
    assert!(matches!(fault(&m), BinaryFault::ContextCycle { .. }));

    let mut m = base();
    let with = m.rule_with(15);
    let own = m.rules[with].head.u32(14);
    m.rules[with].head.set(15, Field::List(vec![own]));
    assert!(matches!(fault(&m), BinaryFault::RuleCycle { .. }));
}

/// Ten thousand tags on consecutive hashes leave the tag interner no seed
/// for a new tag whose hash falls at their start; one fewer leaves it one.
/// Each crowding tag is `foo` at its own seed, so every stored hash is still
/// the one the tag's text and seed give it.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn crowded_tag_hashes_are_refused() {
    let with_run = |len: u32| {
        let mut m = base();
        let first = cg3::inlines::hash_value_str("foo", 0);
        let start = m.tags.len() as u32;
        for i in 0..len {
            let mut t = Rec::default();
            t.set(0, Field::U32(start + i));
            t.set(1, Field::U32(first.wrapping_add(i)));
            t.set(2, Field::U32(first));
            if i > 0 {
                t.set(3, Field::U32(i));
            }
            t.set(8, Field::Bytes(b"foo".to_vec()));
            m.tags.push(t);
        }
        m
    };
    assert!(matches!(
        fault(&with_run(10_000)),
        BinaryFault::HashRun { len: 10_000, .. }
    ));
    load(&with_run(9_999).encode()).expect("a run one short of the probe loads");
}

/// The command line reports a truncated `.cg3b` and exits nonzero, without
/// a panic.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn vislcg3_reports_a_truncated_grammar() {
    let blob = base().encode();
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

/// A `?` position loads where a template override stands in for it, as the
/// fixture's `-1* T:anywhere` does, and is refused where a run would reach it
/// bare: from a rule directly, or through a reference without the override.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn unknown_position_needs_an_override() {
    const UNKNOWN: u64 = 1 << 25;
    const OVERRIDE: u64 = 1 << 24;
    let base = base();
    let bare = base
        .contexts
        .iter()
        .position(|c| c.fields.get(&1) == Some(&Field::Pos(UNKNOWN)))
        .expect("the `?` template body");
    let hash = base.contexts[bare].u32(0);

    let mut m = base.clone();
    m.rules[0].tests.push(hash);
    assert!(matches!(fault(&m), BinaryFault::UnknownPosition { .. }));

    // The named template wraps the `?` test in a one-alternative OR; the
    // test that uses the template carries the override.
    let mut m = base.clone();
    let wraps = |c: &&Rec| c.fields.get(&10) == Some(&Field::List(vec![hash]));
    let template = m.contexts.iter().find(wraps).expect("the template").u32(0);
    let user = m
        .contexts
        .iter()
        .position(|c| c.u32(3) == template)
        .expect("its user");
    let Some(Field::Pos(pos)) = m.contexts[user].fields.get(&1).cloned() else {
        panic!("the user has a position")
    };
    assert_ne!(pos & OVERRIDE, 0);
    m.contexts[user].set(1, Field::Pos(pos & !OVERRIDE));
    assert!(matches!(fault(&m), BinaryFault::UnknownPosition { .. }));
}

/// A tag's stored hashes must be the ones its text, type and seed give it: a
/// run finds tags by recomputing them. A type bit that changes the hash — a
/// varstring that would then expand to itself forever, a `SET:` reference —
/// or a changed text is refused.
// [spec:cg3:req:robustness.binary-grammar-validated/test]
#[test]
fn tag_hashes_must_match_their_content() {
    let varstring = cg3::tag::T_VARSTRING.bits();
    let set = cg3::tag::T_SET.bits();
    let base = base();
    let plain = base.tag("n");
    let cases: [(&str, Damage<Rec>); 4] = [
        (
            "hash",
            Box::new(move |t| t.set(4, Field::U32(t.u32(4) | varstring))),
        ),
        (
            "hash",
            Box::new(move |t| t.set(4, Field::U32(t.u32(4) | set))),
        ),
        ("hash", Box::new(|t| t.set(8, Field::Bytes(b"m".to_vec())))),
        (
            "plain hash",
            Box::new(|t| t.set(2, Field::U32(t.u32(2) ^ 1))),
        ),
    ];
    for (i, (which, damage)) in cases.iter().enumerate() {
        let mut m = base.clone();
        damage(&mut m.tags[plain]);
        match fault(&m) {
            BinaryFault::TagHash { what, .. } => assert_eq!(what, *which, "case {i}"),
            f => panic!("case {i}: {f}"),
        }
    }
}
