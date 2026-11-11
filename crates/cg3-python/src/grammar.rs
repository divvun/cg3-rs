//! A loaded grammar, the three ways to load one, its runs, and what it is
//! written out and relabelled as.

use std::path::{Path, PathBuf};

use cg3::embed::{CompiledGrammar, FileResolver, MemoryResolver};
use cg3::error::Cg3Error as LibraryError;
use cg3::grammar_sources::sidecar_path;
use cg3::inlines::is_cg3b;
use cg3::textual_parser::MEMORY_SOURCE_NAME;
use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::{PyBackedBytes, PyBackedStr};
use pyo3::types::{PyBytes, PyList, PyMapping, PyString};

use crate::error::{GrammarError, RunError, message};
use crate::log;
use crate::options;
use crate::tools::stream;

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

    // [spec:cg3:req:python.tools]
    /// Runs the grammar on `input` as cg-proc does, with the options a
    /// ProcOptions gives, and returns what cg-proc writes. The options the
    /// grammar sets with CMDARGS apply as they do in cg-proc. A run that fails
    /// raises RunError, and the grammar stays usable.
    #[pyo3(signature = (input, /, options = None))]
    fn proc(
        &self,
        py: Python<'_>,
        input: &str,
        options: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<String> {
        let options = options::of_proc(options)?;
        let grammar = &self.inner;
        stream(log::detached(py, || {
            grammar.proc(input.as_bytes(), &options)
        })?)
    }

    // [spec:cg3:req:python.tools]
    /// The grammar cg-relabel makes of this one and the relabel rules in
    /// `relabels`, as a new grammar; both are left as they were. cg-relabel
    /// relabels a `.cg3b`, so a grammar loaded from text is relabelled as the
    /// `.cg3b` to_cg3b returns for it. Load the rules with load_grammar to
    /// read them as cg-relabel reads its rule file, compiled or textual. A
    /// relabel that fails raises GrammarError.
    fn relabel(&self, py: Python<'_>, relabels: PyRef<'_, Grammar>) -> PyResult<Grammar> {
        let grammar = &self.inner;
        let rules = &relabels.inner;
        log::detached(py, || grammar.relabel(rules))?
            .map(|inner| Grammar { inner })
            .map_err(grammar_error)
    }

    // [spec:cg3:req:python.write]
    /// The `.cg3b` holding the grammar: what cg-comp writes for a grammar
    /// loaded from text, and vislcg3 --grammar-bin for one loaded from a
    /// `.cg3b`. The grammar is left as it was.
    fn to_cg3b<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let grammar = &self.inner;
        let cg3b = log::detached(py, || grammar.to_cg3b())?.map_err(grammar_error)?;
        Ok(PyBytes::new(py, &cg3b))
    }

    // [spec:cg3:req:python.write]
    /// The grammar as text, as vislcg3 --grammar-out writes it. The grammar
    /// is left as it was.
    fn to_text(&self, py: Python<'_>) -> PyResult<String> {
        let grammar = &self.inner;
        log::detached(py, || grammar.to_text())
    }

    // [spec:cg3:req:python.write]
    /// Writes the `.cg3b` to_cg3b returns to the file at `path`, and, as
    /// cg-comp does, the companion file beside it, at `path` with `.cg3src`
    /// added, which holds the grammar's sources so that a runtime error can
    /// quote them. A grammar loaded from text has one; a grammar loaded from a
    /// `.cg3b` has none, and nothing is written beside it. The grammar is left
    /// as it was. A file that cannot be written raises what `open()` would.
    fn write_cg3b(&self, py: Python<'_>, path: PathBuf) -> PyResult<()> {
        let grammar = &self.inner;
        let written = log::detached(py, || -> Result<(), WriteFailure> {
            let cg3b = grammar.to_cg3b()?;
            let companion = grammar.companion()?;
            let write = |at: PathBuf, bytes: &[u8]| {
                std::fs::write(&at, bytes).map_err(|e| WriteFailure::File(e, at))
            };
            write(path.clone(), &cg3b)?;
            if let Some(companion) = companion {
                write(sidecar_path(&path), &companion)?;
            }
            Ok(())
        })?;
        written.map_err(|failure| match failure {
            WriteFailure::Library(e) => grammar_error(e),
            WriteFailure::File(e, at) => os_error(py, e, &at),
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

/// Why `write_cg3b` wrote nothing, or not all of it.
enum WriteFailure {
    Library(LibraryError),
    File(std::io::Error, PathBuf),
}

impl From<LibraryError> for WriteFailure {
    fn from(e: LibraryError) -> WriteFailure {
        WriteFailure::Library(e)
    }
}

// [spec:cg3:req:python.errors]
fn grammar_error(e: LibraryError) -> PyErr {
    GrammarError::new_err(message(&e))
}

/// Loads a grammar without the GIL, handing its events to `logging` after.
fn load(
    py: Python<'_>,
    f: impl FnOnce() -> Result<CompiledGrammar, LibraryError> + Send,
) -> PyResult<Grammar> {
    log::detached(py, f)?
        .map(|inner| Grammar { inner })
        .map_err(grammar_error)
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
// [spec:cg3:req:python.includes]
/// Parses a textual grammar from `source`, calling it `name` in its
/// diagnostics, under the load options a LoadOptions gives. Given `files`, a
/// mapping from names to text or bytes, every INCLUDE reads from those files
/// and no others: a relative name is found beside the source that includes
/// it, else at the top of the names, as on disk. Without it, a relative
/// INCLUDE resolves against the directory of the file `name` names, or the
/// current directory without one, as vislcg3 resolves it. A grammar that does
/// not parse raises GrammarError, quoting every error.
#[pyfunction]
#[pyo3(signature = (source, name = None, *, options = None, files = None))]
pub fn parse_grammar(
    py: Python<'_>,
    source: &str,
    name: Option<&str>,
    options: Option<&Bound<'_, PyAny>>,
    files: Option<&Bound<'_, PyAny>>,
) -> PyResult<Grammar> {
    let options = options::of_load(options, "parse_grammar")?;
    let files = files.map(memory_files).transpose()?;
    let name = name.unwrap_or(MEMORY_SOURCE_NAME);
    let source = source.as_bytes();
    match files {
        Some(files) => load(py, || {
            CompiledGrammar::from_text(source, name, files, &options)
        }),
        None => load(py, || {
            CompiledGrammar::from_text(source, name, FileResolver, &options)
        }),
    }
}

// [spec:cg3:req:python.includes]
/// The files a `files` mapping holds, as the library's in-memory resolver: a
/// `str` is read as its UTF-8, and `bytes` as they are.
fn memory_files(files: &Bound<'_, PyAny>) -> PyResult<MemoryResolver> {
    let wrong = |what: &Bound<'_, PyAny>, expected: &str| {
        let given = what
            .get_type()
            .name()
            .map_or_else(|_| "object".to_owned(), |n| n.to_string());
        PyTypeError::new_err(format!(
            "parse_grammar() argument 'files' must be {expected}, not {given}"
        ))
    };
    let mapping = files
        .cast::<PyMapping>()
        .map_err(|_| wrong(files, "a mapping"))?;
    let mut resolver = MemoryResolver::default();
    for item in mapping.items()?.iter() {
        let (name, content): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item.extract()?;
        let name: PyBackedStr = name
            .extract()
            .map_err(|_| wrong(&name, "a mapping whose names are str"))?;
        if let Ok(text) = content.cast::<PyString>() {
            resolver.insert(&name, text.to_str()?);
        } else if let Ok(bytes) = content.extract::<PyBackedBytes>() {
            resolver.insert(&name, &*bytes);
        } else {
            return Err(wrong(&content, "a mapping to str or bytes"));
        }
    }
    Ok(resolver)
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
        .map_err(|e| os_error(py, e, &path))?;
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
fn os_error(py: Python<'_>, error: std::io::Error, path: &Path) -> PyErr {
    let Some(errno) = error.raw_os_error() else {
        return error.into();
    };
    let strerror = py
        .import("os")
        .and_then(|os| os.call_method1("strerror", (errno,)))
        .and_then(|s| s.extract::<String>())
        .unwrap_or_else(|_| error.to_string());
    PyOSError::new_err((errno, strerror, path.as_os_str().to_owned()))
}
