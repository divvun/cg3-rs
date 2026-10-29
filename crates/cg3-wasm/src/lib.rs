//! `@divvun/cg3`: the library's load-and-run API, for JavaScript.
//!
//! The `#[wasm_bindgen]` layer only converts values. What it converts to and
//! from — [`ApplyOptions`], [`Applied`], [`Info`] — and the run itself,
//! [`apply`], are plain Rust, so the host test suite covers them.

use std::sync::{Arc, Mutex};

use cg3::embed::{CompiledGrammar, Format, GrammarInfo, RunOptions};
use serde::{Deserialize, Serialize};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata};
use wasm_bindgen::prelude::*;

// [spec:cg3:req:wasm.module]
// The declarations package.sh ships beside the ES module wasm-bindgen writes.
#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** A stream format, as vislcg3's `--in-*` and `--out-*` options name them. */
export type Format = "cg" | "niceline" | "apertium" | "fst" | "plain" | "jsonl";

/**
 * The options a run reads, each named after its vislcg3 long option. An option
 * that takes an optional value is `true` without one.
 */
export interface ApplyOptions {
  input?: Format;
  output?: Format;
  ordered?: boolean;
  unsafe?: boolean;
  sections?: string;
  rules?: string;
  rule?: string;
  debugRules?: string;
  verbose?: boolean | string;
  noMappings?: boolean;
  noCorrections?: boolean;
  noBeforeSections?: boolean;
  noSections?: boolean;
  noAfterSections?: boolean;
  trace?: boolean | string;
  traceNameOnly?: boolean;
  traceNoRemoved?: boolean;
  traceEncl?: boolean;
  deleted?: boolean;
  singleRun?: boolean;
  maxRuns?: number;
  unicodeTags?: boolean;
  uniqueTags?: boolean;
  printIds?: boolean;
  printDep?: boolean;
  numWindows?: number;
  alwaysSpan?: boolean;
  softLimit?: number;
  hardLimit?: number;
  textDelimit?: boolean | string;
  depDelimit?: boolean | string;
  depAbsolute?: boolean;
  depOriginal?: boolean;
  depAllowLoops?: boolean;
  depNoCrossing?: boolean;
  noMagicReadings?: boolean;
  noPassOrigin?: boolean;
  splitMappings?: boolean;
  showEndTags?: boolean;
  noBreak?: boolean;
}

/** A run's output, and every warning and non-fatal error it logged, in order. */
export interface ApplyResult {
  output: string;
  warnings: string[];
}

/** What vislcg3's verbose summary reports about a loaded grammar. */
export interface GrammarInfo {
  sections: number;
  templates: number;
  rules: number;
  sets: number;
  tags: number;
  rulesAny: number;
  hasDependencyRules: boolean;
  hasRelationRules: boolean;
}
"#;

#[wasm_bindgen]
extern "C" {
    /// The options object JavaScript passes, declared as `ApplyOptions`.
    #[wasm_bindgen(typescript_type = "ApplyOptions")]
    pub type JsApplyOptions;
}

/// A compiled grammar, loaded once and applied any number of times.
#[wasm_bindgen]
pub struct Grammar {
    inner: CompiledGrammar,
}

#[wasm_bindgen]
impl Grammar {
    // [spec:cg3:req:wasm.grammar-load]
    // [spec:cg3:req:wasm.errors]
    #[wasm_bindgen(js_name = fromBytes)]
    pub fn from_bytes(bytes: &[u8]) -> Result<Grammar, JsError> {
        // A panic traps; every run needs a grammar first, so the hook that writes
        // the panic's message to the console is in place before any run.
        console_error_panic_hook::set_once();
        let inner = CompiledGrammar::from_cg3b(bytes).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Grammar { inner })
    }

    #[wasm_bindgen(unchecked_return_type = "GrammarInfo")]
    pub fn info(&self) -> Result<JsValue, JsError> {
        to_js(&Info::from(self.inner.info()))
    }

    // [spec:cg3:req:wasm.apply+1]
    // [spec:cg3:req:wasm.errors]
    #[wasm_bindgen(unchecked_return_type = "ApplyResult")]
    pub fn apply(&self, input: &str, options: Option<JsApplyOptions>) -> Result<JsValue, JsError> {
        let options = match options.map(JsValue::from).filter(|o| !o.is_null()) {
            None => ApplyOptions::default(),
            Some(options) => {
                let json = js_sys::JSON::stringify(&options)
                    .map_err(|_| JsError::new("options are not a plain object"))?;
                parse_options(&String::from(json)).map_err(|e| JsError::new(&e))?
            }
        };
        let applied = apply(&self.inner, input, &options).map_err(|e| JsError::new(&e))?;
        to_js(&applied)
    }
}

/// A value as the plain JavaScript object its JSON is.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    let json = serde_json::to_string(value)?;
    js_sys::JSON::parse(&json).map_err(|_| JsError::new("unreadable result"))
}

/// Reads `ApplyOptions` from the JSON of the object JavaScript passed. The
/// options go through JSON because only a deserializer that sees every key can
/// refuse the ones it does not know.
pub fn parse_options(json: &str) -> Result<ApplyOptions, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

/// [`GrammarInfo`] as JavaScript sees it.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub sections: usize,
    pub templates: usize,
    pub rules: usize,
    pub sets: usize,
    pub tags: usize,
    pub rules_any: usize,
    pub has_dependency_rules: bool,
    pub has_relation_rules: bool,
}

impl From<GrammarInfo> for Info {
    fn from(i: GrammarInfo) -> Info {
        Info {
            sections: i.sections,
            templates: i.templates,
            rules: i.rules,
            sets: i.sets,
            tags: i.tags,
            rules_any: i.rules_any,
            has_dependency_rules: i.has_dependency_rules,
            has_relation_rules: i.has_relation_rules,
        }
    }
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

/// A run's output and what it logged.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Applied {
    pub output: String,
    pub warnings: Vec<String>,
}

// [spec:cg3:req:wasm.no-process]
/// Runs `grammar` on `input`. A failure is the library error's message; an
/// `EXTERNAL` rule fails here, where its process would start, because a target
/// without processes refuses to start one.
pub fn apply(
    grammar: &CompiledGrammar,
    input: &str,
    options: &ApplyOptions,
) -> Result<Applied, String> {
    let collector = Arc::new(Collector::default());
    let dispatch = tracing::Dispatch::from(Arc::clone(&collector));
    let ran = tracing::dispatcher::with_default(&dispatch, || {
        grammar.run(input.as_bytes(), &RunOptions::from(options))
    });
    let output = ran.map_err(|e| e.to_string())?;
    let output = String::from_utf8(output).map_err(|e| e.to_string())?;
    let warnings = std::mem::take(&mut *collector.messages.lock().map_err(|e| e.to_string())?);
    Ok(Applied { output, warnings })
}

/// Collects the message of every warning and error a run logs. The library
/// logs an error it carries on past (a malformed reading in a stream, say) at
/// error level, so those are kept too.
#[derive(Default)]
struct Collector {
    messages: Mutex<Vec<String>>,
}

impl tracing::Subscriber for Collector {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= Level::WARN
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut message = Message(String::new());
        event.record(&mut message);
        if let Ok(mut messages) = self.messages.lock() {
            messages.push(message.0);
        }
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

struct Message(String);

impl Visit for Message {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}
