//! The wasm-bindgen glue behind `@divvun/cg3`.
//!
//! These exports are not a public API. The TypeScript wrapper under `js/cg3/`
//! is, and every call reaches the glue through it. The names here are chosen
//! for that wrapper, not for end users.

use wasm_bindgen::prelude::wasm_bindgen;

// [spec:cg3:req:wasm.read-only-core]
// These modules are the exports: they load a `.cg3b`, run a grammar, and
// convert and split streams. None of them compiles or writes a grammar, so the
// linker leaves the textual parser and the writers out of this module, and
// js/build.mjs fails a build whose module has them. They are
// crates/cg3-wasm-compile's.
mod grammar;
mod options;
mod tools;

pub use cg3_wasm_shared::{ErrorCode, GlueError, parse_load_options};
pub use grammar::{Applied, Grammar, apply, read_grammar};
pub use options::{
    ApplyOptions, FlagOrValue, JsFormat, parse_convert_options, parse_options, parse_proc_options,
};
pub use tools::{convert, mwesplit};

// [spec:cg3:req:wasm.module+3]
/// The version of the npm package, which this crate carries as its own.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    // [spec:cg3:req:wasm.module+3/test]
    #[test]
    fn package_is_the_crates_version_under_the_gpl() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        let manifest = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../js/cg3/package.json"
        ))
        .expect("js/cg3/package.json is in the repository");
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest).expect("package.json is JSON");
        assert_eq!(manifest["name"], "@divvun/cg3");
        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(manifest["license"], "GPL-3.0-or-later");
    }
}
