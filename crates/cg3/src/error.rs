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
//! The C++ `CG3Quit` macro terminated the process from deep inside library code,
//! and the port reproduced that with a `panic_any` unwind plus a matching catch
//! at every boundary; the parser reproduced `throw int` the same way. None of
//! that survives: failure travels by value, and a panic from this crate means a
//! bug in this crate. See `[dec:cg3:results-not-unwinding]`.

use crate::process::ProcessError;
use crate::tag_regex::TagRegexError;

// [spec:cg3:req:diagnostics.source-retained]
/// One grammar source a parse read, retained so a failure in it can be quoted.
///
/// A parse spans several buffers the moment a grammar uses `#include`, and the
/// text is the parser's own working buffer, which nothing outside the parse
/// holds: without this the only way to show a user the line that failed would
/// be to re-read the file and hope it had not changed.
#[derive(Debug, Clone)]
pub struct ParseSource {
    /// The path this text was read from, as the parse was told it.
    pub name: String,
    /// The grammar text, free of the parser's leading and trailing NUL padding,
    /// so offsets into it are offsets into what the author wrote.
    pub text: String,
}

// [spec:cg3:req:diagnostics.span]
/// Where in a parsed grammar source a failure sits.
///
/// The source index rather than a file name, because `#include` means one parse
/// covers several files and two of them may share a base name —
/// `[spec:cg3:req:diagnostics.source-identity]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSpan {
    /// Index into the parse's [`ParseSource`] list.
    pub source: usize,
    /// Char offsets into that source's [`text`](ParseSource::text).
    pub range: std::ops::Range<usize>,
}

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
    /// Where the failure sits in the retained sources, when it has a place at
    /// all. `None` for the failures with nothing to point at: an empty input, a
    /// template reference resolved after the buffer walk has finished, and a tag
    /// the running stream asked for (which came from the input, not the
    /// grammar).
    pub span: Option<ParseSpan>,
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
    /// An `#include` whose file could not be read.
    #[error("cannot read included grammar `{path}` ({source}) - bailing out")]
    IncludeUnreadable {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// Nothing to parse.
    #[error("input is empty - cannot continue")]
    EmptyInput,
    // [spec:cg3:req:diagnostics.runtime-placed]
    /// A failure the running stream hit, attributed to the rule that caused it.
    ///
    /// What went wrong and what the failure is POINTING AT are two different
    /// axes, and they only coincide for a parse error: a tag that will not
    /// compile is marked on the tag while the grammar is being read, and on the
    /// whole RULE that asked for it while the grammar is being run — the tag
    /// itself was built from the stream and is nowhere in the source. This wraps
    /// the cause rather than replacing it, so the message is unchanged and a
    /// consumer can still match on what actually failed.
    ///
    /// The tag text lives here because this is the headline a rendered report
    /// shows, and `parseTag failed` without the tag it failed on is the shape of
    /// diagnostic this work exists to remove.
    ///
    /// `Box<str>` rather than `String`: this variant is built once on a failure
    /// path and never appended to, and a growable one would make
    /// [`ParseErrorKind`] the largest thing every `Result` in the parser carries.
    #[error("cannot construct tag `{text}` ({cause})")]
    RuntimeTag {
        text: Box<str>,
        cause: Box<ParseErrorKind>,
    },
}

impl ParseErrorKind {
    /// Whether this failure ends the parse rather than just its directive.
    ///
    /// Recovery is the default — `[spec:cg3:req:errors.parse-reports-all]` has
    /// the directive loop skip to the next line and carry on, so a bad grammar
    /// reports every error it has. These two cannot be recovered from, because
    /// neither leaves a next line to resume into: the text that failed to load
    /// IS the rest of the grammar, and an empty input has no rest at all. The
    /// C++ terminated the process at both.
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            ParseErrorKind::IncludeUnreadable { .. } | ParseErrorKind::EmptyInput
        )
    }
}

/// Render each item on its own indented line, so a collection of failures
/// stays readable instead of collapsing to a count.
fn indented(items: &[impl std::fmt::Display]) -> String {
    items.iter().map(|e| format!("\n  {e}")).collect()
}

/// A grammar that would not load, or would not be written back out.
#[derive(Debug, thiserror::Error)]
pub enum GrammarError {
    #[error("cannot read grammar `{path}` ({source}) - bailing out")]
    Unreadable {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not read the first 4 bytes of the grammar")]
    TruncatedHeader,

    #[error("grammar does not begin with the CG3B magic bytes - cannot load as binary")]
    NotBinary,

    /// A contextual test reached the binary writer with no hash. The C++ wrote
    /// the diagnostic and quit from inside the serialiser.
    #[error("contextual test on line {line} has no hash - the grammar cannot be written")]
    ContextHashZero { line: u32 },

    // [spec:cg3:req:diagnostics.errors-carried]
    /// Recoverable parse errors, all of those found in one pass, together with
    /// the sources their spans index.
    ///
    /// The sources travel with the errors because a span is worthless without
    /// the text it points into, and the parser's buffers do not outlive the
    /// parse — `[spec:cg3:req:diagnostics.source-retained]`.
    #[error("grammar could not be parsed: {} error(s)", .errors.len())]
    Parse {
        errors: Vec<ParseError>,
        sources: Vec<ParseSource>,
    },

    #[error("{} tag regex(es) failed to compile{}", .0.len(), indented(.0))]
    TagRegex(Vec<TagRegexError>),

    #[error("grammar revision {found} is not supported; this loader reads {min}..={max}")]
    Revision { found: u32, min: u32, max: u32 },

    #[error("legacy .cg3b revision {found} is not supported (readBinaryGrammar_10043 not ported)")]
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
    #[error("EXTERNAL on line {line} could not be started: {source}")]
    ExternalStart {
        line: u32,
        #[source]
        source: ProcessError,
    },
    #[error("EXTERNAL on line {line} could not be written to: {source}")]
    ExternalWrite {
        line: u32,
        #[source]
        source: ProcessError,
    },
    #[error("EXTERNAL returned data for cohort {got}, expected {expected}")]
    ExternalCohortMismatch { expected: u32, got: u32 },
    #[error("EXTERNAL returned data for window {got}, expected {expected}")]
    ExternalWindowMismatch { expected: u32, got: u32 },
    /// A tag the running stream asked for could not be constructed.
    ///
    /// The text is carried because the offending input IS the whole tag, and the
    /// inner `ParseError` says which rule or input line asked for it. See
    /// `[dec:cg3:parse-tag-aborts-on-invalid]`.
    // [spec:cg3:req:diagnostics.runtime-placed]
    /// `sources` is the grammar the rule was written in, resolved on this
    /// failure path and only here — empty when it could not be
    /// (`[spec:cg3:req:diagnostics.source-lazy]`). It travels with the error for
    /// the same reason [`GrammarError::Parse`]'s does: the span is worthless
    /// without the text it points into.
    ///
    /// The message is the inner error's alone: that already names the tag (via
    /// [`ParseErrorKind::RuntimeTag`]) as well as the rule and file it was asked
    /// for in, and saying the tag twice on one line is worse than saying it once.
    #[error("{source}")]
    TagConstruction {
        text: String,
        #[source]
        source: Box<ParseError>,
        sources: Vec<ParseSource>,
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
    #[error("input format {format} cannot be read here")]
    UnsupportedInputFormat { format: String },
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
}

impl Cg3Error {
    /// The tag-regex diagnostics, if this error carries any.
    pub fn tag_regex_errors(&self) -> &[TagRegexError] {
        match self {
            Cg3Error::Grammar(g) => g.tag_regex_errors(),
            _ => &[],
        }
    }
}

/// Emit the CLI-facing diagnostic for an error that is about to end the
/// process.
///
/// Every error carries its own diagnostic now, so surfacing it here is the only
/// thing that gets an embedder-facing message to a CLI user.
///
/// A failed grammar parse is rendered rather than logged: it has the sources to
/// quote and the spans to mark, and a grammar author reading a syntax error
/// wants the line they wrote. The one-line summary still follows, because a
/// count is the one thing the reports do not say.
///
/// A failure while RUNNING goes through the same renderer when it was placed in
/// the grammar that caused it — a rule that asked for a tag no parser will
/// accept is a grammar bug, and its author wants the rule quoted just as much
/// (`[spec:cg3:req:diagnostics.runtime-placed]`). A failure that could not be
/// placed carries no sources and falls through to the summary alone, which is
/// what it always was.
// [spec:cg3:req:errors.tag-regex-diagnostic]
// [spec:cg3:req:diagnostics.rendered]
// [spec:cg3:req:diagnostics.runtime-placed]
pub fn report_cli(e: &Cg3Error) {
    match e {
        Cg3Error::Grammar(GrammarError::Parse { errors, sources }) => {
            crate::diagnostics::report_parse_failure(errors, sources);
        }
        Cg3Error::Run(RunError::TagConstruction {
            source, sources, ..
        }) if !sources.is_empty() => {
            crate::diagnostics::report_parse_failure(
                std::slice::from_ref(source.as_ref()),
                sources,
            );
        }
        _ => {}
    }
    tracing::error!("{e}");
}
