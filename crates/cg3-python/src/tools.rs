//! cg-conv and cg-mwesplit, over strings. cg-proc is `Grammar.proc` and
//! cg-relabel `Grammar.relabel`, since each takes a grammar.

use cg3::error::Cg3Error as LibraryError;
use pyo3::prelude::*;

use crate::error::{RunError, message};
use crate::log;
use crate::options;

// [spec:cg3:req:python.tools]
/// Converts `input` from one stream format to another, as cg-conv does, with
/// the options a ConvertOptions gives, and returns what cg-conv writes. A
/// conversion that fails raises RunError.
#[pyfunction]
#[pyo3(signature = (input, /, options = None))]
pub fn convert(
    py: Python<'_>,
    input: &str,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    let options = options::of_convert(options)?;
    stream(log::detached(py, || {
        cg3::convert(input.as_bytes(), &options)
    })?)
}

// [spec:cg3:req:python.tools]
/// Splits each cohort whose wordform tags give its words into one cohort per
/// word, as cg-mwesplit does, and returns what cg-mwesplit writes. A split
/// that fails raises RunError.
#[pyfunction]
#[pyo3(signature = (input, /))]
pub fn mwesplit(py: Python<'_>, input: &str) -> PyResult<String> {
    stream(log::detached(py, || cg3::mwesplit(input.as_bytes()))?)
}

// [spec:cg3:req:python.tools]
// [spec:cg3:req:python.errors]
/// A tool's output as a string, or its failure as RunError with the library's
/// message. Output that is not UTF-8 is a failure too: a string cannot carry
/// it.
pub fn stream(written: Result<Vec<u8>, LibraryError>) -> PyResult<String> {
    let output = written.map_err(|e| RunError::new_err(message(&e)))?;
    String::from_utf8(output).map_err(|e| RunError::new_err(e.to_string()))
}
