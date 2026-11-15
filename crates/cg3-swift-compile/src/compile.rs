//! Compiling a textual grammar, writing one out as text, and relabelling one.
//! Grammars come in and go out as `.cg3b` bytes, as they cross between the
//! two entry points of `@divvun/cg3`.

use std::collections::HashMap;
use std::sync::Arc;

use cg3::embed::{CompiledGrammar, LoadOptions as LibraryLoadOptions, MemoryResolver};
use cg3::inlines::is_cg3b;
use cg3::textual_parser::MEMORY_SOURCE_NAME;
use cg3_swift::log::{collecting, logging};

use crate::failure::CompileError;
use crate::files::{CallerFiles, IncludeFiles};

// [spec:cg3:req:swift.compile]
/// The options vislcg3 applies to a grammar while it loads it, as the CG3
/// glue's `LoadOptions` has them.
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct LoadOptions {
    pub nrules: Option<String>,
    pub nrules_v: Option<String>,
    pub vislcg_compat: bool,
    pub prefix: Option<String>,
}

impl From<LoadOptions> for cg3_swift::options::LoadOptions {
    fn from(o: LoadOptions) -> cg3_swift::options::LoadOptions {
        cg3_swift::options::LoadOptions {
            nrules: o.nrules,
            nrules_v: o.nrules_v,
            vislcg_compat: o.vislcg_compat,
            prefix: o.prefix,
        }
    }
}

// [spec:cg3:req:swift.compile]
/// A compiled grammar, as cg-comp writes it, and what its parse logged.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CompileResult {
    pub cg3b: Vec<u8>,
    /// The companion file cg-comp writes beside the `.cg3b`.
    pub companion: Option<Vec<u8>>,
    /// Every warning and error the parse logged, in order.
    pub warnings: Vec<String>,
}

// [spec:cg3:req:swift.compile]
// [spec:cg3:req:swift.compile-includes]
/// Compiles `source` with its `INCLUDE`s read from `files`, by the names it
/// gives them.
#[uniffi::export]
pub fn compile_grammar(
    source: String,
    name: Option<String>,
    files: HashMap<String, String>,
    options: LoadOptions,
) -> Result<CompileResult, CompileError> {
    compile(&source, name, CallerFiles::given(files), options)
}

// [spec:cg3:req:swift.compile]
// [spec:cg3:req:swift.compile-includes]
/// Compiles `source` with its `INCLUDE`s read from what `files` gives.
#[uniffi::export]
pub fn compile_grammar_asking(
    source: String,
    name: Option<String>,
    files: Arc<dyn IncludeFiles>,
    options: LoadOptions,
) -> Result<CompileResult, CompileError> {
    compile(&source, name, CallerFiles::new(Some(files)), options)
}

// [spec:cg3:req:swift.compile]
// [spec:cg3:req:swift.compile-parity]
/// Compiles `source`, called `name` in its diagnostics and in the companion
/// file, through `CompiledGrammar::from_text`, and writes it through the
/// writers cg-comp writes through, so the bytes are cg-comp's for the same
/// files.
fn compile(
    source: &str,
    name: Option<String>,
    files: CallerFiles,
    options: LoadOptions,
) -> Result<CompileResult, CompileError> {
    let options = cg3_swift::options::load_options(options.into())?;
    let name = name.unwrap_or_else(|| MEMORY_SOURCE_NAME.to_owned());
    let read = files.names_read();
    let (written, warnings) = collecting(|| {
        let grammar = CompiledGrammar::from_text(source.as_bytes(), &name, files, &options)?;
        Ok::<_, cg3::error::Cg3Error>((grammar.to_cg3b()?, grammar.companion()?))
    });
    let (cg3b, companion) = written.map_err(|e| CompileError::failed(e, &read.borrow()))?;
    Ok(CompileResult {
        cg3b,
        companion,
        warnings,
    })
}

fn read_cg3b(cg3b: &[u8]) -> Result<CompiledGrammar, CompileError> {
    CompiledGrammar::from_cg3b(cg3b, &LibraryLoadOptions::default())
        .map_err(CompileError::invalid_grammar)
}

// [spec:cg3:req:swift.compile]
// [spec:cg3:req:swift.compile-parity]
/// What vislcg3 `--grammar-out` writes for the grammar in `cg3b`, since
/// vislcg3 writes through `to_text`.
#[uniffi::export]
pub fn grammar_text(cg3b: Vec<u8>) -> Result<String, CompileError> {
    logging(|| Ok(read_cg3b(&cg3b)?.to_text()))
}

// [spec:cg3:req:swift.compile]
// [spec:cg3:req:swift.compile-parity]
/// The `.cg3b` of the grammar `CompiledGrammar::relabel` makes from the
/// `.cg3b` `cg3b` and the relabel rules in `relabels`, read as cg-relabel
/// reads its rule file: as a `.cg3b` when they begin with its magic, and as a
/// textual grammar otherwise. cg-relabel relabels and writes through the same
/// functions, so the bytes are its.
#[uniffi::export]
pub fn relabel(cg3b: Vec<u8>, relabels: Vec<u8>) -> Result<Vec<u8>, CompileError> {
    logging(|| {
        let grammar = read_cg3b(&cg3b)?;
        let rules = if is_cg3b(&relabels) {
            read_cg3b(&relabels)?
        } else {
            let resolver = MemoryResolver::default();
            let options = LibraryLoadOptions::default();
            CompiledGrammar::from_text(&relabels, MEMORY_SOURCE_NAME, resolver, &options)
                .map_err(|e| CompileError::failed(e, &HashMap::new()))?
        };
        let relabelled = grammar.relabel(&rules).and_then(|g| g.to_cg3b());
        relabelled.map_err(CompileError::invalid_grammar)
    })
}
