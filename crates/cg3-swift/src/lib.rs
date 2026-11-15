//! The UniFFI glue behind CG3, the product of the Swift package divvun.cg3
//! that loads compiled grammars and runs them, as `@divvun/cg3`'s main entry
//! point does.
//!
//! crates/cg3-swift-ffi links it into the one static library the package's
//! products link. Its log sink and level serve every glue crate that library
//! holds.

pub mod error;
pub mod grammar;
pub mod log;
pub mod options;
pub mod tools;

uniffi::setup_scaffolding!();

/// The Swift package's version, which this crate carries, apart from the
/// library's.
#[uniffi::export]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
