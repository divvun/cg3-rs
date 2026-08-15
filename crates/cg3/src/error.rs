//! Library error handling.
//!
//! Errors are layered by boundary rather than collected into one crate-wide
//! enum: [`ParseError`] is a single recoverable grammar parse error,
//! [`GrammarError`] is a grammar that would not load, [`RunError`] is a stream
//! that would not run, and [`Cg3Error`] is the outermost composition the
//! binaries see. Each layer names only what it can produce, so a consumer
//! matching on a load failure never has to consider a stream variant. See
//! `[dec:cg3:layered-error-types]`.
//!
//! TRANSITIONAL. The C++ `CG3Quit` macro terminated the process from deep
//! inside library code, and the port reproduced that with a `panic_any` unwind
//! ([`Cg3Exit`]) plus a matching catch at every boundary; the parser reproduced
//! `throw int` the same way ([`crate::textual_parser::ParseError`]). That
//! apparatus — [`cg3_exit`], [`catch_fatal`], [`install_panic_filter`] and
//! [`Cg3Error::Fatal`] — is being removed by `errors-idiomatic`, and shrinks as
//! each phase converts its sites. Nothing new should reach for it. See
//! `[dec:cg3:results-not-unwinding]`.

use std::panic::{self, AssertUnwindSafe};

use crate::tag_regex::TagRegexError;

/// TRANSITIONAL. The payload of an unconverted `CG3Quit(code)` / `exit(code)`,
/// raised with `panic_any` and captured by [`catch_fatal`] or [`run_cli`].
/// Deleted by `errors-idiomatic.teardown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cg3Exit(pub i32);

impl std::fmt::Display for Cg3Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cg3 fatal exit with code {}", self.0)
    }
}

impl std::error::Error for Cg3Exit {}

// [spec:cg3:req:errors.layered]
/// One recoverable grammar parse error, with the position needed to find it.
///
/// The parser recovers per directive and keeps going, so a failed parse yields
/// a collection of these rather than a single failure —
/// `[spec:cg3:req:errors.parse-reports-all]`.
#[derive(Debug, thiserror::Error)]
#[error("{file}: {kind}, on line {line} near `{near}`")]
pub struct ParseError {
    /// The grammar file's base name, or `<utf8-memory>` for an in-memory parse.
    pub file: String,
    pub line: u32,
    /// Up to 20 characters of source at the failure, with control characters
    /// rendered visibly.
    pub near: String,
    pub kind: ParseErrorKind,
}

/// What went wrong at one parse site.
#[derive(Debug, thiserror::Error)]
pub enum ParseErrorKind {
    /// The catch-all the C++ raised from ~120 distinct sites, each with its own
    /// message. The port collapsed them to one; recovering the per-site text is
    /// separate work.
    #[error("syntax error")]
    Syntax,
    /// The cause is in the message AND in `source()`: the message so a log
    /// reader sees which construct failed, `source()` so a consumer can inspect
    /// it without parsing text.
    #[error("{cause}")]
    TagRegex {
        #[source]
        cause: Box<TagRegexError>,
    },
    #[error("unknown template `{name}`")]
    UnknownTemplate { name: String },
    #[error("empty tag — forgot to fill in a ()?")]
    EmptyTag,
    #[error("tag `{tag}` cannot start with (")]
    TagStartsWithParen { tag: String },
    #[error("redefinition of template `{name}`")]
    TemplateRedefined { name: String },
    #[error("redefinition of anchor `{name}`")]
    AnchorRedefined { name: String },
    #[error("set `{name}` is already defined")]
    SetRedefined { name: String },
    #[error("content-hash collision between sets")]
    SetContentCollision,
    #[error("numeric branch resulted in an empty set")]
    EmptyNumericBranch,
}

/// Render each item on its own indented line, so a collection of failures
/// stays readable instead of collapsing to a count.
fn indented(items: &[impl std::fmt::Display]) -> String {
    items.iter().map(|e| format!("\n  {e}")).collect()
}

/// A grammar that would not load.
#[derive(Debug, thiserror::Error)]
pub enum GrammarError {
    /// Recoverable parse errors, all of those found in one pass.
    ///
    /// Carries a count rather than the errors themselves until the parser
    /// conversion (`errors-idiomatic.parser`) can hand them over — today the
    /// parser discards everything but the tally.
    #[error("grammar could not be parsed: {count} error(s)")]
    Parse { count: u32 },

    #[error("{} tag regex(es) failed to compile{}", .0.len(), indented(.0))]
    TagRegex(Vec<TagRegexError>),

    #[error("grammar revision {found} is not supported; this loader reads {min}..={max}")]
    Revision { found: u32, min: u32, max: u32 },

    #[error("legacy .cg3b revision {found} is not supported")]
    LegacyRevision { found: u32 },

    #[error("static set `{name}` on line {line} is an alias")]
    StaticSetAlias { name: String, line: u32 },

    #[error("static set `{name}` on line {line} is already defined as set {existing}")]
    StaticSetRedefined {
        name: String,
        existing: u32,
        line: u32,
    },
}

impl GrammarError {
    /// The tag-regex diagnostics, if this is a [`GrammarError::TagRegex`].
    pub fn tag_regex_errors(&self) -> &[TagRegexError] {
        match self {
            GrammarError::TagRegex(errors) => errors,
            _ => &[],
        }
    }
}

/// A stream that would not run.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// `detail` is a message rather than a typed cause because
    /// `crate::process::Process` still reports `Result<(), String>` — its own
    /// C-ism, tracked by `errors-idiomatic.process`.
    #[error("EXTERNAL on line {line} could not be started: {detail}")]
    ExternalStart { line: u32, detail: String },
    #[error("EXTERNAL on line {line} could not be written to: {detail}")]
    ExternalWrite { line: u32, detail: String },
    #[error("EXTERNAL returned data for cohort {got}, expected {expected}")]
    ExternalCohortMismatch { expected: u32, got: u32 },
    #[error("EXTERNAL returned data for window {got}, expected {expected}")]
    ExternalWindowMismatch { expected: u32, got: u32 },
    /// A tag the running stream asked for could not be constructed.
    ///
    /// The text is carried because a runtime tag has no file and no useful
    /// "near" context — the offending input IS the whole tag — and the inner
    /// `ParseError` says which rule or input line asked for it. See
    /// `[dec:cg3:parse-tag-aborts-on-invalid]`.
    #[error("tag `{text}` could not be constructed: {source}")]
    TagConstruction {
        text: String,
        #[source]
        source: Box<ParseError>,
    },
    /// C++ `addTagToReading`: a reading may carry at most one mapping tag, and
    /// a second distinct one was `CG3Quit(1)` — a fatal from the middle of the
    /// hot loop. `line` is the grammar line in flight.
    #[error("cannot add a mapping tag to a reading which already is mapped, on line {line}")]
    MappingTagConflict { line: u32 },
    #[error("input contains sub-readings, which this output format cannot represent")]
    SubReadingsUnsupported,
    #[error("output format {format} cannot be written here")]
    UnsupportedOutputFormat { format: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// [spec:cg3:req:errors.layered]
/// The outermost error a binary or an embedder sees.
///
/// Not `Clone` or `PartialEq`: the layers below carry `std::io::Error`, which is
/// neither.
#[derive(Debug, thiserror::Error)]
pub enum Cg3Error {
    #[error(transparent)]
    Grammar(#[from] GrammarError),

    #[error(transparent)]
    Run(#[from] RunError),

    /// TRANSITIONAL. A `CG3Quit(code)` / `exit(code)` fatal that has not been
    /// converted yet. `msg` is an optional note; the human-facing diagnostic was
    /// already emitted at the fatal site. Deleted by `errors-idiomatic.teardown`
    /// — every site that still constructs one is a site left to convert.
    #[error("cg3 fatal (exit {code}){}", .msg.as_deref().map(|m| format!(": {m}")).unwrap_or_default())]
    Fatal { code: i32, msg: Option<String> },
}

impl Cg3Error {
    /// Construct a transitional fatal carrying `code` and an optional note.
    pub fn fatal(code: i32, msg: Option<String>) -> Cg3Error {
        Cg3Error::Fatal { code, msg }
    }

    /// The tag-regex diagnostics, if this error carries any.
    pub fn tag_regex_errors(&self) -> &[TagRegexError] {
        match self {
            Cg3Error::Grammar(g) => g.tag_regex_errors(),
            _ => &[],
        }
    }
}

impl From<Cg3Exit> for Cg3Error {
    fn from(e: Cg3Exit) -> Cg3Error {
        Cg3Error::Fatal {
            code: e.0,
            msg: None,
        }
    }
}

/// Emit the CLI-facing diagnostic for an error that is about to end the
/// process.
///
/// [`Cg3Error::Fatal`] printed its own diagnostic at the fatal site, so it stays
/// silent here. The variants that CARRY their diagnostic must be surfaced, or an
/// embedder-facing message never reaches a CLI user.
// [spec:cg3:req:errors.tag-regex-diagnostic]
pub fn report_cli(e: &Cg3Error) {
    match e {
        Cg3Error::Fatal { .. } => {}
        Cg3Error::Grammar(_) | Cg3Error::Run(_) => tracing::error!("{e}"),
    }
}

/// Emit the `CG3Quit` diagnostic that [`crate::inlines::cg3_quit`] would print
/// before terminating: `"CG3Quit triggered from {file} line {line}."`, but ONLY
/// when `line != 0` (the C++ `__LINE__ != 0` guard). Sites that returned via
/// `cg3_quit(1, Some(file!()), line)` now call this, then `return Err(...)`, so
/// the diagnostic is preserved byte-for-byte.
pub fn emit_cg3quit_line(file: &str, line: u32) {
    if line != 0 {
        tracing::error!("CG3Quit triggered from {} line {}.", file, line);
    }
}

/// Raise a residual library-side fatal (the C++ `CG3Quit` termination) as an
/// unwind carrying the exit code. Backs [`crate::inlines::cg3_quit`]; captured
/// at the nearest public boundary by [`catch_fatal`] (→ `Err(Cg3Error)`), or at
/// a CLI entry point by [`run_cli`].
pub fn cg3_exit(code: i32) -> ! {
    panic::panic_any(Cg3Exit(code))
}

/// Keep the parser's `throw`-port control flow off stderr.
///
/// The parser's recoverable errors no longer unwind, so the only payload left
/// to suppress is [`Cg3Exit`] — the unconverted `CG3Quit` sites. This filter
/// disappears with them in `errors-idiomatic.teardown`.
///
/// Installed once, chains to whatever hook was already set, and suppresses ONLY
/// the payload this crate raises and always catches. Every other panic —
/// including a genuine bug in this crate — prints as usual.
///
/// Installing a process-global hook from library code is a real side effect, so
/// it happens at the boundaries that can actually observe these payloads
/// ([`catch_fatal`] and [`run_cli`]) rather than at load time. A host that sets
/// its own hook afterwards wins, and the noise comes back.
// [spec:cg3:req:errors.control-flow-quiet]
pub fn install_panic_filter() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let default_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !info.payload().is::<Cg3Exit>() {
                default_hook(info);
            }
        }));
    });
}

pub fn catch_fatal<T>(body: impl FnOnce() -> T) -> Result<T, Cg3Error> {
    install_panic_filter();
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(v) => Ok(v),
        Err(e) => {
            if let Some(exit) = e.downcast_ref::<Cg3Exit>() {
                return Err(Cg3Error::from(*exit));
            }
            panic::resume_unwind(e)
        }
    }
}

/// Run a CLI tool body, translating a [`Cg3Exit`] unwind into its exit code
/// (any other panic is resumed). Installs a panic hook that silences the
/// default "panicked at ..." print for `Cg3Exit` payloads — the C++ `CG3Quit`
/// exits without extra output beyond its own diagnostics.
pub fn run_cli(body: impl FnOnce() -> i32) -> i32 {
    install_panic_filter();
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(code) => code,
        Err(e) => match e.downcast::<Cg3Exit>() {
            Ok(exit) => exit.0,
            Err(other) => panic::resume_unwind(other),
        },
    }
}
