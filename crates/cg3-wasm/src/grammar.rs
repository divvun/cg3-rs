//! A compiled grammar and its runs. The `#[wasm_bindgen]` layer only converts
//! values; [`read_grammar`] and [`apply`] are plain Rust, so the host test
//! suite covers them.

use cg3::embed::{CompiledGrammar, GrammarInfo, RunOptions};
use wasm_bindgen::prelude::*;

use crate::error::{ErrorCode, GlueError, set};
use crate::options::{ApplyOptions, parse_load_options, parse_options, parse_proc_options};
use crate::tools::run_failed;

/// A compiled grammar, loaded once and applied any number of times.
#[wasm_bindgen]
pub struct Grammar {
    inner: CompiledGrammar,
}

// [spec:cg3:req:wasm.grammar-load+1]
// [spec:cg3:req:wasm.errors+1]
// [spec:cg3:req:wasm.load-options]
/// Loads a `.cg3b` under the options in `options`, the JSON of a
/// `LoadOptions` object.
#[wasm_bindgen(js_name = readGrammar)]
pub fn read_grammar(bytes: &[u8], options: &str) -> Result<Grammar, GlueError> {
    CompiledGrammar::from_cg3b(bytes, &parse_load_options(options)?)
        .map(|inner| Grammar { inner })
        .map_err(|e| GlueError::new(ErrorCode::InvalidGrammar, e.to_string()))
}

#[wasm_bindgen]
impl Grammar {
    // [spec:cg3:req:wasm.grammar-load+1]
    /// What `CompiledGrammar::info` reports, as `GrammarInfo` in
    /// `js/cg3/grammar.ts` declares it.
    pub fn info(&self) -> JsValue {
        info_object(self.inner.info())
    }

    // [spec:cg3:req:wasm.apply+1]
    // [spec:cg3:req:wasm.errors+1]
    /// Runs the grammar on `input` with the options in `options`, the JSON of
    /// an `ApplyOptions` object, and returns an `ApplyResult` object.
    pub fn apply(&self, input: &str, options: &str) -> Result<JsValue, GlueError> {
        let applied = apply(&self.inner, input, &parse_options(options)?)?;
        let object = js_sys::Object::new();
        set(&object, "output", &JsValue::from_str(&applied.output));
        let warnings: js_sys::Array = applied
            .warnings
            .iter()
            .map(|w| JsValue::from_str(w))
            .collect();
        set(&object, "warnings", &warnings);
        Ok(object.into())
    }

    // [spec:cg3:req:wasm.tools]
    /// What cg-proc writes for `input` with this grammar under the options in
    /// `options`, the JSON of a `ProcOptions` object, through
    /// `CompiledGrammar::proc`.
    pub fn proc(&self, input: &str, options: &str) -> Result<String, GlueError> {
        let options = parse_proc_options(options)?;
        let run = self.inner.proc(input.as_bytes(), &options);
        String::from_utf8(run.map_err(run_failed)?).map_err(run_failed)
    }
}

impl Grammar {
    /// The loaded grammar, for the host tests.
    pub fn compiled(&self) -> &CompiledGrammar {
        &self.inner
    }
}

fn info_object(info: GrammarInfo) -> JsValue {
    let object = js_sys::Object::new();
    let counts = [
        ("sections", info.sections),
        ("templates", info.templates),
        ("rules", info.rules),
        ("sets", info.sets),
        ("tags", info.tags),
        ("rulesAny", info.rules_any),
    ];
    for (key, count) in counts {
        set(&object, key, &JsValue::from_f64(count as f64));
    }
    set(
        &object,
        "hasDependencyRules",
        &JsValue::from_bool(info.has_dependency_rules),
    );
    set(
        &object,
        "hasRelationRules",
        &JsValue::from_bool(info.has_relation_rules),
    );
    object.into()
}

/// A run's output and what it logged.
#[derive(Debug, PartialEq, Eq)]
pub struct Applied {
    pub output: String,
    pub warnings: Vec<String>,
}

// [spec:cg3:req:wasm.no-process]
// [spec:cg3:req:wasm.parity+2]
// [spec:cg3:req:wasm.errors+1]
/// Runs `grammar` on `input` through `CompiledGrammar::run`, the operation
/// vislcg3 runs every grammar through, so the two write the same bytes;
/// tests/parity.rs checks that over the corpus on the host, and
/// js/test/parity.ts through the built package in every runtime. A failure is the library
/// error's message, under `run-failed`; an `EXTERNAL` rule fails here, where
/// its process would start, because a target without processes refuses to
/// start one.
pub fn apply(
    grammar: &CompiledGrammar,
    input: &str,
    options: &ApplyOptions,
) -> Result<Applied, GlueError> {
    let (ran, warnings) =
        crate::log::collecting(|| grammar.run(input.as_bytes(), &RunOptions::from(options)));
    let failed = |message: String| GlueError::new(ErrorCode::RunFailed, message);
    let output = ran.map_err(|e| failed(e.to_string()))?;
    let output = String::from_utf8(output).map_err(|e| failed(e.to_string()))?;
    Ok(Applied { output, warnings })
}
