//! A compiled grammar and its runs.

use std::sync::Arc;

use cg3::embed::CompiledGrammar;

use crate::error::Cg3Error;
use crate::log::{collecting, logging};
use crate::options::{ApplyOptions, LoadOptions, ProcRunOptions, load_options};
use crate::tools::stream;

// [spec:cg3:req:swift.concurrency]
/// A compiled grammar, loaded once and applied any number of times. It does
/// not change, so threads may apply it at once: each apply is a run of its own
/// over the one grammar.
#[derive(uniffi::Object)]
pub struct Grammar {
    inner: CompiledGrammar,
}

// [spec:cg3:req:swift.grammar-load]
/// What vislcg3's verbose summary reports about a loaded grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GrammarInfo {
    pub sections: u64,
    pub templates: u64,
    pub rules: u64,
    pub sets: u64,
    pub tags: u64,
    pub rules_any: u64,
    pub has_dependency_rules: bool,
    pub has_relation_rules: bool,
}

// [spec:cg3:req:swift.apply]
/// A run's output, and every warning and non-fatal error it logged, in order.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ApplyResult {
    pub output: String,
    pub warnings: Vec<String>,
}

#[uniffi::export]
impl Grammar {
    // [spec:cg3:req:swift.grammar-load]
    // [spec:cg3:req:swift.load-options]
    /// Loads the `.cg3b` `cg3b` under `options`. Bytes that are not a `.cg3b`
    /// the library reads are `InvalidGrammar`; a load option it cannot use is
    /// `InvalidArgument`.
    #[uniffi::constructor]
    pub fn new(cg3b: Vec<u8>, options: LoadOptions) -> Result<Arc<Grammar>, Cg3Error> {
        let options = load_options(options)?;
        let inner = logging(|| CompiledGrammar::from_cg3b(&cg3b, &options))
            .map_err(Cg3Error::invalid_grammar)?;
        Ok(Arc::new(Grammar { inner }))
    }

    // [spec:cg3:req:swift.grammar-load]
    pub fn info(&self) -> GrammarInfo {
        let info = self.inner.info();
        let count = |n: usize| n as u64;
        GrammarInfo {
            sections: count(info.sections),
            templates: count(info.templates),
            rules: count(info.rules),
            sets: count(info.sets),
            tags: count(info.tags),
            rules_any: count(info.rules_any),
            has_dependency_rules: info.has_dependency_rules,
            has_relation_rules: info.has_relation_rules,
        }
    }

    // [spec:cg3:req:swift.apply]
    // [spec:cg3:req:swift.errors]
    // [spec:cg3:req:swift.external]
    /// Runs the grammar on `input` through `CompiledGrammar::run`, the
    /// operation vislcg3 runs every grammar through, so the two write the same
    /// bytes. A failed run is `RunFailed`, and the grammar stays usable. An
    /// `EXTERNAL` rule starts its process through `std::process`, which fails
    /// where the platform lets no app start one, failing the run.
    pub fn apply(&self, input: String, options: ApplyOptions) -> Result<ApplyResult, Cg3Error> {
        let (ran, warnings) = collecting(|| self.inner.run(input.as_bytes(), &options.into()));
        Ok(ApplyResult {
            output: stream(ran)?,
            warnings,
        })
    }

    // [spec:cg3:req:swift.tools]
    /// What cg-proc writes for `input` with this grammar, through
    /// `CompiledGrammar::proc`, which cg-proc runs through.
    pub fn proc(&self, input: String, options: ProcRunOptions) -> Result<String, Cg3Error> {
        stream(logging(|| {
            self.inner.proc(input.as_bytes(), &options.into())
        }))
    }
}
