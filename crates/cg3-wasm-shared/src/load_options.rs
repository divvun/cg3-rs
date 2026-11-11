//! The load options JavaScript passes, as `LoadOptions` in `js/cg3/grammar.ts`
//! declares them: what `readGrammar` and `compileGrammar` load under.

use cg3::embed::LoadOptions;
use serde::Deserialize;

use crate::error::{ErrorCode, GlueError};

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
pub fn prefix(given: Option<&str>) -> Result<Option<char>, GlueError> {
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
