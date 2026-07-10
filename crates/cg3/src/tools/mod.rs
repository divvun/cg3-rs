//! CLI tool entry points — the final piece of the Wave-2 port.
//!
//! Each C++ tool `main()` (under `../../../../src/*.cpp`) becomes a `pub fn`
//! taking the process `argv` as `&[String]` (element `0` is the program name,
//! matching C `argv[0]`) and returning the process exit code as `i32`. The
//! translations are LITERAL, bug-for-bug; every function/type carries its
//! verbatim `[spec:cg3:def:<id>]` + `[spec:cg3:sem:<id>]` annotation.
//!
//! ## Shared conventions
//! * **Options.** Arg parsing routes through [`crate::options`] /
//!   [`crate::options_conv`] tables and [`crate::options_parser::parse_opts`] /
//!   [`crate::icu_uoptions::u_parseArgs`], exactly as the C++ does. The mutable
//!   global `std::array` option tables become owned local copies (the port
//!   exposes them as constructor functions — see the module NOTE in
//!   [`crate::options`]), so each tool owns its `options` / `options_conv` /
//!   `*_default` / `*_override` arrays and mutates them in place.
//! * **`u_parseArgs` argv.** The ICU parser consumes `&mut [Vec<char>]`; each
//!   tool converts its incoming `&[String]` argv into that shape and reads the
//!   returned "remaining" count (negative on error), exactly as C++'s
//!   `argc = u_parseArgs(...)`.
//! * **ICU init / codepage / locale.** The C++ `u_init` / `ucnv_setDefaultName`
//!   / `uloc_setDefault` calls have no analogue in this UTF-8 port; they are
//!   dropped (noted at each site). `UErrorCode status` starts at
//!   `U_ZERO_ERROR == 0` and is returned as the exit code where the C++ returns
//!   `status` (raw ICU `UErrorCode`, per the flagged-bug convention).
//! * **Grammar ownership.** The ported parsers OWN their `Grammar` (see
//!   [`crate::textual_parser`] / [`crate::binary_grammar`]); the C++ passes an
//!   externally-held `Grammar&` and later `parser.reset()`s. The port therefore
//!   moves the built grammar OUT of the parser (`parser.grammar`) after parsing,
//!   which is the faithful analogue of "the grammar outlives the parser".
//! * **Run flow.** Every tool's run flow is LIVE: the base
//!   `GrammarApplicator::run_grammar_on_text`, the `ApertiumApplicator` /
//!   `MatxinApplicator` / `BinaryApplicator` / `MweSplitApplicator` drivers, and
//!   the `FormatConverter` dispatch are all ported and wired. The ported
//!   drivers take `R: Read + Seek`; stdin is not seekable, so each tool buffers
//!   its input stream into a `std::io::Cursor<Vec<u8>>` before running —
//!   faithful for the char-by-char/line-by-line state machines the drivers run.
//!   `FormatConverter` base members (`fmt_input`/`fmt_output`/flags,
//!   `set_grammar`/`set_options`) are reached through its public `base()` /
//!   `base_mut()` accessors — the composition analogue of the C++ public
//!   inheritance.

pub mod vislcg3;
pub mod cg_comp;
pub mod cg_conv;
pub mod cg_proc;
pub mod cg_relabel;
pub mod cg_mwesplit;
pub mod cg_annotate;
pub mod cg_merge_annotations;

// --- Diagnostics ----------------------------------------------------------------

/// Install the process-wide tracing subscriber for the CLI binaries: every
/// diagnostic the engine emits (the C++ `ux_stderr`/`std::cerr` messages, now
/// `tracing::{error,warn,info}!` events) is written to stderr, message-first
/// and timestamp-free so the output stays close to the classic CG-3 stderr
/// text. Idempotent: a second call (e.g. from tests driving two tool mains in
/// one process) is a no-op.
pub fn init_diagnostics() {
    use tracing_subscriber::util::SubscriberInitExt as _;
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish()
        .try_init();
}

// --- Shared version constants (C++ `version.hpp`) ------------------------------

/// ICU `UErrorCode` values used as tool exit codes (per the flagged-bug
/// convention "exit codes = raw ICU UErrorCode"). `U_ZERO_ERROR == 0`;
/// `U_ILLEGAL_ARGUMENT_ERROR == 1` (from ICU's `utypes.h`).
pub const U_ZERO_ERROR: i32 = 0;
pub const U_ILLEGAL_ARGUMENT_ERROR: i32 = 1;

/// C++ `version.hpp` `constexpr` version numbers, transcribed verbatim.
pub const CG3_VERSION_MAJOR: u32 = 1;
pub const CG3_VERSION_MINOR: u32 = 6;
pub const CG3_VERSION_PATCH: u32 = 7;
pub const CG3_REVISION: u32 = 13898;
pub const CG3_TOO_OLD: u32 = 10373;
pub const CG3_COPYRIGHT_STRING: &str =
    "Copyright (C) 2007-2025 GrammarSoft ApS. Licensed under GPLv3+";

// --- Shared argv helper --------------------------------------------------------

/// Build the `u_parseArgs`-shaped argv (`Vec<Vec<char>>`, NUL-free tokens) from a
/// process `&[String]` argv. Element `0` (the program name) is preserved so the
/// ICU parser's `i = 1` start and its non-option compaction behave exactly as in
/// C++.
pub(crate) fn to_uargv(args: &[String]) -> Vec<Vec<char>> {
    args.iter().map(|s| s.chars().collect()).collect()
}

/// C++ `basename(argv[0])` — the trailing path component, used in the various
/// `endProgram` usage banners. A faithful stand-in for POSIX `basename(3)`
/// (splits on `/`; returns the whole string when there is no separator).
pub(crate) fn basename(name: &str) -> &str {
    match name.rfind('/') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}
