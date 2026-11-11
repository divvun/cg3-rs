//! What the two WebAssembly modules behind `@divvun/cg3` share: the errors
//! they hand JavaScript, the tracing they forward and collect, the sink a
//! panic and an event reach the wrapper through, and the load options.
//!
//! Each module links its own copy, so each has its own sink, logger and level,
//! and a trap in one leaves the other alone. Its exports, `install` and
//! `setLogLevel`, are exports of both modules. `crates/cg3-wasm` cannot be
//! linked by `crates/cg3-wasm-compile` instead: its exports, and the reader
//! and runner behind them, would come with it.

pub mod error;
pub mod load_options;
pub mod log;
pub mod sink;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;

pub use error::{ErrorCode, GlueError, set};
pub use load_options::{parse_load_options, prefix};
