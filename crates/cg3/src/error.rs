//! Library error handling (wave 4, `w4-library-errors`).
//!
//! The C++ `CG3Quit` macro (and the scattered `exit()` calls) terminate the
//! PROCESS from deep inside library code. The Rust port must not kill its
//! host: every library-side fatal now raises a [`Cg3Exit`] unwind instead,
//! and only the `src/bin` entry points translate it into an actual
//! `process::exit` (via [`run_cli`]). Embedders can catch the unwind (or run
//! behind their own `catch_unwind`) instead of losing the process.

use std::panic::{self, AssertUnwindSafe};

/// The payload of a library-side fatal (the C++ `CG3Quit(code)` / `exit(code)`).
/// Raised with `panic_any`, caught by [`run_cli`] in the binaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cg3Exit(pub i32);

impl std::fmt::Display for Cg3Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cg3 fatal exit with code {}", self.0)
    }
}

impl std::error::Error for Cg3Exit {}

/// Raise a library-side fatal (the C++ `CG3Quit` termination), as an unwind
/// carrying the exit code.
pub fn cg3_exit(code: i32) -> ! {
    panic::panic_any(Cg3Exit(code))
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
