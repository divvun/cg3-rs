//! Stream-format applicator integration tests — Apertium, Matxin, Binary
//! stream (.cg3bsf), FST, JSONL, FormatConverter, Niceline, Plaintext, and
//! MweSplit applicators.
//!
//! Drives are real end-to-end runs of the ported binaries (`cg-proc`,
//! `cg-conv`, `cg-comp`, `cg-mwesplit`) over the `test/` fixture corpus (the
//! same protocol as the fixtures' `run.pl` scripts), plus in-process library
//! calls for lower-level serializer/parser details and for Matxin output
//! (which follows the upstream converter's missing switch arm), plus
//! `ApertiumApplicator::testPR` which is a
//! commented-out debug block in the C++ `cg-proc.cpp`).

/// Build the minimal "conversion" grammar + applicator base exactly the way
/// `FormatConverter::new` / cg-conv do (dummy set, one delimiter set holding
/// the dummy tag, reindex, set_grammar), for driving wrapper applicators
/// in-process.
fn conv_base() -> cg3::grammar_applicator::GrammarApplicator {
    let mut grammar = cg3::grammar::GrammarCore::default();
    grammar.allocate_dummy_set();
    let delim = grammar.allocate_set();
    grammar.delimiters = Some(delim);
    let dummy_tag = grammar.allocate_tag("__CG3_DUMMY_STRINGBIT__").unwrap();
    grammar.add_tag_to_set(dummy_tag, delim);
    let grammar = grammar.finish().unwrap();
    let mut base = cg3::grammar_applicator::GrammarApplicator::new(grammar.into());
    base.set_grammar().unwrap();
    base
}

// ApertiumApplicator::testPR — the C++ call site is a commented-out debug
// block in cg-proc.cpp ("Add a / in front to enable this test"), so it is
// unreachable from every binary; the honest drive is the in-process library
// call. The C++ block runs AFTER cg-proc loads a real grammar (testPR reads
// `grammar->single_tags[grammar->tag_any]`, and `tag_any` is only initialized
// by a grammar parse/load — the conv grammar never reaches that state), so
// the test parses a minimal grammar first, exactly like the real call site.
// It round-trips six hard-coded analysis strings through processReading +
// printReading.
// [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.test-pr-fn/test]
#[test]
fn apertium_test_pr_roundtrip() {
    let mut p =
        cg3::textual_parser::TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    p.parse_grammar_utf8(b"DELIMITERS = \".\" ;\nSELECT (foo) ;\n")
        .expect("minimal grammar failed to parse");
    let g = p.grammar.finish().unwrap();
    let mut base = cg3::grammar_applicator::GrammarApplicator::new(g.into());
    base.set_grammar().unwrap();
    let mut a = cg3::apertium_applicator::ApertiumApplicator::new(base);
    let mut out: Vec<u8> = Vec::new();
    a.test_pr(&mut out)
        .expect("testPR round-trips its fixtures");
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        6,
        "testPR must emit its six fixture readings:\n{text}"
    );
    // processReading prepends the wform tag (here `*`, the tag_any tag testPR
    // passes) to every reading, and printReading skips only BASEFORM/WORDFORM
    // typed tags — so the faithful output carries an extra `<*>` after the
    // baseform, exactly as the C++ debug block would emit.
    assert_eq!(
        lines[0], "venir<*><vblex><imp><p2><sg>",
        "testPR line 1 wrong: {}",
        lines[0]
    );
    // The subreading chain fixture keeps its + joins.
    assert!(text.contains("+"), "testPR lost subreading joins:\n{text}");
    // The wordbound-space fixture keeps the escaped `# ` part.
    assert!(text.contains("be"), "testPR lost baseform text:\n{text}");
}

// A library caller can build a reading with no cohort. A tag with an effect
// on the cohort is refused on it; the C++ followed the null cohort.
// [spec:cg3:req:robustness.accepted-grammars-run/test]
#[test]
fn cohortless_reading_refuses_cohort_tags() {
    let mut p =
        cg3::textual_parser::TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    p.parse_grammar_utf8(b"DELIMITERS = \".\" ;\nSELECT (vblex) ;\n")
        .expect("the grammar parses");
    let g = p.grammar.finish().unwrap();
    let mut base = cg3::grammar_applicator::GrammarApplicator::new(g.into());
    base.set_grammar().unwrap();
    let tag = base.add_tag("vblex", cg3::tag::TagType::empty()).unwrap();
    let reading = cg3::reading::alloc_reading(&mut base.doc.store, None);
    let result = base.engine().add_tag_to_reading(reading, tag);
    assert!(
        matches!(
            result,
            Err(cg3::error::RunError::ReadingWithoutCohort { .. })
        ),
        "{result:?}"
    );
}

// A grammar that names a testPR fixture tag in a set, an enclosure or a bag
// of tags, or whose mapping prefix makes one a mapping tag, has testPR reach
// the cohort the fixture reading belongs to. The C++ gives the reading no
// cohort and dereferences it.
// [spec:cg3:req:robustness.no-input-panics+1/test]
#[test]
fn apertium_test_pr_with_grammar_tags() {
    for grammar in [
        "DELIMITERS = \".\" ;\nSELECT (vblex) ;\n",
        "DELIMITERS = \".\" ;\nMAPPING-PREFIX = v ;\nSELECT (foo) ;\n",
        "DELIMITERS = \".\" ;\nPARENTHESES = (vblex inf) ;\nSELECT (foo) ;\n",
        "DELIMITERS = \".\" ;\nSELECT (foo) (B (vblex)) ;\n",
    ] {
        let mut p =
            cg3::textual_parser::TextualParser::new(cg3::grammar::GrammarCore::default(), false);
        p.parse_grammar_utf8(grammar.as_bytes())
            .expect("grammar parses");
        let g = p.grammar.finish().unwrap();
        let mut base = cg3::grammar_applicator::GrammarApplicator::new(g.into());
        base.set_grammar().unwrap();
        let mut a = cg3::apertium_applicator::ApertiumApplicator::new(base);
        let mut out: Vec<u8> = Vec::new();
        a.test_pr(&mut out)
            .expect("testPR round-trips its fixtures");
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 6, "{grammar}:\n{text}");
    }
}

// ===========================================================================
// FSTApplicator — supplement the converter/CLI coverage below with a direct
// wrapper run: conv grammar + is_conv + trace, then
// runGrammarOnText over `wordform<TAB>analysis[<TAB>weight]` lines. The
// is_conv fast path prints each finished cohort (printCohort → printReading)
// and the EOF drain prints the remaining window (printSingleWindow).
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.fst-applicator-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-reading-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-cohort-fn/test]
// [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-single-window-fn/test]
#[test]
fn fst_applicator_in_process() {
    let mut base = conv_base();
    base.cfg.is_conv = true;
    base.cfg.trace = true;
    base.cfg.verbosity_level = 0;
    let mut fst = cg3::fst_applicator::FSTApplicator::new(base);
    assert_eq!(fst.wtag, "W");
    assert_eq!(fst.sub_delims, "#");

    // Two cohorts: one plain, one with a weight (exercises the wtag path) and
    // a second ambiguous reading.
    let input = "blah\tblah+N+Sg\nblah\tblah+V+Inf\t0.5\n\ndog\tdog+N+Sg\n\n";
    let mut cursor = std::io::Cursor::new(input.as_bytes().to_vec());
    let mut out: Vec<u8> = Vec::new();
    fst.run_grammar_on_text(&mut cursor, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();

    // printReading re-emits the FST `base+tags` shape per reading.
    assert!(
        text.contains("blah\tblah+N+Sg"),
        "FST reading 1 missing:\n{text}"
    );
    assert!(
        text.contains("blah\tblah+V+Inf"),
        "FST reading 2 missing:\n{text}"
    );
    // The weight came back as a <W:...> tag.
    assert!(text.contains("+<W:0.5"), "FST weight tag missing:\n{text}");
    assert!(
        text.contains("dog\tdog+N+Sg"),
        "FST second cohort missing:\n{text}"
    );
}

/// Convert one JSONL cohort carrying `deps` (its `"ds"`/`"dp"` members) with
/// dependencies parsed, as `cg-conv --in-jsonl -D` does.
fn convert_jsonl_deps(deps: &str) -> Result<String, cg3::error::Cg3Error> {
    use cg3::grammar_applicator::StreamFormatKind;

    let base = cg3::grammar_applicator::GrammarApplicator::new(cg3::grammar::Grammar::default());
    let mut fc = cg3::format_converter::FormatConverter::new(base)?;
    fc.base_mut().cfg.fmt_input = StreamFormatKind::Jsonl;
    fc.base_mut().cfg.parse_dep = true;
    fc.base_mut().doc.deps.has_dep = true;
    let input = format!(
        "{{\"w\":\"a\",\"rs\":[{{\"l\":\"a\",\"ts\":[\"N\"]}}],{deps}}}\n\
         {{\"w\":\"b\",\"rs\":[{{\"l\":\"b\",\"ts\":[\"V\"]}}],\"ds\":2,\"dp\":1}}\n"
    );
    let mut out: Vec<u8> = Vec::new();
    fc.run_grammar_on_text(&mut std::io::Cursor::new(input.into_bytes()), &mut out)?;
    Ok(String::from_utf8(out).expect("UTF-8 output"))
}

// A JSONL `"ds"` or `"dp"` equal to a dependency-map sentinel is refused as it
// is read, naming the member; it used to reach the map and trip its sentinel
// assert. A `"dp"` of u32::MAX is DEP_NO_PARENT and converts.
// [spec:cg3:req:robustness.reserved-keys/test]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+1/test]
#[test]
fn jsonl_reserved_dependency_numbers_are_refused() {
    use cg3::error::{Cg3Error, NumberRole, RunError};

    let cases = [
        (
            "\"ds\":4294967295,\"dp\":0",
            "ds",
            NumberRole::DependencySelf,
            u32::MAX,
        ),
        (
            "\"ds\":4294967294,\"dp\":0",
            "ds",
            NumberRole::DependencySelf,
            u32::MAX - 1,
        ),
        (
            "\"ds\":1,\"dp\":4294967294",
            "dp",
            NumberRole::DependencyParent,
            u32::MAX - 1,
        ),
    ];
    for (deps, member, role, value) in cases {
        match convert_jsonl_deps(deps) {
            Err(Cg3Error::Run(RunError::ReservedNumber { source, .. })) => {
                assert_eq!(
                    (&*source.text, source.role, source.value),
                    (member, role, value)
                );
            }
            other => panic!("{deps}: expected a reserved-number refusal, got {other:?}"),
        }
    }
    let out = convert_jsonl_deps("\"ds\":1,\"dp\":4294967295").expect("no parent converts");
    assert!(out.contains("\"a\" N #1->1"), "{out}");
}

// FormatConverter's four print dispatchers — supplement the end-to-end tool
// coverage with direct calls: build the converter exactly like cg-conv does,
// hand-build one window + cohort + reading in the shared base, and invoke each
// dispatcher for a live
// fmt_output arm (NICELINE for printCohort, JSONL for printSingleWindow /
// printStreamCommand / printPlainTextLine), asserting the format-specific
// encodings come out.
// [spec:cg3:sem:format-converter.cg3.format-converter.print-cohort-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.print-single-window-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.print-stream-command-fn/test]
// [spec:cg3:sem:format-converter.cg3.format-converter.print-plain-text-line-fn/test]
#[test]
fn format_converter_print_dispatch() {
    use cg3::grammar_applicator::StreamFormatKind;

    let base = cg3::grammar_applicator::GrammarApplicator::new(cg3::grammar::Grammar::default());
    let mut fc = cg3::format_converter::FormatConverter::new(base).unwrap();
    fc.base_mut().cfg.is_conv = true;
    fc.base_mut().cfg.trace = true;
    fc.base_mut().cfg.verbosity_level = 0;

    // Hand-build one window with one cohort ("<word>" with reading "word" X).
    let (sw, cohort) = {
        let b = fc.base_mut();
        let sw = b.doc.stream.alloc_append_single_window(&mut b.doc.store);
        b.engine().init_empty_single_window(sw).unwrap();
        let c = cg3::cohort::alloc_cohort(&mut b.doc.store, Some(sw));
        let wf = b.add_tag("\"<word>\"", cg3::tag::TagType::empty()).unwrap();
        {
            let co = b.doc.store.cohorts.get_mut(c.0);
            co.wordform = Some(wf);
            co.global_number = cg3::types::GlobalNumber(1);
        }
        let r = cg3::reading::alloc_reading(&mut b.doc.store, Some(c));
        b.engine().add_tag_to_reading(r, wf).unwrap();
        let bf = b.add_tag("\"word\"", cg3::tag::TagType::empty()).unwrap();
        b.engine().add_tag_to_reading(r, bf).unwrap();
        let t = b.add_tag("X", cg3::tag::TagType::empty()).unwrap();
        b.engine().add_tag_to_reading(r, t).unwrap();
        cg3::cohort::append_reading(&mut b.doc.store, c, r);
        cg3::single_window::append_cohort(
            &mut b.doc.store,
            &mut b.doc.cohorts,
            &mut b.doc.deps,
            sw,
            c,
        );
        b.doc.store.cohorts.get_mut(c.0).local_number = 1;
        (sw, c)
    };

    // printCohort → NICELINE arm.
    fc.base_mut().cfg.fmt_output = StreamFormatKind::Niceline;
    let mut out: Vec<u8> = Vec::new();
    fc.print_cohort(cohort, &mut out, false).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("word\t[word]"),
        "printCohort NICELINE dispatch wrong: {text}"
    );
    assert!(text.contains(" X"), "reading tag lost: {text}");

    // printSingleWindow → JSONL arm (one JSON object per cohort).
    fc.base_mut().cfg.fmt_output = StreamFormatKind::Jsonl;
    let mut out: Vec<u8> = Vec::new();
    fc.print_single_window(sw, &mut out, false).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("\"w\":\"word\""),
        "printSingleWindow JSONL dispatch wrong: {text}"
    );
    assert!(
        text.contains("\"l\":\"word\""),
        "JSONL reading lost: {text}"
    );

    // printStreamCommand → JSONL arm.
    let mut out: Vec<u8> = Vec::new();
    fc.print_stream_command("<STREAMCMD:FLUSH>", &mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "{\"cmd\":\"<STREAMCMD:FLUSH>\"}\n",
        "printStreamCommand JSONL dispatch wrong"
    );

    // printPlainTextLine → JSONL arm.
    let mut out: Vec<u8> = Vec::new();
    fc.print_plain_text_line("some text", &mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "{\"t\":\"some text\"}\n",
        "printPlainTextLine JSONL dispatch wrong"
    );
}

// ===========================================================================
// PlaintextApplicator — supplement the converter/CLI coverage with a direct
// wrapper run: conv grammar + is_conv, then runGrammarOnText over raw
// plaintext. The driver tokenizes lines into cohorts (peeling ASCII
// punctuation) and the print side re-emits space-separated wordforms
// (printSingleWindow → printCohort).
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.plaintext-applicator-fn/test]
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.run-grammar-on-text-fn/test]
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-cohort-fn/test]
// [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-single-window-fn/test]
#[test]
fn plaintext_applicator_in_process() {
    let mut base = conv_base();
    base.cfg.is_conv = true;
    base.cfg.trace = true;
    base.cfg.verbosity_level = 0;
    let mut app = cg3::plaintext_applicator::PlaintextApplicator::new(base);
    // Constructor behavior: magic readings allowed, add_tags defaults off.
    assert!(app.base.cfg.allow_magic_readings);
    assert!(!app.add_tags);

    let input = "Hello brave world.\n";
    let mut cursor = std::io::Cursor::new(input.as_bytes().to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();

    // Tokenized wordforms come back space-separated; the final '.' is peeled
    // into its own cohort.
    assert!(text.contains("Hello"), "token 1 lost:\n{text:?}");
    assert!(text.contains("brave"), "token 2 lost:\n{text:?}");
    assert!(text.contains("world"), "token 3 lost:\n{text:?}");
    assert!(text.contains("."), "punctuation cohort lost:\n{text:?}");
}
