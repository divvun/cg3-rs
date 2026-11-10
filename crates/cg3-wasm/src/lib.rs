//! The wasm-bindgen glue behind `@divvun/cg3`.
//!
//! These exports are not a public API. The TypeScript wrapper under `js/cg3/`
//! is, and every call reaches the glue through it. The names here are chosen
//! for that wrapper, not for end users.

use wasm_bindgen::prelude::wasm_bindgen;

mod error;
mod grammar;
mod log;
mod options;
mod sink;
#[cfg(feature = "test-hooks")]
mod test_hooks;

pub use error::{ErrorCode, GlueError};
pub use grammar::{Applied, Grammar, apply, read_grammar};
pub use log::set_level;
pub use options::{ApplyOptions, FlagOrValue, JsFormat, parse_options};
pub use sink::install;
#[cfg(feature = "test-hooks")]
pub use test_hooks::{TestObject, test_log, test_panic};

// [spec:cg3:req:wasm.module+2]
/// The version of the npm package, which this crate carries as its own.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    // [spec:cg3:req:wasm.module+2/test]
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
