//! cg-conv and cg-mwesplit, over strings. cg-proc is `Grammar::proc`, since it
//! runs a grammar. None of them compiles or writes a grammar, so an app that
//! links only CG3 carries neither the parser nor the writers.

use crate::error::Cg3Error;
use crate::log::logging;
use crate::options::{ConvertOptions, convert_options};

// [spec:cg3:req:swift.tools]
/// What cg-conv writes for `input` under `options`, through `cg3::convert`,
/// which cg-conv converts through.
#[uniffi::export]
pub fn convert(input: String, options: ConvertOptions) -> Result<String, Cg3Error> {
    let options = convert_options(options)?;
    stream(logging(|| cg3::convert(input.as_bytes(), &options)))
}

// [spec:cg3:req:swift.tools]
/// What cg-mwesplit writes for `input`, through `cg3::mwesplit`, which
/// cg-mwesplit splits through.
#[uniffi::export]
pub fn mwesplit(input: String) -> Result<String, Cg3Error> {
    stream(logging(|| cg3::mwesplit(input.as_bytes())))
}

// [spec:cg3:req:swift.errors]
/// The text a run, conversion or split wrote, or its failure as `RunFailed`
/// with the library's message.
pub(crate) fn stream(written: Result<Vec<u8>, cg3::error::Cg3Error>) -> Result<String, Cg3Error> {
    String::from_utf8(written.map_err(Cg3Error::run_failed)?).map_err(Cg3Error::run_failed)
}
