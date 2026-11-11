//! cg-conv and cg-mwesplit, over strings. cg-proc is `Grammar::proc`, since it
//! runs a grammar. None of them compiles or writes a grammar, so they keep the
//! module free of the parser and the writers.

use cg3_wasm_shared::{ErrorCode, GlueError};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::options::parse_convert_options;

// [spec:cg3:req:wasm.tools]
// [spec:cg3:req:wasm.compile-parity]
/// What cg-conv writes for `input` under the options in `options`, the JSON
/// of a `ConvertOptions` object, through `cg3::convert`, which cg-conv
/// converts through; js/test/tool-parity.ts checks the bytes are the same.
#[wasm_bindgen]
pub fn convert(input: &str, options: &str) -> Result<String, GlueError> {
    let options = parse_convert_options(options)?;
    let converted = cg3::convert(input.as_bytes(), &options).map_err(run_failed)?;
    String::from_utf8(converted).map_err(run_failed)
}

// [spec:cg3:req:wasm.tools]
// [spec:cg3:req:wasm.compile-parity]
/// What cg-mwesplit writes for `input`, through `cg3::mwesplit`, which
/// cg-mwesplit splits through.
#[wasm_bindgen]
pub fn mwesplit(input: &str) -> Result<String, GlueError> {
    let split = cg3::mwesplit(input.as_bytes()).map_err(run_failed)?;
    String::from_utf8(split).map_err(run_failed)
}

// [spec:cg3:req:wasm.tools]
/// A tool's failure under `run-failed`, with the library's message.
pub(crate) fn run_failed(error: impl std::fmt::Display) -> GlueError {
    GlueError::new(ErrorCode::RunFailed, error.to_string())
}
