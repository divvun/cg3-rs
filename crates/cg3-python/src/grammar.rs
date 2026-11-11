//! A loaded grammar, the three ways to load one, and its runs.

use std::path::PathBuf;

use cg3::embed::{CompiledGrammar, FileResolver};
use cg3::error::Cg3Error as LibraryError;
use cg3::inlines::is_cg3b;
use cg3::textual_parser::MEMORY_SOURCE_NAME;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::{PyList, PyString};

use crate::error::{GrammarError, RunError, message};
use crate::log;
use crate::options;

// [spec:cg3:req:python.threads]
/// A loaded grammar, applied any number of times. It is immutable, so threads
/// may apply it at the same time: each apply is a run of its own over the one
/// grammar, without the GIL.
#[pyclass(frozen, module = "cg3")]
pub struct Grammar {
    inner: CompiledGrammar,
}

// [spec:cg3:req:python.grammar-load]
/// What vislcg3's verbose summary reports about a loaded grammar.
#[pyclass(frozen, get_all, eq, module = "cg3")]
#[derive(PartialEq, Eq)]
pub struct GrammarInfo {
    sections: usize,
    templates: usize,
    rules: usize,
    sets: usize,
    tags: usize,
    /// Rules no tag index can skip.
    rules_any: usize,
    has_dependency_rules: bool,
    has_relation_rules: bool,
}

#[pymethods]
impl GrammarInfo {
    fn __repr__(&self) -> String {
        let flag = |b: bool| if b { "True" } else { "False" };
        format!(
            "GrammarInfo(sections={}, templates={}, rules={}, sets={}, tags={}, rules_any={}, \
             has_dependency_rules={}, has_relation_rules={})",
            self.sections,
            self.templates,
            self.rules,
            self.sets,
            self.tags,
            self.rules_any,
            flag(self.has_dependency_rules),
            flag(self.has_relation_rules),
        )
    }
}

// [spec:cg3:req:python.apply+1]
/// A run's output, and every warning and non-fatal error it logged, in order.
#[pyclass(frozen, get_all, eq, module = "cg3")]
#[derive(PartialEq, Eq)]
pub struct ApplyResult {
    output: String,
    warnings: Vec<String>,
}

#[pymethods]
impl ApplyResult {
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "ApplyResult(output={}, warnings={})",
            PyString::new(py, &self.output).repr()?,
            PyList::new(py, &self.warnings)?.repr()?,
        ))
    }
}

#[pymethods]
impl Grammar {
    // [spec:cg3:req:python.grammar-load]
    /// What vislcg3's verbose summary reports about the grammar.
    fn info(&self) -> GrammarInfo {
        let info = self.inner.info();
        GrammarInfo {
            sections: info.sections,
            templates: info.templates,
            rules: info.rules,
            sets: info.sets,
            tags: info.tags,
            rules_any: info.rules_any,
            has_dependency_rules: info.has_dependency_rules,
            has_relation_rules: info.has_relation_rules,
        }
    }

    // [spec:cg3:req:python.apply+1]
    // [spec:cg3:req:python.threads]
    // [spec:cg3:req:python.errors]
    // [spec:cg3:req:python.parity]
    // The input and options reach `CompiledGrammar::run`, the operation vislcg3
    // runs every grammar through, unchanged, and the output comes back as it
    // wrote it, so a run gives what vislcg3 gives; python/tests/test_parity.py
    // checks that over the corpus.
    /// Runs the grammar on `input`, as vislcg3 runs it on one standard input,
    /// with the options a RunOptions gives, and returns the output with every
    /// warning and non-fatal error the run logged. A run that fails raises
    /// RunError, and the grammar stays usable.
    #[pyo3(signature = (input, /, options = None))]
    fn apply(
        &self,
        py: Python<'_>,
        input: &str,
        options: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<ApplyResult> {
        let options = options::of_apply(options)?;
        let grammar = &self.inner;
        let threshold = log::threshold(py)?;
        let (ran, kept) =
            py.detach(|| log::collecting(threshold, || grammar.run(input.as_bytes(), &options)));
        kept.hand_over(py)?;
        let output = ran.map_err(|e| RunError::new_err(message(&e)))?;
        let output = String::from_utf8(output).map_err(|e| RunError::new_err(e.to_string()))?;
        Ok(ApplyResult {
            output,
            warnings: kept.warnings(),
        })
    }

    fn __repr__(&self) -> String {
        let info = self.inner.info();
        format!(
            "<cg3.Grammar: {} rules, {} sets, {} sections>",
            info.rules, info.sets, info.sections
        )
    }
}

/// Loads a grammar without the GIL, handing its events to `logging` after.
fn load(
    py: Python<'_>,
    f: impl FnOnce() -> Result<CompiledGrammar, LibraryError> + Send,
) -> PyResult<Grammar> {
    let threshold = log::threshold(py)?;
    let (loaded, kept) = py.detach(|| log::collecting(threshold, f));
    kept.hand_over(py)?;
    loaded
        .map(|inner| Grammar { inner })
        .map_err(|e| GrammarError::new_err(message(&e)))
}

// [spec:cg3:req:python.grammar-load]
// [spec:cg3:req:python.threads]
// [spec:cg3:req:python.load-options]
/// Loads a compiled grammar from the bytes of a `.cg3b`, as cg-comp writes
/// it, under the load options a LoadOptions gives. Bytes that are not one
/// raise GrammarError.
#[pyfunction]
#[pyo3(signature = (data, *, options = None))]
pub fn read_grammar(
    py: Python<'_>,
    data: PyBackedBytes,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<Grammar> {
    let options = options::of_load(options, "read_grammar")?;
    let data: &[u8] = &data;
    load(py, || CompiledGrammar::from_cg3b(data, &options))
}

// [spec:cg3:req:python.grammar-load]
// [spec:cg3:req:python.threads]
// [spec:cg3:req:python.load-options]
/// Parses a textual grammar from `source`, calling it `name` in its
/// diagnostics, under the load options a LoadOptions gives. A relative INCLUDE
/// resolves against the directory of the file `name` names, or the current
/// directory without one. A grammar that does not parse raises GrammarError,
/// quoting every error.
#[pyfunction]
#[pyo3(signature = (source, name = None, *, options = None))]
pub fn parse_grammar(
    py: Python<'_>,
    source: &str,
    name: Option<&str>,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<Grammar> {
    let options = options::of_load(options, "parse_grammar")?;
    let name = name.unwrap_or(MEMORY_SOURCE_NAME);
    load(py, || {
        CompiledGrammar::from_text(source.as_bytes(), name, FileResolver, &options)
    })
}

// [spec:cg3:req:python.grammar-load]
// [spec:cg3:req:python.threads]
// [spec:cg3:req:python.load-options]
/// Loads the grammar in the file at `path`: compiled when it begins with the
/// `.cg3b` magic, as vislcg3 decides, and textual otherwise, with relative
/// INCLUDEs resolved against its directory, under the load options a
/// LoadOptions gives.
#[pyfunction]
#[pyo3(signature = (path, *, options = None))]
pub fn load_grammar(
    py: Python<'_>,
    path: PathBuf,
    options: Option<&Bound<'_, PyAny>>,
) -> PyResult<Grammar> {
    let options = options::of_load(options, "load_grammar")?;
    let name = path
        .to_str()
        .ok_or_else(|| PyValueError::new_err(format!("the grammar path {path:?} is not UTF-8")))?;
    let bytes = py
        .detach(|| std::fs::read(&path))
        .map_err(|e| os_error(py, e, name))?;
    if is_cg3b(&bytes) {
        load(py, || CompiledGrammar::from_cg3b(&bytes, &options))
    } else {
        load(py, || {
            CompiledGrammar::from_text(&bytes, name, FileResolver, &options)
        })
    }
}

// [spec:cg3:req:python.errors]
/// The `OSError` `open()` raises for `path`: `OSError` picks the subclass,
/// such as `FileNotFoundError`, from the errno.
fn os_error(py: Python<'_>, error: std::io::Error, path: &str) -> PyErr {
    let Some(errno) = error.raw_os_error() else {
        return error.into();
    };
    let strerror = py
        .import("os")
        .and_then(|os| os.call_method1("strerror", (errno,)))
        .and_then(|s| s.extract::<String>())
        .unwrap_or_else(|_| error.to_string());
    PyOSError::new_err((errno, strerror, path.to_owned()))
}
