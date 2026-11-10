//! The run options Python passes to `Grammar.apply`, as keyword arguments.

use cg3::embed::{Format, RunOptions};
use pyo3::exceptions::{PyOverflowError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedStr;
use pyo3::types::{PyBool, PyDict, PyString};

// [spec:cg3:req:python.apply]
/// Reads the keyword arguments of `Grammar.apply`, each named after its
/// vislcg3 long option in snake_case. `None` is an option left off, as is one
/// not passed. A name it does not know raises the `TypeError` Python raises for
/// an unexpected keyword argument, and a value of the wrong type the
/// `TypeError` its parameter would.
pub fn run_options(options: Option<&Bound<'_, PyDict>>) -> PyResult<RunOptions> {
    let mut o = RunOptions::default();
    let Some(options) = options else {
        return Ok(o);
    };
    for (key, value) in options.iter() {
        let key: PyBackedStr = key.extract()?;
        let name: &str = &key;
        let v = Value {
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
                    "apply() got an unexpected keyword argument '{name}'"
                )));
            }
        }
    }
    Ok(o)
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

/// One keyword argument's value, named for the errors it raises. Each reader
/// takes `None` for the option left off.
struct Value<'a, 'py> {
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
            "apply() argument '{}' must be {expected}, not {given}",
            self.name
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
                    "apply() argument '{}' must be from 0 to 4294967295, not {}",
                    self.name, self.value
                ))
            }
        })
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
                    "apply() argument '{}' must be one of {}, not '{text}'",
                    self.name,
                    names.join(", ")
                ))
            })
    }
}
