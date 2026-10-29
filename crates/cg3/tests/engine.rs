//! Engine-phase facet tests for the `GrammarApplicator` core — the port of
//! `src/GrammarApplicator.{hpp,cpp}` + its five `_*.cpp` partials.
//!
//! Fixture-driven: each test runs the `vislcg3` binary over the `test/T_*`
//! golden fixtures that genuinely execute that sub-area's functions (same
//! protocol as `tests/golden.rs` / `test/runall.pl`: cwd = fixture dir, extra
//! flags from `args.txt`, output diffed blank-line-insensitively against
//! `expected.txt`). Functions not reachable from any fixture/flag are called
//! in-process on a directly-constructed `GrammarApplicator`.

// ===========================================================================
// 5. In-process unit calls for functions no fixture/flag reaches:
// - error-fn: the RT-diagnostic label selection ("RT RULE" with a live
//   current_rule, else "RT INPUT" + numLines); emission is deferred I/O in the
//   port, so the returned (label, line) pair is asserted directly.
// - get-grammar-fn: trivial accessor, never called by the engine paths (the
//   parser-helpers trait uses its own `grammar()`); called directly.
// - check-options-fn: C++ `_check_options` is dead code even in the C++ TU
//   (documented as such in the port); exercised directly over its three
//   branches.
// - tag-set-subset-of-t-set-fn: template helper with no live Rust caller;
//   exercised directly against an interned Grammar tag and a hash vector.
// ===========================================================================
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.error-fn/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-grammar-fn/test]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.check-options-fn/test]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.tag-set-subset-of-t-set-fn/test]
#[test]
fn engine_inprocess_error_getters_and_dead_helpers() {
    use cg3::arena::ReadingId;
    use cg3::contextual_test::{POS_CAREFUL, PosFlags};
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::grammar_applicator::match_set::{check_options, tag_set_subset_of_t_set};
    use cg3::sorted_vector::Uint32SortedVector;
    use cg3::tag::TagSortedVector;

    // See also `current_rule_does_not_outlive_the_run` below: the label these
    // return depends on current_rule being unset when no rule is in flight.

    // error(): no current rule -> ("RT INPUT", numLines).
    let mut grammar = cg3::grammar::GrammarCore::default();
    let aa = grammar.allocate_tag("enginetag-aa").unwrap();
    let bb = grammar.allocate_tag("enginetag-bb").unwrap();
    let aa_hash = grammar.single_tags_list[aa.0].hash.get();
    let bb_hash = grammar.single_tags_list[bb.0].hash.get();

    let app = GrammarApplicator::new(grammar.into());
    assert_eq!(app.error("%s: some diagnostic\n", None), ("RT INPUT", 0));
    assert_eq!(app.error_s("%s: %s\n", "detail", None), ("RT INPUT", 0));
    assert_eq!(app.error_ss("%s: %s %s\n", "a", "b", None), ("RT INPUT", 0));

    // get_grammar(): returns the owned grammar (our two tags are interned).
    let g = app.get_grammar();
    assert_eq!(&*g.single_tags_list[aa.0].tag, "enginetag-aa");

    // _check_options: CAREFUL demands all readings matched; DEPREL bypasses;
    // otherwise any match suffices.
    let rv = [ReadingId(0), ReadingId(1)];
    assert!(
        check_options(&rv, PosFlags::empty(), 3),
        "plain: non-empty rv matches"
    );
    assert!(
        !check_options(&[], PosFlags::empty(), 3),
        "plain: empty rv fails"
    );
    assert!(!check_options(&rv, POS_CAREFUL, 3), "careful: 2 of 3 fails");
    assert!(check_options(&rv, POS_CAREFUL, 2), "careful: all match");

    // TagSet_SubsetOf_TSet: {aa} subset-of {aa_hash, bb_hash}; {aa, bb} not
    // subset-of {bb_hash} alone.
    let mut a = TagSortedVector::new();
    a.insert(aa);
    let mut b = Uint32SortedVector::new();
    b.insert(aa_hash);
    b.insert(bb_hash);
    assert!(tag_set_subset_of_t_set(app.get_grammar(), &a, &b));
    a.insert(bb);
    let mut b2 = Uint32SortedVector::new();
    b2.insert(bb_hash);
    assert!(!tag_set_subset_of_t_set(app.get_grammar(), &a, &b2));
}

/// `current_rule` names the rule in flight, and nothing is in flight once a
/// window's rules have run.
///
/// It used to be set on entry to each rule and never cleared, so after the
/// first window it only meant "some rule ran at some point". Stream reading is
/// interleaved with rule running — the reader tests DELIMITERS against each
/// cohort as it reads — so a failure raised while reading a later window was
/// attributed to whichever rule happened to finish last.
///
/// Asserted on the state rather than through a diagnostic because the
/// mislabelling needs a failure that only fires mid-read, which is a far more
/// elaborate fixture than the property is worth. The label itself is pinned by
/// `tools_cli::runtime_input_failure_names_the_input`.
#[test]
fn current_rule_does_not_outlive_the_run() {
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::textual_parser::TextualParser;

    let src = b"DELIMITERS = \"<$.>\" ;\nLIST N = n ;\nSECTION\nSELECT N ;\n";
    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(src, "current-rule.cg3")
        .expect("grammar parses");
    let grammar = parser.grammar.finish().expect("reindex");

    let mut app = GrammarApplicator::new(grammar.into());
    app.set_grammar().expect("applicator setup");
    let mut cursor = std::io::Cursor::new(b"\"<a>\"\n\t\"a\" n\n\n\"<b>\"\n\t\"b\" n\n\n".to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out)
        .expect("the run completes");

    assert!(
        app.scratch.current_rule.is_none(),
        "a rule that has finished must not still be the current rule"
    );
}

/// The stream's tag type flags are the RUN's, not the grammar's.
///
/// A mapped tag read off the stream gets `T_MAPPING` stamped on it, and a
/// runtime regex/icase tag stamps `T_TEXTUAL` across the whole arena. The C++
/// writes both into the loaded `Tag`, which is why a grammar cannot be shared:
/// two pipelines applying different streams would overwrite each other's flags,
/// and a `.cg3b` written after a run would carry them. Here the writes land in
/// `Grammar::tag_flags` and `Tag::r#type` keeps the value the loader gave it.
///
/// `@dyn` deliberately appears nowhere in the grammar, so the tag is interned
/// by the stream reader and its load-time flags genuinely lack `T_MAPPING` —
/// a tag the grammar already listed would be indistinguishable either way.
#[test]
fn runtime_tag_flags_do_not_reach_the_grammar() {
    use cg3::grammar::Grammar;
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::textual_parser::TextualParser;

    let src = b"DELIMITERS = \"<$.>\" ;\nLIST N = n ;\nSECTION\nSELECT N ;\n";
    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(src, "tag-flags.cg3")
        .expect("grammar parses");
    let grammar = parser.grammar.finish().expect("reindex");
    let grammar = Grammar::from(grammar);

    // Materialised flags are exactly the load-time ones, for every live tag.
    for i in 0..grammar.single_tags_list.capacity() {
        if let Some(t) = grammar.single_tags_list.try_get(i) {
            assert_eq!(
                grammar.tag_type(cg3::arena::TagId(i)),
                t.r#type,
                "tag {i} materialised with flags the loader never set"
            );
        }
    }
    let before: Vec<Option<cg3::tag::TagType>> = (0..grammar.single_tags_list.capacity())
        .map(|i| grammar.single_tags_list.try_get(i).map(|t| t.r#type))
        .collect();

    let mut app = GrammarApplicator::new(grammar);
    app.set_grammar().expect("applicator setup");
    app.cfg.apply_mappings = true;
    let mut cursor = std::io::Cursor::new(b"\"<a>\"\n\t\"a\" n @dyn\n\n".to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out)
        .expect("the run completes");

    let grammar = &app.grammar;
    for (i, was) in before.iter().enumerate() {
        if let Some(was) = was {
            assert_eq!(
                grammar.single_tags_list[i as u32].r#type, *was,
                "the run rewrote tag {i}'s load-time flags"
            );
        }
    }

    let dyn_id = grammar
        .single_tags()
        .find(cg3::inlines::hash_value_str("@dyn", 0))
        .get()
        .1;
    assert_eq!(
        &*grammar.single_tags_list[dyn_id.0].tag, "@dyn",
        "the stream's mapped tag was interned"
    );
    assert!(
        grammar.tag_type(dyn_id).intersects(cg3::tag::T_MAPPING),
        "the run must see the stream tag as mapped"
    );
    assert!(
        !grammar.single_tags_list[dyn_id.0]
            .r#type
            .intersects(cg3::tag::T_MAPPING),
        "T_MAPPING belongs to the run, not to the interned Tag"
    );
}

/// Interning at runtime still dedups against the tags the grammar was compiled
/// with, including the ones the parser had to park at a seeded hash.
///
/// `single_tags` is two maps now — the core's load-time entries and the run's
/// own — and `addTag` dedups by walking `hash + seed` upwards until a key
/// nothing answers for. A probe that consulted only the run's half would find
/// `hash + 0` free on the first step and mint a SECOND id for text the core
/// already has, and two ids for one text silently changes what matches what.
///
/// `"aac0c"` and `"aaepa"` hash to the same `hash_value_str`, so the
/// grammar parks the second at `hash + 1`: re-interning THAT one only comes
/// back with the core's id if step 0 saw the core's `"aac0c"` sitting in the
/// way and kept walking.
#[test]
fn runtime_interning_dedups_against_the_shared_core() {
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::tag::TagType;
    use cg3::textual_parser::TextualParser;

    let src = b"DELIMITERS = \"<$.>\" ;\nLIST N = n aac0c aaepa ;\nSECTION\nSELECT N ;\n";
    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(src, "collide.cg3")
        .expect("grammar parses");
    let grammar = parser.grammar.finish().expect("reindex");

    let collide_hash = cg3::inlines::hash_value_str("aac0c", 0);
    assert_eq!(
        collide_hash,
        cg3::inlines::hash_value_str("aaepa", 0),
        "the fixture's two texts must genuinely collide"
    );
    let seed0 = grammar.single_tags().find(collide_hash).get().1;
    let seed1 = grammar
        .single_tags()
        .find(collide_hash.wrapping_add(1))
        .get()
        .1;
    assert_eq!(&*grammar.single_tags_list[seed0.0].tag, "aac0c");
    assert_eq!(
        &*grammar.single_tags_list[seed1.0].tag, "aaepa",
        "the colliding tag must be parked one seed along"
    );

    let core_len = grammar.single_tags_list.capacity();
    let num_tags = grammar.num_tags;

    let mut app = GrammarApplicator::new(grammar.into());
    app.set_grammar().expect("applicator setup");
    // setGrammar's own begin/end/subst/mprefix tags are already the run's.
    let after_setup = app.grammar.single_tags_list.capacity();

    // Both come back as the CORE's tags — the un-seeded one at step 0, the
    // seeded one only because step 0 saw the core's entry and walked on.
    assert_eq!(
        app.add_tag("aac0c", TagType::empty()).expect("interned"),
        seed0
    );
    assert_eq!(
        app.add_tag("aaepa", TagType::empty()).expect("interned"),
        seed1
    );

    // A text the grammar does not have is the run's, and lands above the core.
    let fresh = app
        .add_tag("aaqqz", TagType::empty())
        .expect("a fresh tag interns");
    assert!(
        fresh.0 >= core_len,
        "a tag the grammar never had belongs to the run, not to the core"
    );
    assert_eq!(
        app.add_tag("aaqqz", TagType::empty()).expect("interned"),
        fresh,
        "the run dedups against its own tags too"
    );
    assert_eq!(
        app.grammar
            .single_tags()
            .find(cg3::inlines::hash_value_str("aaqqz", 0))
            .get()
            .1,
        fresh,
        "the hash index spans the run's half"
    );

    // And the grammar itself did not grow: a `.cg3b` written after this run
    // serialises `num_tags` core tags, the same ones it would have before.
    assert_eq!(app.grammar.core().single_tags_list.capacity(), core_len);
    assert_eq!(app.grammar.num_tags, num_tags);
    assert_eq!(app.grammar.single_tags_list.capacity(), after_setup + 1);
}

/// The payoff, end to end: one loaded grammar's core read from several threads
/// at once, which is what a process running N pipelines off one grammar does.
#[test]
fn a_loaded_core_is_shareable() {
    use cg3::textual_parser::TextualParser;

    let src = b"DELIMITERS = \"<$.>\" ;\nLIST N = n ;\nSECTION\nSELECT N ;\n";
    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(src, "shared.cg3")
        .expect("grammar parses");
    let grammar = parser.grammar.finish().expect("reindex");

    let core = std::sync::Arc::new(grammar);
    let tags = core.single_tags_list.capacity();
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let core = std::sync::Arc::clone(&core);
            std::thread::spawn(move || core.single_tags_list.capacity())
        })
        .collect();
    for r in readers {
        assert_eq!(r.join().expect("reader thread"), tags);
    }
}

/// Two pipelines over ONE grammar — the thing the whole split was for.
///
/// Both applicators are built from the same `Arc<GrammarCore>`: no second
/// parse, no second copy of the sets, rules, contexts or load-time tags. Each
/// then applies a stream that interns a tag the other never sees, and has to
/// emit exactly what it would have emitted owning the grammar outright.
///
/// The two run tags land at the SAME id — each overlay numbers its own tags
/// from the core's capacity up — which is the sharpest form the isolation takes:
/// one shared arena would have had to give them different ids, and one shared
/// overlay would have shown each run the other's tag at that id.
#[test]
fn two_pipelines_share_one_grammar_core() {
    use cg3::arena::TagId;
    use cg3::grammar::GrammarCore;
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::inlines::hash_value_str;
    use cg3::tag::T_MAPPING;
    use cg3::textual_parser::TextualParser;
    use std::sync::Arc;

    const SRC: &[u8] =
        b"DELIMITERS = \"<$.>\" ;\nLIST N = n ;\nLIST V = v ;\nSECTION\nSELECT N ;\n";
    const STREAM_A: &[u8] = b"\"<wa>\"\n\t\"wa\" n @a-only\n\t\"wa\" v\n\n";
    const STREAM_B: &[u8] = b"\"<wb>\"\n\t\"wb\" n @b-only\n\t\"wb\" v\n\n";

    fn load() -> GrammarCore {
        let mut parser = TextualParser::new(GrammarCore::default(), false);
        parser
            .parse_grammar_named(SRC, "shared-core.cg3")
            .expect("grammar parses");
        parser.grammar.finish().expect("reindex")
    }

    fn owned() -> GrammarApplicator {
        let mut app = GrammarApplicator::new(load().into());
        app.set_grammar().expect("applicator setup");
        app
    }

    fn apply(app: &mut GrammarApplicator, stream: &[u8]) -> String {
        app.cfg.apply_mappings = true;
        let mut out: Vec<u8> = Vec::new();
        app.run_grammar_on_text(&mut std::io::Cursor::new(stream.to_vec()), &mut out)
            .expect("the run completes");
        String::from_utf8(out).expect("utf-8 output")
    }

    // What each stream produces down the load-then-run path — the behaviour the
    // golden corpus already pins, and the definition of "correct" here.
    let want_a = apply(&mut owned(), STREAM_A);
    let want_b = apply(&mut owned(), STREAM_B);
    assert_ne!(want_a, want_b, "the two streams must be distinguishable");

    // One grammar, loaded once, held the way a host holds it.
    let core = Arc::new(load());
    let core_tags = core.single_tags_list.capacity();
    let core_hashes = core.tags_by_hash.size();

    let mut a = GrammarApplicator::from_core(Arc::clone(&core)).expect("pipeline a");
    let mut b = GrammarApplicator::from_core(Arc::clone(&core)).expect("pipeline b");
    assert_eq!(
        Arc::strong_count(&core),
        3,
        "two pipelines plus this handle — three handles on ONE core"
    );
    assert!(
        std::ptr::eq(a.grammar.core(), &*core) && std::ptr::eq(b.grammar.core(), &*core),
        "both pipelines must read the same grammar, not a copy of it"
    );

    assert_eq!(apply(&mut a, STREAM_A), want_a, "shared core, same output");
    assert_eq!(apply(&mut b, STREAM_B), want_b, "shared core, same output");

    // Neither run can see the other's tags.
    let (ha, hb) = (hash_value_str("@a-only", 0), hash_value_str("@b-only", 0));
    let ia = a.grammar.single_tags().find(ha);
    let ib = b.grammar.single_tags().find(hb);
    assert_ne!(ia, a.grammar.single_tags().end(), "a interned its own tag");
    assert_ne!(ib, b.grammar.single_tags().end(), "b interned its own tag");
    assert_eq!(
        a.grammar.single_tags().find(hb),
        a.grammar.single_tags().end(),
        "a must not see b's tag"
    );
    assert_eq!(
        b.grammar.single_tags().find(ha),
        b.grammar.single_tags().end(),
        "b must not see a's tag"
    );

    let (ia, ib) = (ia.get().1, ib.get().1);
    assert_eq!(ia, ib, "each overlay numbers its own tags from the core up");
    assert_eq!(&*a.grammar.single_tags_list[ia.0].tag, "@a-only");
    assert_eq!(&*b.grammar.single_tags_list[ib.0].tag, "@b-only");
    assert!(a.grammar.tag_type(ia).intersects(T_MAPPING));
    assert!(b.grammar.tag_type(ib).intersects(T_MAPPING));

    // And nothing either run did reached the grammar they share: same tags,
    // same hash entries, and every core tag's flags still the loader's in both.
    assert_eq!(core.single_tags_list.capacity(), core_tags);
    assert_eq!(core.tags_by_hash.size(), core_hashes);
    for i in 0..core_tags {
        if let Some(t) = core.single_tags_list.try_get(i) {
            assert_eq!(a.grammar.tag_type(TagId(i)), t.r#type, "a rewrote tag {i}");
            assert_eq!(b.grammar.tag_type(TagId(i)), t.r#type, "b rewrote tag {i}");
        }
    }
    assert_eq!(
        Arc::strong_count(&core),
        3,
        "still one core after both runs"
    );
}

/// A grammar someone is applying cannot be written out from under them.
///
/// Both writers EDIT what they serialise — the binary one reverses each rule's
/// test lists, the textual one renames every unnamed set — so both take an
/// owned `GrammarCore`, and the only way back to one from a shared grammar is
/// `Arc::try_unwrap`: refused while a pipeline holds the core, granted once it
/// is gone. That is the hand-back `vislcg3` does before `--grammar-bin`.
#[test]
fn a_shared_core_cannot_reach_the_writers() {
    use cg3::binary_grammar::BinaryGrammar;
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::textual_parser::TextualParser;
    use std::sync::Arc;

    let src = b"DELIMITERS = \"<$.>\" ;\nLIST N = n ;\nSECTION\nSELECT N ;\n";
    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(src, "writers.cg3")
        .expect("grammar parses");
    let grammar = parser.grammar.finish().expect("reindex");

    let core = Arc::new(grammar);
    let pipeline = GrammarApplicator::from_core(Arc::clone(&core)).expect("pipeline");
    let Err(core) = Arc::try_unwrap(core) else {
        panic!("a core a pipeline holds must stay shared");
    };

    drop(pipeline);
    let grammar = Arc::try_unwrap(core)
        .ok()
        .expect("the last handle takes the grammar back");
    let mut blob: Vec<u8> = Vec::new();
    BinaryGrammar::new(grammar)
        .write_binary_grammar(&mut blob)
        .expect("an owned grammar writes");
    assert!(!blob.is_empty());
}

// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn/test]
// [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn/test]
#[test]
fn add_tag_reflow_ignores_non_dependency_union_roles() {
    use cg3::grammar::Grammar;
    use cg3::grammar::TagSpace;
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::reading::alloc_reading;
    use cg3::tag::{T_NUMERIC_MATH, Tag};

    let mut app = GrammarApplicator::new(Grammar::default());
    let mut engine = app.engine();
    let reading = alloc_reading(&mut engine.doc.store, None);

    let mut numeric = Tag {
        tag: "<number=1+2>".into(),
        ..Tag::default()
    };
    numeric.parse_numeric(true).expect("a valid expression");
    assert!(numeric.r#type.intersects(T_NUMERIC_MATH));
    assert_ne!(numeric.comparison_offset(), 0);
    let tag = engine.grammar.add_tag(numeric);
    let hash = engine.grammar.single_tags_list[tag.0].hash.get();

    engine
        .add_tag_to_reading_rehash(reading, tag, false)
        .expect("the reading takes the tag");

    assert!(
        engine
            .doc
            .store
            .readings
            .get(reading.0)
            .tags_list
            .contains(&hash)
    );
}

// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn/test]
// [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn/test]
#[test]
fn add_tag_reflow_preserves_dependency_parent() {
    use cg3::cohort::alloc_cohort;
    use cg3::grammar::Grammar;
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::reading::alloc_reading;
    use cg3::single_window::alloc_swindow;
    use cg3::tag::{T_DEPENDENCY, TagType};
    use cg3::types::GlobalNumber;

    let mut app = GrammarApplicator::new(Grammar::default());
    app.cfg.parse_dep = true;
    let mut engine = app.engine();
    let window = alloc_swindow(&mut engine.doc.store, None);
    let cohort = alloc_cohort(&mut engine.doc.store, Some(window));
    let reading = alloc_reading(&mut engine.doc.store, Some(cohort));
    let tag = engine
        .add_tag("#7->3", TagType::empty())
        .expect("tag interns");
    assert!(
        engine.grammar.single_tags_list[tag.0]
            .r#type
            .intersects(T_DEPENDENCY)
    );

    engine
        .add_tag_to_reading_rehash(reading, tag, false)
        .expect("the reading takes the tag");

    let cohort = engine.doc.store.cohorts.get(cohort.0);
    assert_eq!(cohort.dep_self, Some(GlobalNumber(7)));
    assert_eq!(cohort.dep_parent, Some(GlobalNumber(3)));
}

// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn/test]
// [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn/test]
#[test]
fn add_tag_reflow_preserves_relation_target() {
    use cg3::cohort::alloc_cohort;
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::reading::alloc_reading;
    use cg3::single_window::alloc_swindow;
    use cg3::tag::{T_RELATION, TagType};

    let mut grammar = cg3::grammar::GrammarCore::default();
    grammar.has_relations = true;
    let mut app = GrammarApplicator::new(grammar.into());
    let mut engine = app.engine();
    let window = alloc_swindow(&mut engine.doc.store, None);
    let cohort = alloc_cohort(&mut engine.doc.store, Some(window));
    let reading = alloc_reading(&mut engine.doc.store, Some(cohort));
    let tag = engine
        .add_tag("R:linked:3", TagType::empty())
        .expect("tag interns");
    let relation = &engine.grammar.single_tags_list[tag.0];
    assert!(relation.r#type.intersects(T_RELATION));
    assert_eq!(relation.dep_parent(), 3);
    let relation_hash = relation.comparison_hash;

    engine
        .add_tag_to_reading_rehash(reading, tag, false)
        .expect("the reading takes the tag");

    let targets = engine
        .doc
        .store
        .cohorts
        .get(cohort.0)
        .relations_input
        .get(&relation_hash);
    assert!(targets.is_some_and(|targets| targets.contains(3)));
}

// ===========================================================================
// 11b. Hash-table keys from the stream. Stream variables and dependency and
// relation numbers land in the flat hash containers, whose probe must end
// however many tombstones a long stream leaves, and whose two sentinel keys
// must never arrive from input.
// ===========================================================================

/// Parse `grammar` and run the CG text stream `input` through it in-process.
fn run_text(grammar: &[u8], input: &str) -> Result<String, cg3::error::Cg3Error> {
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::textual_parser::TextualParser;

    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(grammar, "keys.cg3")
        .expect("grammar parses");
    let grammar = parser.grammar.finish().expect("reindex");
    let mut app = GrammarApplicator::new(grammar.into());
    app.set_grammar().expect("applicator setup");
    let mut cursor = std::io::Cursor::new(input.as_bytes().to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out)?;
    Ok(String::from_utf8(out).expect("UTF-8 output"))
}

/// One variable held throughout, then `cycles` rounds of `round(i)` stream
/// commands, then a cohort.
fn variable_churn(held: &str, cycles: usize, round: impl Fn(usize) -> String) -> String {
    let mut s = format!("<STREAMCMD:{held}>\n");
    for i in 0..cycles {
        s.push_str(&round(i));
    }
    s.push_str("\"<w>\"\n\t\"w\" n\n");
    s
}

/// A stream that sets and removes variables for as long as it runs fills the
/// variable tables with tombstones until no empty slot is left; removing a
/// variable neither table holds then used to probe forever. The first stream
/// saturates the set-variable map, the second the removed-variable set.
// [spec:cg3:req:robustness.hash-probe-bounded/test]
// [spec:cg3:sem:flat-unordered-map.cg3.flat-unordered-map.erase-fn+1/test]
// [spec:cg3:sem:flat-unordered-set.cg3.flat-unordered-set.erase-fn+1/test]
#[test]
fn variable_churn_does_not_hang_the_stream() {
    let grammar = b"DELIMITERS = \"<$.>\" ;\nLIST N = n ;\nSECTION\nSELECT N ;\n";
    let map_churn = variable_churn("SETVAR:keep=x", 2000, |i| {
        format!("<STREAMCMD:SETVAR:v{i}=a>\n<STREAMCMD:REMVAR:v{i}>\n<STREAMCMD:REMVAR:z{i}>\n")
    });
    let set_churn = variable_churn("REMVAR:keep", 2000, |i| {
        format!("<STREAMCMD:REMVAR:v{i}>\n<STREAMCMD:SETVAR:v{i}=a>\n<STREAMCMD:SETVAR:w{i}=a>\n")
    });
    for input in [map_churn, set_churn] {
        let out = run_text(grammar, &input).expect("the churned stream runs");
        assert!(
            out.contains("\"<w>\""),
            "the cohort after the churn is lost"
        );
    }
}

/// A dependency grammar: the window's dependency map is built and probed.
const DEP_GRAMMAR: &[u8] = b"DELIMITERS = \"<$.>\" ;\nADD (@child) (*) (p (*)) ;\n";

/// A dependency or relation number equal to a hash-table sentinel is refused
/// where the stream tag is parsed, naming the tag and what it was read as; it
/// used to reach the dependency or relation map and trip its sentinel assert.
// [spec:cg3:req:robustness.reserved-keys/test]
#[test]
fn reserved_numbers_in_stream_tags_are_refused() {
    use cg3::error::{Cg3Error, NumberRole, RunError};

    let cases = [
        ("#-1->2", NumberRole::DependencySelf, u32::MAX),
        ("#1->-2", NumberRole::DependencyParent, u32::MAX - 1),
        ("#1->4294967294", NumberRole::DependencyParent, u32::MAX - 1),
        ("ID:4294967294", NumberRole::RelationId, u32::MAX - 1),
        ("ID:4294967295", NumberRole::RelationId, u32::MAX),
        ("R:x:-2", NumberRole::RelationTarget, u32::MAX - 1),
    ];
    for (tag, role, value) in cases {
        let input = format!("\"<a>\"\n\t\"a\" n #1->2\n\"<b>\"\n\t\"b\" n {tag}\n");
        match run_text(DEP_GRAMMAR, &input) {
            Err(Cg3Error::Run(RunError::ReservedNumber { source, .. })) => {
                assert_eq!(
                    (&*source.text, source.role, source.value),
                    (tag, role, value)
                );
            }
            other => panic!("{tag}: expected a reserved-number refusal, got {other:?}"),
        }
    }
}

/// `#x->-1` is the C++'s `DEP_NO_PARENT`: the cohort has no parent, and the
/// stream runs. It used to be kept as parent 4294967295 and looked up.
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn/test]
// [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn/test]
#[test]
fn dependency_parent_minus_one_is_no_parent() {
    let input = "\"<a>\"\n\t\"a\" n #1->-1\n\"<b>\"\n\t\"b\" n #2->1\n";
    let out = run_text(DEP_GRAMMAR, input).expect("the stream runs");
    let reading = |base: &str| {
        out.lines()
            .find(|l| l.contains(base))
            .unwrap_or_else(|| panic!("{base} lost:\n{out}"))
            .to_string()
    };
    let a = reading("\"a\"");
    assert!(a.contains("#1->1") && !a.contains("@child"), "{out}");
    assert!(reading("\"b\"").contains("@child"), "{out}");
}

// ===========================================================================
// 13. Every grammar that loads runs on every input
// (`[spec:cg3:req:robustness.accepted-grammars-run]`). Each grammar and input
// below panicked; each now runs, or stops with a run error naming the rule.
// ===========================================================================

/// Parse `grammar`, apply it to the CG text `input` with `configure` applied
/// to the engine options, and return the output or the run's error.
fn apply_grammar(
    grammar: &str,
    input: &str,
    configure: fn(&mut cg3::grammar_applicator::EngineConfig),
) -> Result<String, cg3::error::Cg3Error> {
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::textual_parser::TextualParser;

    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(grammar.as_bytes(), "robustness.cg3")
        .expect("grammar parses");
    let core = parser.grammar.finish().expect("reindex");
    let mut app = GrammarApplicator::new(core.into());
    app.set_grammar().expect("applicator setup");
    configure(&mut app.cfg);
    let mut cursor = std::io::Cursor::new(input.as_bytes().to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out)?;
    Ok(String::from_utf8(out).expect("output is UTF-8"))
}

/// [`apply_grammar`] with the default engine options.
fn apply(grammar: &str, input: &str) -> Result<String, cg3::error::Cg3Error> {
    apply_grammar(grammar, input, |_| {})
}

/// Why a run stopped at a rule it could not apply, and the rule's line.
fn inapplicable(result: Result<String, cg3::error::Cg3Error>) -> (String, u32) {
    use cg3::error::{Cg3Error, ParseErrorKind, RunError};
    match result {
        Err(Cg3Error::Run(RunError::RuleInapplicable { source, .. })) => match &source.kind {
            ParseErrorKind::RuleInapplicable(why) => (format!("{why:?}"), source.line),
            other => panic!("a rule failure carries its reason, not {other:?}"),
        },
        other => panic!("expected the run to stop at the rule, got {other:?}"),
    }
}

/// `a(N) b(V,N) c(N) .` — one window.
const ABC: &str =
    "\"<a>\"\n\t\"a\" N\n\"<b>\"\n\t\"b\" V\n\t\"b\" N\n\"<c>\"\n\t\"c\" N\n\"<.>\"\n\t\".\" CLB\n";

/// A careful context that attaches gives SELECT, REMOVE and COPY the reading
/// head it matched, as a normal one does. The careful matcher recorded only
/// the sub-reading, so all three dereferenced a missing reading.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-careful-fn+1/test]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-careful-fn+1/test]
#[test]
fn careful_attaching_context_acts_on_its_reading() {
    let head = "DELIMITERS = \"<.>\" ;\n";
    let cv = "\"<a>\"\n\t\"a\" N\n\"<b>\"\n\t\"b\" V\n\t\"b\" N\n\"<c>\"\n\t\"c\" N\n\t\"c\" V\n";
    let out = apply(&format!("{head}SELECT (V) (-1CA (*)) ;"), cv).expect("SELECT runs");
    assert!(
        out.contains("\"<b>\"\n\t\"b\" N\n\"<c>\""),
        "SELECT keeps the attached cohort's matched reading:\n{out}"
    );
    apply(&format!("{head}REMOVE (V) (1CA (*)) ;"), ABC).expect("REMOVE runs");
    let out = apply(&format!("{head}COPY (Y) (V) (1CA (*)) ;"), ABC).expect("COPY runs");
    assert!(
        out.contains("\t\"c\" N Y\n"),
        "COPY copies the attached reading:\n{out}"
    );
}

/// An attaching context that matched the wordform-line tags has no reading
/// for SELECT, REMOVE or COPY to act on.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
#[test]
fn attaching_to_wordform_tags_is_a_run_error() {
    let input = "\"<a>\"\n\t\"a\" N\n\"<b>\"\n\t\"b\" V\n\t\"b\" N\n\"<c>\" WTAG\n\t\"c\" N\n";
    for rule in ["SELECT (V)", "REMOVE (V)", "COPY (Y) (V)"] {
        let (why, line) = inapplicable(apply(&format!("\n{rule} (1A (WTAG)) ;"), input));
        assert!(why.starts_with("AttachedToWordformTags"), "{rule}: {why}");
        assert_eq!(line, 2, "{rule}: the error names the rule's line");
    }
}

/// A tag list a cohort cannot be built from is refused when the rule fires:
/// no wordform first, or a tag before any baseform. The first used to leave a
/// cohort with no wordform, which panicked when it was given a reading.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn+2/test]
// [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn+2/test]
#[test]
fn cohort_tag_list_without_wordform_is_refused() {
    let cases = [
        (
            "ADDCOHORT (\"x\" X) AFTER (V) ;",
            "WordformFirst { rule: \"ADDCOHORT\" }",
        ),
        (
            "MERGECOHORTS (\"m\" M) (N) WITH (1 (V)) ;",
            "WordformFirst { rule: \"MERGECOHORTS\" }",
        ),
        (
            "SPLITCOHORT (\"x\" X \"<y>\" \"y\" Y) (V) ;",
            "WordformFirst { rule: \"SPLITCOHORT\" }",
        ),
        (
            "SPLITCOHORT (X) (V) ;",
            "WordformFirst { rule: \"SPLITCOHORT\" }",
        ),
        (
            "ADDCOHORT (\"<x>\" X) AFTER (V) ;",
            "BaseformAfterWordform { rule: \"ADDCOHORT\" }",
        ),
        (
            "SPLITCOHORT (\"<x>\" \"x\" X \"<y>\" Y) (V) ;",
            "BaseformAfterWordform { rule: \"SPLITCOHORT\" }",
        ),
        (
            "APPEND (X \"x\") (V) ;",
            "BaseformFirst { rule: \"APPEND\" }",
        ),
    ];
    for (rule, want) in cases {
        let (why, line) = inapplicable(apply(&format!("DELIMITERS = \"<.>\" ;\n{rule}"), ABC));
        assert_eq!(why, want, "{rule}");
        assert_eq!(line, 2, "{rule}: the error names the rule's line");
    }
    let out = apply("ADDCOHORT (\"<x>\" \"x\" X) AFTER (V) ;", ABC).expect("a full tag list runs");
    assert!(out.contains("\"<x>\"\n\t\"x\" X\n"), "{out}");
}

/// A `SET:` tag resolves through the names `STATIC-SETS` keep for run time;
/// any other name is a run error rather than a lookup in a cleared table.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2/test]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2/test]
#[test]
fn set_tag_resolves_only_static_sets() {
    let out = apply(
        "STATIC-SETS = NN ;\nLIST NN = N ;\nADD (@x) (SET:NN) ;",
        ABC,
    )
    .expect("a static set resolves");
    assert!(out.contains("\t\"a\" N @x\n"), "{out}");
    for grammar in ["\nADD (@x) (SET:N) ;", "LIST NN = N ;\nADD (@x) (SET:NN) ;"] {
        let (why, line) = inapplicable(apply(grammar, ABC));
        assert!(why.starts_with("SetNotStatic"), "{grammar}: {why}");
        assert_eq!(line, 2, "{grammar}: the error names the rule's line");
    }
}

/// A `&&` set reached through a `SET:` tag in DELIMITERS is tested while the
/// stream is read, when no rule has pushed a unification frame.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1/test]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn+1/test]
#[test]
fn unified_set_matches_outside_a_rule() {
    let grammar = "STATIC-SETS = U ;\nLIST A = N ;\nLIST B = V ;\nSET X = A OR B ;\n\
                   SET U = &&X - (ZZ) ;\nDELIMITERS = SET:U ;\nADD (@x) (*) ;";
    let out = apply(grammar, ABC).expect("the delimiter test runs");
    assert!(out.contains("\t\"c\" N @x\n"), "{out}");
}

/// Variable tags carrying a value share the tag's union with other roles, and
/// each reader now checks the role it reads.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
#[test]
fn variable_tags_with_values_run() {
    // Numerical as well as variable: compared numerically, as in the C++.
    let numeric = "\"<a>\"\n\t\"a\" N <x=5>\n\"<b>\"\n\t\"b\" V <x=7>\n";
    let out = apply("ADD (@x) (VAR:<x=5>) ;", numeric).expect("VAR:<x=5> runs");
    assert!(
        out.contains("\t\"a\" N <x=5> @x\n") && !out.contains("<x=7> @x"),
        "{out}"
    );

    // A failed math offset overwrote the value: a bare test, as in the C++.
    let setvar = "<STREAMCMD:SETVAR:<x=y>>\n\"<a>\"\n\t\"a\" N\n";
    let out = apply("ADD (@v) (VAR:<x=5+>) ;", setvar).expect("VAR:<x=5+> runs");
    assert!(out.contains("\t\"a\" N @v\n"), "{out}");

    // A WITH body's target set holding a valued variable tag.
    let grammar = "LIST N = N ;\nLIST V = V ;\nWITH N (1 V) { ADD (@y) (VAR:foo=bar) ; } ;";
    apply(grammar, ABC).expect("WITH over VAR:foo=bar runs");
}

/// A numeric-math variable tag keeps its math offset through a `.cg3b`: the
/// writer stores the union member, and the reader labels it by the tag's type.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
#[test]
fn numeric_math_variable_tag_survives_cg3b() {
    use cg3::binary_grammar::BinaryGrammar;
    use cg3::grammar::GrammarCore;
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::textual_parser::TextualParser;

    let mut parser = TextualParser::new(GrammarCore::default(), false);
    parser
        .parse_grammar_named(b"ADD (@m) (VAR:<x=5+1>) ;\n", "math.cg3")
        .expect("grammar parses");
    let core = parser.grammar.finish().expect("reindex");
    let mut blob: Vec<u8> = Vec::new();
    BinaryGrammar::new(core)
        .write_binary_grammar(&mut blob)
        .expect("the grammar writes");
    let mut reader = BinaryGrammar::new(GrammarCore::default());
    reader.parse_grammar_buffer(&blob).expect("the .cg3b reads");
    let core = reader.grammar.finish().expect("reindex");

    let mut app = GrammarApplicator::new(core.into());
    app.set_grammar().expect("applicator setup");
    let mut cursor = std::io::Cursor::new(b"\"<a>\"\n\t\"a\" N <x=6>\n".to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out)
        .expect("the loaded grammar runs");
    let out = String::from_utf8(out).expect("output is UTF-8");
    assert!(out.contains("\t\"a\" N <x=6> @m\n"), "{out}");
}

/// SWITCHPARENT checks that its TARGET has a parent, but an attaching context
/// makes it act on another cohort, which may have none: that cohort is left
/// alone.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
#[test]
fn switchparent_leaves_a_parentless_cohort_alone() {
    let setparent = "DELIMITERS = \"<.>\" ;\nSETPARENT (\"a\") TO (1 (\"b\")) ;\n";
    let before = apply(setparent, ABC).expect("SETPARENT runs");
    let grammar = format!("{setparent}SWITCHPARENT (\"a\") (1A (\"b\")) ;");
    let after = apply(&grammar, ABC).expect("SWITCHPARENT runs");
    assert_eq!(after, before, "nothing to switch with, nothing switched");
}

/// A jump moves the position into the jump target's window; the SELF probe
/// looked it up in the window the test left, past that window's end.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn+2/test]
// [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn+2/test]
#[test]
fn jump_self_probe_reads_the_jump_window() {
    let input = "\"<a>\"\n\t\"a\" A\n\"<.>\"\n\t\".\" CLB\n\"<c>\"\n\t\"c\" F\n\"<d>\"\n\t\"d\" F\n\
                 \"<e>\"\n\t\"e\" F\n\"<f>\"\n\t\"f\" E\n\"<.>\"\n\t\".\" CLB\n";
    let head = "DELIMITERS = \"<.>\" ;\n";
    let hit = apply(&format!("{head}ADD (@x) (A) (1*>X (E)) (jMS (E)) ;"), input)
        .expect("the jump test runs");
    assert!(
        hit.contains("\t\"a\" A @x\n"),
        "SELF is the jump target:\n{hit}"
    );
    let miss = apply(&format!("{head}ADD (@x) (A) (1*>X (E)) (jMS (A)) ;"), input)
        .expect("the jump test runs");
    assert!(
        !miss.contains("@x"),
        "SELF is not the origin's window:\n{miss}"
    );
}

/// A `%u`/`%l` case marker at the end of the text has no character to recase
/// and vanishes, as in the C++ — including one carried in by captured text.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
#[test]
fn case_marker_at_end_of_text_vanishes() {
    let out = apply("ADD (VSTR:x$1%u) TARGET (\"<(.*)>\"r) ;", ABC).expect("%u at the end runs");
    assert!(out.contains("\t\"a\" N xa\n"), "{out}");
    let input = "\"<a\u{1}u>\"\n\t\"a\" N\n";
    let out = apply("ADD (VSTR:x$1) (\"<(.*)>\"r) ;", input).expect("a captured marker runs");
    assert!(out.contains("\t\"a\" N xa\n"), "{out}");
}

/// A dependency cut can leave the current window holding only its `>>>`
/// cohort when relations are resolved.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-relation-window-fn+1/test]
// [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-relation-window-fn+1/test]
#[test]
fn relations_resolve_beside_a_bare_window() {
    let mut input = String::new();
    for i in 1..=500 {
        input.push_str(&format!("\"<w{i}>\"\n\t\"w\" N #{i}->{}\n", i - 1));
    }
    input.push_str("\"<z>\"\n\t\"z\" N #1->0\n");
    let out = apply_grammar("ADDRELATION (r) (N) TO (1 (*)) ;", &input, |cfg| {
        cfg.dep_delimit = 10;
        cfg.parse_dep = true;
    })
    .expect("the run completes");
    assert!(out.contains("\"<z>\""), "{out}");
}

/// A `B` test reads the window's bag of tags, which belongs to no cohort: a
/// tag that asks about the cohort of the reading it tests is a run error
/// naming the rule, and a numerical comparison that needs no cohort runs.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.test-tag-numerical-fn/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2/test]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2/test]
#[test]
fn bag_of_tags_belongs_to_no_cohort() {
    let input = "\"<a>\"\n\t\"a\" N <x=5>\n\"<b>\"\n\t\"b\" V <x=7>\n";
    let out = apply("ADD (@x) (N) (B (<x>6>)) ;", input).expect("a plain comparison runs");
    assert!(out.contains("\t\"a\" N <x=5> @x\n"), "{out}");
    for tag in ["\"<.*>\"r", "<x=MAX>", "VAR:foo", "LVAR:foo", "_ENCL_"] {
        let (why, line) = inapplicable(apply(&format!("\nADD (@x) (N) (B ({tag})) ;"), input));
        assert!(why.starts_with("BagOfTagsCohort"), "{tag}: {why}");
        assert_eq!(line, 2, "{tag}: the error names the rule's line");
    }
    let enclosed =
        "\"<a>\"\n\t\"a\" N\n\"<(>\"\n\t\"(\" L\n\"<b>\"\n\t\"b\" N\n\"<)>\"\n\t\")\" R\n";
    for tag in ["_LEFT_", "_RIGHT_"] {
        let grammar = format!("PARENTHESES = (\"<(>\" \"<)>\") ;\nADD (@x) (N) (B ({tag})) ;");
        let (why, line) = inapplicable(apply(&grammar, enclosed));
        assert!(why.starts_with("BagOfTagsCohort"), "{tag}: {why}");
        assert_eq!(line, 2, "{tag}: the error names the rule's line");
    }
}
