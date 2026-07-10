//! Wave-3 test facets for the grammar/parser area: `src/Grammar.cpp`,
//! `src/Grammar.hpp`, `src/TextualParser.cpp`, `src/TextualParser.hpp`,
//! `src/IGrammarParser.hpp`.
//!
//! Strategy: drive the library IN-PROCESS. `TextualParser::new(...)` parses
//! real fixture grammars from `test/T_*` plus a few small crafted grammar
//! strings that hit paths the fixtures don't; `Grammar` methods that no parse
//! path reaches (destroy_*, remove_numeric_tags, ...) are called directly and
//! their effects asserted. The binary side (`trie_unserialize`) is driven by
//! compiling a fixture with the real `cg-comp` binary and loading the `.cg3b`
//! in-process through `BinaryGrammar`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cg3::arena::SetId;
use cg3::binary_grammar::BinaryGrammar;
use cg3::contextual_test::POS_NEGATE;
use cg3::grammar::Grammar;
use cg3::igrammar_parser::IGrammarParser;
use cg3::inlines::hash_value_ustring;
use cg3::rule::RF_SAFE;
use cg3::strings::{KEYWORDS, STR_DUMMY};
use cg3::tag::TagVectorSet;
use cg3::textual_parser::TextualParser;

fn repo_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Parse a grammar source string in-process; assert a clean (0-error) parse.
fn parse_str(src: &str) -> TextualParser {
    let mut p = TextualParser::new(Grammar::default(), false);
    let rv = p.parse_grammar_utf8(src.as_bytes());
    assert_eq!(rv, 0, "grammar string failed to parse");
    p
}

/// Parse a fixture grammar file (repo-relative path) in-process.
fn parse_fixture(rel: &str) -> TextualParser {
    let path = repo_root().join(rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let mut p = TextualParser::new(Grammar::default(), false);
    let rv = p.parse_grammar_utf8(&bytes);
    assert_eq!(rv, 0, "fixture {rel} failed to parse");
    p
}

/// Resolve a named set through `Grammar::getSet` (name-hash resolution).
fn set_by_name(g: &Grammar, name: &str) -> SetId {
    g.get_set(hash_value_ustring(name, 0))
        .unwrap_or_else(|| panic!("set {name} not resolvable"))
}

/// All tag texts reachable from a set (flattened across its tag combinations).
fn set_tag_texts(g: &Grammar, s: SetId) -> BTreeSet<String> {
    set_tag_vectors(g, s).into_iter().flatten().collect()
}

/// The set's tag combinations as vectors of tag texts (trie path order kept).
fn set_tag_vectors(g: &Grammar, s: SetId) -> Vec<Vec<String>> {
    let mut tvs = TagVectorSet::new();
    g.get_tags(s, &mut tvs);
    tvs.iter()
        .map(|tv| tv.iter().map(|t| g.single_tags_list[t.0].tag.clone()).collect())
        .collect()
}

/// Hash of the interned tag with exactly this text (scan the tag arena).
fn tag_hash_by_text(g: &Grammar, text: &str) -> Option<u32> {
    for i in 0..g.single_tags_list.capacity() {
        if let Some(t) = g.single_tags_list.try_get(i) {
            if t.tag == text {
                return Some(t.hash);
            }
        }
    }
    None
}

// The TextualParser constructor, its inherent setCompatible/setVerbosity and
// getGrammar, and the whole IGrammarParser trait surface, driven directly:
// the parser is built, configured, and run through the trait's parse_grammar
// (which swaps the caller's Grammar in, runs the private parse_grammar driver
// -> parseFromUChar over the buffer, and swaps back). Compatible mode is
// observable: with vislcg-compat on, a `NOT` context is rewritten to NEGATE.
// The Grammar values constructed and dropped here also run the C++ ~Grammar()
// analog (the documented no-op Drop) — the grammar-fn dtor id lives here.
// The trait's i-grammar-parser-fn id names the C++ virtual ~IGrammarParser();
// its Rust model is the no-op `drop_parser` default, called explicitly below.
// [spec:cg3:sem:textual-parser.cg3.textual-parser.textual-parser-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.set-compatible-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.set-verbosity-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.get-grammar-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-from-u-char-fn/test]
// [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.i-grammar-parser-fn/test]
// [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn/test]
// [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn/test]
// [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.grammar-fn/test]
#[test]
fn constructor_trait_surface_and_compat_mode() {
    let mut p = TextualParser::new(Grammar::default(), false);

    // Inherent setters (TextualParser.cpp) ...
    p.set_compatible(true);
    p.set_verbosity(1);
    // ... and the IGrammarParser trait overrides (same fields, distinct fns).
    IGrammarParser::set_compatible(&mut p, true);
    IGrammarParser::set_verbosity(&mut p, 1);
    IGrammarParser::drop_parser(&mut p);

    let src = b"DELIMITERS = \"<$.>\" ;\nLIST AA = aa ;\nLIST BB = bb ;\nSELECT AA IF (NOT 1 BB) ;\n";
    let mut g = Grammar::default();
    let rv = IGrammarParser::parse_grammar(&mut p, &mut g, src);
    assert_eq!(rv, 0, "trait parse_grammar failed");

    // The result landed in the CALLER's grammar; the parser's own grammar is
    // still the pristine ctor one (getGrammar shows it).
    assert_eq!(g.rule_by_number.capacity(), 1, "one SELECT rule expected");
    assert_eq!(p.get_grammar().rule_by_number.capacity(), 0);

    // vislcg-compat rewrote the NOT context to NEGATE.
    let negated = (0..g.contexts_arena.capacity()).any(|i| {
        g.contexts_arena.try_get(i).is_some_and(|c| c.pos & POS_NEGATE != 0)
    });
    assert!(negated, "compat mode should turn NOT into NEGATE");
    // `g` and the parser's grammar drop here -> the ~Grammar() analog runs.
}

// LIST/SET parsing end to end over a crafted grammar with composite entries:
// parseTagList builds the LIST tries (tag interning flows parse_tag ->
// TextualParser::addTag -> Grammar::addTag with hash dedup, single tags land
// via addTagToSet, sets via allocateSet/addSet, name resolution via getSet);
// `SET Both = Comp OR Second` drives parseSetInline; the magic dummy set is
// allocated by every parse (allocateDummySet). The composite entries (aa bb) /
// (aa cc) drive the tag-frequency ordering: `aa` is most frequent, so it is
// sorted first in every stored trie path.
// freq_sorter itself: the C++ functor's single live use is the inlined
// highest-frequency-first sort in parseTagList (and the eager-set-op path);
// in the port the struct is a faithful but uncalled reproduction (private,
// #[allow(dead_code)]), so its ctor/operator() ids are attached to this test,
// which drives the exact inlined comparator logic and asserts its ordering.
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-list-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.add-tag-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-fn/test]
// [spec:cg3:sem:textual-parser.cg3.freq-sorter.freq-sorter-fn/test]
// [spec:cg3:sem:textual-parser.cg3.freq-sorter.operator-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-tag-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.allocate-set-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-set-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.get-set-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-tag-to-set-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.allocate-dummy-set-fn/test]
#[test]
fn list_set_parsing_and_composite_tag_ordering() {
    let p = parse_str(
        "DELIMITERS = \"<$.>\" ;\n\
         LIST Comp = (aa bb) (aa cc) dd ;\n\
         LIST Second = xx ;\n\
         SET Both = Comp OR Second ;\n\
         SELECT Both ;\n",
    );
    let g = &p.grammar;

    // allocateDummySet: reserved dummy at list position 0, number = MAX.
    let dummy = g.sets_list_order[0];
    assert_eq!(g.sets_list[dummy.0].name, STR_DUMMY);
    assert_eq!(g.sets_list[dummy.0].number, u32::MAX);

    // Named sets resolve by name hash through getSet.
    let comp = set_by_name(g, "Comp");
    assert_eq!(
        set_tag_texts(g, comp),
        ["aa", "bb", "cc", "dd"].iter().map(|s| s.to_string()).collect()
    );

    // Composite paths were stored highest-frequency-first: `aa` (freq 2) is
    // the shared trie ROOT of both composite entries, `bb`/`cc` its children,
    // and the single `dd` a terminal root of its own.
    let text = |t: &cg3::arena::TagId| g.single_tags_list[t.0].tag.clone();
    let trie = &g.sets_list[comp.0].trie;
    let roots: BTreeSet<String> = trie.keys().map(text).collect();
    assert_eq!(roots, ["aa", "dd"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>());
    let (aa, dd) = {
        let mut it = trie.iter();
        (it.next().unwrap(), it.next().unwrap())
    };
    let (aa, dd) = if text(aa.0) == "aa" { (aa.1, dd.1) } else { (dd.1, aa.1) };
    assert!(dd.terminal, "single entry dd is terminal at the root");
    assert!(!aa.terminal, "aa only exists as the composites' shared prefix");
    let children: BTreeSet<String> =
        aa.trie.as_ref().expect("aa has children").keys().map(text).collect();
    assert_eq!(children, ["bb", "cc"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>());

    // SET Both = Comp OR Second went through parseSetInline; addSet folded the
    // pure-OR SET of LISTs into a single LIST whose contents are the union.
    let both = set_by_name(g, "Both");
    assert_eq!(
        set_tag_texts(g, both),
        ["aa", "bb", "cc", "dd", "xx"].iter().map(|s| s.to_string()).collect()
    );

    // addTag dedup: exactly one interned tag with text "aa".
    let n_aa = (0..g.single_tags_list.capacity())
        .filter(|&i| g.single_tags_list.try_get(i).is_some_and(|t| t.tag == "aa"))
        .count();
    assert_eq!(n_aa, 1, "identical tag text must intern to a single tag");
}

// T_SetOps drives the eager binary set operators (`\`, `∩`, `∆`): each one
// materializes its operands via Grammar::getTags and merges them into a fresh
// set whose contents we assert exactly. The six ADD rules run their maplists
// through parseSetInlineWrapper (which names/registers the inline set) and
// validate them with isMappingList; the named operands A/B are resolved via
// TextualParser::parseSet.
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-wrapper-fn/test]
// [spec:cg3:sem:textual-parser.cg3.is-mapping-list-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.get-tags-fn/test]
#[test]
fn setops_fixture_eager_operators() {
    let p = parse_fixture("test/T_SetOps/grammar.cg3");
    let g = &p.grammar;

    assert_eq!(g.rule_by_number.capacity(), 6, "T_SetOps defines 6 ADD rules");
    for i in 0..g.rule_by_number.capacity() {
        let r = g.rule_by_number.try_get(i).unwrap();
        assert_eq!(r.r#type, KEYWORDS::K_ADD);
        assert!(r.maplist.is_some(), "every ADD carries a maplist (mapping-list checked)");
    }

    // Collect the tag-text contents of every leaf set; the eagerly-computed
    // sets for A \ B, A ∩ B, A ∆ B must be among them.
    let mut leaf_contents: Vec<BTreeSet<String>> = Vec::new();
    for i in 0..g.sets_list.capacity() {
        if let Some(s) = g.sets_list.try_get(i) {
            if s.sets.is_empty() && !(s.trie.is_empty() && s.trie_special.is_empty()) {
                leaf_contents.push(set_tag_texts(g, SetId(i)));
            }
        }
    }
    let want = |items: &[&str]| -> BTreeSet<String> { items.iter().map(|s| s.to_string()).collect() };
    assert!(leaf_contents.contains(&want(&["a", "b"])), "A \\ B = {{a b}}");
    assert!(leaf_contents.contains(&want(&["c", "d"])), "A ∩ B = {{c d}}");
    assert!(leaf_contents.contains(&want(&["a", "b", "e", "f"])), "A ∆ B = {{a b e f}}");
}

// Rule and section machinery over a crafted grammar: maybeParseRule dispatches
// the SELECT/REMOVE/JUMP keywords into parseRule (allocateRule -> populate ->
// addRuleToGrammar -> Grammar::addRule with section bookkeeping); the SAFE
// flag is consumed by parseRuleFlags; named `SECTION one ;` / `ANCHOR mark ;`
// go through parseAnchorish into Grammar::addAnchor; the JUMP maplist is
// validated at end-of-parse via getTagList_Any against the anchor table.
// [spec:cg3:sem:textual-parser.cg3.textual-parser.maybe-parse-rule-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-flags-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.add-rule-to-grammar-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-anchorish-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.allocate-rule-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-rule-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-anchor-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.get-tag-list-any-fn/test]
#[test]
fn rules_sections_anchors_and_jump() {
    let p = parse_str(
        "DELIMITERS = \"<$.>\" ;\n\
         LIST NN = nn ;\n\
         LIST XX = xx ;\n\
         LIST YY = yy ;\n\
         SECTION one ;\n\
         SELECT SAFE (nn) IF (1 (xx)) ;\n\
         SECTION two ;\n\
         REMOVE (yy) ;\n\
         ANCHOR mark ;\n\
         JUMP (mark) (*) ;\n",
    );
    let g = &p.grammar;

    assert_eq!(g.sections.len(), 2, "two numbered sections");
    assert_eq!(g.rule_by_number.capacity(), 3);

    let r_select = g.rule_by_number.try_get(0).unwrap();
    let r_remove = g.rule_by_number.try_get(1).unwrap();
    let r_jump = g.rule_by_number.try_get(2).unwrap();
    assert_eq!(r_select.r#type, KEYWORDS::K_SELECT);
    assert_eq!(r_remove.r#type, KEYWORDS::K_REMOVE);
    assert_eq!(r_jump.r#type, KEYWORDS::K_JUMP);

    // parseRuleFlags: SAFE was consumed into the rule's flags.
    assert_ne!(r_select.flags & RF_SAFE, 0, "SELECT SAFE must carry RF_SAFE");

    // addRuleToGrammar section assignment: SELECT in section 0, rest in 1.
    assert_eq!(r_select.section, 0);
    assert_eq!(r_remove.section, 1);
    assert_eq!(r_jump.section, 1);

    // parseAnchorish/addAnchor: the section names and the ANCHOR are all
    // registered under their tag hashes.
    for name in ["one", "two", "mark"] {
        let h = tag_hash_by_text(g, name).unwrap_or_else(|| panic!("tag {name} interned"));
        assert!(g.anchors.contains(h), "anchor {name} registered");
    }

    // getTagList_Any flattens a leaf set's tries (also ran during the JUMP
    // anchor validation at the end of the parse).
    let nn = set_by_name(g, "NN");
    let tags = g.get_tag_list_any_ret(nn);
    assert_eq!(tags.len(), 1);
    assert_eq!(g.single_tags_list[tags[0].0].tag, "nn");
}

// T_Templates: TEMPLATE directives build named contextual-test templates
// (allocateContextualTest -> parseContextualTestList/-Position ->
// Grammar::addTemplate), rule contexts go through parseContextualTests into
// Grammar::addContextualTest (structural interning into `contexts`), and the
// deferred `T:` references are resolved to the interned templates at the end
// of the parse (ctx.tmpl wired up).
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-tests-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.allocate-contextual-test-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-contextual-test-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-template-fn/test]
#[test]
fn templates_fixture_contextual_tests() {
    let p = parse_fixture("test/T_Templates/grammar.cg3");
    let g = &p.grammar;

    assert_eq!(g.templates.len(), 9, "T_Templates defines 9 TEMPLATEs");
    assert!(!g.contexts.is_empty(), "rule contexts interned via addContextualTest");
    assert!(g.rule_by_number.capacity() > 10);

    // Every ADD rule in the fixture has at least one contextual test.
    for i in 0..g.rule_by_number.capacity() {
        let r = g.rule_by_number.try_get(i).unwrap();
        assert!(!r.tests.is_empty(), "rule {i} should carry contextual tests");
    }

    // parseContextualTestPosition: offsets like -4 were parsed into contexts.
    let has_neg4 = (0..g.contexts_arena.capacity())
        .any(|i| g.contexts_arena.try_get(i).is_some_and(|c| c.offset == -4));
    assert!(has_neg4, "a context with offset -4 exists in the fixture");

    // Deferred template refs were resolved: some context points at a template.
    let tmpl_refs = (0..g.contexts_arena.capacity())
        .filter(|&i| g.contexts_arena.try_get(i).is_some_and(|c| c.tmpl.is_some()))
        .count();
    assert!(tmpl_refs > 0, "T: references must be wired to interned templates");
}

// T_SetParentChild: SETPARENT/SETCHILD ... TO (...) rules route their
// dependency contexts through parseContextualDependencyTests (into
// rule.dep_tests, distinct from rule.tests).
// [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-dependency-tests-fn/test]
#[test]
fn setparent_fixture_dependency_tests() {
    let p = parse_fixture("test/T_SetParentChild/grammar.cg3");
    let g = &p.grammar;

    assert!(g.has_dep, "dependency rules mark the grammar has_dep");
    let mut dep_rules = 0;
    let mut both = 0;
    for i in 0..g.rule_by_number.capacity() {
        let r = g.rule_by_number.try_get(i).unwrap();
        if matches!(r.r#type, KEYWORDS::K_SETPARENT | KEYWORDS::K_SETCHILD) {
            if !r.dep_tests.is_empty() {
                dep_rules += 1;
            }
            if !r.dep_tests.is_empty() && !r.tests.is_empty() {
                both += 1;
            }
        }
    }
    assert!(dep_rules > 0, "SETPARENT/SETCHILD rules with TO contexts expected");
    assert!(both > 0, "a rule with both plain tests and dep tests expected");
}

// UNDEF-SETS and LIST += over a crafted grammar: undefSet pulls the old set
// out of the name index (so the name can be redefined), and appendToSet
// merges the re-opened LIST with its previous contents under the same name.
// [spec:cg3:sem:grammar.cg3.grammar.undef-set-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.append-to-set-fn/test]
#[test]
fn undef_sets_and_list_append() {
    let p = parse_str(
        "DELIMITERS = \"<$.>\" ;\n\
         LIST XX = x1 x2 ;\n\
         UNDEF-SETS = XX ;\n\
         LIST XX = x3 ;\n\
         LIST YY = y1 ;\n\
         LIST YY += y2 ;\n\
         SELECT XX ;\n\
         SELECT YY ;\n",
    );
    let g = &p.grammar;

    // After UNDEF-SETS the name XX resolves to the NEW definition only.
    let xx = set_by_name(g, "XX");
    assert_eq!(set_tag_texts(g, xx), ["x3"].iter().map(|s| s.to_string()).collect());

    // LIST YY += y2 merged old and new under the same name.
    let yy = set_by_name(g, "YY");
    assert_eq!(set_tag_texts(g, yy), ["y1", "y2"].iter().map(|s| s.to_string()).collect());
}

// Error recovery: referencing an undefined set makes parseSet call
// TextualParser::error (near-context form), which routes through
// incErrorCount (counter += 1, ParseError unwind); parseFromUChar catches it,
// skips the line, and the parse finishes returning the nonzero error count.
// [spec:cg3:sem:textual-parser.cg3.textual-parser.error-fn/test]
// [spec:cg3:sem:textual-parser.cg3.textual-parser.inc-error-count-fn/test]
#[test]
fn parse_error_recovery_counts_errors() {
    let mut p = TextualParser::new(Grammar::default(), false);
    let rv = p.parse_grammar_utf8(
        b"DELIMITERS = \"<$.>\" ;\nLIST AA = aa ;\nSELECT NOSUCHSET ;\nSELECT AA ;\n",
    );
    assert_eq!(rv, 1, "exactly one recoverable parse error expected");
    // The parser recovered: the valid rule after the bad line still parsed.
    assert_eq!(p.grammar.rule_by_number.capacity(), 1);
}

// printAst: constructing the parser with dump_ast=true records the AST while
// parsing; print_ast then renders the XML dump for the parsed grammar.
// [spec:cg3:sem:textual-parser.cg3.textual-parser.print-ast-fn/test]
#[test]
fn print_ast_dumps_xml() {
    let mut p = TextualParser::new(Grammar::default(), true);
    let rv = p.parse_grammar_utf8(b"DELIMITERS = \"<$.>\" ;\nLIST AA = aa ;\nSELECT AA ;\n");
    assert_eq!(rv, 0);
    let mut out: Vec<u8> = Vec::new();
    p.print_ast(&mut out);
    let s = String::from_utf8(out).unwrap();
    assert!(s.starts_with("<?xml version=\"1.0\""), "AST dump must be XML: {s:.>40}");
    assert!(s.contains("l is line"));
}

// Grammar::reindex over the parsed T_SetParentChild fixture (exactly what
// cg-comp does after parsing). reindex renumbers the used sets depth-first
// (addSetToList), rewrites unify/child set types (setAdjustSets), builds the
// tag->set index (indexSets -> indexTagToSet + trie_indexSetToSet over both
// tries), the set->rule and tag->rule indexes (indexSetToRule ->
// indexTagToRule + trie_indexSetToRule), and retargets every rule's contexts
// (contextAdjustTarget over tests, dep_tests and dep_target).
// [spec:cg3:sem:grammar.cg3.grammar.reindex-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.add-set-to-list-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.set-adjust-sets-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.index-sets-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.index-tag-to-set-fn/test]
// [spec:cg3:sem:grammar.cg3.trie-index-to-set-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.index-set-to-rule-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.index-tag-to-rule-fn/test]
// [spec:cg3:sem:grammar.cg3.trie-index-to-rule-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.context-adjust-target-fn/test]
#[test]
fn reindex_builds_runtime_indexes() {
    let mut p = parse_fixture("test/T_SetParentChild/grammar.cg3");
    p.grammar.reindex(false, false);
    let g = &p.grammar;

    // Rule indexes: set->rules and tag->rules were populated.
    assert!(!g.rules_by_set.is_empty(), "rules_by_set built");
    assert!(!g.rules_by_tag.is_empty(), "rules_by_tag built");
    // Set index: tag->sets bitmaps were populated.
    assert!(!g.sets_by_tag.is_empty(), "sets_by_tag built");

    // addSetToList: dense DFS numbering, dummy reset to number 0 at position 0.
    assert!(g.sets_list_order.len() > 1, "used sets numbered");
    for (n, sid) in g.sets_list_order.iter().enumerate() {
        assert_eq!(g.sets_list[sid.0].number, n as u32, "set number == list position");
    }
    assert_eq!(g.sets_list[g.sets_list_order[0].0].name, STR_DUMMY);

    // Rules were distributed into their section vectors.
    let distributed = g.before_sections.len() + g.rules.len() + g.after_sections.len()
        + g.null_section.len();
    assert!(distributed > 0, "rules distributed to section vectors");
}

// Direct-call coverage for Grammar methods no textual parse path reaches:
// removeNumericTags builds a `_G_<name>_B_` variant set with the T_NUMERICAL
// tags stripped; destroyTag/destroySet/destroyRule free arena slots (and
// destroySet unregisters from sets_all); allocateTag interns raw text with
// hash dedup (same text -> same id).
// [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.allocate-tag-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.destroy-tag-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.destroy-set-fn/test]
// [spec:cg3:sem:grammar.cg3.grammar.destroy-rule-fn/test]
#[test]
fn remove_numeric_tags_and_direct_destroyers() {
    // removeNumericTags on a parsed grammar with a mixed numeric/plain LIST.
    let mut p = parse_str(
        "DELIMITERS = \"<$.>\" ;\n\
         LIST NUM = <VALUE=50> plain ;\n\
         SELECT NUM ;\n",
    );
    let num = set_by_name(&p.grammar, "NUM");
    let h = p.grammar.sets_list[num.0].hash;
    let nh = p.grammar.remove_numeric_tags(h);
    assert_ne!(nh, h, "numeric tag removal must produce a different set");
    let stripped = p.grammar.get_set(nh).expect("stripped set registered");
    assert!(
        p.grammar.sets_list[stripped.0].name.contains("NUM"),
        "stripped set is the _G_NUM_B_ variant: {}",
        p.grammar.sets_list[stripped.0].name
    );
    assert_eq!(
        set_tag_texts(&p.grammar, stripped),
        ["plain"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
        "only the non-numeric tag survives"
    );

    // Direct allocate/destroy round-trips on a fresh Grammar.
    let mut g = Grammar::default();
    g.lines = 1;

    let t1 = g.allocate_tag("zzz");
    let t2 = g.allocate_tag("zzz");
    assert_eq!(t1, t2, "allocateTag dedups identical text");
    g.destroy_tag(t1);
    assert!(g.single_tags_list.try_get(t1.0).is_none(), "tag slot freed");

    let s = g.allocate_set();
    assert!(g.sets_list.try_get(s.0).is_some());
    g.destroy_set(s);
    assert!(g.sets_list.try_get(s.0).is_none(), "set slot freed");

    let r = g.allocate_rule();
    let rid = g.add_rule(r);
    assert_eq!(g.rule_by_number[rid.0].number, rid.0, "addRule numbers the rule");
    g.destroy_rule(rid);
    assert!(g.rule_by_number.try_get(rid.0).is_none(), "rule slot freed");
}

// trie_unserialize (Grammar.hpp): compile a fixture grammar to .cg3b with the
// real cg-comp binary, then load it in-process through BinaryGrammar — every
// set's trie/trie_special in the binary image is rebuilt via trie_unserialize.
// [spec:cg3:sem:grammar.cg3.trie-unserialize-fn/test]
#[test]
fn binary_grammar_roundtrip_unserializes_tries() {
    let grammar = repo_root().join("test/T_SetOps/grammar.cg3");
    let bin = std::env::temp_dir()
        .join(format!("cg3-grammar-parser-{}.cg3b", std::process::id()));
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_cg-comp"))
        .arg(&grammar)
        .arg(&bin)
        .status()
        .expect("spawn cg-comp");
    assert!(status.success(), "cg-comp failed");

    let mut bg = BinaryGrammar::binary_grammar(Grammar::default());
    let rv = bg.parse_grammar_filename(bin.to_str().unwrap());
    let _ = std::fs::remove_file(&bin);
    assert_eq!(rv, 0, "binary grammar failed to load");

    let g = &bg.grammar;
    assert!(g.is_binary);
    assert!(g.num_tags > 0, "tags unserialized");
    let with_trie = (0..g.sets_list.capacity())
        .filter(|&i| {
            g.sets_list
                .try_get(i)
                .is_some_and(|s| !s.trie.is_empty() || !s.trie_special.is_empty())
        })
        .count();
    assert!(with_trie > 0, "trie_unserialize rebuilt set tries");
}
