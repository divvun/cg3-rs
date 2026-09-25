//! Input that nests deep (`[spec:cg3:req:robustness.depth-bounded]`).
//!
//! Each input here used to overflow the stack, and a test binary runs on a
//! thread with a limited stack, so each of these tests used to abort. Depth
//! that is inherent in data — a sub-reading chain, a dependency chain, a set
//! built from sets, a trie, a `LINK` chain read from a `.cg3b` — is now walked
//! without recursion and must come out as it did. Depth that an author writes
//! is bounded by `MAX_NESTING`: refused while the grammar is read, or a run
//! error naming the rule while it runs, with the neighbouring input at the
//! limit still working.

use std::io::{Cursor, Write as _};
use std::process::{Command, Stdio};

use cg3::binary_grammar::BinaryGrammar;
use cg3::error::{Cg3Error, GrammarError, Nesting, ParseError, ParseErrorKind, RunError};
use cg3::format_converter::FormatConverter;
use cg3::grammar::GrammarCore;
use cg3::grammar_applicator::{GrammarApplicator, StreamFormatKind};
use cg3::grammar_writer::GrammarWriter;
use cg3::matxin_applicator::{MatxinApplicator, Node};
use cg3::nesting::MAX_NESTING;
use cg3::textual_parser::TextualParser;

/// Parse and reindex a textual grammar.
fn parse(src: &str) -> Result<GrammarCore, Cg3Error> {
    let mut parser = TextualParser::new(GrammarCore::default(), false);
    parser.parse_grammar_named(src.as_bytes(), "depth.cg3")?;
    let mut grammar = parser.grammar;
    let _ = grammar.reindex(false, false)?;
    Ok(grammar)
}

/// The first error a grammar is refused with.
fn refusal(src: &str) -> ParseError {
    match parse(src) {
        Err(Cg3Error::Grammar(GrammarError::Parse { mut errors, .. })) => errors.remove(0),
        Ok(_) => panic!("the grammar loaded, and must not"),
        Err(e) => panic!("expected a parse failure, got {e:?}"),
    }
}

/// Assert `src` is refused for nesting `what` too deep.
fn refused_too_deep(src: &str, what: Nesting) {
    let e = refusal(src);
    assert!(
        matches!(e.kind, ParseErrorKind::NestingTooDeep { what: w, limit } if w == what && limit == MAX_NESTING),
        "{e:?}"
    );
}

/// Run a grammar over CG text, one window however long the input.
fn run(grammar: GrammarCore, input: &str) -> Result<String, Cg3Error> {
    let mut app = GrammarApplicator::new(grammar.into());
    app.set_grammar()?;
    app.cfg.hard_limit = 1_000_000;
    app.cfg.soft_limit = 1_000_000;
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut Cursor::new(input.as_bytes().to_vec()), &mut out)?;
    Ok(String::from_utf8(out).expect("utf-8 output"))
}

/// The kind and rule line of a run stopped for nesting too deep.
fn run_too_deep(result: Result<String, Cg3Error>) -> (Nesting, u32) {
    match result {
        Err(Cg3Error::Run(RunError::NestingTooDeep { what, line, limit })) => {
            assert_eq!(limit, MAX_NESTING);
            (what, line)
        }
        other => panic!("expected the run to stop for nesting too deep, got {other:?}"),
    }
}

/// One cohort with two readings, so a `SELECT` has something to do.
const AMBIGUOUS: &str = "\"<w>\"\n\t\"a\" N x\n\t\"b\" V x\n";

/// `SELECT (N)` if a test at `position` and `links` more `LINK`ed to it
/// match.
fn link_chain(links: usize, position: &str) -> String {
    alternating_chain(links, position, position)
}

/// A `LINK` chain whose tests alternate between positions `a` and `b`.
fn alternating_chain(links: usize, a: &str, b: &str) -> String {
    let mut s = format!("SELECT (N) IF ({a} (*)");
    for i in 0..links {
        let position = if i % 2 == 0 { b } else { a };
        s.push_str(&format!(" LINK {position} (*)"));
    }
    s.push_str(") ;\n");
    s
}

// ---------------------------------------------------------------------------
// Authoring constructs, bounded while the grammar is read
// ---------------------------------------------------------------------------

// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+2/test]
#[test]
fn link_chain_past_the_limit_is_refused() {
    refused_too_deep(&link_chain(MAX_NESTING + 1, "1"), Nesting::Link);
    refused_too_deep(&link_chain(2000, "1"), Nesting::Link);
    parse(&link_chain(MAX_NESTING, "1")).expect("a chain at the limit loads");
}

// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+2/test]
#[test]
fn inline_templates_past_the_limit_are_refused() {
    let nested = |k: usize| {
        let (open, close) = ("(".repeat(k + 1), ")".repeat(k + 1));
        format!("SELECT (x) IF {open}1 (a){close} ;\n")
    };
    refused_too_deep(&nested(MAX_NESTING + 1), Nesting::InlineTemplate);
    refused_too_deep(&nested(2000), Nesting::InlineTemplate);
    parse(&nested(MAX_NESTING)).expect("templates nested to the limit load");
}

// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+2/test]
#[test]
fn template_list_past_the_limit_is_refused() {
    let list = |items: usize| format!("SELECT (x) IF ([{}(a)]) ;\n", "(a), ".repeat(items - 1));
    refused_too_deep(&list(MAX_NESTING + 2), Nesting::Link);
    refused_too_deep(&list(20_000), Nesting::Link);
    parse(&list(MAX_NESTING + 1)).expect("a list of a link per level loads");
}

// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-fn+1/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn+2/test]
#[test]
fn with_blocks_past_the_limit_are_refused() {
    let nested = |k: usize| {
        let (open, close) = ("WITH (x) {\n".repeat(k), "}\n".repeat(k));
        format!("{open}SELECT (N) ;{close}\n")
    };
    refused_too_deep(&nested(MAX_NESTING + 1), Nesting::With);
    refused_too_deep(&nested(2000), Nesting::With);
    let grammar = parse(&nested(MAX_NESTING)).expect("blocks nested to the limit load");
    let out = run(grammar, AMBIGUOUS).expect("and run");
    assert!(!out.contains("\"b\""), "the innermost rule applied: {out}");
}

// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:parser-helpers.cg3.parse-tag-fn+2/test]
#[test]
fn variable_tag_chain_past_the_limit_is_refused() {
    let chain = |k: usize| format!("LIST X = {}x ;\nSELECT X ;\n", "VAR:a=".repeat(k));
    refused_too_deep(&chain(MAX_NESTING + 1), Nesting::VariableTag);
    refused_too_deep(&chain(5000), Nesting::VariableTag);
    parse(&chain(MAX_NESTING)).expect("a chain at the limit loads");
}

// A LINK chain inside WITH blocks is as deep as both together.
// [spec:cg3:req:robustness.depth-bounded/test]
#[test]
fn nesting_kinds_count_toward_one_limit() {
    let half = MAX_NESTING / 2;
    let mixed = |with: usize, links: usize| {
        let (open, close) = ("WITH (x) {\n".repeat(with), "}\n".repeat(with));
        format!("{open}{}{close}\n", link_chain(links, "0"))
    };
    refused_too_deep(&mixed(half, half + 1), Nesting::Link);
    let grammar = parse(&mixed(half, half)).expect("the limit, between them, loads");
    run(grammar, AMBIGUOUS).expect("and runs");
}

// ---------------------------------------------------------------------------
// Authoring constructs, bounded while a rule runs
// ---------------------------------------------------------------------------

// The deepest LINK chain a textual grammar can have runs on a test thread,
// through each way a LINK is reached: the run-time limit leaves the stack
// room to spare.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1/test]
#[test]
fn link_chain_at_the_limit_runs() {
    let input = format!("\"<v>\"\n\t\"v\" V\n{AMBIGUOUS}");
    for (a, b) in [
        ("0", "0"),
        ("0C", "0C"),
        ("0*", "0*"),
        ("0**", "0**"),
        ("-1", "1"),
    ] {
        let grammar = parse(&alternating_chain(MAX_NESTING, a, b)).expect("loads");
        let out = run(grammar, &input).expect("runs");
        assert!(!out.contains("\"b\""), "{a} {b}: the chain matched");
    }
    // Down a dependency chain, a child per LINK.
    let rule = link_chain(MAX_NESTING, "c").replace("SELECT (N)", "ADD (y) (first)");
    let grammar = parse(&format!("DELIMITERS = \"<$$$>\" ;\n{rule}")).expect("loads");
    let out = run(grammar, &dependency_chain(MAX_NESTING + 2, "")).expect("runs");
    assert!(out.contains("\"w\" N first y"), "the chain matched");
}

// A chain of templates, each naming the next: every name is a level at run
// time. Five thousand of them also have to get through reindexing.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1/test]
#[test]
fn template_chain_past_the_limit_is_run_error() {
    let chain = |k: usize| {
        let mut s = String::new();
        for i in 0..k {
            s.push_str(&format!("TEMPLATE T{i} = T:T{} ;\n", i + 1));
        }
        s.push_str(&format!(
            "TEMPLATE T{k} = 0 (N) ;\nSELECT (N) IF (T:T0) ;\n"
        ));
        s
    };
    let grammar = parse(&chain(5000)).expect("loads");
    let (what, line) = run_too_deep(run(grammar, AMBIGUOUS));
    assert_eq!((what, line), (Nesting::Template, 5002));
    // The rule's own reference and each template's are a level each.
    let grammar = parse(&chain(MAX_NESTING)).expect("loads");
    run_too_deep(run(grammar, AMBIGUOUS));
    let grammar = parse(&chain(MAX_NESTING - 1)).expect("loads");
    let out = run(grammar, AMBIGUOUS).expect("a chain at the limit runs");
    assert!(!out.contains("\"b\""), "and the rule applied: {out}");
}

// The test/T_Templates idiom, a template recursing through a later OR
// alternative, recurses for as long as no earlier alternative decides.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1/test]
#[test]
fn endless_template_recursion_is_run_error() {
    let src = "TEMPLATE loop = (1 (nothing)) OR (T:loop) ;\nSELECT (x) IF (T:loop) ;\n";
    let (_, line) = run_too_deep(run(parse(src).expect("loads"), AMBIGUOUS));
    assert_eq!(line, 2);
}

// ... while recursion that an earlier alternative stops works as before.
// [spec:cg3:req:robustness.depth-bounded/test]
#[test]
fn recursive_template_within_limit_still_matches() {
    let src = "TEMPLATE scan = (1 (N)) OR (1 (A) LINK T:scan) ;\n\
               ADD (found) (w) IF (T:scan) ;\n";
    let mut input = String::from("\"<w>\"\n\t\"w\" w\n");
    for _ in 0..10 {
        input.push_str("\"<a>\"\n\t\"a\" A\n");
    }
    input.push_str("\"<n>\"\n\t\"n\" N\n");
    let out = run(parse(src).expect("loads"), &input).expect("runs");
    assert!(out.contains("\"w\" w found"), "{out}");
    let without_n = input.replace("\"n\" N", "\"n\" V");
    let out = run(parse(src).expect("loads"), &without_n).expect("runs");
    assert!(!out.contains("found"), "{out}");
}

// A LINK chain read from a `.cg3b` is as long as the file makes it. Both
// writers and the reader walk it, and running it stops at the limit.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1/test]
#[test]
fn long_link_chain_from_cg3b_is_run_error() {
    let links = 50_000;
    let text = as_text(deep_link_grammar(links));
    assert_eq!(text.matches("LINK 0 ").count(), links as usize);
    let grammar = through_cg3b(deep_link_grammar(links));
    let (what, line) = run_too_deep(run(grammar, AMBIGUOUS));
    assert_eq!((what, line), (Nesting::Link, 1));
}

/// `SELECT (x) IF (0 (*) LINK 0 (*) ...)` with `links` LINKs, built past
/// what the parser accepts, as a `.cg3b` can hold it.
fn deep_link_grammar(links: u32) -> GrammarCore {
    let mut grammar = parse("SELECT (x) IF (0 (*)) ;\n").expect("loads");
    let (&head_hash, &head) = grammar.contexts.iter().next().expect("the rule's test");
    let proto = grammar.contexts_arena[head.0].clone();
    let mut last = head;
    for i in 0..links {
        let next = grammar.allocate_contextual_test();
        let hash = head_hash.wrapping_add(0x1000_0000 + i);
        grammar.contexts_arena[next.0] = cg3::contextual_test::ContextualTest {
            hash,
            ..proto.clone()
        };
        grammar.contexts.insert(hash, next);
        grammar.contexts_arena[last.0].linked = Some(next);
        last = next;
    }
    grammar
}

// `WITH` blocks read from a `.cg3b` nest as deep as the file makes them. The
// text writer writes them all, and the parser refuses what it wrote.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-rule-fn/test]
#[test]
fn deep_with_nesting_writes_as_text() {
    let depth = 5000;
    let mut grammar = parse("WITH (x) {\nSELECT (N) ;\n}\n").expect("loads");
    let with = (0..grammar.rule_by_number.capacity())
        .find(|&i| grammar.rule_by_number[i].r#type == cg3::strings::Keywords::KWith)
        .expect("the WITH rule");
    let mut inner = grammar.rule_by_number[with].sub_rules.clone();
    for _ in 0..depth {
        let mut rule = grammar.rule_by_number[with].clone();
        rule.sub_rules = inner;
        rule.section = -3;
        let id = grammar.rule_by_number.alloc(rule);
        grammar.rule_by_number[id].number = id;
        inner = vec![cg3::arena::RuleId(id)];
    }
    grammar.rule_by_number[with].sub_rules = inner;
    let text = as_text(grammar);
    assert_eq!(text.matches("WITH KEEPORDER").count(), depth + 1);
    refused_too_deep(&text, Nesting::With);
}

// Captured text that reads as a varstring, tested in a context: the matcher
// expanded it by recursion, forever.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2/test]
#[test]
fn self_expanding_varstring_match_is_run_error() {
    let src = "DELIMITERS = \"<$.>\" ;\nADD (z) (\"<(.*)>\"r) IF (0 (VSTR:$1)) ;\n";
    let got = run(parse(src).expect("loads"), "\"<VSTR:$1>\"\n\t\"x\" N\n");
    assert!(
        matches!(
            got,
            Err(Cg3Error::Run(RunError::VarstringLoop { line: 2, .. }))
        ),
        "{got:?}"
    );
    let got = run(parse(src).expect("loads"), "\"<b>\"\n\t\"x\" N b\n").expect("runs");
    assert!(
        got.contains("\"x\" N b z"),
        "one expansion still matches: {got}"
    );
}

// Sets that name each other through `SET:` tags are followed at run time.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2/test]
#[test]
fn set_tag_chain_past_the_limit_fails() {
    let chain = |k: usize| {
        let names: Vec<String> = (0..=k).map(|i| format!("S{i}")).collect();
        let mut s = format!("STATIC-SETS = {} ;\nLIST S0 = N ;\n", names.join(" "));
        for i in 1..=k {
            s.push_str(&format!("LIST S{i} = SET:S{} ;\n", i - 1));
        }
        s.push_str(&format!("SELECT (N) IF (0 S{k}) ;\n"));
        s
    };
    let (what, _) = run_too_deep(run(parse(&chain(200)).expect("loads"), AMBIGUOUS));
    assert_eq!(what, Nesting::SetTag);
    let out = run(parse(&chain(MAX_NESTING / 2)).expect("loads"), AMBIGUOUS).expect("runs");
    assert!(!out.contains("\"b\""), "a short chain matches: {out}");
}

// ---------------------------------------------------------------------------
// Data, walked without recursion
// ---------------------------------------------------------------------------

/// Run `f` on a thread with a stack small enough that a chain a few hundred
/// long overflows it when walked by recursion, so a test can stay short when
/// a longer chain would take too long for reasons other than its depth.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("no overflow")
}

/// Two sets built from sets `k` deep: `S1 = S0 - Z`, `S2 = S1 - Z`, ... and
/// `M1 = M0 OR Z`, `M2 = M1 OR Z`, ...
fn set_chains(k: usize) -> String {
    let mut s = String::from("LIST S0 = N V ;\nLIST Z = z ;\nLIST M0 = @m ;\n");
    for i in 1..=k {
        s.push_str(&format!("SET S{i} = S{} - Z ;\n", i - 1));
        s.push_str(&format!("SET M{i} = M{} OR Z ;\n", i - 1));
    }
    s
}

/// `grammar` written out and read back as a `.cg3b`.
fn through_cg3b(grammar: GrammarCore) -> GrammarCore {
    let mut blob = Vec::new();
    BinaryGrammar::new(grammar)
        .write_binary_grammar(&mut blob)
        .expect("writes");
    let mut reader = BinaryGrammar::new(GrammarCore::default());
    reader.parse_grammar_buffer(&blob).expect("reads back");
    let mut grammar = reader.grammar;
    let _ = grammar.reindex(false, false).expect("reindexes");
    grammar
}

/// `grammar` written out as text.
fn as_text(mut grammar: GrammarCore) -> String {
    let mut text = Vec::new();
    GrammarWriter::new(&grammar).write_grammar(&mut grammar, &mut text);
    String::from_utf8(text).expect("utf-8")
}

// A set chain goes through reindexing, matching, both writers, the reader, a
// numeric branch, a mapping list and the tag list a MAP adds, and comes out
// of each as it went in.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1/test]
// [spec:cg3:sem:set.cg3.set.reindex-fn/test]
#[test]
fn long_set_chain_loads_runs_and_writes() {
    on_small_stack(|| {
        let k = 500;
        let src = format!(
            "{}SELECT (N) IF (0 S{k}) ;\nADD (f) (N) IF (1*f S{k}) ;\nMAP M{k} (N) ;\n",
            set_chains(k)
        );
        let expected = "\"<w>\"\n\t\"a\" N x z @m\n";
        let out = run(parse(&src).expect("loads"), AMBIGUOUS).expect("runs");
        assert_eq!(out, expected);
        let text = as_text(parse(&src).expect("loads"));
        let out = run(parse(&text).expect("the text loads"), AMBIGUOUS).expect("runs");
        assert_eq!(out, expected);
        let grammar = through_cg3b(parse(&src).expect("loads"));
        assert_eq!(run(grammar, AMBIGUOUS).expect("runs"), expected);
    });
}

// A composite tag is a trie as deep as it has tags: inserting it, cloning
// the set, reindexing, matching and both writers all walk it.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:tag-trie.cg3.trie-insert-fn/test]
// [spec:cg3:sem:tag-trie.cg3.trie-serialize-fn/test]
#[test]
fn deep_composite_tag_loads_runs_and_writes() {
    let tags = |k: usize| {
        (0..k)
            .map(|i| format!("t{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let deep = tags(20_000);
    let src = format!("LIST X = ({deep}) ;\nSELECT (N) IF (0 X) ;\n");
    let text = as_text(parse(&src).expect("loads"));
    let list = text
        .lines()
        .find(|l| l.contains("LIST X"))
        .expect("the set");
    let written = list
        .split(' ')
        .filter(|t| t.trim_start_matches('(').starts_with('t'));
    assert_eq!(written.count(), 20_000);
    let out = run(parse(&text).expect("the text loads"), AMBIGUOUS).expect("runs");
    assert_eq!(out, AMBIGUOUS);
    let out = run(through_cg3b(parse(&src).expect("loads")), AMBIGUOUS).expect("runs");
    assert_eq!(out, AMBIGUOUS);

    // Matching walks as deep as the reading has the tags.
    let some = tags(2000);
    let src = format!("LIST X = ({some}) ;\nSELECT (N) IF (0 X) ;\n");
    let input = format!("\"<w>\"\n\t\"a\" N x {some}\n\t\"b\" V x\n");
    let out = run(parse(&src).expect("loads"), &input).expect("runs");
    assert!(!out.contains("\"b\""), "the composite tag matched: {out}");
}

/// `cg-conv` over `input` with `args`.
fn conv(args: &[&str], input: String) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cg-conv"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cg-conv");
    let mut stdin = child.stdin.take().expect("stdin");
    let feed = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let out = child.wait_with_output().expect("wait cg-conv");
    feed.join().expect("feed");
    out
}

// An Apertium reading with ten thousand sub-readings, printed in every
// format that prints them whole.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-reading-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-reading-fn/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn/test]
#[test]
fn deep_subreading_chain_prints_in_every_format() {
    let k = 10_000;
    let input = format!("^a/{}x<n>$\n", "x<n>+".repeat(k));
    for (format, expect) in [
        ("--out-apertium", "x<n>+".repeat(k)),
        ("--out-fst", "x+n#".repeat(k)),
        ("--out-jsonl", "{\"l\":\"x\",\"s\":".repeat(k)),
        ("--out-binary", String::new()),
    ] {
        let out = conv(&["--in-apertium", format], input.clone());
        assert!(out.status.success(), "{format} exited with {}", out.status);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains(&expect), "{format} printed the whole chain");
    }
}

// The CG printer indents each sub-reading a tab further, so its output grows
// with the square of the chain; a shorter chain does.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-reading-fn/test]
#[test]
fn deep_subreading_chain_prints_as_cg() {
    let k = 3000;
    let input = format!("^a/{}x<n>$\n", "x<n>+".repeat(k));
    let out = conv(&["--in-apertium", "--out-cg"], input);
    assert!(out.status.success(), "exited with {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.matches("\"x\" n").count(), k + 1);
}

/// Little-endian bytes of a `CGBF` window body.
#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn u16(mut self, v: u16) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn str(self, s: &str) -> Self {
        let mut b = self.u16(s.len() as u16);
        b.0.extend_from_slice(s.as_bytes());
        b
    }
}

/// A `CGBF` stream of one window: one cohort, one reading, and `subs`
/// sub-readings under it.
fn binary_stream_with_subreadings(subs: u16) -> Vec<u8> {
    let mut body = Body::default()
        .u16(0)
        .u16(3)
        .str("\"<a>\"")
        .str("\"x\"")
        .str("n")
        .u16(0)
        .str("")
        .str("")
        .u16(1)
        .u16(0)
        .u16(0)
        .u16(0)
        .u32(1)
        .u32(u32::MAX)
        .u16(0)
        .str("")
        .str("")
        .u16(subs + 1);
    for i in 0..=subs {
        body = body.u16(u16::from(i > 0)).u16(1).u16(1).u16(2);
    }
    let mut s = b"CGBF".to_vec();
    s.extend_from_slice(&1u32.to_le_bytes());
    s.push(1);
    s.extend_from_slice(&(body.0.len() as u32).to_le_bytes());
    s.extend_from_slice(&body.0);
    s
}

// A binary stream reading with thirty thousand sub-readings: read, rehashed,
// printed and freed.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:reading.cg3.reading.rehash-fn/test]
// [spec:cg3:sem:reading.cg3.free-reading-fn/test]
#[test]
fn deep_subreading_chain_in_binary_stream() {
    let subs = 30_000;
    let stream = binary_stream_with_subreadings(subs);
    for output in [StreamFormatKind::Apertium, StreamFormatKind::Binary] {
        let base = GrammarApplicator::new(cg3::grammar::Grammar::default());
        let mut fc = FormatConverter::new(base).expect("conversion grammar");
        let cfg = &mut fc.base_mut().cfg;
        cfg.fmt_input = StreamFormatKind::Binary;
        cfg.fmt_output = output;
        cfg.is_conv = true;
        let mut out = Vec::new();
        fc.run_grammar_on_text(&mut Cursor::new(stream.clone()), &mut out)
            .expect("converts");
        if output == StreamFormatKind::Apertium {
            let text = String::from_utf8(out).expect("utf-8");
            assert_eq!(text.matches("x<n>").count(), usize::from(subs) + 1);
        } else {
            assert!(
                out.len() > usize::from(subs) * 6,
                "every sub-reading written"
            );
        }
    }
}

/// `k` cohorts, each the dependency child of the one before, the first
/// marked `first` and the last carrying `tail`.
fn dependency_chain(k: usize, tail: &str) -> String {
    let mut input = String::new();
    for i in 1..=k {
        let tags = match i {
            1 => " first",
            _ if i == k => tail,
            _ => "",
        };
        input.push_str(&format!("\"<w{i}>\"\n\t\"w\" N{tags} #{i}->{}\n", i - 1));
    }
    input
}

// A deep dependency test (`c*`) from the head of a five-thousand-long chain.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-dependency-test-fn/test]
#[test]
fn deep_dependency_chain_is_searched_to_the_end() {
    let src = "DELIMITERS = \"<$$$>\" ;\nADD (y) (first) IF (c* (x)) ;\n";
    let found = run(parse(src).expect("loads"), &dependency_chain(5000, " x")).expect("runs");
    assert!(
        found.contains("\"w\" N first y #1->0"),
        "found at the end of the chain"
    );
    let missing = run(parse(src).expect("loads"), &dependency_chain(5000, "")).expect("runs");
    assert!(!missing.contains(" y "), "and not where it is absent");
}

// The Matxin writer prints the dependency tree depth first; a chain a few
// thousand long is deep on a small stack.
// [spec:cg3:req:robustness.depth-bounded/test]
// [spec:cg3:sem:matxin-applicator.cg3.matxin-applicator.proc-node-fn/test]
#[test]
fn matxin_prints_a_deep_dependency_tree() {
    let k: i32 = 3000;
    let printed = on_small_stack(move || {
        let base = GrammarApplicator::new(cg3::grammar::Grammar::default());
        let matxin = MatxinApplicator::new(base);
        let mut nodes = std::collections::BTreeMap::new();
        let mut deps = std::collections::BTreeMap::new();
        for n in 1..=k {
            let node = Node {
                self_: n,
                ..Node::default()
            };
            nodes.insert(n, node);
            deps.insert(n - 1, vec![n]);
        }
        let mut out = Vec::new();
        let mut depth = 0;
        matxin.proc_node(&mut depth, &nodes, &deps, 0, &mut out);
        String::from_utf8(out).expect("utf-8")
    });
    let indent = |n: i32| " ".repeat(2 * (n as usize + 1));
    let open = |n: i32, end: &str| {
        let attrs = "alloc=\"0\" form=\"\" lem=\"\" mi=\"\" si=\"\"";
        format!("{}<NODE ord=\"{n}\" {attrs}{end}>\n", indent(n))
    };
    let mut expected = String::new();
    for n in 1..k {
        expected.push_str(&open(n, ""));
    }
    expected.push_str(&open(k, "/"));
    for n in (1..k).rev() {
        expected.push_str(&format!("{}</NODE>\n", indent(n)));
    }
    assert_eq!(printed, expected);
}
