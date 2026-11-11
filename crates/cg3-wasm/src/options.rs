//! The options JavaScript passes: `ApplyOptions` and `LoadOptions` as
//! `js/cg3/grammar.ts` declares them, and `ConvertOptions` and `ProcOptions`
//! as `js/cg3/tools.ts` does.

use cg3::ConvertOptions;
use cg3::embed::{Format, LoadOptions, ProcFormat, ProcOptions, RunOptions};
use serde::Deserialize;

use crate::error::{ErrorCode, GlueError};

/// Reads `ApplyOptions` from the JSON of the object JavaScript passed. The
/// options go through JSON because only a deserializer that sees every key can
/// refuse the ones it does not know.
pub fn parse_options(json: &str) -> Result<ApplyOptions, GlueError> {
    serde_json::from_str(json)
        .map_err(|e| GlueError::new(ErrorCode::InvalidArgument, e.to_string()))
}

/// Every stream format but the binary one, which a string cannot carry.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum JsFormat {
    Cg,
    Niceline,
    Apertium,
    Fst,
    Plain,
    Jsonl,
}

impl From<JsFormat> for Format {
    fn from(f: JsFormat) -> Format {
        match f {
            JsFormat::Cg => Format::Cg,
            JsFormat::Niceline => Format::Niceline,
            JsFormat::Apertium => Format::Apertium,
            JsFormat::Fst => Format::Fst,
            JsFormat::Plain => Format::Plain,
            JsFormat::Jsonl => Format::Jsonl,
        }
    }
}

/// An option whose value vislcg3 makes optional: `true` gives it without one.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum FlagOrValue {
    Flag(bool),
    Value(String),
}

fn optional_value(v: &Option<FlagOrValue>) -> Option<String> {
    match v {
        None | Some(FlagOrValue::Flag(false)) => None,
        Some(FlagOrValue::Flag(true)) => Some(String::new()),
        Some(FlagOrValue::Value(s)) => Some(s.clone()),
    }
}

/// `ApplyOptions` from JavaScript. An option it does not know is an error, so a
/// misspelt one is not silently ignored.
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ApplyOptions {
    pub input: Option<JsFormat>,
    pub output: Option<JsFormat>,
    pub ordered: bool,
    #[serde(rename = "unsafe")]
    pub unsafe_mode: bool,
    pub sections: Option<String>,
    pub rules: Option<String>,
    pub rule: Option<String>,
    pub debug_rules: Option<String>,
    pub verbose: Option<FlagOrValue>,
    pub no_mappings: bool,
    pub no_corrections: bool,
    pub no_before_sections: bool,
    pub no_sections: bool,
    pub no_after_sections: bool,
    pub trace: Option<FlagOrValue>,
    pub trace_name_only: bool,
    pub trace_no_removed: bool,
    pub trace_encl: bool,
    pub deleted: bool,
    pub single_run: bool,
    pub max_runs: Option<u32>,
    pub unicode_tags: bool,
    pub unique_tags: bool,
    pub print_ids: bool,
    pub print_dep: bool,
    pub num_windows: Option<u32>,
    pub always_span: bool,
    pub soft_limit: Option<u32>,
    pub hard_limit: Option<u32>,
    pub text_delimit: Option<FlagOrValue>,
    pub dep_delimit: Option<FlagOrValue>,
    pub dep_absolute: bool,
    pub dep_original: bool,
    pub dep_allow_loops: bool,
    pub dep_no_crossing: bool,
    pub no_magic_readings: bool,
    pub no_pass_origin: bool,
    pub split_mappings: bool,
    pub show_end_tags: bool,
    pub no_break: bool,
}

impl From<&ApplyOptions> for RunOptions {
    fn from(o: &ApplyOptions) -> RunOptions {
        RunOptions {
            input: o.input.map(Format::from),
            output: o.output.map(Format::from),
            ordered: o.ordered,
            unsafe_mode: o.unsafe_mode,
            sections: o.sections.clone(),
            rules: o.rules.clone(),
            rule: o.rule.clone(),
            debug_rules: o.debug_rules.clone(),
            verbose: optional_value(&o.verbose),
            no_mappings: o.no_mappings,
            no_corrections: o.no_corrections,
            no_before_sections: o.no_before_sections,
            no_sections: o.no_sections,
            no_after_sections: o.no_after_sections,
            trace: optional_value(&o.trace),
            trace_name_only: o.trace_name_only,
            trace_no_removed: o.trace_no_removed,
            trace_encl: o.trace_encl,
            deleted: o.deleted,
            single_run: o.single_run,
            max_runs: o.max_runs,
            unicode_tags: o.unicode_tags,
            unique_tags: o.unique_tags,
            print_ids: o.print_ids,
            print_dep: o.print_dep,
            num_windows: o.num_windows,
            always_span: o.always_span,
            soft_limit: o.soft_limit,
            hard_limit: o.hard_limit,
            text_delimit: optional_value(&o.text_delimit),
            dep_delimit: optional_value(&o.dep_delimit),
            dep_absolute: o.dep_absolute,
            dep_original: o.dep_original,
            dep_allow_loops: o.dep_allow_loops,
            dep_no_crossing: o.dep_no_crossing,
            no_magic_readings: o.no_magic_readings,
            no_pass_origin: o.no_pass_origin,
            split_mappings: o.split_mappings,
            show_end_tags: o.show_end_tags,
            no_break: o.no_break,
        }
    }
}

// [spec:cg3:req:wasm.load-options]
/// `LoadOptions` from JavaScript. An option it does not know is an error, so a
/// misspelt one is not silently ignored.
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct JsLoadOptions {
    nrules: Option<String>,
    nrules_v: Option<String>,
    vislcg_compat: bool,
    prefix: Option<String>,
}

// [spec:cg3:req:wasm.load-options]
/// Reads the library's `LoadOptions` from the JSON of the `LoadOptions` object
/// JavaScript passed: each key a vislcg3 load option in camelCase, `prefix` one
/// character, and the rule filters patterns that compile.
pub fn parse_load_options(json: &str) -> Result<LoadOptions, GlueError> {
    let invalid = |message: String| GlueError::new(ErrorCode::InvalidArgument, message);
    let js: JsLoadOptions = serde_json::from_str(json).map_err(|e| invalid(e.to_string()))?;
    for (pattern, flag) in [(&js.nrules, "--nrules"), (&js.nrules_v, "--nrules-v")] {
        if let Some(pattern) = pattern {
            cg3::embed::rule_filter(pattern, flag).map_err(|e| invalid(e.to_string()))?;
        }
    }
    Ok(LoadOptions {
        prefix: prefix(js.prefix.as_deref())?,
        nrules: js.nrules,
        nrules_v: js.nrules_v,
        vislcg_compat: js.vislcg_compat,
    })
}

/// A mapping prefix, which JavaScript gives as a string of one character.
fn prefix(given: Option<&str>) -> Result<Option<char>, GlueError> {
    let Some(given) = given else {
        return Ok(None);
    };
    let mut chars = given.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(Some(c)),
        _ => Err(GlueError::new(
            ErrorCode::InvalidArgument,
            "prefix must be one character",
        )),
    }
}

/// An option whose value cg-conv makes optional and reads as a number: `true`
/// gives it without one.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(untagged)]
enum FlagOrNumber {
    Flag(bool),
    Number(u32),
}

// [spec:cg3:req:wasm.tools]
/// `ConvertOptions` from JavaScript: each cg-conv option in camelCase, the
/// `--in-*` and `--out-*` options as `input` and `output`. An option it does
/// not know is an error, so a misspelt one is not silently ignored.
#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct JsConvertOptions {
    input: Option<JsFormat>,
    in_auto: bool,
    add_tags: bool,
    output: Option<JsFormat>,
    prefix: Option<String>,
    wfactor: Option<f64>,
    wtag: Option<String>,
    sub_delim: Option<String>,
    /// cg-conv's default, which `ltr` overrides, so cg-conv does nothing with it.
    rtl: bool,
    ltr: bool,
    ordered: bool,
    parse_dep: bool,
    dep_delimit: Option<FlagOrNumber>,
    unicode_tags: bool,
    deleted: bool,
    no_break: bool,
}

// [spec:cg3:req:wasm.tools]
/// Reads the library's `ConvertOptions` from the JSON of the `ConvertOptions`
/// object JavaScript passed. `depDelimit: true` is `--dep-delimit` without a
/// value, which cg-conv reads as 10.
pub fn parse_convert_options(json: &str) -> Result<ConvertOptions, GlueError> {
    let js: JsConvertOptions = serde_json::from_str(json)
        .map_err(|e| GlueError::new(ErrorCode::InvalidArgument, e.to_string()))?;
    Ok(ConvertOptions {
        input: js.input.map(Format::from),
        in_auto: js.in_auto,
        add_tags: js.add_tags,
        output: js.output.map(Format::from),
        prefix: prefix(js.prefix.as_deref())?,
        wfactor: js.wfactor,
        wtag: js.wtag,
        sub_delim: js.sub_delim,
        ltr: js.ltr,
        ordered: js.ordered,
        parse_dep: js.parse_dep,
        dep_delimit: match js.dep_delimit {
            None | Some(FlagOrNumber::Flag(false)) => None,
            Some(FlagOrNumber::Flag(true)) => Some(10),
            Some(FlagOrNumber::Number(n)) => Some(n),
        },
        unicode_tags: js.unicode_tags,
        deleted: js.deleted,
        no_break: js.no_break,
    })
}

/// cg-proc's stream formats but the binary one, by name rather than by the
/// number `--stream-format` takes.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
enum JsProcFormat {
    Cg,
    Apertium,
    Matxin,
}

// [spec:cg3:req:wasm.tools]
/// `ProcOptions` from JavaScript: each cg-proc option in camelCase. An option
/// it does not know is an error, so a misspelt one is not silently ignored.
#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct JsProcOptions {
    stream_format: Option<JsProcFormat>,
    sections: Option<u32>,
    rule: Option<String>,
    trace: bool,
    wordform_case: bool,
    no_word_forms: bool,
    generation: bool,
    first: bool,
    /// Disambiguation is all cg-proc does, so it does nothing with `-d`.
    disambiguation: bool,
    /// cg-proc always flushes on a null byte, so it does nothing with `-z`.
    null_flush: bool,
}

// [spec:cg3:req:wasm.tools]
/// Reads the library's `ProcOptions` from the JSON of the `ProcOptions` object
/// JavaScript passed.
pub fn parse_proc_options(json: &str) -> Result<ProcOptions, GlueError> {
    let js: JsProcOptions = serde_json::from_str(json)
        .map_err(|e| GlueError::new(ErrorCode::InvalidArgument, e.to_string()))?;
    Ok(ProcOptions {
        stream_format: match js.stream_format {
            Some(JsProcFormat::Cg) => ProcFormat::Cg,
            None | Some(JsProcFormat::Apertium) => ProcFormat::Apertium,
            Some(JsProcFormat::Matxin) => ProcFormat::Matxin,
        },
        sections: js.sections,
        rule: js.rule,
        trace: js.trace,
        wordform_case: js.wordform_case,
        no_word_forms: js.no_word_forms,
        generation: js.generation,
        first: js.first,
    })
}
