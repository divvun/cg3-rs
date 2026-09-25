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
//!   [`crate::arg_parser::parse_args`], exactly as the C++ does. The mutable
//!   global `std::array` option tables become owned local copies (the port
//!   exposes them as constructor functions — see the module NOTE in
//!   [`crate::options`]), so each tool owns its `options` / `options_conv` /
//!   `*_default` / `*_override` arrays and mutates them in place.
//! * **`parse_args` argv.** The argument parser consumes `&mut [Vec<char>]`;
//!   each tool converts its incoming `&[String]` argv into that shape and reads
//!   the returned "remaining" count (negative on error), exactly as the C++
//!   reassigns `argc` from it.
//! * **Library init / codepage / locale.** The C++ tools initialise their
//!   Unicode library and default codepage and locale; a UTF-8 port has none of
//!   that, so those calls are dropped. Where the C++ returns its status as the
//!   exit code, the port returns `EXIT_SUCCESS` or `EXIT_FAILURE`.
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

use std::io::Write;

#[cfg(feature = "profiler")]
pub mod cg_annotate;
pub mod cg_comp;
pub mod cg_conv;
#[cfg(feature = "profiler")]
pub mod cg_merge_annotations;
pub mod cg_mwesplit;
pub mod cg_proc;
pub mod cg_relabel;
pub mod vislcg3;

// --- Diagnostics ----------------------------------------------------------------

/// Handle to the reloadable level filter installed by [`init_diagnostics`], so
/// [`enable_debug_logging`] (`--debug`) can raise the verbosity to DEBUG after
/// the subscriber is already running — the CLI mains install diagnostics before
/// they parse their options, so the level can't be known up front.
static LEVEL_HANDLE: std::sync::OnceLock<
    tracing_subscriber::reload::Handle<
        tracing_subscriber::filter::LevelFilter,
        tracing_subscriber::Registry,
    >,
> = std::sync::OnceLock::new();

/// Install the process-wide tracing subscriber for the CLI binaries: every
/// diagnostic the engine emits (the C++ stderr messages, now
/// `tracing::{error,warn,info,debug}!` events) is written to stderr, message-
/// first and timestamp-free so the output stays close to the classic CG-3 stderr
/// text. The level starts at INFO and is reloadable (see [`enable_debug_logging`]).
/// Idempotent: a second call (e.g. from tests driving two tool mains in one
/// process) is a no-op.
pub fn init_diagnostics() {
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let (filter, handle) = tracing_subscriber::reload::Layer::new(LevelFilter::INFO);
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .with_ansi(false);
    if tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init()
        .is_ok()
    {
        let _ = LEVEL_HANDLE.set(handle);
    }
}

/// Raise the diagnostic level to DEBUG when `enabled`. Wired from `--debug` (the
/// C++ `debug_level` flag, whose numeric level the port collapses to "DEBUG
/// on"): diagnostics are the only thing `--debug` controlled, so it lives here
/// rather than as engine state. Takes the flag by value so the caller stays
/// branch-free. Idempotent, and a no-op if diagnostics were never installed
/// (e.g. in-process tests).
pub fn enable_debug_logging(enabled: bool) {
    use tracing_subscriber::filter::LevelFilter;
    if enabled && let Some(handle) = LEVEL_HANDLE.get() {
        let _ = handle.modify(|filter| *filter = LevelFilter::DEBUG);
    }
}

// --- CLI failure mapping ---------------------------------------------------------

/// A tool that did its job.
pub(crate) const EXIT_SUCCESS: i32 = 0;

/// The C `EXIT_FAILURE` every `endProgram` / `CG3Quit(1)` in the C++ tools
/// terminates with, and the code a bad command line exits with.
pub(crate) const EXIT_FAILURE: i32 = 1;

// [spec:cg3:req:errors.exit-codes-at-cli]
/// Report a library failure on the way out of a CLI main, and derive the process
/// exit code it maps to.
///
/// The mapping lives here rather than on the error because the code a failure
/// maps to is a property of THIS command-line contract, not of the failure: an
/// embedder handling the same value cares about the variant and wants nothing to
/// do with an exit status.
pub(crate) fn fail(e: &crate::error::Cg3Error) -> i32 {
    crate::error::report_cli(e);
    EXIT_FAILURE
}

// --- Profile databases -----------------------------------------------------------

/// A profile database the profile tools could not read.
#[cfg(feature = "profiler")]
#[derive(Debug, thiserror::Error)]
#[error("Error: cannot read profile database {path}: {source}")]
pub(crate) struct ProfileReadError {
    path: String,
    #[source]
    source: rusqlite::Error,
}

// [spec:cg3:req:robustness.cli-arguments]
/// C++ `Profiler p; p.read(path)` for the profile tools, whose read failure
/// throws. The tools refuse the database rather than go on with an empty
/// profile.
#[cfg(feature = "profiler")]
pub(crate) fn read_profile(path: &str) -> Result<crate::profiler::Profiler, ProfileReadError> {
    let mut profiler = crate::profiler::Profiler::default();
    match profiler.read(path) {
        Ok(()) => Ok(profiler),
        Err(source) => Err(ProfileReadError {
            path: path.to_string(),
            source,
        }),
    }
}

// --- Option-table merging --------------------------------------------------------

/// Merge one pair of option tables onto `options`: `defaults` fill only what is
/// still unset, `overrides` win outright.
///
/// Each tool runs this twice — once for the `CG3_DEFAULT` / `CG3_OVERRIDE`
/// environment tables, once for the grammar's own `cmdargs`. The second pass
/// additionally declines to overwrite anything the environment already forced,
/// which is what `already_forced` names.
pub(crate) fn merge_options(
    options: &mut crate::options::OptionsTable,
    defaults: &crate::options::OptionsTable,
    overrides: &crate::options::OptionsTable,
    already_forced: Option<&crate::options::OptionsTable>,
) {
    for i in 0..crate::options::Opt::NumOptions as usize {
        if defaults[i].does_occur && !options[i].does_occur {
            options[i] = defaults[i].clone();
        }
        let forced = already_forced.is_some_and(|t| t[i].does_occur);
        if overrides[i].does_occur && !forced {
            options[i] = overrides[i].clone();
        }
    }
}

// --- Divvun package identity ---------------------------------------------------

/// Release metadata supplied by Cargo from this package's manifest.
pub const DIVVUN_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DIVVUN_REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
pub const DIVVUN_BUILD_DATE: &str = env!("CG3_BUILD_DATE");
pub const DIVVUN_GIT_HASH: &str = env!("CG3_GIT_HASH");
pub const DIVVUN_COPYRIGHT_STRING: &str = "Copyright (C) 2026 UiT The Arctic University of Norway";

/// The first line of every tool's banner.
pub(crate) fn divvun_version_line(product: &str) -> String {
    format!("Divvun CG-3 {product} v{DIVVUN_VERSION} ({DIVVUN_BUILD_DATE} {DIVVUN_GIT_HASH})\n")
}

/// Everything the `--version` banner says below its first line.
pub(crate) fn divvun_copyright() -> String {
    format!("{DIVVUN_COPYRIGHT_STRING}\n{CG3_COPYRIGHT_STRING}\nSource: {DIVVUN_REPOSITORY}\n")
}

/// The complete `--version` banner.
pub(crate) fn divvun_version(product: &str) -> String {
    divvun_version_line(product) + &divvun_copyright()
}

// [spec:cg3:req:tools.divvun-version-banner+2]
/// Handle a binary's package-identity flags before its ported argument parser.
/// Returns `true` after printing the complete banner so the wrapper can exit
/// successfully without running the tool or initializing diagnostics.
pub fn handle_divvun_version(args: &[String], product: &str, short_aliases: &[&str]) -> bool {
    let requested =
        args.len() == 2 && (args[1] == "--version" || short_aliases.contains(&args[1].as_str()));
    if requested {
        emit(std::io::stdout(), &divvun_version(product));
    }
    requested
}

// --- Standard streams ------------------------------------------------------------

// [spec:cg3:req:robustness.cli-output]
/// Write `text` to `stream` and flush it; a stream that has closed ends the
/// output there, quietly.
///
/// The `print!` family panics once the reader has gone — `vislcg3 --help | head
/// -1` — where the C++ tools are killed by `SIGPIPE` without a word. This stops
/// the same way and leaves the tool to return the exit code it already had.
/// Nothing is reported: the reader that would see it is the one that left.
pub(crate) fn emit(mut stream: impl Write, text: &str) {
    if stream.write_all(text.as_bytes()).is_ok() {
        let _ = stream.flush();
    }
}

/// Emit a usage text where the C++ `out = (argc < 0) ? stderr : stdout` sends
/// it, and derive the exit code: a refused command line fails, `--help` does
/// not.
pub(crate) fn emit_usage(text: &str, refused: bool) -> i32 {
    if refused {
        emit(std::io::stderr(), text);
        return EXIT_FAILURE;
    }
    emit(std::io::stdout(), text);
    EXIT_SUCCESS
}

// --- Process entry ---------------------------------------------------------------

/// What a tool binary hands [`run_tool`]: who it is, and the ported `main` to
/// run.
pub struct Tool {
    /// The product name its `--version` banner carries.
    pub product: &'static str,
    /// Short flags that also ask for the banner (`vislcg3 -V`, `cg-proc -v`).
    pub version_aliases: &'static [&'static str],
    /// The environment variables it reads options from (`CG3_DEFAULT`, ...).
    pub option_env: &'static [&'static str],
    /// The ported C++ `main`, taking `argv` and returning the exit code.
    pub main: fn(&[String]) -> i32,
}

/// A command line the tools cannot take as text.
#[derive(Debug, thiserror::Error)]
enum CommandLineError {
    #[error("{program}: error in command line argument \"{lossy}\": not valid UTF-8")]
    Argument { program: String, lossy: String },
    #[error("{program}: error in environment variable {name}: not valid UTF-8")]
    Environment { program: String, name: &'static str },
}

// [spec:cg3:req:robustness.cli-arguments]
/// Run a tool binary: take its command line, answer `--version`, install
/// diagnostics and hand over to its ported `main`. Returns the exit code.
///
/// DIVERGENCE: the C++ takes `argv` and its option variables as bytes and
/// passes them on unread. Every option value here is a `String`, so an
/// argument or option variable that is not UTF-8 is refused by name before the
/// tool starts, where an argument used to panic and a variable was passed over
/// as though unset.
pub fn run_tool(tool: &Tool) -> i32 {
    let args = match utf8_args(std::env::args_os()) {
        Ok(args) => args,
        Err(e) => return refuse(&e),
    };
    if handle_divvun_version(&args, tool.product, tool.version_aliases) {
        return EXIT_SUCCESS;
    }
    init_diagnostics();
    match check_option_env(&args, tool.option_env) {
        Ok(()) => (tool.main)(&args),
        Err(e) => refuse(&e),
    }
}

/// The process `argv` as UTF-8, or the first argument that is not.
///
/// `argv[0]` is converted lossily rather than checked: it is the path the tool
/// was started by, not something the user typed as an argument, and it is
/// only ever shown as a name.
fn utf8_args(
    argv: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Vec<String>, CommandLineError> {
    let mut argv = argv.into_iter();
    let program = argv.next().map(|p| p.to_string_lossy().into_owned());
    let mut args: Vec<String> = program.iter().cloned().collect();
    for arg in argv {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(arg) => {
                return Err(CommandLineError::Argument {
                    program: program.unwrap_or_default(),
                    lossy: arg.to_string_lossy().into_owned(),
                });
            }
        }
    }
    Ok(args)
}

/// Refuse an option variable in `names` that is set but not UTF-8, which
/// `parse_opts_env` would otherwise pass over as though it were unset.
fn check_option_env(args: &[String], names: &[&'static str]) -> Result<(), CommandLineError> {
    let not_utf8 =
        |name: &&str| matches!(std::env::var(name), Err(std::env::VarError::NotUnicode(_)));
    match names.iter().copied().find(not_utf8) {
        Some(name) => Err(CommandLineError::Environment {
            program: args.first().cloned().unwrap_or_default(),
            name,
        }),
        None => Ok(()),
    }
}

/// Report a command line the tool cannot take, and derive the exit code.
fn refuse(e: &CommandLineError) -> i32 {
    init_diagnostics();
    tracing::error!("{e}");
    EXIT_FAILURE
}

// --- Shared upstream version constants (C++ `version.hpp`) --------------------

pub const CG3_TOO_OLD: u32 = 10373;
pub const CG3_COPYRIGHT_STRING: &str =
    "Copyright (C) 2007-2025 GrammarSoft ApS. Licensed under GPLv3+";

// --- Shared argv helper --------------------------------------------------------

/// Build the `parse_args`-shaped argv (`Vec<Vec<char>>`, NUL-free tokens) from a
/// process `&[String]` argv. Element `0` (the program name) is preserved so the
/// argument parser's `i = 1` start and its non-option compaction behave exactly as in
/// C++.
pub(crate) fn to_argv(args: &[String]) -> Vec<Vec<char>> {
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
