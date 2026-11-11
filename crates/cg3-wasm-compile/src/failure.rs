//! A compile that failed, on its way to JavaScript: a `compile-failed` error
//! carrying the parse's diagnostics, an error `wasm.errors` names, or what the
//! caller's include function threw.

use std::ops::Range;

use cg3::diagnostics::render_parse_errors;
use cg3::error::{Cg3Error, GrammarError, ParseError, ParseSource};
use cg3_wasm_shared::{ErrorCode, GlueError, set};
use wasm_bindgen::JsValue;

// [spec:cg3:req:wasm.compile-errors]
/// One error a failed parse found, as `Diagnostic` in `js/cg3/errors.ts`
/// declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// What went wrong, in the library's words.
    pub message: String,
    /// The name of the source it is in: the compiled source's, or an included
    /// file's as the grammar's `INCLUDE` resolved it.
    pub source: String,
    pub line: u32,
    /// The error as vislcg3 prints it, quoting the grammar, without colour.
    pub text: String,
    /// Where it sits in its source's text, in UTF-16 code units, which is how
    /// JavaScript indexes a string. `None` for an error with no place.
    pub span: Option<Range<usize>>,
}

// [spec:cg3:req:wasm.compile-errors]
/// Why a call into the compile module failed.
#[derive(Debug)]
pub enum CompileError {
    /// A failure `wasm.errors` names: an argument the module cannot use, or
    /// bytes that are not a `.cg3b`.
    Glue(GlueError),
    /// A textual grammar the library would not compile, with every error its
    /// parse found; none when the failure came after the parse.
    Failed {
        message: String,
        diagnostics: Vec<Diagnostic>,
    },
    /// What the caller's include function threw, rethrown as it was.
    Thrown(JsValue),
}

impl CompileError {
    // [spec:cg3:req:wasm.compile-errors]
    /// The `compile-failed` error for `error`, which the library returned for
    /// a textual grammar. Its message is the library's, followed by each error
    /// the parse found on a line of its own.
    pub fn failed(error: Cg3Error) -> CompileError {
        let Cg3Error::Grammar(GrammarError::Parse { errors, sources }) = &error else {
            return CompileError::Failed {
                message: error.to_string(),
                diagnostics: Vec::new(),
            };
        };
        let lines =
            std::iter::once(error.to_string()).chain(errors.iter().map(ToString::to_string));
        CompileError::Failed {
            message: lines.collect::<Vec<_>>().join("\n"),
            diagnostics: errors.iter().map(|e| diagnostic(e, sources)).collect(),
        }
    }

    /// The code JavaScript sees.
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            CompileError::Glue(e) => Some(e.code()),
            CompileError::Failed { .. } => Some(ErrorCode::CompileFailed),
            CompileError::Thrown(_) => None,
        }
    }

    /// The message JavaScript sees, but for a thrown value, which is the
    /// caller's own.
    pub fn message(&self) -> Option<&str> {
        match self {
            CompileError::Glue(e) => Some(e.message()),
            CompileError::Failed { message, .. } => Some(message),
            CompileError::Thrown(_) => None,
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            CompileError::Failed { diagnostics, .. } => diagnostics,
            _ => &[],
        }
    }
}

impl From<GlueError> for CompileError {
    fn from(error: GlueError) -> CompileError {
        CompileError::Glue(error)
    }
}

/// `error` rendered alone, as vislcg3 renders each error of a failed parse,
/// and placed in the source its span is in, else in the file the parse named.
fn diagnostic(error: &ParseError, sources: &[ParseSource]) -> Diagnostic {
    let mut text = Vec::new();
    // The render only fails if the sink does, and the sink is a Vec.
    let _ = render_parse_errors(std::slice::from_ref(error), sources, &mut text, false);
    let placed = error
        .span
        .as_ref()
        .and_then(|span| Some((sources.get(span.source)?, span.range.clone())));
    Diagnostic {
        message: error.kind.to_string(),
        source: placed
            .as_ref()
            .map_or_else(|| error.file.clone(), |(source, _)| source.name.clone()),
        line: error.line,
        text: String::from_utf8_lossy(&text).into_owned(),
        span: placed.map(|(source, chars)| utf16(&source.text, chars)),
    }
}

/// The char offsets `chars` into `text` as UTF-16 offsets.
fn utf16(text: &str, chars: Range<usize>) -> Range<usize> {
    let offset = |count: usize| text.chars().take(count).map(char::len_utf16).sum();
    offset(chars.start)..offset(chars.end)
}

impl From<CompileError> for JsValue {
    fn from(error: CompileError) -> JsValue {
        let diagnostics = match error {
            CompileError::Glue(error) => return error.into(),
            CompileError::Thrown(thrown) => return thrown,
            CompileError::Failed {
                ref diagnostics, ..
            } => diagnostics
                .iter()
                .map(diagnostic_object)
                .collect::<js_sys::Array>(),
        };
        let object = js_sys::Object::new();
        set(
            &object,
            "code",
            &JsValue::from_str(ErrorCode::CompileFailed.as_str()),
        );
        set(
            &object,
            "message",
            &JsValue::from_str(error.message().unwrap_or_default()),
        );
        set(&object, "diagnostics", &diagnostics);
        object.into()
    }
}

fn diagnostic_object(diagnostic: &Diagnostic) -> JsValue {
    let object = js_sys::Object::new();
    set(&object, "message", &JsValue::from_str(&diagnostic.message));
    set(&object, "source", &JsValue::from_str(&diagnostic.source));
    set(&object, "line", &JsValue::from(diagnostic.line));
    set(&object, "text", &JsValue::from_str(&diagnostic.text));
    let span = match &diagnostic.span {
        None => JsValue::NULL,
        Some(range) => {
            let span = js_sys::Object::new();
            set(&span, "start", &JsValue::from_f64(range.start as f64));
            set(&span, "end", &JsValue::from_f64(range.end as f64));
            span.into()
        }
    };
    set(&object, "span", &span);
    object.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:cg3:req:wasm.compile-errors/test]
    #[test]
    fn spans_count_utf16_code_units() {
        let text = "a\u{1F600}b\u{e5}c";
        assert_eq!(utf16(text, 0..1), 0..1);
        assert_eq!(
            utf16(text, 1..2),
            1..3,
            "one char outside the BMP is two units"
        );
        assert_eq!(utf16(text, 2..5), 3..6);
    }
}
