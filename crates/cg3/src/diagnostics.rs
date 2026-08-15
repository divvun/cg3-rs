//! Rendering a failed grammar parse for a person.
//!
//! [`crate::error`] governs what a failure VALUE carries; this module turns one
//! into what a grammar author actually needs: the line they wrote, with the part
//! that failed marked in it. See `docs/spec/port/src/diagnostics.md`.
//!
//! The C++ printed a line number and twenty characters of context at the raise
//! site and then terminated, because that was all a `[[noreturn]]` error
//! reporter could do. A `ParseError` now carries a span into a source the parse
//! retained, which is enough to quote the grammar, so it does.
//!
//! ## Why this bypasses `tracing`
//! A rendered report is a multi-line block laid out for a terminal — box-drawing
//! characters, a gutter, colour — not an event with fields. Passing it through
//! the subscriber [`crate::tools::init_diagnostics`] installs would prefix every
//! line with `ERROR` and strip the colour it deliberately disables
//! (`.with_ansi(false)`). So the report goes to the stream directly, and the
//! per-error `tracing` event that used to carry the same text is a `debug!`
//! rather than an `error!`: the value is the embedder's channel, the report is
//! the user's, and neither needs the log to repeat it
//! (`[spec:cg3:req:diagnostics.rendered]`).

use std::io::{IsTerminal, Write};

use ariadne::{Config, IndexType, Label, Report, ReportKind};

use crate::error::{ParseError, ParseSource};

// [spec:cg3:req:diagnostics.rendered]
/// Render every error of a failed parse to stderr, then flush.
///
/// The default for a failed grammar load on every CLI path — a grammar author is
/// exactly who a syntax error is for, so it is not behind a flag.
pub fn report_parse_failure(errors: &[ParseError], sources: &[ParseSource]) {
    let mut out = Vec::new();
    let colour = std::io::stderr().is_terminal();
    // The render only fails if the sink does, and the sink is a Vec.
    let _ = render_parse_errors(errors, sources, &mut out, colour);
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(&out);
    let _ = stderr.flush();
}

// [spec:cg3:req:diagnostics.colour-at-tty]
/// Render every error of a failed parse into `out`.
///
/// `colour` is the caller's, because only the caller knows what the bytes end up
/// in: [`report_parse_failure`] takes it from whether stderr is a terminal, and
/// a test takes it as `false` so it can assert on the text. Ariadne's own
/// `auto-color` feature is not enabled — a second detector reading process
/// state could only disagree with the stream in hand.
pub fn render_parse_errors(
    errors: &[ParseError],
    sources: &[ParseSource],
    out: &mut impl Write,
    colour: bool,
) -> std::io::Result<()> {
    // Built once and reborrowed per report: `Cache` is implemented for
    // `&mut C`, so the sources are not cloned per error.
    let mut cache = ariadne::sources(sources.iter().map(|s| (s.name.clone(), s.text.clone())));
    // The parser is char-indexed end to end — `UChar` is `char` and a parse
    // buffer is a `Vec<char>` — so spans are char offsets and ariadne needs no
    // byte-mapping table. `IndexType::Char` is ariadne 0.6's default; it is set
    // here anyway, because the whole span contract turns on it.
    let config = Config::new()
        .with_color(colour)
        .with_index_type(IndexType::Char);

    for error in errors {
        let Some(placed) = place(error, sources) else {
            // Nothing to quote: an empty input, a template reference resolved
            // after the buffer walk ended, a tag the running stream asked for.
            // The error still knows its file, line and near-context, so it says
            // what it can rather than being dropped.
            writeln!(
                out,
                "{} {error}",
                if colour {
                    "\u{1b}[31mError:\u{1b}[0m"
                } else {
                    "Error:"
                }
            )?;
            continue;
        };
        Report::build(ReportKind::Error, placed.clone())
            .with_config(config)
            // What went wrong, in the kind's own words — already specific
            // (`unknown template 'x'`, `cannot compile regex for tag "…"`), and
            // long enough for the headline to be the only place it fits.
            .with_message(&error.kind)
            .with_label(
                Label::new(placed)
                    .with_message(marked(&error.kind))
                    .with_color(ariadne::Color::Red),
            )
            .finish()
            .write(&mut cache, &mut *out)?;
    }
    Ok(())
}

/// What the underlined text IS.
///
/// Not a second diagnostic message — the report's headline already carries the
/// kind's own words, and repeating a 150-character regex complaint on the arrow
/// would say nothing twice. This names the marked span so the arrow means
/// something, which ariadne needs a label message to draw at all.
fn marked(kind: &crate::error::ParseErrorKind) -> &'static str {
    use crate::error::ParseErrorKind as K;
    match kind {
        K::Syntax => "the parse stopped here",
        K::TagRegex { .. } | K::EmptyTag | K::TagStartsWithParen { .. } => "this tag",
        K::UnknownTemplate { .. } => "this reference",
        K::TemplateRedefined { .. }
        | K::AnchorRedefined { .. }
        | K::SetRedefined { .. }
        | K::SetContentCollision => "this definition",
        K::EmptyNumericBranch => "this branch",
        K::IncludeUnreadable { .. } => "this directive",
        // Marked on the whole rule: the tag that failed came off the running
        // stream, so the only thing in the grammar to point at is the rule that
        // asked for it (`[spec:cg3:req:diagnostics.runtime-placed]`).
        K::RuntimeTag { .. } => "this rule asked for it",
        // Spanless in practice — an empty input has no line to mark — so this
        // arm exists to keep the match total rather than to be read.
        K::EmptyInput => "here",
    }
}

/// The ariadne span for an error, if it has a place in a source that still
/// exists.
///
/// Clamped rather than trusted: ariadne panics on a reversed or out-of-bounds
/// range, and a diagnostic renderer is the last place that should be able to
/// bring down the process it is explaining a failure to.
fn place(error: &ParseError, sources: &[ParseSource]) -> Option<(String, std::ops::Range<usize>)> {
    let span = error.span.as_ref()?;
    let source = sources.get(span.source)?;
    let len = source.text.chars().count();
    if len == 0 {
        return None;
    }
    let start = span.range.start.min(len - 1);
    let end = span.range.end.max(start + 1).min(len);
    Some((source.name.clone(), start..end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ParseErrorKind, ParseSpan};

    fn source() -> Vec<ParseSource> {
        vec![ParseSource {
            name: "nb.cg3".to_string(),
            text: "DELIMITERS = \"<.>\" ;\nLIST a = bogus ;\n".to_string(),
        }]
    }

    fn render(errors: &[ParseError]) -> String {
        let mut out = Vec::new();
        render_parse_errors(errors, &source(), &mut out, false).expect("a Vec cannot fail");
        String::from_utf8(out).expect("ariadne emits UTF-8")
    }

    /// A placed error is rendered as its source line with the failure marked in
    /// it, headed by the file it came from — not as a line number and twenty
    /// characters of context.
    // [spec:cg3:req:diagnostics.rendered/test]
    #[test]
    fn a_placed_error_quotes_its_line() {
        let rendered = render(&[ParseError {
            file: "nb.cg3".to_string(),
            line: 2,
            near: "bogus ;".to_string(),
            span: Some(ParseSpan {
                source: 0,
                range: 30..35,
            }),
            kind: ParseErrorKind::UnknownTemplate {
                name: "t".to_string(),
            },
        }]);
        assert!(rendered.contains("unknown template `t`"), "{rendered}");
        assert!(rendered.contains("nb.cg3:2:10"), "{rendered}");
        assert!(
            rendered.contains("LIST a = bogus ;"),
            "the offending line must be quoted: {rendered}"
        );
        assert!(
            rendered.contains('┬') && rendered.contains("this reference"),
            "the failure must be marked in the quoted line: {rendered}"
        );
    }

    /// Colour is the caller's call, and a non-terminal caller gets none.
    // [spec:cg3:req:diagnostics.colour-at-tty/test]
    #[test]
    fn colour_is_off_when_asked_for_off() {
        let error = ParseError {
            file: "nb.cg3".to_string(),
            line: 2,
            near: String::new(),
            span: Some(ParseSpan {
                source: 0,
                range: 30..35,
            }),
            kind: ParseErrorKind::Syntax,
        };
        assert!(!render(&[error]).contains('\u{1b}'));
    }

    /// An error with nowhere to point still says everything it knows, rather
    /// than being dropped for want of a span.
    // [spec:cg3:req:diagnostics.rendered/test]
    #[test]
    fn an_unplaced_error_still_reports() {
        let rendered = render(&[ParseError {
            file: "nb.cg3".to_string(),
            line: 0,
            near: String::new(),
            span: None,
            kind: ParseErrorKind::EmptyInput,
        }]);
        assert!(rendered.contains("input is empty"), "{rendered}");
    }

    /// A span past the end of its source is clamped, not trusted: ariadne
    /// panics on an out-of-range one, and a renderer must not be able to bring
    /// down the process it is explaining a failure to.
    // [spec:cg3:req:diagnostics.rendered/test]
    #[test]
    fn an_impossible_span_does_not_panic() {
        let rendered = render(&[ParseError {
            file: "nb.cg3".to_string(),
            line: 9,
            near: String::new(),
            span: Some(ParseSpan {
                source: 0,
                range: 9000..9001,
            }),
            kind: ParseErrorKind::Syntax,
        }]);
        assert!(rendered.contains("syntax error"), "{rendered}");
    }
}
