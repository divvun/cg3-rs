//! Tag regex compilation — the single ICU-compatibility seam.
//!
//! PORT DIVERGENCE. The C++ compiles every tag pattern with ICU's `uregex_open`.
//! Grammars in the wild are authored against ICU, so their patterns use ICU
//! syntax; the port must accept that syntax or quietly stop matching.
//!
//! The engine here is [`fancy_regex`], not the `regex` crate. `regex` is a
//! finite-automaton engine and structurally cannot do lookaround or
//! backreferences; worse, it accepts possessive quantifiers and reads `a*+` as
//! `(?:a*)+`, matching where ICU would not. fancy-regex covers all of those and
//! is ICU-exact on possessive semantics. It routes patterns with no
//! backtracking construct to regex-automata internally, so ordinary tags keep
//! the linear-time engine for free.
//!
//! Everything that compiles a tag or text-delimiter pattern goes through
//! [`compile_tag_regex`]. Before this seam existed the four compile sites had
//! four different failure behaviours — two discarded the underlying error, and
//! the delimiter sites printed nothing at all.
//!
//! Translated, because ICU spells these differently:
//!   * `\Q...\E` literal quoting → an escaped literal. fancy-regex has no `\Q`.
//!   * `\uXXXX` / `\UXXXXXXXX` → `\u{XXXX}`.
//!   * `\Z` → an explicit lookahead over ICU's line-terminator set. Both
//!     engines have a `\Z`, but fancy-regex's ignores ALL trailing newlines and
//!     recognises only `\n`, where ICU allows exactly one terminator drawn from
//!     a wider set. Translating is what makes it exact; it is also only
//!     expressible at all because this engine has lookahead.
//!
//! Reported by name, because the engine would otherwise match the WRONG thing
//! without complaining — the failure mode worth spending code on:
//!   * `[:name=value:]` in-set properties, which parse as a literal character
//!     set.
//!   * `\N{NAME}`, where fancy-regex's `\N` is "any char except newline" and
//!     the brace is literal, versus ICU's named character.
//!   * `\0ooo` octal escapes, which fancy-regex reads as a backreference.
//!
//! KNOWN DIVERGENCE, deliberately not translated: ICU's bare `$` matches before
//! a final line terminator (its `$` is `\Z`), while both Rust engines treat `$`
//! as end of haystack. Tag haystacks are single tags with no trailing newline,
//! so the two coincide there. Rewriting every `$` would change matching for
//! every anchored tag in every grammar, which is not a change to make on
//! spec-reading alone.

use std::sync::Arc;

use fancy_regex::{Regex, RegexBuilder};

/// Ceiling on backtracking steps for one match attempt.
///
/// Patterns come from user-authored grammars, so a catastrophic pattern is
/// reachable input, not a hypothetical. The limit turns what would be a hang
/// into an error the match sites report and treat as "no match". This is
/// fancy-regex's own default, set explicitly so it is a stated policy rather
/// than an inherited one.
// [spec:cg3:req:tag-regex.engine]
pub const BACKTRACK_LIMIT: usize = 1_000_000;

/// Why a tag's pattern would not compile.
#[derive(Debug, Clone)]
pub enum TagRegexErrorKind {
    /// An ICU construct this engine would accept but interpret differently.
    /// `construct` is the user-facing name, `offset` its char index in the
    /// original pattern.
    Unsupported {
        construct: &'static str,
        offset: usize,
    },
    /// The pattern reached the engine and was rejected there. Carries the
    /// underlying error so a consumer can inspect it rather than parse text.
    ///
    /// `Arc` because [`fancy_regex::Error`] is neither `Clone` nor `PartialEq`,
    /// and [`crate::error::Cg3Error`] is both.
    Syntax(Arc<fancy_regex::Error>),
}

/// Compares [`TagRegexErrorKind::Syntax`] by rendered message, since
/// [`fancy_regex::Error`] does not implement `PartialEq`.
impl PartialEq for TagRegexErrorKind {
    fn eq(&self, other: &TagRegexErrorKind) -> bool {
        match (self, other) {
            (
                TagRegexErrorKind::Unsupported {
                    construct: a,
                    offset: x,
                },
                TagRegexErrorKind::Unsupported {
                    construct: b,
                    offset: y,
                },
            ) => a == b && x == y,
            (TagRegexErrorKind::Syntax(a), TagRegexErrorKind::Syntax(b)) => {
                a.to_string() == b.to_string()
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for TagRegexErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagRegexErrorKind::Unsupported { construct, offset } => write!(
                f,
                "{construct} at offset {offset} is an ICU regex feature this engine reads differently"
            ),
            TagRegexErrorKind::Syntax(e) => write!(f, "{e}"),
        }
    }
}

/// A tag whose regex failed to compile, with everything needed to find it in the
/// grammar. `tag` and `line` are filled in where the call site knows them —
/// the binary reader has the tag text but no line, the textual parser has both.
#[derive(Debug, Clone, PartialEq)]
pub struct TagRegexError {
    /// The pattern as handed to the compiler, before translation.
    pub pattern: String,
    /// The tag text this pattern came from, when the call site knows it.
    pub tag: Option<String>,
    /// Grammar line, when the call site knows it.
    pub line: Option<u32>,
    pub kind: TagRegexErrorKind,
}

impl TagRegexError {
    /// Attach the tag text a call site knows but [`compile_tag_regex`] does not.
    pub fn with_tag(mut self, tag: impl Into<String>) -> TagRegexError {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a grammar line number.
    pub fn with_line(mut self, line: u32) -> TagRegexError {
        self.line = Some(line);
        self
    }
}

impl std::fmt::Display for TagRegexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot compile regex for tag ")?;
        match &self.tag {
            Some(t) => write!(f, "{t}")?,
            None => write!(f, "<unknown>")?,
        }
        if let Some(line) = self.line {
            write!(f, " on line {line}")?;
        }
        write!(f, ": {}", self.kind)?;
        if self.tag.as_deref() != Some(self.pattern.as_str()) {
            write!(f, " (pattern: {})", self.pattern)?;
        }
        Ok(())
    }
}

impl std::error::Error for TagRegexError {}

/// Compile a tag pattern, translating ICU-only syntax first.
///
/// `case_insensitive` is applied via [`RegexBuilder`] rather than by injecting
/// `(?i)` into the pattern text, so `Regex::as_str()` still round-trips the bare
/// pattern the way C++ `uregex_pattern` does — the binary writer serialises
/// `as_str()`, so an injected `(?i)` would leak into every `.cg3b` we emit. The
/// flag itself survives a `.cg3b` round-trip in `Tag::type`'s
/// `T_CASE_INSENSITIVE` bit, which the reader re-derives.
// [spec:cg3:req:tag-regex.single-seam]
pub fn compile_tag_regex(
    pattern: &str,
    case_insensitive: bool,
) -> Result<Regex, Box<TagRegexError>> {
    let err = |kind| {
        Box::new(TagRegexError {
            pattern: pattern.to_string(),
            tag: None,
            line: None,
            kind,
        })
    };

    let translated = translate_icu_pattern(pattern).map_err(err)?;

    RegexBuilder::new(&translated)
        .case_insensitive(case_insensitive)
        .backtrack_limit(BACKTRACK_LIMIT)
        .build()
        .map_err(|e| err(TagRegexErrorKind::Syntax(Arc::new(e))))
}

/// Match `haystack`, treating a runtime failure as "no match".
///
/// Most match sites are predicates returning `u32`/`bool` with no error channel
/// — threading `Result` through the matcher cluster would change signatures all
/// the way up for a case that only fires on a pathological pattern. The C++
/// `uregex_find` error path was `CG3Quit(1)`, so degrading to "no match" is a
/// divergence; it is logged rather than swallowed so a grammar that trips
/// [`BACKTRACK_LIMIT`] is diagnosable instead of merely mysterious.
// [spec:cg3:req:tag-regex.engine]
pub fn is_match_or_false(re: &Regex, haystack: &str) -> bool {
    match re.is_match(haystack) {
        Ok(matched) => matched,
        Err(e) => {
            tracing::warn!(
                "Warning: regex match failed for pattern `{}` - treating as no match: {}",
                re.as_str(),
                e
            );
            false
        }
    }
}

/// ICU's `\Z`: end of input, or before a single final line terminator.
///
/// ICU's terminator set is `\n \v \f \r` plus U+0085, U+2028, U+2029 and the
/// `\r\n` pair. Spelled as a lookahead so it stays zero-width, matching `\Z`'s
/// match offsets as well as its match/no-match answer.
const ICU_END_ANCHOR: &str = r"(?=(?:\r\n|[\n\x{0b}\x{0c}\r\x{85}\x{2028}\x{2029}])?\z)";

/// Escape a `\Q...\E` span for use as literal text.
///
/// `regex::escape` is exact for everything except whitespace, which it leaves
/// bare — and bare whitespace changes meaning under `(?x)` free-spacing, which
/// ICU honours inside a quoted span. Emit those as `\x{..}` so the span stays
/// literal under every flag combination.
fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_whitespace() {
            out.push_str(&format!("\\x{{{:x}}}", c as u32));
        } else {
            out.push_str(&regex::escape(c.encode_utf8(&mut [0u8; 4])));
        }
    }
    out
}

/// Rewrite the ICU constructs this engine spells differently, and reject the
/// ones it would silently misread.
///
/// Single pass, because `\Q...\E` spans suppress all other interpretation: a
/// `\Z` inside a quoted span is literal text, so detection and translation
/// cannot be separate passes over the same string.
// [spec:cg3:req:tag-regex.icu-translation]
// [spec:cg3:req:tag-regex.silent-divergence]
pub fn translate_icu_pattern(pattern: &str) -> Result<String, TagRegexErrorKind> {
    let cs: Vec<char> = pattern.chars().collect();
    let n = cs.len();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0usize;
    // Character classes suppress grouping syntax: `[(?=]` is a class of four
    // literal chars.
    let mut in_class = false;
    // Index at which a `]` is still literal (directly after `[` or `[^`).
    let mut class_lit_bracket = usize::MAX;

    let unsupported = |construct, offset| TagRegexErrorKind::Unsupported { construct, offset };

    while i < n {
        let c = cs[i];

        if c == '\\' {
            let Some(&next) = cs.get(i + 1) else {
                // Trailing backslash — leave it for the engine to complain
                // about, so the error names the real problem rather than ours.
                out.push('\\');
                i += 1;
                continue;
            };
            match next {
                // Literal span. Everything up to the FIRST `\E` is literal,
                // including backslashes — so `\Q a\b \E` quotes `a\b`, and an
                // unterminated `\Q` runs to the end of the pattern (ICU).
                'Q' => {
                    let start = i + 2;
                    let mut j = start;
                    while j < n && !(cs[j] == '\\' && cs.get(j + 1) == Some(&'E')) {
                        j += 1;
                    }
                    let literal: String = cs[start..j.min(n)].iter().collect();
                    out.push_str(&escape_literal(&literal));
                    i = if j < n { j + 2 } else { n };
                }
                // A bare `\E` with no open `\Q` is NOT ignored: ICU has no `E`
                // row in its backslash state table and `E` is not in its
                // unescape set, so it falls through to an escaped literal.
                // Verified against ICU 78.3 — `a\Eb` matches `aEb`, not `ab`.
                'E' => {
                    out.push('E');
                    i += 2;
                }
                // Both engines have `\Z`, with different answers. See
                // ICU_END_ANCHOR.
                'Z' => {
                    out.push_str(ICU_END_ANCHOR);
                    i += 2;
                }
                // ICU's fixed-width escapes, spelled braced.
                'u' | 'U' => {
                    let want = if next == 'u' { 4 } else { 8 };
                    let digits: String = cs[(i + 2).min(n)..(i + 2 + want).min(n)].iter().collect();
                    if digits.chars().count() == want
                        && digits.chars().all(|d| d.is_ascii_hexdigit())
                    {
                        out.push_str("\\u{");
                        out.push_str(&digits);
                        out.push('}');
                        i += 2 + want;
                    } else {
                        out.push('\\');
                        out.push(next);
                        i += 2;
                    }
                }
                // ICU: the named character. fancy-regex: `\N` is "any char
                // except newline" and `{NAME}` is literal — a silent mismatch.
                'N' if cs.get(i + 2) == Some(&'{') => {
                    return Err(unsupported("`\\N{NAME}` named character", i));
                }
                // ICU: an octal escape. fancy-regex: a backreference.
                '0' => return Err(unsupported("`\\0` octal escape", i)),
                other => {
                    out.push('\\');
                    out.push(other);
                    i += 2;
                    // Consume a brace argument (`\p{Lu}`, `\x{1F600}`, …) as
                    // one unit so its contents are never rescanned as syntax.
                    if matches!(other, 'p' | 'P' | 'x') && cs.get(i) == Some(&'{') {
                        while i < n {
                            out.push(cs[i]);
                            i += 1;
                            if cs[i - 1] == '}' {
                                break;
                            }
                        }
                    }
                }
            }
            continue;
        }

        // ICU in-set property syntax. Both Rust engines read `[:script=Greek:]`
        // as a literal set of the characters `:scriptGek=` and match the wrong
        // thing without complaining. (`[[:alpha:]]`, which has no `=`, is a
        // real POSIX class and passes.)
        if c == '[' && cs.get(i + 1) == Some(&':') {
            let mut j = i + 2;
            while j + 1 < n && !(cs[j] == ':' && cs[j + 1] == ']') {
                j += 1;
            }
            if j + 1 < n && cs[i + 2..j].contains(&'=') {
                return Err(unsupported("ICU in-set property `[:name=value:]`", i));
            }
        }

        if in_class {
            if c == ']' && i != class_lit_bracket {
                in_class = false;
            }
            out.push(c);
            i += 1;
            continue;
        }

        if c == '[' {
            in_class = true;
            out.push(c);
            i += 1;
            if cs.get(i) == Some(&'^') {
                out.push('^');
                i += 1;
            }
            class_lit_bracket = i;
            continue;
        }

        out.push(c);
        i += 1;
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(p: &str) -> String {
        translate_icu_pattern(p).expect("should translate")
    }

    fn kind(p: &str) -> TagRegexErrorKind {
        translate_icu_pattern(p).expect_err("should reject")
    }

    fn matches(pattern: &str, haystack: &str) -> bool {
        compile_tag_regex(pattern, false)
            .unwrap_or_else(|e| panic!("{pattern} should compile: {e}"))
            .is_match(haystack)
            .expect("match must not fail")
    }

    #[test]
    fn quoted_span_becomes_escaped_literal() {
        assert_eq!(tr(r"\Qa.b\E"), regex::escape("a.b"));
        assert_eq!(tr(r"x\Q.\Ey"), format!("x{}y", regex::escape(".")));
    }

    /// The reproducer: `grc-disambiguator.bin` in the `se.drb` bundle.
    // [spec:cg3:req:tag-regex.icu-translation/test]
    #[test]
    fn se_drb_reproducer_compiles() {
        let pattern = r#""\Q$1\E.*"S$"#;
        assert!(matches(pattern, r#""$1foo"S"#));
        // The quoted span is literal: `$1` must not act as an anchor or group.
        assert!(!matches(pattern, r#""xfoo"S"#));
    }

    /// `\\Q` is an escaped backslash followed by a literal `Q`, NOT a quote start.
    #[test]
    fn escaped_backslash_does_not_open_a_span() {
        assert_eq!(tr(r"\\Q.\\E"), r"\\Q.\\E");
        assert!(matches(r"\\Q", r"\Q"));
    }

    /// ICU runs an unterminated `\Q` to the end of the pattern.
    #[test]
    fn unterminated_span_runs_to_end() {
        assert_eq!(tr(r"\Qa.b"), regex::escape("a.b"));
    }

    /// A bare `\E` is an escaped literal `E` in ICU, NOT a no-op. Verified
    /// against ICU 78.3: `a\Eb` matches `aEb` and does not match `ab`.
    // [spec:cg3:req:tag-regex.icu-translation/test]
    #[test]
    fn bare_end_quote_is_a_literal_e() {
        assert_eq!(tr(r"a\Eb"), "aEb");
        assert!(matches(r"a\Eb", "aEb"));
        assert!(!matches(r"a\Eb", "ab"));
    }

    /// The first `\E` closes the span, so a preceding backslash stays literal.
    #[test]
    fn first_end_marker_wins() {
        assert_eq!(tr(r"\Qa\\Eb"), format!("{}b", regex::escape(r"a\")));
    }

    /// A quoted span stays literal under free-spacing, which ICU honours
    /// inside `\Q...\E`.
    #[test]
    fn quoted_whitespace_survives_free_spacing() {
        assert!(matches(r"(?x)\Qa b\E", "a b"));
        assert!(!matches(r"(?x)\Qa b\E", "ab"));
    }

    #[test]
    fn fixed_width_unicode_escapes_are_braced() {
        assert_eq!(tr("\\u00e6"), r"\u{00e6}");
        assert_eq!(tr(r"\U0001F600"), r"\u{0001F600}");
        // Too few digits: left alone rather than mangled.
        assert_eq!(tr(r"\u12"), r"\u12");
        assert!(matches("\\u00e6", "æ"));
    }

    /// ICU's `\Z` allows exactly one trailing terminator, from a set wider than
    /// `\n`. fancy-regex's own `\Z` gets both halves wrong, so it is translated.
    // [spec:cg3:req:tag-regex.icu-translation/test]
    #[test]
    fn end_anchor_matches_icu() {
        for (haystack, expected) in [
            ("ab", true),
            ("ab\n", true),
            ("ab\r\n", true),
            ("ab\r", true),
            ("ab\u{85}", true),
            ("ab\u{2029}", true),
            // Two terminators: ICU allows only the final one.
            ("ab\n\n", false),
            ("ab\nx", false),
        ] {
            assert_eq!(matches(r"ab\Z", haystack), expected, "for {haystack:?}");
        }
    }

    /// The whole point of the engine swap: these are SUPPORTED now, and must
    /// not be rejected by a stale guard.
    // [spec:cg3:req:tag-regex.engine/test]
    #[test]
    fn backtracking_constructs_are_supported() {
        assert!(matches(r"foo(?=bar)", "foobar"));
        assert!(!matches(r"foo(?=bar)", "foobaz"));
        assert!(matches(r"(?<=foo)bar", "foobar"));
        assert!(matches(r"(?<!foo)bar", "bazbar"));
        assert!(matches(r"(a)\1", "aa"));
        assert!(matches(r"(?<x>a)\k<x>", "aa"));
        assert!(matches(r"(?>a*)b", "aaab"));
    }

    /// ICU-exact possessive semantics — `a*+a` must NOT match, because the
    /// possessive quantifier never gives characters back. The `regex` crate
    /// silently read this as `(?:a*)+` and matched.
    // [spec:cg3:req:tag-regex.engine/test]
    #[test]
    fn possessive_quantifiers_are_icu_exact() {
        assert!(!matches(r"a*+a", "aaa"));
        assert!(!matches(r"a++a", "aaa"));
        assert!(matches(r"a*+b", "aaab"));
    }

    /// Constructs this engine would misread rather than reject.
    // [spec:cg3:req:tag-regex.silent-divergence/test]
    #[test]
    fn silent_divergences_are_named() {
        for (pattern, expected) in [
            (r"[:script=Greek:]", "ICU in-set property `[:name=value:]`"),
            (r"\N{LATIN SMALL LETTER A}", "`\\N{NAME}` named character"),
            (r"\0101", "`\\0` octal escape"),
        ] {
            match kind(pattern) {
                TagRegexErrorKind::Unsupported { construct, .. } => {
                    assert_eq!(construct, expected, "for {pattern}")
                }
                other => panic!("{pattern}: expected Unsupported, got {other:?}"),
            }
        }
    }

    /// A `}` closing a Unicode-property brace must not be rescanned as syntax.
    /// Regression test for `test/Apertium/T_MergeCohorts`.
    #[test]
    fn brace_escapes_survive() {
        for pattern in [
            r#"^"(\p{Lu}\p{L}+)"$"#,
            r"\p{Lu}+",
            r"\x{1F600}+",
            r"[\p{L}]+",
        ] {
            assert!(
                compile_tag_regex(pattern, false).is_ok(),
                "{pattern} should compile"
            );
        }
    }

    /// Supported grouping must not be mistaken for something else.
    #[test]
    fn supported_groups_pass_through() {
        for pattern in [r"(?:ab)", r"(?i)ab", r"(?<name>a)", r"(?s).*"] {
            assert!(
                compile_tag_regex(pattern, false).is_ok(),
                "{pattern} should compile"
            );
        }
    }

    /// Character classes suppress grouping syntax.
    #[test]
    fn character_classes_are_inert() {
        for pattern in [
            r"[(?=]",
            r"[*+]",
            r"[]a]",
            r"[^]a]",
            r"[\]]",
            r"[[:alpha:]]",
        ] {
            assert!(
                compile_tag_regex(pattern, false).is_ok(),
                "{pattern} should compile"
            );
        }
    }

    // [spec:cg3:req:tag-regex.single-seam/test]
    #[test]
    fn case_insensitivity_leaves_the_pattern_bare() {
        let re = compile_tag_regex("abc", true).expect("must compile");
        assert_eq!(re.as_str(), "abc", "(?i) must not leak into the pattern");
        assert!(re.is_match("ABC").unwrap());
    }

    /// Group numbering drives the capture loops and the `gc == 0` memo
    /// fast-path, so the group-0 convention must hold.
    #[test]
    fn captures_len_counts_group_zero() {
        let re = compile_tag_regex(r"(a)(b)", false).expect("must compile");
        assert_eq!(re.captures_len() - 1, 2, "two capture groups");
    }

    #[test]
    fn syntax_errors_carry_the_engine_error() {
        let e = compile_tag_regex("(unclosed", false).expect_err("must fail");
        assert!(matches!(e.kind, TagRegexErrorKind::Syntax(_)));
    }

    #[test]
    fn display_names_the_tag_and_the_construct() {
        let e = compile_tag_regex(r"[:script=Greek:]", false)
            .expect_err("must fail")
            .with_tag(r#""foo"r"#)
            .with_line(42);
        let s = e.to_string();
        assert!(s.contains(r#""foo"r"#), "{s}");
        assert!(s.contains("line 42"), "{s}");
        assert!(s.contains("in-set property"), "{s}");
    }

    /// A catastrophic pattern must surface as a bounded failure, not a hang.
    /// `is_match_or_false` turns it into "no match" for the predicate sites.
    // [spec:cg3:req:tag-regex.engine/test]
    #[test]
    fn backtrack_limit_degrades_to_no_match() {
        let re = RegexBuilder::new(r"((a+)+)b\2")
            .backtrack_limit(1_000)
            .build()
            .expect("pattern compiles");
        let haystack = "a".repeat(64);
        assert!(
            re.is_match(&haystack).is_err(),
            "the limit must actually trip"
        );
        assert!(!is_match_or_false(&re, &haystack), "degrades to no match");
    }
}
