//! The pyo3 glue behind `divvun-cg3`.
//!
//! maturin builds this crate as the extension module `cg3._cg3`. The `cg3`
//! package in `python/cg3/` re-exports it and is the public API.

mod error;
mod grammar;
mod log;
mod options;

/// VISL CG-3 for Python: loads Constraint Grammars, compiled or textual, and
/// runs them as vislcg3 does.
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

    #[pymodule_init]
    fn init(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        // `Format` is the `typing.Literal` of the stream formats `apply` reads,
        // made from the same table, so the two cannot disagree.
        let names = PyTuple::new(m.py(), crate::options::format_names())?;
        let literal = m.py().import("typing")?.getattr("Literal")?;
        m.add("Format", literal.get_item(names)?)
    }
}
