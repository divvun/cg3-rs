//! Compiling a textual grammar, writing one out as text, and relabelling one.
//! Grammars come in and go out as `.cg3b` bytes, which is how they cross to
//! the main module: the two are separate instances with separate memories.

use cg3::embed::{CompiledGrammar, IncludeResolver, LoadOptions, MemoryResolver};
use cg3::error::Cg3Error;
use cg3::inlines::is_cg3b;
use cg3::textual_parser::MEMORY_SOURCE_NAME;
use cg3_wasm_shared::log::collecting;
use cg3_wasm_shared::{ErrorCode, GlueError, set};
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::failure::CompileError;

/// A compiled grammar, as cg-comp writes it, and what its parse logged.
#[derive(Debug, PartialEq, Eq)]
pub struct Compiled {
    pub cg3b: Vec<u8>,
    /// The companion file cg-comp writes beside the `.cg3b`.
    pub companion: Option<Vec<u8>>,
    /// Every warning and error the parse logged, in order.
    pub warnings: Vec<String>,
}

// [spec:cg3:req:wasm.compile]
/// Compiles `source`, called `name` in its diagnostics and in the companion
/// file, through `CompiledGrammar::from_text` under `options`, with every
/// `INCLUDE` read through `files`, and writes it as cg-comp does.
pub fn compile(
    source: &str,
    name: &str,
    files: impl IncludeResolver + 'static,
    options: &LoadOptions,
) -> Result<Compiled, CompileError> {
    let (written, warnings) = collecting(|| {
        let grammar = CompiledGrammar::from_text(source.as_bytes(), name, files, options)?;
        Ok::<_, Cg3Error>((grammar.to_cg3b()?, grammar.companion()?))
    });
    let (cg3b, companion) = written.map_err(CompileError::failed)?;
    Ok(Compiled {
        cg3b,
        companion,
        warnings,
    })
}

impl From<Compiled> for JsValue {
    fn from(compiled: Compiled) -> JsValue {
        let object = js_sys::Object::new();
        set(
            &object,
            "cg3b",
            &js_sys::Uint8Array::from(&compiled.cg3b[..]),
        );
        let companion = compiled
            .companion
            .map_or(JsValue::NULL, |c| js_sys::Uint8Array::from(&c[..]).into());
        set(&object, "companion", &companion);
        let warnings: js_sys::Array = compiled
            .warnings
            .iter()
            .map(|w| JsValue::from_str(w))
            .collect();
        set(&object, "warnings", &warnings);
        object.into()
    }
}

fn invalid_grammar(error: Cg3Error) -> GlueError {
    GlueError::new(ErrorCode::InvalidGrammar, error.to_string())
}

fn read_cg3b(bytes: &[u8]) -> Result<CompiledGrammar, GlueError> {
    CompiledGrammar::from_cg3b(bytes, &LoadOptions::default()).map_err(invalid_grammar)
}

// [spec:cg3:req:wasm.compile]
/// What `to_text` writes for the grammar in the `.cg3b` `bytes`: what vislcg3
/// `--grammar-out` writes for it.
#[wasm_bindgen(js_name = toText)]
pub fn to_text(bytes: &[u8]) -> Result<String, GlueError> {
    let options = LoadOptions::default();
    let grammar = CompiledGrammar::from_cg3b(bytes, &options).map_err(invalid_grammar)?;
    Ok(grammar.to_text())
}

// [spec:cg3:req:wasm.compile]
/// The `.cg3b` of the grammar `CompiledGrammar::relabel` makes from the `.cg3b`
/// `bytes` and the relabel rules in `relabels`, which are read as cg-relabel
/// reads its rule file: as a `.cg3b` when they begin with its magic, and as a
/// textual grammar otherwise.
#[wasm_bindgen]
pub fn relabel(bytes: &[u8], relabels: &[u8]) -> Result<Vec<u8>, CompileError> {
    let grammar = read_cg3b(bytes)?;
    let rules = if is_cg3b(relabels) {
        read_cg3b(relabels)?
    } else {
        let resolver = MemoryResolver::default();
        let options = LoadOptions::default();
        CompiledGrammar::from_text(relabels, MEMORY_SOURCE_NAME, resolver, &options)
            .map_err(CompileError::failed)?
    };
    let relabelled = grammar.relabel(&rules).and_then(|g| g.to_cg3b());
    Ok(relabelled.map_err(invalid_grammar)?)
}
