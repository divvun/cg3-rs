//! Binary-grammar reads across the ICU-compatibility seam.
//!
//! A `.cg3b` in the wild was compiled by C++ `vislcg3` against ICU, so its
//! stored patterns may use ICU syntax the `regex` crate does not share. These
//! tests build such a file the only way the port can — by compiling a grammar
//! and splicing the pattern bytes — and pin both halves of the contract:
//! ICU-only-but-translatable loads, genuinely-unsupported names itself.
//!
//! The live reproducer is `grc-disambiguator.bin` in the `se.drb` bundle, whose
//! tag `"\Q$1\E.*"S$` is the `icu_literal_quoting_loads` case below.

// [spec:cg3:req:errors.tag-regex-diagnostic/test]
// [spec:cg3:req:tag-regex.single-seam/test]
use cg3::binary_grammar::BinaryGrammar;
use cg3::error::Cg3Error;
use cg3::grammar::Grammar;
use cg3::textual_parser::TextualParser;

/// Compile a one-tag grammar whose regex tag is `marker`, then rewrite the
/// stored pattern to `pattern`.
///
/// Patterns are length-prefixed, so splicing a different length is just a
/// matter of rewriting the `u32` alongside the bytes — everything after shifts
/// naturally.
fn cg3b_with_pattern(marker: &str, pattern: &str) -> Vec<u8> {
    let src = format!("DELIMITERS = \"<.>\" ;\nLIST t = \"{marker}\"r ;\n");
    let mut parser = TextualParser::new(Grammar::default(), false);
    parser
        .parse_grammar_utf8(src.as_bytes())
        .expect("fixture grammar must compile");
    let mut grammar = parser.grammar;
    grammar.reindex(false, false).unwrap();
    let mut writer = BinaryGrammar::new(grammar);
    let mut blob: Vec<u8> = Vec::new();
    writer.write_binary_grammar(&mut blob).unwrap();

    // The stored pattern is the anchored form the compiler produced.
    let stored = format!("^\"{marker}\"$");
    let needle: Vec<u8> = (stored.len() as u32)
        .to_be_bytes()
        .iter()
        .copied()
        .chain(stored.bytes())
        .collect();
    let at = blob
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or_else(|| panic!("stored pattern {stored:?} not found in blob"));

    let replacement: Vec<u8> = (pattern.len() as u32)
        .to_be_bytes()
        .iter()
        .copied()
        .chain(pattern.bytes())
        .collect();
    let mut patched = blob[..at].to_vec();
    patched.extend_from_slice(&replacement);
    patched.extend_from_slice(&blob[at + needle.len()..]);
    patched
}

fn load(blob: &[u8]) -> Result<(), Cg3Error> {
    BinaryGrammar::new(Grammar::default()).parse_grammar_buffer(blob)
}

/// A `.cg3b` we write MUST carry the pattern the grammar author wrote, not
/// this engine's translation of it — otherwise the file stops being readable as
/// ICU and diverges from what C++ `vislcg3` would emit. The translation is an
/// implementation detail of matching and must not reach the wire.
// [spec:cg3:req:tag-regex.source-fidelity/test]
#[test]
fn written_patterns_keep_icu_spelling() {
    for tag in [r"\\Qa.b\\E", r"x\\Zy", r"^ab$"] {
        let src = format!("DELIMITERS = \"<.>\" ;\nLIST t = \"{tag}\"r ;\n");
        let mut parser = TextualParser::new(Grammar::default(), false);
        parser
            .parse_grammar_utf8(src.as_bytes())
            .unwrap_or_else(|e| panic!("{tag} must compile: {e}"));
        let mut grammar = parser.grammar;
        grammar.reindex(false, false).unwrap();
        let mut writer = BinaryGrammar::new(grammar);
        let mut blob: Vec<u8> = Vec::new();
        writer.write_binary_grammar(&mut blob).unwrap();

        // The parser anchors a bare tag as `^<tag>$`; that exact ICU-spelled
        // string is what must appear in the file.
        let unescaped = tag.replace("\\\\", "\\");
        let stored = format!("^\"{unescaped}\"$");
        assert!(
            blob.windows(stored.len()).any(|w| w == stored.as_bytes()),
            "{stored:?} not found in the written .cg3b (translation leaked)"
        );
    }
}

/// The `se.drb` case: ICU literal quoting must load, not abort the read.
#[test]
fn icu_literal_quoting_loads() {
    let blob = cg3b_with_pattern("abc", r#""\Q$1\E.*"S$"#);
    load(&blob).expect("a \\Q...\\E grammar must load");
}

/// Backtracking constructs are the reason for the engine choice: a `.cg3b`
/// written by C++ `vislcg3` may use any of them, and all must load.
#[test]
fn backtracking_constructs_load() {
    for pattern in [
        r"foo(?=bar)",
        r"(?<=foo)bar",
        r"(?<!foo)bar",
        r"(a)\1",
        r"(?>a*)b",
        r"a*+b",
    ] {
        let blob = cg3b_with_pattern("abc", pattern);
        load(&blob).unwrap_or_else(|e| panic!("{pattern} must load: {e}"));
    }
}

/// A construct this engine would MISREAD must name both the tag and the
/// construct, rather than surfacing as a bare exit code — or, worse, than
/// compiling to something that silently matches the wrong thing.
#[test]
fn unsupported_construct_names_tag_and_construct() {
    let blob = cg3b_with_pattern("abc", r"[:script=Greek:]");
    let err = load(&blob).expect_err("in-set property must be rejected");

    assert!(
        matches!(err, Cg3Error::Grammar(_)),
        "a grammar that will not load is a load failure, got {err:?}"
    );
    let reported = err.tag_regex_errors();
    assert_eq!(reported.len(), 1, "one bad tag");
    assert_eq!(reported[0].tag.as_deref(), Some("\"abc\""), "tag text");
    assert_eq!(
        reported[0].pattern, r"[:script=Greek:]",
        "offending pattern"
    );

    let rendered = err.to_string();
    assert!(rendered.contains("in-set property"), "{rendered}");
    assert!(rendered.contains("abc"), "{rendered}");
}

/// Every bad tag in one file is reported together, so a grammar author fixes
/// them in one pass instead of recompiling per tag.
#[test]
fn all_bad_tags_are_reported_together() {
    // Two regex tags, both patched to distinct unsupported constructs.
    let src = "DELIMITERS = \"<.>\" ;\nLIST a = \"one\"r ;\nLIST b = \"two\"r ;\n";
    let mut parser = TextualParser::new(Grammar::default(), false);
    parser.parse_grammar_utf8(src.as_bytes()).unwrap();
    let mut grammar = parser.grammar;
    grammar.reindex(false, false).unwrap();
    let mut writer = BinaryGrammar::new(grammar);
    let mut blob: Vec<u8> = Vec::new();
    writer.write_binary_grammar(&mut blob).unwrap();

    for (marker, pattern) in [("one", r"[:script=Greek:]"), ("two", r"\0101")] {
        let stored = format!("^\"{marker}\"$");
        let needle: Vec<u8> = (stored.len() as u32)
            .to_be_bytes()
            .iter()
            .copied()
            .chain(stored.bytes())
            .collect();
        let at = blob
            .windows(needle.len())
            .position(|w| w == needle)
            .unwrap();
        let replacement: Vec<u8> = (pattern.len() as u32)
            .to_be_bytes()
            .iter()
            .copied()
            .chain(pattern.bytes())
            .collect();
        let mut patched = blob[..at].to_vec();
        patched.extend_from_slice(&replacement);
        patched.extend_from_slice(&blob[at + needle.len()..]);
        blob = patched;
    }

    let err = load(&blob).expect_err("both tags must be rejected");
    let reported = err.tag_regex_errors();
    assert_eq!(reported.len(), 2, "both bad tags reported in one pass");

    let rendered = err.to_string();
    assert!(rendered.contains("in-set property"), "{rendered}");
    assert!(rendered.contains("octal escape"), "{rendered}");
}
