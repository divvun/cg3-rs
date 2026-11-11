//! A run's options, a load's, cg-conv's and cg-proc's, as the fields of
//! `cg3.RunOptions`, `cg3.LoadOptions`, `cg3.ConvertOptions` and
//! `cg3.ProcOptions`, the dataclasses in `python/cg3/_options.py`.

use cg3::ConvertOptions;
use cg3::embed::{Format, LoadOptions, ProcFormat, ProcOptions, RunOptions};
use pyo3::exceptions::{PyOverflowError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::pybacked::PyBackedStr;
use pyo3::types::{PyBool, PyFloat, PyInt, PyString};

// [spec:cg3:req:python.apply+1]
/// The options `Grammar.apply` was given: those of a `cg3.RunOptions`, or none.
pub fn of_apply(options: Option<&Bound<'_, PyAny>>) -> PyResult<RunOptions> {
    match given(options, "RunOptions", "apply")? {
        Some(options) => run_options(options),
        None => Ok(RunOptions::default()),
    }
}

/// `options`, when it is an instance of the class `class` of `cg3._options`,
/// or none when the call was given none. `function` is the call, for the
/// error a value of another type raises.
fn given<'a, 'py>(
    options: Option<&'a Bound<'py, PyAny>>,
    class: &str,
    function: &str,
) -> PyResult<Option<&'a Bound<'py, PyAny>>> {
    let Some(options) = options else {
        return Ok(None);
    };
    let expected = options.py().import("cg3._options")?.getattr(class)?;
    if !options.is_instance(&expected)? {
        let given = options.get_type().name()?;
        return Err(PyTypeError::new_err(format!(
            "{function}() argument 'options' must be {class}, not {given}"
        )));
    }
    Ok(Some(options))
}

/// Each field of the dataclass `options`, by name, with its value.
fn fields<'py>(options: &Bound<'py, PyAny>) -> PyResult<Vec<(PyBackedStr, Bound<'py, PyAny>)>> {
    let fields = options
        .py()
        .import("dataclasses")?
        .call_method1("fields", (options,))?;
    fields
        .try_iter()?
        .map(|field| {
            let name: PyBackedStr = field?.getattr("name")?.extract()?;
            let value = options.getattr(&*name)?;
            Ok((name, value))
        })
        .collect()
}

// [spec:cg3:req:python.apply+1]
/// Reads each field of a `cg3.RunOptions`, named after its vislcg3 long option
/// in snake_case, into the option it stands for. A value of the wrong type
/// raises the `TypeError` a typed parameter would, a number out of range
/// `OverflowError`, and a format the package does not offer `ValueError`.
/// `RunOptions.__post_init__` reads its fields here, so options that were made
/// are options a run can read.
pub fn run_options(options: &Bound<'_, PyAny>) -> PyResult<RunOptions> {
    let mut o = RunOptions::default();
    for (field_name, value) in fields(options)? {
        let name: &str = &field_name;
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
    match given(options, "LoadOptions", function)? {
        Some(options) => load_options(options),
        None => Ok(LoadOptions::default()),
    }
}

// [spec:cg3:req:python.load-options]
/// Reads each field of a `cg3.LoadOptions`, named after its vislcg3 long
/// option in snake_case, into the option it stands for. A value of the wrong
/// type raises `TypeError`, and a `prefix` that is not one character or a rule
/// filter that does not compile `ValueError`.
pub fn load_options(options: &Bound<'_, PyAny>) -> PyResult<LoadOptions> {
    let mut o = LoadOptions::default();
    for (field_name, value) in fields(options)? {
        let name: &str = &field_name;
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

// [spec:cg3:req:python.tools]
/// The options `convert` was given: those of a `cg3.ConvertOptions`, or none.
pub fn of_convert(options: Option<&Bound<'_, PyAny>>) -> PyResult<ConvertOptions> {
    match given(options, "ConvertOptions", "convert")? {
        Some(options) => convert_options(options),
        None => Ok(ConvertOptions::default()),
    }
}

// [spec:cg3:req:python.tools]
/// Reads each field of a `cg3.ConvertOptions`, named after its cg-conv long
/// option in snake_case, into the option it stands for, with `input_format`
/// and `output_format` standing for `--in-*` and `--out-*`. `rtl` is
/// cg-conv's default, which `ltr` overrides, so it is read and does nothing,
/// as it does in cg-conv. `dep_delimit=True` is `--dep-delimit` without a
/// value, which cg-conv reads as 10.
pub fn convert_options(options: &Bound<'_, PyAny>) -> PyResult<ConvertOptions> {
    let mut o = ConvertOptions::default();
    for (field_name, value) in fields(options)? {
        let name: &str = &field_name;
        let v = Value {
            class: "ConvertOptions",
            name,
            value: &value,
        };
        match name {
            "input_format" => o.input = v.format()?,
            "in_auto" => o.in_auto = v.flag()?,
            "add_tags" => o.add_tags = v.flag()?,
            "output_format" => o.output = v.format()?,
            "prefix" => o.prefix = v.character()?,
            "wfactor" => o.wfactor = v.number()?,
            "wtag" => o.wtag = v.text()?,
            "sub_delim" => o.sub_delim = v.text()?,
            "rtl" => {
                v.flag()?;
            }
            "ltr" => o.ltr = v.flag()?,
            "ordered" => o.ordered = v.flag()?,
            "parse_dep" => o.parse_dep = v.flag()?,
            "dep_delimit" => o.dep_delimit = v.flag_or_count(10)?,
            "unicode_tags" => o.unicode_tags = v.flag()?,
            "deleted" => o.deleted = v.flag()?,
            "no_break" => o.no_break = v.flag()?,
            _ => {
                return Err(PyTypeError::new_err(format!(
                    "ConvertOptions has a field '{name}' no conversion reads"
                )));
            }
        }
    }
    Ok(o)
}

// [spec:cg3:req:python.tools]
/// The options `proc` was given: those of a `cg3.ProcOptions`, or none.
pub fn of_proc(options: Option<&Bound<'_, PyAny>>) -> PyResult<ProcOptions> {
    match given(options, "ProcOptions", "proc")? {
        Some(options) => proc_options(options),
        None => Ok(ProcOptions::default()),
    }
}

// [spec:cg3:req:python.tools]
/// Reads each field of a `cg3.ProcOptions`, named after its cg-proc long
/// option in snake_case, into the option it stands for. Disambiguation is all
/// cg-proc does and it always flushes on a null byte, so `disambiguation` and
/// `null_flush` are read and do nothing, as `-d` and `-z` do in cg-proc.
pub fn proc_options(options: &Bound<'_, PyAny>) -> PyResult<ProcOptions> {
    let mut o = ProcOptions::default();
    for (field_name, value) in fields(options)? {
        let name: &str = &field_name;
        let v = Value {
            class: "ProcOptions",
            name,
            value: &value,
        };
        match name {
            "stream_format" => o.stream_format = v.proc_format()?,
            "sections" => o.sections = v.count()?,
            "rule" => o.rule = v.text()?,
            "trace" => o.trace = v.flag()?,
            "wordform_case" => o.wordform_case = v.flag()?,
            "no_word_forms" => o.no_word_forms = v.flag()?,
            "generation" => o.generation = v.flag()?,
            "first" => o.first = v.flag()?,
            "disambiguation" | "null_flush" => {
                v.flag()?;
            }
            _ => {
                return Err(PyTypeError::new_err(format!(
                    "ProcOptions has a field '{name}' no cg-proc run reads"
                )));
            }
        }
    }
    Ok(o)
}

/// Checks a `cg3.ConvertOptions` as it is made, raising what a conversion
/// reading its fields would raise.
#[pyfunction]
#[pyo3(name = "_check_convert_options")]
pub fn check_convert_options(options: &Bound<'_, PyAny>) -> PyResult<()> {
    convert_options(options).map(drop)
}

/// Checks a `cg3.ProcOptions` as it is made, raising what a cg-proc run
/// reading its fields would raise.
#[pyfunction]
#[pyo3(name = "_check_proc_options")]
pub fn check_proc_options(options: &Bound<'_, PyAny>) -> PyResult<()> {
    proc_options(options).map(drop)
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

/// The streams `ProcOptions.stream_format` names, as cg-proc's
/// `--stream-format` numbers them 0, 1 and 2. The binary format, 3, is left
/// out: a string cannot carry it.
const PROC_FORMATS: [(&str, ProcFormat); 3] = [
    ("cg", ProcFormat::Cg),
    ("apertium", ProcFormat::Apertium),
    ("matxin", ProcFormat::Matxin),
];

pub fn proc_format_names() -> impl ExactSizeIterator<Item = &'static str> {
    PROC_FORMATS.iter().map(|(name, _)| *name)
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

    /// An option whose value the tool makes optional and reads as a whole
    /// number: `True` gives it without one, which the tool reads as
    /// `without`, and `False` leaves it off.
    fn flag_or_count(&self, without: u32) -> PyResult<Option<u32>> {
        if let Ok(b) = self.value.cast::<PyBool>() {
            return Ok(b.is_true().then_some(without));
        }
        if self.value.is_none() || self.value.is_instance_of::<PyInt>() {
            return self.count();
        }
        Err(self.wrong_type("bool or int"))
    }

    /// A number, as a `float` or an `int`. A bool is refused although Python
    /// counts it as an int.
    fn number(&self) -> PyResult<Option<f64>> {
        if self.value.is_none() {
            return Ok(None);
        }
        if self.value.is_instance_of::<PyBool>()
            || !(self.value.is_instance_of::<PyFloat>() || self.value.is_instance_of::<PyInt>())
        {
            return Err(self.wrong_type("float"));
        }
        self.value.extract::<f64>().map(Some)
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

    fn proc_format(&self) -> PyResult<ProcFormat> {
        let Some(text) = self.text()? else {
            return Ok(ProcFormat::default());
        };
        PROC_FORMATS
            .iter()
            .find(|(name, _)| *name == text)
            .map(|(_, format)| *format)
            .ok_or_else(|| {
                let names: Vec<String> =
                    PROC_FORMATS.iter().map(|(n, _)| format!("'{n}'")).collect();
                PyValueError::new_err(format!(
                    "{}() argument '{}' must be one of {}, not '{text}'",
                    self.class,
                    self.name,
                    names.join(", ")
                ))
            })
    }
}
