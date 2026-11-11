//! Errors on their way to JavaScript.

use wasm_bindgen::JsValue;

// [spec:cg3:req:wasm.errors+1]
// [spec:cg3:req:wasm.compile-errors]
/// The codes of the errors the modules raise themselves. The wrapper raises
/// the rest of `Cg3ErrorCode` in `js/cg3/errors.ts`, which documents them all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    /// A run option the package cannot use.
    InvalidArgument,
    /// Bytes that are not a `.cg3b` the library reads.
    InvalidGrammar,
    RunFailed,
    /// A textual grammar the library would not compile. Only the compile
    /// module raises it, with the parse's diagnostics beside the message.
    CompileFailed,
}

impl ErrorCode {
    pub const ALL: [ErrorCode; 4] = [
        ErrorCode::InvalidArgument,
        ErrorCode::InvalidGrammar,
        ErrorCode::RunFailed,
        ErrorCode::CompileFailed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidArgument => "invalid-argument",
            ErrorCode::InvalidGrammar => "invalid-grammar",
            ErrorCode::RunFailed => "run-failed",
            ErrorCode::CompileFailed => "compile-failed",
        }
    }
}

// [spec:cg3:req:wasm.errors+1]
/// An error for the wrapper: a stable code and the message. Every export that
/// can fail returns it, and it reaches JavaScript as a plain object with
/// `code` and `message`, which the wrapper rethrows as a `Cg3Error`. A failed
/// compile reaches it as such an object with `diagnostics` too, which the
/// compile module builds itself.
#[derive(Debug, PartialEq, Eq)]
pub struct GlueError {
    code: ErrorCode,
    message: String,
}

impl GlueError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        GlueError {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<GlueError> for JsValue {
    fn from(error: GlueError) -> Self {
        let object = js_sys::Object::new();
        set(&object, "code", &JsValue::from_str(error.code.as_str()));
        set(&object, "message", &JsValue::from_str(&error.message));
        object.into()
    }
}

/// Sets `key` on a plain object a module has just made.
pub fn set(object: &js_sys::Object, key: &str, value: &JsValue) {
    js_sys::Reflect::set(object, &JsValue::from_str(key), value)
        .expect("a fresh plain object takes any property");
}

#[cfg(test)]
mod tests {
    use super::*;

    // [spec:cg3:req:wasm.errors+1/test]
    #[test]
    fn the_wrapper_documents_every_code() {
        let errors_ts = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../js/cg3/errors.ts"
        ))
        .expect("js/cg3/errors.ts is in the repository");
        for code in ErrorCode::ALL {
            assert!(
                errors_ts.contains(&format!("\"{}\"", code.as_str())),
                "js/cg3/errors.ts lists {}",
                code.as_str()
            );
        }
    }

    #[test]
    fn an_error_keeps_its_code_and_message() {
        let error = GlueError::new(ErrorCode::RunFailed, "it failed");
        assert_eq!(error.code(), ErrorCode::RunFailed);
        assert_eq!(error.code().as_str(), "run-failed");
        assert_eq!(error.message(), "it failed");
    }
}
