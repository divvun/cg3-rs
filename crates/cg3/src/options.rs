//! Port of `src/options.hpp` + `src/options.cpp`.
//!
//! The vislcg3 CLI option enum ([`OPTIONS`]) and the `UOption` table that backs
//! it (`options`, plus the four default/override copies). Every table entry is
//! indexed by its matching [`OPTIONS`] enumerator, so the table order below is
//! identical to the enum order — do not reorder one without the other.
//!
//! ## Local `UOption` representation (NOTE)
//! The C++ tables use the vendored `Options::UOption` struct from
//! `include/uoptions.hpp` (out of scope for this wave). We reproduce a faithful
//! local equivalent here so the tables type-check and so `options_parser` /
//! `icu_uoptions` have a concrete type to populate:
//!
//! ```text
//! struct UOption {
//!     const char *longName = nullptr;  // "foo" for --foo
//!     char shortName = 0;              // 'f' for -f
//!     uint8_t hasArg = UOPT_NO_ARG;    // no/requires/optional argument
//!     std::string description;         // help text (Tino's addition)
//!     bool doesOccur = false;          // set when the option is seen
//!     std::string value;               // the consumed argument, if any
//! };
//! ```
//!
//! Mapping: `const char* longName` -> `Option<&'static str>` (the `nullptr`
//! default becomes `None`, matching u_parseArgs' `if (longName && ...)` guard);
//! `char shortName` -> [`crate::types::UChar`] (`char`), with the C++ `0`
//! "no short name" sentinel written as `'\0'`; `uint8_t hasArg` -> `u8` (the
//! [`UOPT_NO_ARG`]/[`UOPT_REQUIRES_ARG`]/[`UOPT_OPTIONAL_ARG`] constants);
//! `std::string description`/`value` -> `String`.
//!
//! ## Global-vs-function (NOTE / reconcile)
//! `options.cpp` exposes `options`, `options_default`, `options_override`,
//! `grammar_options_default`, `grammar_options_override` as **mutable global**
//! `std::array` objects that the tools layer mutates in place (via
//! `parse_opts_env`). Because a `UOption` array is not `const` (it owns
//! `String`s) and Rust mutable statics are unsafe/non-thread-safe, the port
//! exposes them as **constructor functions** returning fresh arrays instead;
//! the tools layer is expected to own the mutable copies. The table data is a
//! 1:1 transcription.

use crate::types::UChar;

// --- values of UOption.hasArg (from include/uoptions.hpp; local port) ---
// NOTE: sourced from the vendored `enum : uint8_t { ... }`; no spec id in scope.
pub const UOPT_NO_ARG: u8 = 0;
pub const UOPT_REQUIRES_ARG: u8 = 1;
pub const UOPT_OPTIONAL_ARG: u8 = 2;

// Local port of `Options::UOption` (include/uoptions.hpp). No spec id in scope;
// see the module NOTE for the field mapping.
#[derive(Clone, Debug)]
pub struct UOption {
    pub long_name: Option<&'static str>,
    pub short_name: UChar,
    pub has_arg: u8,
    pub description: String,
    pub does_occur: bool,
    pub value: String,
}

impl UOption {
    /// Four-field aggregate init `UOption{long, short, hasArg, desc}`.
    fn new(long: &'static str, short: UChar, has_arg: u8, desc: &'static str) -> Self {
        UOption {
            long_name: Some(long),
            short_name: short,
            has_arg,
            description: desc.to_string(),
            does_occur: false,
            value: String::new(),
        }
    }

    /// Three-field aggregate init `UOption{long, short, hasArg}` (description
    /// defaults to empty).
    fn new3(long: &'static str, short: UChar, has_arg: u8) -> Self {
        UOption {
            long_name: Some(long),
            short_name: short,
            has_arg,
            description: String::new(),
            does_occur: false,
            value: String::new(),
        }
    }
}

// [spec:cg3:def:options.options.options]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OPTIONS {
    HELP1,
    HELP2,
    VERSION,
    VERSION_TOO_OLD,
    GRAMMAR,
    GRAMMAR_OUT,
    GRAMMAR_BIN,
    GRAMMAR_ONLY,
    ORDERED,
    UNSAFE,
    SECTIONS,
    RULES,
    RULE,
    NRULES,
    NRULES_INV,
    DODEBUG,
    DEBUG_RULES,
    VERBOSE,
    QUIET,
    VISLCGCOMPAT,
    STDIN,
    STDOUT,
    STDERR,
    CODEPAGE_GLOBAL,
    CODEPAGE_GRAMMAR,
    CODEPAGE_INPUT,
    CODEPAGE_OUTPUT,
    NOMAPPINGS,
    NOCORRECTIONS,
    NOBEFORESECTIONS,
    NOSECTIONS,
    NOAFTERSECTIONS,
    TRACE,
    TRACE_NAME_ONLY,
    TRACE_NO_REMOVED,
    TRACE_ENCL,
    PIPE_DELETED,
    DRYRUN,
    SINGLERUN,
    MAXRUNS,
    PROFILING,
    MAPPING_PREFIX,
    UNICODE_TAGS,
    UNIQUE_TAGS,
    PRINT_IDS,
    PRINT_DEP,
    NUM_WINDOWS,
    ALWAYS_SPAN,
    SOFT_LIMIT,
    HARD_LIMIT,
    TEXT_DELIMIT,
    DEP_DELIMIT,
    DEP_ABSOLUTE,
    DEP_ORIGINAL,
    DEP_ALLOW_LOOPS,
    DEP_BLOCK_CROSSING,
    MAGIC_READINGS,
    NO_PASS_ORIGIN,
    SPLIT_MAPPINGS,
    SHOW_END_TAGS,
    SHOW_UNUSED_SETS,
    SHOW_TAGS,
    SHOW_TAG_HASHES,
    SHOW_SET_HASHES,
    DUMP_AST,
    NO_BREAK,
    IN_CG,
    IN_NICELINE,
    IN_APERTIUM,
    IN_FST,
    IN_PLAIN,
    IN_JSONL,
    IN_BINARY,
    OUT_CG,
    OUT_APERTIUM,
    OUT_FST,
    OUT_MATXIN,
    OUT_NICELINE,
    OUT_PLAIN,
    OUT_JSONL,
    OUT_BINARY,
    NUM_OPTIONS,
}

/// `using options_t = std::array<UOption, NUM_OPTIONS>;`
pub type options_t = [UOption; OPTIONS::NUM_OPTIONS as usize];

/// The base `Options::options` table (indexed by [`OPTIONS`]).
///
/// C++ counterpart: the mutable global `options_t options{...}` in
/// `options.cpp`; see the module NOTE on the global-vs-function deviation.
pub fn options() -> options_t {
    [
        UOption::new("help", 'h', UOPT_NO_ARG, "shows this help"),
        UOption::new("?", '?', UOPT_NO_ARG, "shows this help"),
        UOption::new("version", 'V', UOPT_NO_ARG, "prints copyright and version information"),
        UOption::new("min-binary-revision", '\0', UOPT_NO_ARG, "prints the minimum usable binary grammar revision"),
        UOption::new("grammar", 'g', UOPT_REQUIRES_ARG, "specifies the grammar file to use for disambiguation"),
        UOption::new("grammar-out", '\0', UOPT_REQUIRES_ARG, "writes the compiled grammar in textual form to a file"),
        UOption::new("grammar-bin", '\0', UOPT_REQUIRES_ARG, "writes the compiled grammar in binary form to a file"),
        UOption::new("grammar-only", '\0', UOPT_NO_ARG, "only compiles the grammar; implies --verbose"),
        UOption::new("ordered", '\0', UOPT_NO_ARG, "(will in future allow full ordered matching)"),
        UOption::new("unsafe", 'u', UOPT_NO_ARG, "allows the removal of all readings in a cohort, even the last one"),
        UOption::new("sections", 's', UOPT_REQUIRES_ARG, "number or ranges of sections to run; defaults to all sections"),
        UOption::new("rules", '\0', UOPT_REQUIRES_ARG, "number or ranges of rules to run; defaults to all rules"),
        UOption::new("rule", '\0', UOPT_REQUIRES_ARG, "a name or number of a single rule to run"),
        UOption::new("nrules", '\0', UOPT_REQUIRES_ARG, "a regex for which rule names to parse/run; defaults to all rules"),
        UOption::new("nrules-v", '\0', UOPT_REQUIRES_ARG, "a regex for which rule names not to parse/run"),
        UOption::new("debug", 'd', UOPT_OPTIONAL_ARG, "enables debug output (very noisy)"),
        UOption::new("debug-rules", '\0', UOPT_REQUIRES_ARG, "number or ranges of rules to debug"),
        UOption::new("verbose", 'v', UOPT_OPTIONAL_ARG, "increases verbosity"),
        UOption::new("quiet", '\0', UOPT_NO_ARG, "squelches warnings (same as -v 0)"),
        UOption::new("vislcg-compat", '2', UOPT_NO_ARG, "enables compatibility mode for older CG-2 and vislcg grammars"),

        UOption::new("stdin", 'I', UOPT_REQUIRES_ARG, "file to read input from instead of stdin"),
        UOption::new("stdout", 'O', UOPT_REQUIRES_ARG, "file to print output to instead of stdout"),
        UOption::new("stderr", 'E', UOPT_REQUIRES_ARG, "file to print errors to instead of stderr"),

        UOption::new3("codepage-all", 'C', UOPT_REQUIRES_ARG),
        UOption::new3("codepage-grammar", '\0', UOPT_REQUIRES_ARG),
        UOption::new3("codepage-input", '\0', UOPT_REQUIRES_ARG),
        UOption::new3("codepage-output", '\0', UOPT_REQUIRES_ARG),

        UOption::new("no-mappings", '\0', UOPT_NO_ARG, "disables all MAP, ADD, and REPLACE rules"),
        UOption::new("no-corrections", '\0', UOPT_NO_ARG, "disables all SUBSTITUTE and APPEND rules"),
        UOption::new("no-before-sections", '\0', UOPT_NO_ARG, "disables all rules in BEFORE-SECTIONS parts"),
        UOption::new("no-sections", '\0', UOPT_NO_ARG, "disables all rules in SECTION parts"),
        UOption::new("no-after-sections", '\0', UOPT_NO_ARG, "disables all rules in AFTER-SECTIONS parts"),

        UOption::new("trace", 't', UOPT_OPTIONAL_ARG, "prints debug output alongside normal output; optionally stops execution"),
        UOption::new("trace-name-only", '\0', UOPT_NO_ARG, "if a rule is named, omit the line number; implies --trace"),
        UOption::new("trace-no-removed", '\0', UOPT_NO_ARG, "does not print removed readings; implies --trace"),
        UOption::new("trace-encl", '\0', UOPT_NO_ARG, "traces which enclosure pass is currently happening; implies --trace"),

        UOption::new("deleted", '\0', UOPT_NO_ARG, "read deleted readings as such, instead of as text"),

        UOption::new("dry-run", '\0', UOPT_NO_ARG, "make no actual changes to the input"),
        UOption::new("single-run", '\0', UOPT_NO_ARG, "runs each section only once; same as --max-runs 1"),
        UOption::new("max-runs", '\0', UOPT_REQUIRES_ARG, "runs each section max N times; defaults to unlimited (0)"),
        UOption::new("profile", '\0', UOPT_REQUIRES_ARG, "gathers profiling statistics and code coverage into a SQLite database"),
        UOption::new("prefix", 'p', UOPT_REQUIRES_ARG, "sets the mapping prefix; defaults to @"),
        UOption::new("unicode-tags", '\0', UOPT_NO_ARG, "outputs Unicode code points for things like ->"),
        UOption::new("unique-tags", '\0', UOPT_NO_ARG, "outputs unique tags only once per reading"),
        UOption::new("print-ids", '\0', UOPT_NO_ARG, "always output IDs"),
        UOption::new("print-dep", '\0', UOPT_NO_ARG, "always output dependencies"),

        UOption::new("num-windows", '\0', UOPT_REQUIRES_ARG, "number of windows to keep in before/ahead buffers; defaults to 2"),
        UOption::new("always-span", '\0', UOPT_NO_ARG, "forces scanning tests to always span across window boundaries"),
        UOption::new("soft-limit", '\0', UOPT_REQUIRES_ARG, "number of cohorts after which the SOFT-DELIMITERS kick in; defaults to 300"),
        UOption::new("hard-limit", '\0', UOPT_REQUIRES_ARG, "number of cohorts after which the window is forcefully cut; defaults to 500"),
        UOption::new("text-delimit", 'T', UOPT_OPTIONAL_ARG, "additional delimit based on non-CG text, ensuring it isn't attached to a cohort; defaults to /(^|\\n)</s/r"),
        UOption::new("dep-delimit", 'D', UOPT_OPTIONAL_ARG, "delimit windows based on dependency instead of DELIMITERS; defaults to 10"),
        UOption::new("dep-absolute", '\0', UOPT_NO_ARG, "outputs absolute cohort numbers rather than relative ones"),
        UOption::new("dep-original", '\0', UOPT_NO_ARG, "outputs the original input dependency tag even if it is no longer valid"),
        UOption::new("dep-allow-loops", '\0', UOPT_NO_ARG, "allows the creation of circular dependencies"),
        UOption::new("dep-no-crossing", '\0', UOPT_NO_ARG, "prevents the creation of dependencies that would result in crossing branches"),

        UOption::new("no-magic-readings", '\0', UOPT_NO_ARG, "prevents running rules on magic readings"),
        UOption::new("no-pass-origin", 'o', UOPT_NO_ARG, "prevents scanning tests from passing the point of origin"),
        UOption::new("split-mappings", '\0', UOPT_NO_ARG, "keep mapped readings separate in output"),
        UOption::new("show-end-tags", 'e', UOPT_NO_ARG, "allows the <<< tags to appear in output"),
        UOption::new("show-unused-sets", '\0', UOPT_NO_ARG, "prints a list of unused sets and their line numbers; implies --grammar-only"),
        UOption::new("show-tags", '\0', UOPT_NO_ARG, "prints a list of unique used tags; implies --grammar-only"),
        UOption::new("show-tag-hashes", '\0', UOPT_NO_ARG, "prints a list of tags and their hashes as they are parsed during the run"),
        UOption::new("show-set-hashes", '\0', UOPT_NO_ARG, "prints a list of sets and their hashes; implies --grammar-only"),
        UOption::new("dump-ast", '\0', UOPT_NO_ARG, "prints the grammar parse tree; implies --grammar-only"),
        UOption::new("no-break", '\0', UOPT_NO_ARG, "inhibits any extra whitespace in output"),
        UOption::new("in-cg", '\0', UOPT_NO_ARG, "sets input format to CG (default)"),
        UOption::new("in-niceline", '\0', UOPT_NO_ARG, "sets input format to Niceline CG"),
        UOption::new("in-apertium", '\0', UOPT_NO_ARG, "sets input format to Apertium"),
        UOption::new("in-fst", '\0', UOPT_NO_ARG, "sets input format to HFST/XFST"),
        UOption::new("in-plain", '\0', UOPT_NO_ARG, "sets input format to plain text"),
        UOption::new("in-jsonl", '\0', UOPT_NO_ARG, "sets input format to JSONL (experimental)"),
        UOption::new("in-binary", '\0', UOPT_NO_ARG, "sets input format to binary (experimental)"),
        UOption::new("out-cg", '\0', UOPT_NO_ARG, "sets output format to CG (default)"),
        UOption::new("out-apertium", '\0', UOPT_NO_ARG, "sets output format to Apertium"),
        UOption::new("out-fst", '\0', UOPT_NO_ARG, "sets output format to HFST/XFST"),
        UOption::new("out-matxin", '\0', UOPT_NO_ARG, "sets output format to Matxin"),
        UOption::new("out-niceline", '\0', UOPT_NO_ARG, "sets output format to Niceline CG"),
        UOption::new("out-plain", '\0', UOPT_NO_ARG, "sets output format to plain text"),
        UOption::new("out-jsonl", '\0', UOPT_NO_ARG, "sets output format to JSONL (experimental)"),
        UOption::new("out-binary", '\0', UOPT_NO_ARG, "sets output format to binary (experimental)"),
    ]
}

// `options_t options_default = options;` and the other copies. Constructor
// functions (see module NOTE); each returns a fresh clone of the base table.
pub fn options_default() -> options_t {
    options()
}
pub fn options_override() -> options_t {
    options()
}
pub fn grammar_options_default() -> options_t {
    options()
}
pub fn grammar_options_override() -> options_t {
    options()
}
