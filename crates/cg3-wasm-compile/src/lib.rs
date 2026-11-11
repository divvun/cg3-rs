//! The wasm-bindgen glue behind `@divvun/cg3/compile`.
//!
//! These exports are not a public API. The TypeScript wrapper under `js/cg3/`
//! is, and every call reaches the glue through it. This module holds the
//! textual parser and the writers, which the main module, `crates/cg3-wasm`,
//! must not link; `js/build.mjs` checks both.

use cg3::embed::LoadOptions;
use cg3::textual_parser::MEMORY_SOURCE_NAME;
use cg3_wasm_shared::parse_load_options;
use wasm_bindgen::prelude::*;

mod compile;
mod failure;
mod files;

pub use compile::{Compiled, compile, relabel, to_text};
pub use failure::{CompileError, Diagnostic};
pub use files::{AskedFiles, memory_files};

// [spec:cg3:req:wasm.module+3]
/// The version of the npm package, which `crates/cg3-wasm` carries and this
/// crate carries with it.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

// [spec:cg3:req:wasm.compile]
// [spec:cg3:req:wasm.compile-includes]
/// Compiles `source`, called `name` or `<utf8-memory>` without one, under the
/// options in `options`, the JSON of a `LoadOptions` object. `files` is what
/// the wrapper makes of the files the caller gave: `undefined`, name and
/// content pairs, or the caller's function. Returns a `CompileResult` object.
#[wasm_bindgen(js_name = compileGrammar)]
pub fn compile_grammar(
    source: &str,
    name: Option<String>,
    files: JsValue,
    options: &str,
) -> Result<JsValue, CompileError> {
    let options: LoadOptions = parse_load_options(options)?;
    let name = name.as_deref().unwrap_or(MEMORY_SOURCE_NAME);
    let compiled = match files.dyn_into::<js_sys::Function>() {
        Ok(function) => {
            let asked = AskedFiles::new(function);
            let thrown = asked.thrown();
            compile(source, name, asked, &options)
                .map_err(|failed| thrown.take().map_or(failed, CompileError::Thrown))?
        }
        Err(files) => compile(source, name, memory_files(&files)?, &options)?,
    };
    Ok(compiled.into())
}

#[cfg(test)]
mod tests {
    // [spec:cg3:req:wasm.module+3/test]
    #[test]
    fn version_is_the_packages() {
        let manifest = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../js/cg3/package.json"
        ))
        .expect("js/cg3/package.json is in the repository");
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest).expect("package.json is JSON");
        assert_eq!(super::version(), manifest["version"]);
        assert_eq!(
            manifest["exports"]["./compile"]["default"],
            "./dist/compile.js"
        );
    }
}
