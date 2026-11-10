//! The exceptions the library's failures reach Python as.

use cg3::diagnostics::render_parse_errors;
use cg3::error::{Cg3Error as LibraryError, GrammarError as LibraryGrammarError};
use pyo3::create_exception;
use pyo3::exceptions::PyException;

// [spec:cg3:req:python.errors]
create_exception!(
    cg3,
    Cg3Error,
    PyException,
    "A failure the cg3 library reports. GrammarError and RunError say which kind."
);
create_exception!(
    cg3,
    GrammarError,
    Cg3Error,
    "A grammar that does not load or parse. The message quotes the source of a parse failure as vislcg3 does."
);
create_exception!(
    cg3,
    RunError,
    Cg3Error,
    "A run that fails, such as one whose EXTERNAL process cannot start. The grammar stays usable."
);

// [spec:cg3:req:python.errors]
/// The library error's message, after the source quotation vislcg3 prints for
/// it: every error of a failed parse, or the rule a failed run was placed in.
/// No colour, because an exception's message is read in places a terminal's
/// escapes do not belong.
pub fn message(error: &LibraryError) -> String {
    let mut quoted = Vec::new();
    // The render fails only if its sink does, and the sink is a Vec.
    let _ = match error {
        LibraryError::Grammar(LibraryGrammarError::Parse { errors, sources }) => {
            render_parse_errors(errors, sources, &mut quoted, false)
        }
        LibraryError::Run(run) => match run.placed() {
            Some((source, sources)) => {
                render_parse_errors(std::slice::from_ref(source), sources, &mut quoted, false)
            }
            None => Ok(()),
        },
        LibraryError::Grammar(_) | LibraryError::Option(_) => Ok(()),
    };
    let mut text = String::from_utf8_lossy(&quoted).into_owned();
    text.push_str(&error.to_string());
    text
}
