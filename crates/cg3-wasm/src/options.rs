//! The run and load options JavaScript passes, as `ApplyOptions` and
//! `LoadOptions` in `js/cg3/grammar.ts` declare them.

use cg3::embed::{Format, LoadOptions, RunOptions};
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
    let prefix = match js.prefix.as_deref().map(|p| {
        let mut chars = p.chars();
        (chars.next(), chars.next())
    }) {
        None => None,
        Some((Some(c), None)) => Some(c),
        Some(_) => return Err(invalid("prefix must be one character".to_owned())),
    };
    Ok(LoadOptions {
        nrules: js.nrules,
        nrules_v: js.nrules_v,
        vislcg_compat: js.vislcg_compat,
        prefix,
    })
}
