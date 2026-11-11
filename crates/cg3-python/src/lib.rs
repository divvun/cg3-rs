//! The pyo3 glue behind `divvun-cg3`.
//!
//! maturin builds this crate as the extension module `cg3._cg3`. The `cg3`
//! package in `python/cg3/` re-exports it and is the public API.

mod error;
mod grammar;
mod log;
mod options;
mod tools;

/// VISL CG-3 for Python: loads Constraint Grammars, compiled or textual, runs
/// them as vislcg3 and cg-proc do, writes them out as cg-comp does, and
/// converts and splits streams as cg-conv and cg-mwesplit do.
#[pyo3::pymodule(name = "_cg3")]
mod module {
    use pyo3::prelude::*;
    use pyo3::types::PyTuple;

    #[pymodule_export]
    use crate::error::{Cg3Error, GrammarError, RunError};
    #[pymodule_export]
    use crate::grammar::{
        ApplyResult, Grammar, GrammarInfo, load_grammar, parse_grammar, read_grammar,
    };
    #[pymodule_export]
    use crate::options::{
        check_convert_options, check_load_options, check_proc_options, check_run_options,
    };
    #[pymodule_export]
    use crate::tools::{convert, mwesplit};

    #[pymodule_init]
    fn init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        // `Format` is the `typing.Literal` of the stream formats `apply` and
        // `convert` read, and `ProcFormat` of those `proc` reads, each made
        // from the table its options are read with, so they cannot disagree.
        let literal = m.py().import("typing")?.getattr("Literal")?;
        let names = PyTuple::new(m.py(), crate::options::format_names())?;
        m.add("Format", literal.get_item(names)?)?;
        let names = PyTuple::new(m.py(), crate::options::proc_format_names())?;
        m.add("ProcFormat", literal.get_item(names)?)
    }
}
