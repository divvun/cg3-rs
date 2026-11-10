//! A loaded grammar, the three ways to load one, and its runs.

use std::path::PathBuf;

use cg3::embed::CompiledGrammar;
use cg3::error::Cg3Error as LibraryError;
use cg3::inlines::is_cg3b;
use cg3::textual_parser::MEMORY_SOURCE_NAME;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedBytes;
use pyo3::types::{PyDict, PyList, PyString};

use crate::error::{GrammarError, RunError, message};
use crate::log;
use crate::options::run_options;

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

// [spec:cg3:req:python.apply]
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

    // [spec:cg3:req:python.apply]
    // [spec:cg3:req:python.threads]
    // [spec:cg3:req:python.errors]
    /// Runs the grammar on `input`, as vislcg3 runs it on one standard input,
    /// and returns the output with every warning and non-fatal error the run
    /// logged. Each keyword argument is a vislcg3 long option in snake_case;
    /// `input_format` and `output_format` stand for its `--in-*` and `--out-*`
    /// options. A run that fails raises RunError, and the grammar stays usable.
    // The signature `help()` and `inspect.signature` show. `_cg3.pyi` declares
    // the same parameters with their types, and python/tests checks that both
    // agree with what `run_options` reads.
    #[pyo3(
        signature = (input, /, **options),
        text_signature = "(self, input, /, *, input_format=None, output_format=None, \
ordered=False, unsafe=False, sections=None, rules=None, rule=None, debug_rules=None, \
verbose=None, no_mappings=False, no_corrections=False, no_before_sections=False, \
no_sections=False, no_after_sections=False, trace=None, trace_name_only=False, \
trace_no_removed=False, trace_encl=False, deleted=False, single_run=False, max_runs=None, \
unicode_tags=False, unique_tags=False, print_ids=False, print_dep=False, num_windows=None, \
always_span=False, soft_limit=None, hard_limit=None, text_delimit=None, dep_delimit=None, \
dep_absolute=False, dep_original=False, dep_allow_loops=False, dep_no_crossing=False, \
no_magic_readings=False, no_pass_origin=False, split_mappings=False, show_end_tags=False, \
no_break=False)"
    )]
    fn apply(
        &self,
        py: Python<'_>,
        input: &str,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<ApplyResult> {
        let options = run_options(options)?;
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
/// Loads a compiled grammar from the bytes of a `.cg3b`, as cg-comp writes
/// it. Bytes that are not one raise GrammarError.
#[pyfunction]
pub fn read_grammar(py: Python<'_>, data: PyBackedBytes) -> PyResult<Grammar> {
    let data: &[u8] = &data;
    load(py, || CompiledGrammar::from_cg3b(data))
}

// [spec:cg3:req:python.grammar-load]
// [spec:cg3:req:python.threads]
/// Parses a textual grammar from `source`, calling it `name` in its
/// diagnostics. A relative INCLUDE resolves against the directory of the file
/// `name` names, or the current directory without one. A grammar that does not
/// parse raises GrammarError, quoting every error.
#[pyfunction]
#[pyo3(signature = (source, name = None))]
pub fn parse_grammar(py: Python<'_>, source: &str, name: Option<&str>) -> PyResult<Grammar> {
    let name = name.unwrap_or(MEMORY_SOURCE_NAME);
    load(py, || CompiledGrammar::from_text(source.as_bytes(), name))
}

// [spec:cg3:req:python.grammar-load]
// [spec:cg3:req:python.threads]
/// Loads the grammar in the file at `path`: compiled when it begins with the
/// `.cg3b` magic, as vislcg3 decides, and textual otherwise, with relative
/// INCLUDEs resolved against its directory.
#[pyfunction]
pub fn load_grammar(py: Python<'_>, path: PathBuf) -> PyResult<Grammar> {
    let name = path
        .to_str()
        .ok_or_else(|| PyValueError::new_err(format!("the grammar path {path:?} is not UTF-8")))?;
    let bytes = py
        .detach(|| std::fs::read(&path))
        .map_err(|e| os_error(py, e, name))?;
    if is_cg3b(&bytes) {
        load(py, || CompiledGrammar::from_cg3b(&bytes))
    } else {
        load(py, || CompiledGrammar::from_text(&bytes, name))
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
