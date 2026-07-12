//! Library error handling (wave 4, `w4-library-errors`).
//!
//! The C++ `CG3Quit` macro (and the scattered `exit()` calls) terminate the
//! PROCESS from deep inside library code. The Rust port must not kill its host:
//! the public library boundaries (grammar load / binary-grammar read+write /
//! textual parse / format-convert / run) now return a [`Cg3Error`] instead, so
//! embedders get a real error value carrying the exact exit code the C++ would
//! have used. Only the `src/bin` / `src/tools` entry points translate a
//! [`Cg3Error`] (via [`Cg3Error::exit_code`]) into an actual `process::exit`.
//!
//! Two escape hatches are RETAINED for residual, non-boundary fatals:
//!   * [`Cg3Exit`] — a `panic_any` unwind carrying the exit code. It still backs
//!     [`cg3_exit`] / [`crate::inlines::cg3_quit`], and is what the deep engine
//!     hot-path fatals (mid-stream input errors reached only through the run
//!     engine) raise. The public boundaries capture it with [`catch_fatal`] and
//!     turn it into an `Err(Cg3Error)`, so it never escapes a boundary as an
//!     unwind.
//!   * [`crate::textual_parser::ParseError`] — the parser's `throw int`
//!     control-flow port (recovered line-by-line inside the parser).
//!
//! [`run_cli`] is the CLI wrapper that catches any [`Cg3Exit`] that a tool main
//! itself raises (the ~16 entry-point `cg3_quit`/`cg3_exit` sites) and silences
//! the Rust panic noise for both payload kinds.

use std::panic::{self, AssertUnwindSafe};

/// The payload of a residual library-side fatal (the C++ `CG3Quit(code)` /
/// `exit(code)`). Raised with `panic_any`; captured either by [`catch_fatal`]
/// (at a public boundary, → `Err(Cg3Error)`) or by [`run_cli`] (at a CLI entry
/// point, → its exit code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cg3Exit(pub i32);

impl std::fmt::Display for Cg3Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cg3 fatal exit with code {}", self.0)
    }
}

impl std::error::Error for Cg3Exit {}

/// A recoverable library-side fatal, carrying the exact `process::exit` code the
/// C++ `CG3Quit(code)` / `exit(code)` would have produced. The public boundaries
/// (grammar load, binary-grammar read+write, textual parse, format-convert, run)
/// return `Result<_, Cg3Error>`; the CLI binaries map it back with
/// [`Cg3Error::exit_code`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cg3Error {
    /// A `CG3Quit(code)` / `exit(code)` fatal. `msg` is an OPTIONAL note; the
    /// human-facing diagnostic was already emitted at the fatal site (via
    /// `tracing::error!`), exactly as the C++ printed it before terminating.
    Fatal {
        /// The process exit code the C++ would have used.
        code: i32,
        /// Optional context (the diagnostic was already printed at the site).
        msg: Option<String>,
    },
}

impl Cg3Error {
    /// Construct a fatal carrying `code` and an optional context message.
    pub fn fatal(code: i32, msg: Option<String>) -> Cg3Error {
        Cg3Error::Fatal { code, msg }
    }

    /// The `process::exit` code this error maps to — byte-identical to what the
    /// C++ `CG3Quit`/`exit` would have used.
    pub fn exit_code(&self) -> i32 {
        match self {
            Cg3Error::Fatal { code, .. } => *code,
        }
    }
}

impl std::fmt::Display for Cg3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cg3Error::Fatal { code, msg } => match msg {
                Some(m) => write!(f, "cg3 fatal (exit {code}): {m}"),
                None => write!(f, "cg3 fatal (exit {code})"),
            },
        }
    }
}

impl std::error::Error for Cg3Error {}

impl From<Cg3Exit> for Cg3Error {
    fn from(e: Cg3Exit) -> Cg3Error {
        Cg3Error::Fatal {
            code: e.0,
            msg: None,
        }
    }
}

/// Emit the `CG3Quit` diagnostic that [`crate::inlines::cg3_quit`] would print
/// before terminating: `"CG3Quit triggered from {file} line {line}."`, but ONLY
/// when `line != 0` (the C++ `__LINE__ != 0` guard). Sites that returned via
/// `cg3_quit(1, Some(file!()), line)` now call this, then `return Err(...)`, so
/// the diagnostic is preserved byte-for-byte.
pub fn emit_cg3quit_line(file: &str, line: u32) {
    if line != 0 {
        tracing::error!("CG3Quit triggered from {} line {}.", file, line);
    }
}

/// Raise a residual library-side fatal (the C++ `CG3Quit` termination) as an
/// unwind carrying the exit code. Backs [`crate::inlines::cg3_quit`]; captured
/// at the nearest public boundary by [`catch_fatal`] (→ `Err(Cg3Error)`), or at
/// a CLI entry point by [`run_cli`].
pub fn cg3_exit(code: i32) -> ! {
    panic::panic_any(Cg3Exit(code))
}

/// Run a library boundary body, converting a residual [`Cg3Exit`] unwind (from a
/// deep engine/grammar-construction fatal) into an `Err(Cg3Error)` that carries
/// the exact exit code. A [`crate::textual_parser::ParseError`] that escapes the
/// parser is likewise a fatal (exit code 1 — the C++ caught it and `CG3Quit(1)`
/// only via the error-count bail, but a raw escape is the `throw int` path). Any
/// OTHER panic (a genuine invariant violation / `unwrap`) is resumed unchanged.
///
/// The panic hook is left untouched here — [`run_cli`] (installed by the CLI
/// entry points) already silences the `Cg3Exit` / `ParseError` payloads, and a
/// resumed non-fatal panic should still print normally.
pub fn catch_fatal<T>(body: impl FnOnce() -> T) -> Result<T, Cg3Error> {
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(v) => Ok(v),
        Err(e) => {
            if let Some(exit) = e.downcast_ref::<Cg3Exit>() {
                return Err(Cg3Error::from(*exit));
            }
            if e.is::<crate::textual_parser::ParseError>() {
                // A ParseError that unwinds out of the parser boundary is the
                // C++ `throw int` reaching the top — a fatal exit(1).
                return Err(Cg3Error::fatal(1, None));
            }
            panic::resume_unwind(e)
        }
    }
}

/// Run a CLI tool body, translating a [`Cg3Exit`] unwind into its exit code
/// (any other panic is resumed). Installs a panic hook that silences the
/// default "panicked at ..." print for `Cg3Exit` payloads — the C++ `CG3Quit`
/// exits without extra output beyond its own diagnostics.
pub fn run_cli(body: impl FnOnce() -> i32) -> i32 {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Cg3Exit is the C++ CG3Quit (exits silently beyond its own
        // diagnostics); ParseError is the C++ caught `throw int` parse-error
        // control flow (each already printed its diagnostic). Neither should
        // produce Rust panic noise on the CLI's stderr.
        let payload = info.payload();
        if payload.downcast_ref::<Cg3Exit>().is_none()
            && payload
                .downcast_ref::<crate::textual_parser::ParseError>()
                .is_none()
        {
            default_hook(info);
        }
    }));
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(code) => code,
        Err(e) => match e.downcast::<Cg3Exit>() {
            Ok(exit) => exit.0,
            Err(other) => panic::resume_unwind(other),
        },
    }
}
