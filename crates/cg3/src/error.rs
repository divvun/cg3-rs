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
    /// A tag whose dependency or relation number is a hash-table sentinel.
    #[error("{cause}")]
    ReservedNumber {
        #[source]
        cause: ReservedNumber,
    },
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
    /// A `()` with no tag in it, where a tag list expected at least one.
    #[error("empty tag list `()`")]
    EmptyTagList,
    /// A regular-expression or case-insensitive tag whose one `/` is both its
    /// opening and its closing delimiter, as in `/r` or `/i`.
    #[error("tag `{tag}` has nothing between its delimiters")]
    TagWithoutBody { tag: String },
    /// A tag that is only fail-fast markers: `^` with nothing after it.
    #[error("`^` marks a tag as fail-fast, but no tag follows it")]
    FailFastWithoutTag,
    /// A list item with nothing in it, as in the `[A,]` context shorthand.
    #[error("empty item in a list")]
    EmptyListItem,
    /// A numeric tag whose expression the grammar cannot mean, such as a
    /// variable outside `A`-`Z`. The cause says which and where. `Box<str>`
    /// for the reason [`RuntimeTag`](Self::RuntimeTag) gives.
    #[error("numeric tag `{tag}`: {cause}")]
    NumericTag {
        tag: Box<str>,
        #[source]
        cause: Box<crate::math_parser::MathError>,
    },
    /// The `f` position flag on a template reference: `f` branches on the
    /// test's target set, and a template reference has no target of its own.
    #[error("the `f` position needs a target set, and a template reference has none")]
    NumericBranchWithoutTarget,
    /// A `(` that input ended inside.
    #[error("`(` is still open at the end of the input")]
    UnclosedParenthesis,
    /// A test with position `?` that is run without an override position to
    /// replace it.
    #[error(
        "position `?` needs an override position, and the rule on line {rule_line} runs it without one"
    )]
    PositionWithoutOverride { rule_line: u32 },
    /// A number in the grammar too large for what it counts.
    #[error("number `{text}` is out of range")]
    NumberOutOfRange { text: String },
    /// A varstring tag with a `{` that no `}` closes.
    #[error("varstring `{tag}` has a `{{` with no closing `}}`")]
    UnclosedVarstringBrace { tag: String },
    /// An `INCLUDE` of a file that is already being included. The cycle runs
    /// from the file that is included again, back to itself.
    #[error("INCLUDE cycle: {}", .cycle.join(" includes "))]
    IncludeCycle { cycle: Vec<String> },
    /// A template that must refer to itself again before any test can decide,
    /// directly or through other templates. The cycle runs from a template
    /// back to itself.
    #[error("template cycle: {}", .cycle.join(" refers to "))]
    TemplateCycle { cycle: Vec<String> },
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
    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// A rule the running stream put where the engine cannot apply it. Marked
    /// on the whole rule, as [`ParseErrorKind::RuntimeTag`] is: the grammar
    /// loaded, and only this input showed the rule has nothing it can do.
    #[error("{0}")]
    RuleInapplicable(RuleInapplicable),
}

// [spec:cg3:req:robustness.accepted-grammars-run]
/// Why a rule could not be applied to the input in hand.
#[derive(Debug, thiserror::Error)]
pub enum RuleInapplicable {
    /// The C++ reported this and quit. A tag list with no wordform at all lands
    /// here too; the C++ built a cohort without one and crashed using it.
    #[error("there must be a wordform before any other tags in {rule}")]
    WordformFirst { rule: &'static str },
    /// The C++ reported this and quit.
    #[error("there must be a baseform after the wordform in {rule}")]
    BaseformAfterWordform { rule: &'static str },
    /// The C++ reported this and quit.
    #[error("there must be a baseform before any other tags in {rule}")]
    BaseformFirst { rule: &'static str },
    /// Only the sets listed in `STATIC-SETS` keep their names at run time; the
    /// C++ read past the end of its name table for any other.
    #[error("`SET:{name}` names no set kept for run time; list it in STATIC-SETS")]
    SetNotStatic { name: Box<str> },
    /// The rule's attaching context matched the cohort's wordform-line tags,
    /// which belong to no reading the rule could select, remove or copy.
    #[error("{rule} attached to a cohort by its wordform tags, which are no reading it can act on")]
    AttachedToWordformTags { rule: &'static str },
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

/// What a [`ReservedNumber`] was read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberRole {
    /// The `x` of a `#x->y` dependency tag, or a JSONL cohort's `ds`.
    DependencySelf,
    /// The `y` of a `#x->y` dependency tag, or a JSONL cohort's `dp`.
    DependencyParent,
    /// The `n` of an `ID:n` relation tag.
    RelationId,
    /// The `n` of an `R:name:n` relation tag.
    RelationTarget,
}

impl std::fmt::Display for NumberRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NumberRole::DependencySelf => "dependency number",
            NumberRole::DependencyParent => "dependency parent",
            NumberRole::RelationId => "relation id",
            NumberRole::RelationTarget => "relation target",
        })
    }
}

// [spec:cg3:req:robustness.reserved-keys]
/// A number read from input that the flat hash containers reserve as a
/// sentinel key: `u32::MAX` marks an empty slot, `u32::MAX - 1` a deleted one.
/// Stored as a key it would make a table lose or invent entries — silently,
/// in a release build — so it is refused where it is parsed.
///
/// A parent or relation target of `u32::MAX` is not refused: that is the
/// C++'s `DEP_NO_PARENT`, "no parent", and is never used as a key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{role} {value} in `{text}` is out of range")]
pub struct ReservedNumber {
    /// The tag, or the JSON member, the number was read from.
    pub text: Box<str>,
    pub role: NumberRole,
    pub value: u32,
}

impl ReservedNumber {
    /// `Err` when `value`, read as `role` from `text`, is a sentinel key.
    pub fn check(text: &str, role: NumberRole, value: u32) -> Result<(), ReservedNumber> {
        use crate::flat_unordered_map::Sentinel;
        let reserved = match role {
            NumberRole::DependencySelf | NumberRole::RelationId => {
                value == u32::EMPTY || value == u32::DEL
            }
            NumberRole::DependencyParent | NumberRole::RelationTarget => value == u32::DEL,
        };
        if reserved {
            return Err(ReservedNumber {
                text: text.into(),
                role,
                value,
            });
        }
        Ok(())
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

    /// The byte stream a binary grammar was to be read from failed.
    #[error("cannot read the binary grammar ({source})")]
    BinaryUnreadable {
        #[source]
        source: std::io::Error,
    },

    // [spec:cg3:req:robustness.binary-grammar-validated]
    /// A `.cg3b` that ends inside the field `what`, which starts at `offset`.
    #[error(
        "binary grammar ends early: {what} at byte {offset} needs {needed} byte(s), but {remaining} remain"
    )]
    Truncated {
        what: &'static str,
        offset: usize,
        needed: u64,
        remaining: usize,
    },

    // [spec:cg3:req:robustness.allocation-bounded]
    /// A `.cg3b` count announcing more records than the bytes after it could
    /// hold even at the smallest record size: `needed` is what `count` of
    /// those would take. Refused before anything is sized by the count.
    #[error(
        "binary grammar ends early: {what} {count} at byte {offset} needs at least {needed} byte(s), but {remaining} remain"
    )]
    CountPastEnd {
        what: &'static str,
        count: u32,
        offset: usize,
        needed: u64,
        remaining: usize,
    },

    // [spec:cg3:req:robustness.binary-grammar-validated]
    /// A `.cg3b` whose bytes are all present but do not describe a grammar.
    /// `offset` is where the offending field, or the record holding it, starts.
    #[error("binary grammar is malformed at byte {offset}: {fault}")]
    BinaryMalformed { offset: usize, fault: BinaryFault },

    /// A grammar was due to be written while a running pipeline still shared
    /// it. Both writers EDIT what they serialise, so they need the grammar to
    /// themselves, and there is nothing to do but refuse.
    #[error("the grammar core is shared with a running pipeline and cannot be written")]
    CoreShared,

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

/// What is wrong with a `.cg3b` whose bytes are all present: a number that
/// indexes nothing, a hash that keys nothing, or a structure no grammar has.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BinaryFault {
    /// A number past the end of the table it indexes, or outside its range.
    #[error("{what} {value} is out of range (must be below {limit})")]
    OutOfRange {
        what: &'static str,
        value: u64,
        limit: u64,
    },
    #[error("section {section} is out of range (must lie in -3..={max})")]
    Section { section: i32, max: i32 },
    #[error("{what} {value} is defined more than once")]
    Duplicate { what: &'static str, value: u32 },
    #[error("{what} {hash:#010x} names no tag")]
    UnknownTag { what: &'static str, hash: u32 },
    #[error("{what} {hash:#010x} names no contextual test")]
    UnknownContext { what: &'static str, hash: u32 },
    /// A hash equal to one of the flat hash containers' sentinel keys.
    #[error("{what} {hash:#010x} is a value the hash tables reserve")]
    ReservedHash { what: &'static str, hash: u32 },
    #[error("a contextual test has no hash")]
    ContextWithoutHash,
    #[error("set {set} uses operator {op}, which is not one of OR, +, - or ^")]
    SetOperator { set: u32, op: u32 },
    #[error("set {set} combines {sets} sets with {ops} operator(s)")]
    SetOperatorCount { set: u32, sets: usize, ops: usize },
    #[error("set {set} unifies over its first member set but has none")]
    EmptyUnifiedSet { set: u32 },
    /// The value a tag carries for one role — a variable's value, a context
    /// reference's position — does not fit the roles its type gives it.
    #[error("tag {tag} (type {type_bits:#x}) {problem}")]
    TagRole {
        tag: u32,
        type_bits: u32,
        problem: &'static str,
    },
    /// A tag whose stored hash is not the one its text, type and seed give it.
    #[error(
        "tag {tag} stores {what} {stored:#010x}, but its text, type and seed give {computed:#010x}"
    )]
    TagHash {
        tag: u32,
        what: &'static str,
        stored: u32,
        computed: u32,
    },
    /// So many tags on consecutive hashes that interning another tag whose hash
    /// falls among them would run out of seeds.
    #[error(
        "{len} tags hold consecutive hashes from {first:#010x}, as many as the tag interner probes"
    )]
    HashRun { first: u32, len: u32 },
    #[error("set {set} contains itself, directly or through its member sets")]
    SetCycle { set: u32 },
    #[error(
        "contextual test {hash:#010x} on line {line} reaches itself through its OR and LINK tests"
    )]
    ContextCycle { hash: u32, line: u32 },
    #[error("rule {rule} is among its own WITH sub-rules")]
    RuleCycle { rule: u32 },
    #[error("rule {rule} substitutes or executes but has no tag list for it")]
    MissingSublist { rule: u32 },
    /// A `?` position a run would reach with no template override to stand
    /// in for it.
    #[error(
        "contextual test {hash:#010x} on line {line} has position '?' where no template override supplies one"
    )]
    UnknownPosition { hash: u32, line: u32 },
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
    /// A reply from an `EXTERNAL` process that does not fit the window it was
    /// sent: `window` is that window's number, and the fault says what was
    /// wrong and where in the reply.
    #[error("EXTERNAL reply for window {window} {fault}")]
    ExternalReply {
        window: u32,
        fault: crate::grammar_applicator::external::ExternalFault,
    },
    /// A window of a binary input stream that does not decode: `window` is the
    /// number the window takes in the run, and `offset` the byte of its body
    /// at which the offending read begins.
    #[error("binary stream window {window}, byte {offset} of its body: {fault}")]
    BinaryStreamWindow {
        window: u32,
        offset: usize,
        fault: crate::binary_applicator::BinaryStreamFault,
    },
    /// A window the binary stream writer cannot represent: `count` is more than
    /// the fixed-width field the format stores it in can hold, and writing it
    /// anyway would wrap the count and corrupt the stream.
    #[error(
        "binary stream window {window} cannot be written: {count} {what} exceed the format's limit of {max}"
    )]
    BinaryStreamOverflow {
        window: u32,
        what: crate::binary_applicator::BinaryCount,
        count: usize,
        max: usize,
    },
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
    // [spec:cg3:req:robustness.stream-invalid-utf8]
    /// Bytes in the input stream that are not UTF-8, with the input and the
    /// 1-based line they were read on. The C++ threw from its decoder and the
    /// process terminated; the port keeps the refusal, and never replaces the
    /// bytes with U+FFFD, since the author may not know they are there.
    #[error("{input}: {source} on line {line}")]
    InvalidUtf8 {
        input: String,
        line: u32,
        #[source]
        source: crate::uextras::InvalidUtf8,
    },
    // [spec:cg3:req:robustness.empty-tag]
    /// A tag with empty text reached the interner, which no tag may have: the
    /// C++ asserted against it in debug builds and interned it in release.
    /// `file` and `line` are the input and the line being read, or `RT RULE`
    /// and the rule's line when a rule was in flight (an `EXTERNAL` reply).
    #[error("{file}: empty tag on line {line}")]
    EmptyTag { file: String, line: u32 },
    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// A rule the stream put where the engine cannot apply it, placed at the
    /// rule with the grammar sources it quotes, as
    /// [`RunError::TagConstruction`] is. The inner error's kind is
    /// [`ParseErrorKind::RuleInapplicable`], saying why.
    #[error("{source}")]
    RuleInapplicable {
        #[source]
        source: Box<ParseError>,
        sources: Vec<ParseSource>,
    },
    /// C++ `addTagToReading`: a reading may carry at most one mapping tag, and
    /// a second distinct one was `CG3Quit(1)` — a fatal from the middle of the
    /// hot loop. `line` is the grammar line in flight.
    #[error("cannot add a mapping tag to a reading which already is mapped, on line {line}")]
    MappingTagConflict { line: u32 },
    /// A number in the stream that the hash tables reserve, refused as it was
    /// read. `input` and `line` name the stream and its line.
    #[error("{input}: {source}, on line {line}")]
    ReservedNumber {
        input: String,
        line: u32,
        #[source]
        source: ReservedNumber,
    },
    // [spec:cg3:req:robustness.terminates]
    /// A varstring whose every expansion is another varstring — captured text
    /// reading `VSTR:$1`, say. The C++ expands forever.
    #[error("varstring {tag} in the rule on line {line} keeps expanding into another varstring")]
    VarstringLoop { tag: String, line: u32 },
    /// C++ `runContextualTest`: a test with position `?` run with no override
    /// position was `CG3Quit(1)`. A textual grammar is refused at parse for
    /// this; a compiled one reaches here.
    #[error(
        "contextual test on line {line} has position `?` and no override position to replace it"
    )]
    PositionWithoutOverride { line: u32 },
    #[error("input contains sub-readings, which this output format cannot represent")]
    SubReadingsUnsupported,
    #[error("output format {format} cannot be written here")]
    UnsupportedOutputFormat { format: String },
    #[error("input format {format} cannot be read here")]
    UnsupportedInputFormat { format: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl RunError {
    // [spec:cg3:req:diagnostics.runtime-placed]
    /// The rule-placed diagnostic and the grammar sources it quotes, when this
    /// failure was placed in the grammar that caused it.
    pub fn placed(&self) -> Option<(&ParseError, &[ParseSource])> {
        match self {
            RunError::TagConstruction {
                source, sources, ..
            }
            | RunError::RuleInapplicable { source, sources }
                if !sources.is_empty() =>
            {
                Some((source, sources))
            }
            _ => None,
        }
    }
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

    #[error(transparent)]
    Option(#[from] OptionValueError),
}

// [spec:cg3:req:robustness.cli-arguments]
/// A numeric option given a value that is not a number it can hold.
#[derive(Debug, thiserror::Error)]
#[error("Error: --{option} expects a whole number up to 4294967295, not \"{value}\"")]
pub struct OptionValueError {
    pub option: &'static str,
    pub value: String,
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
        Cg3Error::Run(run) => {
            if let Some((source, sources)) = run.placed() {
                crate::diagnostics::report_parse_failure(std::slice::from_ref(source), sources);
            }
        }
        _ => {}
    }
    tracing::error!("{e}");
}
