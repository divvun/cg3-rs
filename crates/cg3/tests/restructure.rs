//! Restructuring actions across windows and around PARENTHESES enclosures,
//! run in process.

// ===========================================================================
// Restructuring across windows and around enclosures. Each grammar below
// hands an action a cohort the current window does not hold — through a
// spanning `A` context or a dependency — or restructures around a
// PARENTHESES enclosure: the action must index the cohort's own window, leave
// a window it empties alive for the rule's context frames, keep enclosure
// depths at or above zero and keep every window's `>>>`.
// ===========================================================================

/// A CG stream of one-reading cohorts written `"form tags"`: `"a X"` is the
/// cohort `"<a>"` with the reading `"a" X`.
fn cg_stream(cohorts: &[&str]) -> String {
    let mut out = String::new();
    for c in cohorts {
        let (form, tags) = c.split_once(' ').unwrap_or((c, ""));
        out.push_str(&format!("\"<{form}>\"\n\t\"{form}\" {tags}\n"));
    }
    out
}

/// Run `grammar` over `input` in process, `--trace` when `trace`; the output
/// with its blank lines dropped.
fn run_in_process(grammar: &str, input: &str, trace: bool) -> String {
    use cg3::grammar_applicator::GrammarApplicator;
    use cg3::textual_parser::TextualParser;

    let mut parser = TextualParser::new(cg3::grammar::GrammarCore::default(), false);
    parser
        .parse_grammar_named(grammar.as_bytes(), "actions.cg3")
        .expect("grammar parses");
    let mut core = parser.grammar;
    let _ = core.reindex(false, false).expect("reindex");
    let mut app = GrammarApplicator::new(core.into());
    app.cfg.trace = trace;
    app.set_grammar().expect("applicator setup");
    let mut cursor = std::io::Cursor::new(input.as_bytes().to_vec());
    let mut out: Vec<u8> = Vec::new();
    app.run_grammar_on_text(&mut cursor, &mut out)
        .expect("the run completes");
    let out = String::from_utf8(out).expect("UTF-8 output");
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Two windows: `a` alone, then `x y w z`.
const TWO_WINDOWS: &[&str] = &["a X", ". PU", "x X", "y X", "w X", "z X", ". PU"];

// [spec:cg3:req:robustness.cross-window-actions/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn+1/test]
// [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn+1/test]
#[test]
fn addcohort_inserts_into_the_attached_cohorts_window() {
    let grammar = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                   ADDCOHORT (\"<n>\" \"new\" N) AFTER (\"a\") (1*WA (\"w\")) ;\n";
    assert_eq!(
        run_in_process(grammar, &cg_stream(TWO_WINDOWS), false),
        cg_stream(&["a X", ". PU", "x X", "y X", "w X", "new N", "z X", ". PU"])
            .replace("<new>", "<n>"),
    );
}

// [spec:cg3:req:robustness.cross-window-actions/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-single-rule-fn+1/test]
// [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-single-rule-fn+1/test]
#[test]
fn cross_window_addcohort_applies_once_per_target() {
    // The attached `w` sits at a lower position in its window than two of the
    // targets do in theirs; resuming the rule loop at `w`'s position looked up
    // in the current window sends it back to an earlier target, forever.
    let grammar = "DELIMITERS = \"<.>\" ;\nLIST X = X ;\nSECTION\n\
                   ADDCOHORT (\"<n>\" \"new\" N) AFTER (X) (1*WA (\"w\")) ;\n";
    let input = cg_stream(&["p X", "q X", "a X", ". PU", "w X", "z X", ". PU"]);
    let added = cg_stream(&["new N"]).replace("<new>", "<n>");
    let mut want = cg_stream(&["p X", "q X", "a X", ". PU", "w X"]);
    want.push_str(&added.repeat(3));
    want.push_str(&cg_stream(&["z X", ". PU"]));
    assert_eq!(run_in_process(grammar, &input, false), want);
}

// [spec:cg3:req:robustness.cross-window-actions/test]
#[test]
fn merge_and_split_act_on_the_attached_window() {
    let merge = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                 MERGECOHORTS (\"<m>\" \"m\" N) (\"a\") WITH (1*WA (\"w\")) ;\n";
    assert_eq!(
        run_in_process(merge, &cg_stream(TWO_WINDOWS), false),
        cg_stream(&[". PU", "x X", "y X", "w X", "m N", "z X", ". PU"]),
    );
    let split = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                 SPLITCOHORT (\"<x1>\" \"x1\" N \"<x2>\" \"x2\" N) (\"a\") (1*WA (\"w\")) ;\n";
    assert_eq!(
        run_in_process(split, &cg_stream(TWO_WINDOWS), false),
        cg_stream(&["a X", ". PU", "x X", "y X", "x1 N", "x2 N", "z X", ". PU"]),
    );
}

// [spec:cg3:req:robustness.cross-window-actions/test]
#[test]
fn move_and_switch_across_windows_change_nothing() {
    for action in [
        "SWITCH (\"a\") WITH",
        "MOVE (\"a\") AFTER",
        "MOVE (\"a\") BEFORE",
    ] {
        let grammar = format!(
            "DELIMITERS = \"<.>\" ;\nSECTION\n{action} (1*WA (\"w\") LINK -1*W (\"<.>\")) ;\n"
        );
        assert_eq!(
            run_in_process(&grammar, &cg_stream(TWO_WINDOWS), false),
            cg_stream(TWO_WINDOWS),
            "{action}",
        );
    }
}

// [spec:cg3:req:robustness.cross-window-actions/test]
#[test]
fn delimit_splits_the_attached_cohorts_own_window() {
    let grammar = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                   DELIMIT (\"a\") (1*WA (\"y\")) ;\n\
                   ADD (@start) (*) (-1 (>>>)) ;\n";
    assert_eq!(
        run_in_process(grammar, &cg_stream(TWO_WINDOWS), false),
        cg_stream(&[
            "a X @start",
            ". PU",
            "x X @start",
            "y X",
            "w X @start",
            "z X",
            ". PU",
        ]),
    );
}

// [spec:cg3:req:robustness.cross-window-actions/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.delimit-at-fn+1/test]
// [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.delimit-at-fn+1/test]
#[test]
fn delimit_of_a_previous_window_keeps_its_order() {
    let grammar = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                   DELIMIT (\"a\") (-1*WA (\"y\")) ;\n";
    let input = cg_stream(&["p X", "y X", "q X", ". PU", "a X", "b X", ". PU"]);
    assert_eq!(run_in_process(grammar, &input, false), input);
}

// [spec:cg3:req:robustness.cross-window-actions/test]
#[test]
fn remcohort_ignored_renumbers_the_cohorts_own_window() {
    let grammar = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                   REMCOHORT IGNORED (\"a\") (1*WA (\"y\")) ;\n\
                   REMCOHORT IGNORED (\"b\") (1*WA (\"z\")) ;\n";
    let input = cg_stream(&["a X", "b X", ". PU", "x X", "y X", "z X", ". PU"]);
    assert_eq!(run_in_process(grammar, &input, false), input);
}

// [spec:cg3:req:robustness.cross-window-actions/test]
#[test]
fn a_window_emptied_mid_rule_outlives_the_rule() {
    // The WITH's first context holds the `>>>` of the window its sub-rules
    // then empty; the last sub-rule jumps back to it.
    let with = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                WITH (\"c\") (-1*W (\".\") LINK -2 (>>>)) {\n\
                  REMCOHORT (*) IF (-1*WA (\"b\")) ;\n\
                  REMCOHORT (*) IF (-1*WA (\".\")) ;\n\
                  ADD (@x) (*) IF (jC1 (>>>)) ;\n\
                } ;\n";
    let input = cg_stream(&["b X", ". PU", "c X", ". PU"]);
    assert_eq!(
        run_in_process(with, &input, false),
        cg_stream(&["c X @x", ". PU"])
    );
    // A chain of removals through spanning contexts, one aimed at `>>>`.
    let chain = "DELIMITERS = \"<.>\" ;\nLIST PU = PU ;\nSECTION\n\
                 REMCOHORT (\"a\") IF (-1*A (>>>)) ;\n\
                 REMCOHORT (\"c\") IF (-1*WA (\"b\")) ;\n\
                 REMCOHORT (\"d\") IF (-1*WA (PU)) ;\n";
    let input = cg_stream(&["a X", "b X", ". PU", "c X", "d X", ". PU"]);
    assert_eq!(
        run_in_process(chain, &input, false),
        cg_stream(&["a X", "c X", "d X", ". PU"]),
    );
}

// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn+1/test]
// [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn+1/test]
#[test]
fn actions_never_displace_the_window_start() {
    let input = cg_stream(&["p X", "y X", "q X", ". PU", "a X", "b X", ". PU"]);
    for rule in [
        // Split after `>>>`: an empty window, and the rule again in the next.
        "DELIMIT (\"a\") (-1*A (>>>))",
        "COPYCOHORT (copied) (\"a\") TO BEFORE (-1* (>>>))",
        "MOVE REVERSE (\"a\") AFTER (-1* (>>>))",
        "SWITCH REVERSE (\"a\") WITH (-1* (>>>))",
        "ADDCOHORT (\"<n>\" \"n\" N) BEFORE (\"a\") (-1*A (>>>))",
        "MERGECOHORTS (\"<m>\" \"m\" N) (\"a\") WITH (-1 (*))",
        "SPLITCOHORT (\"<s>\" \"s\" N) (\"a\") (-1*A (>>>))",
        "REMCOHORT (\"a\") IF (-1*A (>>>))",
    ] {
        let grammar = format!("DELIMITERS = \"<.>\" ;\nSECTION\n{rule} ;\n");
        assert_eq!(run_in_process(&grammar, &input, false), input, "{rule}");
    }
    let after = "DELIMITERS = \"<.>\" ;\nSECTION\n\
                 ADDCOHORT (\"<m>\" \"m\" N) AFTER (\"b\") (-2*A (>>>)) ;\n";
    assert_eq!(
        run_in_process(after, &input, false),
        cg_stream(&["p X", "y X", "q X", ". PU", "m N", "a X", "b X", ". PU"]),
    );
}

/// A PARENTHESES window whose `x` hangs off the enclosed `e4`.
const ENCLOSED_PARENT: &[&str] = &[
    "( PU #1->0",
    "e1 X #2->0",
    "e2 X #3->0",
    "e3 X #4->0",
    "e4 X #5->0",
    ") PU #6->0",
    "x xx #7->5",
    ". PU #8->0",
];

// [spec:cg3:req:robustness.enclosures/test]
#[test]
fn remcohort_of_an_enclosed_parent_waits_for_unpacking() {
    let head =
        "DELIMITERS = \"<.>\" ;\nLIST xx = xx ;\nPARENTHESES = (\"<(>\" \"<)>\") ;\nSECTION\n";
    let input = cg_stream(ENCLOSED_PARENT);
    let removed = format!("{head}REMCOHORT (xx) IF (pA (*)) ;\n");
    let mut want = cg_stream(&ENCLOSED_PARENT[..4]);
    want.push_str("; \"<e4>\"\n;\t\"e4\" X REMCOHORT:5\n");
    want.push_str(&cg_stream(&[") PU #5->0", "x xx #6->0", ". PU #7->0"]));
    assert_eq!(run_in_process(&removed, &input, true), want);

    let ignored = format!("{head}REMCOHORT IGNORED (xx) IF (pA (*)) ;\n");
    let want = input.replace("#5->0\n", "#5->0 REMCOHORT:5\n");
    assert_eq!(run_in_process(&ignored, &input, true), want);
}

// [spec:cg3:req:robustness.enclosures/test]
#[test]
fn removals_beside_an_enclosure_keep_counts_at_zero() {
    let grammar = "DELIMITERS = \"<.>\" ;\nLIST rem = rem ;\nPARENTHESES = (<pl> <pr>) ;\n\
                   SECTION\nREMCOHORT (rem) ;\n";
    let input = cg_stream(&["a rem", "( <pl>", "b X", ") <pr>", "c rem", ". PU"]);
    let mut want = String::from("; \"<a>\"\n;\t\"a\" rem REMCOHORT:5\n");
    want.push_str(&cg_stream(&["( <pl>", "b X", ") <pr>"]));
    want.push_str("; \"<c>\"\n;\t\"c\" rem REMCOHORT:5\n");
    want.push_str(&cg_stream(&[". PU"]));
    assert_eq!(run_in_process(grammar, &input, true), want);

    let merge = "DELIMITERS = \"<.>\" ;\nPARENTHESES = (\"<(>\" \"<)>\") ;\nSECTION\n\
                 MERGECOHORTS (\"<m>\" \"m\" N) (\"x\") WITH (1 (\"y\")) ;\n";
    let input = cg_stream(&["x X", "( PU", "e X", ") PU", "y X", ". PU"]);
    assert_eq!(
        run_in_process(merge, &input, false),
        cg_stream(&["m N", "( PU", "e X", ") PU", ". PU"]),
    );
}

// [spec:cg3:req:robustness.enclosures/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-window-fn+1/test]
// [spec:cg3:sem:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-window-fn+1/test]
#[test]
fn the_window_start_never_opens_an_enclosure() {
    let grammar = "DELIMITERS = \"<.>\" ;\nPARENTHESES = (>>> <pr>) ;\nSECTION\nADD (@x) (*) ;\n";
    assert_eq!(
        run_in_process(grammar, &cg_stream(&["a X", ". <pr>"]), false),
        cg_stream(&["a X @x", ". <pr> @x"]),
    );
}

// [spec:cg3:req:robustness.enclosures/test]
// [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-window-fn+1/test]
// [spec:cg3:sem:grammar-applicator-run-rules.grammar-applicator.run-grammar-on-window-fn+1/test]
#[test]
fn a_removed_cohort_inside_an_enclosure_stays_hidden() {
    // `e1` is removed from the next window before that window wraps its
    // enclosure; unpacking must not put it back where `e2` can see it.
    let grammar = "DELIMITERS = \"<.>\" ;\nPARENTHESES = (\"<(>\" \"<)>\") ;\nSECTION\n\
                   REMCOHORT (\"a\") IF (1*WA (\"e1\")) ;\n\
                   ADD (@seen) (\"e2\") (-1 (\"e1\")) ;\n";
    let input = cg_stream(&["a X", ". PU", "( PU", "e1 X", "e2 X", ") PU", ". PU"]);
    assert_eq!(
        run_in_process(grammar, &input, false),
        cg_stream(&["a X", ". PU", "( PU", "e2 X", ") PU", ". PU"]),
    );
}
