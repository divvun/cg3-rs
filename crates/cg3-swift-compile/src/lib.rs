//! The UniFFI glue behind CG3Compile, the product of the Swift package
//! divvun.cg3 that compiles textual grammars and writes them, as
//! `@divvun/cg3/compile` does. It logs through crates/cg3-swift's sink.

pub mod compile;
pub mod failure;
pub mod files;

uniffi::setup_scaffolding!();
