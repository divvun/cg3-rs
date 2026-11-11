//! A run's options and a load's, as the fields of `cg3.RunOptions` and
//! `cg3.LoadOptions`, the dataclasses in `python/cg3/_options.py`.

use cg3::embed::{Format, LoadOptions, RunOptions};
use pyo3::exceptions::{PyOverflowError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedStr;
use pyo3::types::{PyBool, PyString};

// [spec:cg3:req:python.apply+1]
/// The options `Grammar.apply` was given: those of a `cg3.RunOptions`, or none.
pub fn of_apply(options: Option<&Bound<'_, PyAny>>) -> PyResult<RunOptions> {
    let Some(options) = options else {
        return Ok(RunOptions::default());
    };
    let class = options.py().import("cg3._options")?.getattr("RunOptions")?;
    if !options.is_instance(&class)? {
        let given = options.get_type().name()?;
        return Err(PyTypeError::new_err(format!(
            "apply() argument 'options' must be RunOptions, not {given}"
        )));
    }
    run_options(options)
}

// [spec:cg3:req:python.apply+1]
/// Reads each field of a `cg3.RunOptions`, named after its vislcg3 long option
/// in snake_case, into the option it stands for. A value of the wrong type
/// raises the `TypeError` a typed parameter would, a number out of range
/// `OverflowError`, and a format the package does not offer `ValueError`.
/// `RunOptions.__post_init__` reads its fields here, so options that were made
/// are options a run can read.
pub fn run_options(options: &Bound<'_, PyAny>) -> PyResult<RunOptions> {
    let fields = options
        .py()
        .import("dataclasses")?
        .call_method1("fields", (options,))?;
    let mut o = RunOptions::default();
    for field in fields.try_iter()? {
        let field_name: PyBackedStr = field?.getattr("name")?.extract()?;
        let name: &str = &field_name;
        let value = options.getattr(name)?;
        let v = Value {
            class: "RunOptions",
            name,
            value: &value,
        };
        match name {
            "input_format" => o.input = v.format()?,
            "output_format" => o.output = v.format()?,
            "ordered" => o.ordered = v.flag()?,
            "unsafe" => o.unsafe_mode = v.flag()?,
            "sections" => o.sections = v.text()?,
            "rules" => o.rules = v.text()?,
            "rule" => o.rule = v.text()?,
            "debug_rules" => o.debug_rules = v.text()?,
            "verbose" => o.verbose = v.flag_or_text()?,
            "no_mappings" => o.no_mappings = v.flag()?,
            "no_corrections" => o.no_corrections = v.flag()?,
            "no_before_sections" => o.no_before_sections = v.flag()?,
            "no_sections" => o.no_sections = v.flag()?,
            "no_after_sections" => o.no_after_sections = v.flag()?,
            "trace" => o.trace = v.flag_or_text()?,
            "trace_name_only" => o.trace_name_only = v.flag()?,
            "trace_no_removed" => o.trace_no_removed = v.flag()?,
            "trace_encl" => o.trace_encl = v.flag()?,
            "deleted" => o.deleted = v.flag()?,
            "single_run" => o.single_run = v.flag()?,
            "max_runs" => o.max_runs = v.count()?,
            "unicode_tags" => o.unicode_tags = v.flag()?,
            "unique_tags" => o.unique_tags = v.flag()?,
            "print_ids" => o.print_ids = v.flag()?,
            "print_dep" => o.print_dep = v.flag()?,
            "num_windows" => o.num_windows = v.count()?,
            "always_span" => o.always_span = v.flag()?,
            "soft_limit" => o.soft_limit = v.count()?,
            "hard_limit" => o.hard_limit = v.count()?,
            "text_delimit" => o.text_delimit = v.flag_or_text()?,
            "dep_delimit" => o.dep_delimit = v.flag_or_text()?,
            "dep_absolute" => o.dep_absolute = v.flag()?,
            "dep_original" => o.dep_original = v.flag()?,
            "dep_allow_loops" => o.dep_allow_loops = v.flag()?,
            "dep_no_crossing" => o.dep_no_crossing = v.flag()?,
            "no_magic_readings" => o.no_magic_readings = v.flag()?,
            "no_pass_origin" => o.no_pass_origin = v.flag()?,
            "split_mappings" => o.split_mappings = v.flag()?,
            "show_end_tags" => o.show_end_tags = v.flag()?,
            "no_break" => o.no_break = v.flag()?,
            _ => {
                return Err(PyTypeError::new_err(format!(
                    "RunOptions has a field '{name}' no run reads"
                )));
            }
        }
    }
    Ok(o)
}

// [spec:cg3:req:python.load-options]
/// The options a load was given: those of a `cg3.LoadOptions`, or none.
/// `function` is the loader, for the error a value of another type raises.
pub fn of_load(options: Option<&Bound<'_, PyAny>>, function: &str) -> PyResult<LoadOptions> {
    let Some(options) = options else {
        return Ok(LoadOptions::default());
    };
    let class = options
        .py()
        .import("cg3._options")?
        .getattr("LoadOptions")?;
    if !options.is_instance(&class)? {
        let given = options.get_type().name()?;
        return Err(PyTypeError::new_err(format!(
            "{function}() argument 'options' must be LoadOptions, not {given}"
        )));
    }
    load_options(options)
}

// [spec:cg3:req:python.load-options]
/// Reads each field of a `cg3.LoadOptions`, named after its vislcg3 long
/// option in snake_case, into the option it stands for. A value of the wrong
/// type raises `TypeError`, and a `prefix` that is not one character or a rule
/// filter that does not compile `ValueError`.
pub fn load_options(options: &Bound<'_, PyAny>) -> PyResult<LoadOptions> {
    let fields = options
        .py()
        .import("dataclasses")?
        .call_method1("fields", (options,))?;
    let mut o = LoadOptions::default();
    for field in fields.try_iter()? {
        let field_name: PyBackedStr = field?.getattr("name")?.extract()?;
        let name: &str = &field_name;
        let value = options.getattr(name)?;
        let v = Value {
            class: "LoadOptions",
            name,
            value: &value,
        };
        match name {
            "nrules" => o.nrules = v.rule_filter("--nrules")?,
            "nrules_v" => o.nrules_v = v.rule_filter("--nrules-v")?,
            "vislcg_compat" => o.vislcg_compat = v.flag()?,
            "prefix" => o.prefix = v.character()?,
            _ => {
                return Err(PyTypeError::new_err(format!(
                    "LoadOptions has a field '{name}' no load reads"
                )));
            }
        }
    }
    Ok(o)
}

/// Checks a `cg3.LoadOptions` as it is made, raising what a load reading its
/// fields would raise.
#[pyfunction]
#[pyo3(name = "_check_load_options")]
pub fn check_load_options(options: &Bound<'_, PyAny>) -> PyResult<()> {
    load_options(options).map(drop)
}

/// Checks a `cg3.RunOptions` as it is made, raising what a run reading its
/// fields would raise.
#[pyfunction]
#[pyo3(name = "_check_run_options")]
pub fn check_run_options(options: &Bound<'_, PyAny>) -> PyResult<()> {
    run_options(options).map(drop)
}

/// The streams `input_format` and `output_format` name, as vislcg3's `--in-*`
/// and `--out-*` options do. The binary format is left out: a string cannot
/// carry it.
const FORMATS: [(&str, Format); 6] = [
    ("cg", Format::Cg),
    ("niceline", Format::Niceline),
    ("apertium", Format::Apertium),
    ("fst", Format::Fst),
    ("plain", Format::Plain),
    ("jsonl", Format::Jsonl),
];

/// The names `input_format` and `output_format` take.
pub fn format_names() -> impl ExactSizeIterator<Item = &'static str> {
    FORMATS.iter().map(|(name, _)| *name)
}

/// One field's value, named, with its class, for the errors it raises. Each
/// reader takes `None` for the option left off.
struct Value<'a, 'py> {
    class: &'static str,
    name: &'a str,
    value: &'a Bound<'py, PyAny>,
}

impl Value<'_, '_> {
    fn wrong_type(&self, expected: &str) -> PyErr {
        let given = self
            .value
            .get_type()
            .name()
            .map_or_else(|_| "object".to_owned(), |n| n.to_string());
        PyTypeError::new_err(format!(
            "{}() argument '{}' must be {expected}, not {given}",
            self.class, self.name
        ))
    }

    /// A flag: `True` gives the option, `False` leaves it off.
    fn flag(&self) -> PyResult<bool> {
        if self.value.is_none() {
            return Ok(false);
        }
        self.value
            .cast::<PyBool>()
            .map(|b| b.is_true())
            .map_err(|_| self.wrong_type("bool"))
    }

    fn text(&self) -> PyResult<Option<String>> {
        if self.value.is_none() {
            return Ok(None);
        }
        let text = self
            .value
            .cast::<PyString>()
            .map_err(|_| self.wrong_type("str"))?;
        Ok(Some(text.to_str()?.to_owned()))
    }

    /// An option whose value vislcg3 makes optional: `True` gives it without
    /// one, `False` leaves it off.
    fn flag_or_text(&self) -> PyResult<Option<String>> {
        if let Ok(b) = self.value.cast::<PyBool>() {
            return Ok(b.is_true().then(String::new));
        }
        if self.value.is_none() || self.value.is_instance_of::<PyString>() {
            return self.text();
        }
        Err(self.wrong_type("bool or str"))
    }

    /// A whole number from 0 to 4294967295, as vislcg3 reads its numeric
    /// options. A bool is refused although Python counts it as an int.
    fn count(&self) -> PyResult<Option<u32>> {
        if self.value.is_none() {
            return Ok(None);
        }
        if self.value.is_instance_of::<PyBool>() {
            return Err(self.wrong_type("int"));
        }
        self.value.extract::<u32>().map(Some).map_err(|e| {
            if e.is_instance_of::<PyTypeError>(self.value.py()) {
                self.wrong_type("int")
            } else {
                PyOverflowError::new_err(format!(
                    "{}() argument '{}' must be from 0 to 4294967295, not {}",
                    self.class, self.name, self.value
                ))
            }
        })
    }

    /// A rule-name pattern for `flag`, which must compile as vislcg3 compiles
    /// it.
    fn rule_filter(&self, flag: &'static str) -> PyResult<Option<String>> {
        let Some(pattern) = self.text()? else {
            return Ok(None);
        };
        cg3::embed::rule_filter(&pattern, flag).map_err(|e| {
            PyValueError::new_err(format!(
                "{}() argument '{}' is not a pattern vislcg3 reads: {e}",
                self.class, self.name
            ))
        })?;
        Ok(Some(pattern))
    }

    fn character(&self) -> PyResult<Option<char>> {
        let Some(text) = self.text()? else {
            return Ok(None);
        };
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(Some(c)),
            _ => Err(PyValueError::new_err(format!(
                "{}() argument '{}' must be one character, not {:?}",
                self.class, self.name, text
            ))),
        }
    }

    fn format(&self) -> PyResult<Option<Format>> {
        let Some(text) = self.text()? else {
            return Ok(None);
        };
        FORMATS
            .iter()
            .find(|(name, _)| *name == text)
            .map(|(_, format)| Some(*format))
            .ok_or_else(|| {
                let names: Vec<String> = FORMATS.iter().map(|(n, _)| format!("'{n}'")).collect();
                PyValueError::new_err(format!(
                    "{}() argument '{}' must be one of {}, not '{text}'",
                    self.class,
                    self.name,
                    names.join(", ")
                ))
            })
    }
}
