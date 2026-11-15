//! A compile that failed, on its way to Swift: `CompileFailed` carrying the
//! parse's diagnostics, or an error `CG3Error` names otherwise.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use cg3::diagnostics::render_parse_errors;
use cg3::error::{Cg3Error as LibraryError, GrammarError, ParseError, ParseSource};

// [spec:cg3:req:swift.compile-errors]
/// Where a diagnostic sits in the text of its source, as UTF-8 byte offsets.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

// [spec:cg3:req:swift.compile-errors]
/// One error a failed parse found.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Diagnostic {
    /// What went wrong, in the library's words.
    pub message: String,
    /// The name of the source it is in: the compiled source's, or an included
    /// file's as the grammar's `INCLUDE` resolved it.
    pub source: String,
    pub line: u32,
    /// The error as vislcg3 prints it, quoting the grammar, without colour.
    pub text: String,
    /// The name the caller gave the included file it is in; `None` in the
    /// compiled source itself.
    pub file: Option<String>,
    /// `None` for an error with no place.
    pub span: Option<Span>,
}

// [spec:cg3:req:swift.compile-errors]
/// Why a call into the compile glue failed, under the code `CG3Error` gives
/// it in Swift.
#[derive(Debug, PartialEq, Eq, uniffi::Error)]
pub enum CompileError {
    /// An argument or option the library cannot use.
    InvalidArgument { message: String },
    /// Bytes that are not a `.cg3b` the library reads, or relabel rules it
    /// cannot relabel with.
    InvalidGrammar { message: String },
    /// A textual grammar the library would not compile, with every error its
    /// parse found; none when the failure came after the parse.
    CompileFailed {
        message: String,
        diagnostics: Vec<Diagnostic>,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (CompileError::InvalidArgument { message }
        | CompileError::InvalidGrammar { message }
        | CompileError::CompileFailed { message, .. }) = self;
        f.write_str(message)
    }
}

impl std::error::Error for CompileError {}

impl From<cg3_swift::error::Cg3Error> for CompileError {
    fn from(error: cg3_swift::error::Cg3Error) -> CompileError {
        use cg3_swift::error::Cg3Error;
        match error {
            Cg3Error::InvalidArgument { message } => CompileError::InvalidArgument { message },
            Cg3Error::InvalidGrammar { message } | Cg3Error::RunFailed { message } => {
                CompileError::InvalidGrammar { message }
            }
        }
    }
}

impl CompileError {
    pub fn invalid_grammar(error: impl fmt::Display) -> CompileError {
        CompileError::InvalidGrammar {
            message: error.to_string(),
        }
    }

    // [spec:cg3:req:swift.compile-errors]
    /// The `CompileFailed` error for `error`, which the library returned for a
    /// textual grammar. Its message is the library's, followed by each error
    /// the parse found on a line of its own. `files` names the caller's file
    /// of each included source the parse read.
    pub fn failed(error: LibraryError, files: &HashMap<String, String>) -> CompileError {
        let LibraryError::Grammar(GrammarError::Parse { errors, sources }) = &error else {
            return CompileError::CompileFailed {
                message: error.to_string(),
                diagnostics: Vec::new(),
            };
        };
        let lines =
            std::iter::once(error.to_string()).chain(errors.iter().map(ToString::to_string));
        CompileError::CompileFailed {
            message: lines.collect::<Vec<_>>().join("\n"),
            diagnostics: errors
                .iter()
                .map(|e| diagnostic(e, sources, files))
                .collect(),
        }
    }
}

/// `error` rendered alone, as vislcg3 renders each error of a failed parse,
/// and placed in the source its span is in, else in the file the parse named.
/// The first source is the one compiled; the others are included files.
fn diagnostic(
    error: &ParseError,
    sources: &[ParseSource],
    files: &HashMap<String, String>,
) -> Diagnostic {
    let mut text = Vec::new();
    // The render only fails if the sink does, and the sink is a Vec.
    let _ = render_parse_errors(std::slice::from_ref(error), sources, &mut text, false);
    let placed = error.span.as_ref().and_then(|span| {
        let source = sources.get(span.source)?;
        Some((span.source, source, span.range.clone()))
    });
    Diagnostic {
        message: error.kind.to_string(),
        source: placed
            .as_ref()
            .map_or_else(|| error.file.clone(), |(_, source, _)| source.name.clone()),
        line: error.line,
        text: String::from_utf8_lossy(&text).into_owned(),
        file: placed
            .as_ref()
            .filter(|(index, _, _)| *index > 0)
            .and_then(|(_, source, _)| files.get(&source.name).cloned()),
        span: placed.map(|(_, source, chars)| utf8(&source.text, chars)),
    }
}

/// The char offsets `chars` into `text` as UTF-8 byte offsets. The parse
/// reads a source's text as the caller gave it, so they are offsets into the
/// caller's string too.
fn utf8(text: &str, chars: Range<usize>) -> Span {
    let offset = |count: usize| {
        text.char_indices()
            .nth(count)
            .map_or(text.len(), |(at, _)| at) as u64
    };
    Span {
        start: offset(chars.start),
        end: offset(chars.end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:cg3:req:swift.compile-errors/test]
    #[test]
    fn spans_count_utf8_bytes() {
        let text = "a\u{1F600}b\u{e5}c";
        assert_eq!(utf8(text, 0..1), Span { start: 0, end: 1 });
        assert_eq!(
            utf8(text, 1..2),
            Span { start: 1, end: 5 },
            "one char outside the BMP is four bytes"
        );
        assert_eq!(utf8(text, 2..5), Span { start: 5, end: 9 });
    }
}
