//! Port of `src/options.hpp` + `src/options.cpp`.
//!
//! The vislcg3 CLI option enum ([`Opt`]) and the `ArgOption` table that backs
//! it (`options`, plus the four default/override copies). Every table entry is
//! indexed by its matching [`Opt`] enumerator, so the table order below is
//! identical to the enum order — do not reorder one without the other.
//!
//! ## Option entries
//! Each entry is an [`ArgOption`]: the long and short names, whether it takes
//! a value, its help text, and — once argv has been parsed — whether it
//! occurred and the value it was given. A C++ entry with no long name is
//! `None`, and no short name is `'\0'`.
//!
//! ## Global-vs-function (NOTE / reconcile)
//! `options.cpp` exposes `options`, `options_default`, `options_override`,
//! `grammar_options_default`, `grammar_options_override` as **mutable global**
//! `std::array` objects that the tools layer mutates in place (via
//! `parse_opts_env`). Because an `ArgOption` array is not `const` (it owns
//! `String`s) and Rust mutable statics are unsafe/non-thread-safe, the port
//! exposes them as **constructor functions** returning fresh arrays instead;
//! the tools layer is expected to own the mutable copies. The table data is a
//! 1:1 transcription.

/// Whether an option takes a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HasArg {
    No,
    Required,
    Optional,
}

/// One command-line option: its names, whether it takes a value, its help
/// text, and what parsing argv found for it.
#[derive(Clone, Debug)]
pub struct ArgOption {
    pub long_name: Option<&'static str>,
    pub short_name: char,
    pub has_arg: HasArg,
    pub description: String,
    pub does_occur: bool,
    pub value: String,
}

impl ArgOption {
    /// An option with help text.
    pub(crate) fn new(
        long: &'static str,
        short: char,
        has_arg: HasArg,
        desc: &'static str,
    ) -> Self {
        ArgOption {
            long_name: Some(long),
            short_name: short,
            has_arg,
            description: desc.to_string(),
            does_occur: false,
            value: String::new(),
        }
    }

    /// An option with no help text, which `--help` does not list.
    pub(crate) fn hidden(long: &'static str, short: char, has_arg: HasArg) -> Self {
        ArgOption {
            long_name: Some(long),
            short_name: short,
            has_arg,
            description: String::new(),
            does_occur: false,
            value: String::new(),
        }
    }
}

// DIVERGENCE (operator decision): three upstream CLI options are removed, not
// transcribed, so this enum + table are intentionally NOT a 1:1 mirror of the
// C++ `options` array (see plan node `option-wiring`):
//   * `--dry-run`         — dead in the reference too; its gate was deleted
//                           upstream (declared + written, never read).
//   * `--out-matxin`      — neither FormatConverter has a Matxin arm, so it
//                           silently emitted CG; dropped rather than reproducing
//                           the no-op quirk.
//   * `--show-tag-hashes` — a stderr hash-dump whose numbers are port-internal
//                           only (port hashes UTF-8, upstream UTF-16) and which
//                           relied on the class-static mutable-stream wart.
// [spec:cg3:def:options.options.options]
/// C++ `enum OPTIONS` (`options.hpp`); each variant camel-cases its C++
/// enumerator (`HELP1` → `Help1`, `IN_CG` → `InCg`, ..., `NUM_OPTIONS` →
/// `NumOptions`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    Help1,
    Help2,
    Version,
    VersionTooOld,
    Grammar,
    GrammarOut,
    GrammarBin,
    GrammarOnly,
    Ordered,
    Unsafe,
    Sections,
    Rules,
    Rule,
    Nrules,
    NrulesInv,
    Dodebug,
    DebugRules,
    Verbose,
    Quiet,
    Vislcgcompat,
    Stdin,
    Stdout,
    Stderr,
    CodepageGlobal,
    CodepageGrammar,
    CodepageInput,
    CodepageOutput,
    Nomappings,
    Nocorrections,
    Nobeforesections,
    Nosections,
    Noaftersections,
    Trace,
    TraceNameOnly,
    TraceNoRemoved,
    TraceEncl,
    PipeDeleted,
    Singlerun,
    Maxruns,
    Profiling,
    MappingPrefix,
    UnicodeTags,
    UniqueTags,
    PrintIds,
    PrintDep,
    NumWindows,
    AlwaysSpan,
    SoftLimit,
    HardLimit,
    TextDelimit,
    DepDelimit,
    DepAbsolute,
    DepOriginal,
    DepAllowLoops,
    DepBlockCrossing,
    MagicReadings,
    NoPassOrigin,
    SplitMappings,
    ShowEndTags,
    ShowUnusedSets,
    ShowTags,
    ShowSetHashes,
    DumpAst,
    NoBreak,
    InCg,
    InNiceline,
    InApertium,
    InFst,
    InPlain,
    InJsonl,
    InBinary,
    OutCg,
    OutApertium,
    OutFst,
    OutNiceline,
    OutPlain,
    OutJsonl,
    OutBinary,
    NumOptions,
}

/// C++ `options_t`: one [`ArgOption`] per [`Opt`].
pub type OptionsTable = [ArgOption; Opt::NumOptions as usize];

/// The base `Options::options` table (indexed by [`Opt`]).
///
/// C++ counterpart: the mutable global `options_t options{...}` in
/// `options.cpp`; see the module NOTE on the global-vs-function deviation.
pub fn options() -> OptionsTable {
    [
        ArgOption::new("help", 'h', HasArg::No, "shows this help"),
        ArgOption::new("?", '?', HasArg::No, "shows this help"),
        ArgOption::new(
            "version",
            'V',
            HasArg::No,
            "prints copyright and version information",
        ),
        ArgOption::new(
            "min-binary-revision",
            '\0',
            HasArg::No,
            "prints the minimum usable binary grammar revision",
        ),
        ArgOption::new(
            "grammar",
            'g',
            HasArg::Required,
            "specifies the grammar file to use for disambiguation",
        ),
        ArgOption::new(
            "grammar-out",
            '\0',
            HasArg::Required,
            "writes the compiled grammar in textual form to a file",
        ),
        ArgOption::new(
            "grammar-bin",
            '\0',
            HasArg::Required,
            "writes the compiled grammar in binary form to a file",
        ),
        ArgOption::new(
            "grammar-only",
            '\0',
            HasArg::No,
            "only compiles the grammar; implies --verbose",
        ),
        ArgOption::new(
            "ordered",
            '\0',
            HasArg::No,
            "(will in future allow full ordered matching)",
        ),
        ArgOption::new(
            "unsafe",
            'u',
            HasArg::No,
            "allows the removal of all readings in a cohort, even the last one",
        ),
        ArgOption::new(
            "sections",
            's',
            HasArg::Required,
            "number or ranges of sections to run; defaults to all sections",
        ),
        ArgOption::new(
            "rules",
            '\0',
            HasArg::Required,
            "number or ranges of rules to run; defaults to all rules",
        ),
        ArgOption::new(
            "rule",
            '\0',
            HasArg::Required,
            "a name or number of a single rule to run",
        ),
        ArgOption::new(
            "nrules",
            '\0',
            HasArg::Required,
            "a regex for which rule names to parse/run; defaults to all rules",
        ),
        ArgOption::new(
            "nrules-v",
            '\0',
            HasArg::Required,
            "a regex for which rule names not to parse/run",
        ),
        ArgOption::new(
            "debug",
            'd',
            HasArg::Optional,
            "enables debug output (very noisy)",
        ),
        ArgOption::new(
            "debug-rules",
            '\0',
            HasArg::Required,
            "number or ranges of rules to debug",
        ),
        ArgOption::new("verbose", 'v', HasArg::Optional, "increases verbosity"),
        ArgOption::new(
            "quiet",
            '\0',
            HasArg::No,
            "squelches warnings (same as -v 0)",
        ),
        ArgOption::new(
            "vislcg-compat",
            '2',
            HasArg::No,
            "enables compatibility mode for older CG-2 and vislcg grammars",
        ),
        ArgOption::new(
            "stdin",
            'I',
            HasArg::Required,
            "file to read input from instead of stdin",
        ),
        ArgOption::new(
            "stdout",
            'O',
            HasArg::Required,
            "file to print output to instead of stdout",
        ),
        ArgOption::new(
            "stderr",
            'E',
            HasArg::Required,
            "file to print errors to instead of stderr",
        ),
        ArgOption::hidden("codepage-all", 'C', HasArg::Required),
        ArgOption::hidden("codepage-grammar", '\0', HasArg::Required),
        ArgOption::hidden("codepage-input", '\0', HasArg::Required),
        ArgOption::hidden("codepage-output", '\0', HasArg::Required),
        ArgOption::new(
            "no-mappings",
            '\0',
            HasArg::No,
            "disables all MAP, ADD, and REPLACE rules",
        ),
        ArgOption::new(
            "no-corrections",
            '\0',
            HasArg::No,
            "disables all SUBSTITUTE and APPEND rules",
        ),
        ArgOption::new(
            "no-before-sections",
            '\0',
            HasArg::No,
            "disables all rules in BEFORE-SECTIONS parts",
        ),
        ArgOption::new(
            "no-sections",
            '\0',
            HasArg::No,
            "disables all rules in SECTION parts",
        ),
        ArgOption::new(
            "no-after-sections",
            '\0',
            HasArg::No,
            "disables all rules in AFTER-SECTIONS parts",
        ),
        ArgOption::new(
            "trace",
            't',
            HasArg::Optional,
            "prints debug output alongside normal output; optionally stops execution",
        ),
        ArgOption::new(
            "trace-name-only",
            '\0',
            HasArg::No,
            "if a rule is named, omit the line number; implies --trace",
        ),
        ArgOption::new(
            "trace-no-removed",
            '\0',
            HasArg::No,
            "does not print removed readings; implies --trace",
        ),
        ArgOption::new(
            "trace-encl",
            '\0',
            HasArg::No,
            "traces which enclosure pass is currently happening; implies --trace",
        ),
        ArgOption::new(
            "deleted",
            '\0',
            HasArg::No,
            "read deleted readings as such, instead of as text",
        ),
        ArgOption::new(
            "single-run",
            '\0',
            HasArg::No,
            "runs each section only once; same as --max-runs 1",
        ),
        ArgOption::new(
            "max-runs",
            '\0',
            HasArg::Required,
            "runs each section max N times; defaults to unlimited (0)",
        ),
        ArgOption::new(
            "profile",
            '\0',
            HasArg::Required,
            "gathers profiling statistics and code coverage into a SQLite database",
        ),
        ArgOption::new(
            "prefix",
            'p',
            HasArg::Required,
            "sets the mapping prefix; defaults to @",
        ),
        ArgOption::new(
            "unicode-tags",
            '\0',
            HasArg::No,
            "outputs Unicode code points for things like ->",
        ),
        ArgOption::new(
            "unique-tags",
            '\0',
            HasArg::No,
            "outputs unique tags only once per reading",
        ),
        ArgOption::new("print-ids", '\0', HasArg::No, "always output IDs"),
        ArgOption::new("print-dep", '\0', HasArg::No, "always output dependencies"),
        ArgOption::new(
            "num-windows",
            '\0',
            HasArg::Required,
            "number of windows to keep in before/ahead buffers; defaults to 2",
        ),
        ArgOption::new(
            "always-span",
            '\0',
            HasArg::No,
            "forces scanning tests to always span across window boundaries",
        ),
        ArgOption::new(
            "soft-limit",
            '\0',
            HasArg::Required,
            "number of cohorts after which the SOFT-DELIMITERS kick in; defaults to 300",
        ),
        ArgOption::new(
            "hard-limit",
            '\0',
            HasArg::Required,
            "number of cohorts after which the window is forcefully cut; defaults to 500",
        ),
        ArgOption::new(
            "text-delimit",
            'T',
            HasArg::Optional,
            "additional delimit based on non-CG text, ensuring it isn't attached to a cohort; defaults to /(^|\\n)</s/r",
        ),
        ArgOption::new(
            "dep-delimit",
            'D',
            HasArg::Optional,
            "delimit windows based on dependency instead of DELIMITERS; defaults to 10",
        ),
        ArgOption::new(
            "dep-absolute",
            '\0',
            HasArg::No,
            "outputs absolute cohort numbers rather than relative ones",
        ),
        ArgOption::new(
            "dep-original",
            '\0',
            HasArg::No,
            "outputs the original input dependency tag even if it is no longer valid",
        ),
        ArgOption::new(
            "dep-allow-loops",
            '\0',
            HasArg::No,
            "allows the creation of circular dependencies",
        ),
        ArgOption::new(
            "dep-no-crossing",
            '\0',
            HasArg::No,
            "prevents the creation of dependencies that would result in crossing branches",
        ),
        ArgOption::new(
            "no-magic-readings",
            '\0',
            HasArg::No,
            "prevents running rules on magic readings",
        ),
        ArgOption::new(
            "no-pass-origin",
            'o',
            HasArg::No,
            "prevents scanning tests from passing the point of origin",
        ),
        ArgOption::new(
            "split-mappings",
            '\0',
            HasArg::No,
            "keep mapped readings separate in output",
        ),
        ArgOption::new(
            "show-end-tags",
            'e',
            HasArg::No,
            "allows the <<< tags to appear in output",
        ),
        ArgOption::new(
            "show-unused-sets",
            '\0',
            HasArg::No,
            "prints a list of unused sets and their line numbers; implies --grammar-only",
        ),
        ArgOption::new(
            "show-tags",
            '\0',
            HasArg::No,
            "prints a list of unique used tags; implies --grammar-only",
        ),
        ArgOption::new(
            "show-set-hashes",
            '\0',
            HasArg::No,
            "prints a list of sets and their hashes; implies --grammar-only",
        ),
        ArgOption::new(
            "dump-ast",
            '\0',
            HasArg::No,
            "prints the grammar parse tree; implies --grammar-only",
        ),
        ArgOption::new(
            "no-break",
            '\0',
            HasArg::No,
            "inhibits any extra whitespace in output",
        ),
        ArgOption::new(
            "in-cg",
            '\0',
            HasArg::No,
            "sets input format to CG (default)",
        ),
        ArgOption::new(
            "in-niceline",
            '\0',
            HasArg::No,
            "sets input format to Niceline CG",
        ),
        ArgOption::new(
            "in-apertium",
            '\0',
            HasArg::No,
            "sets input format to Apertium",
        ),
        ArgOption::new("in-fst", '\0', HasArg::No, "sets input format to HFST/XFST"),
        ArgOption::new(
            "in-plain",
            '\0',
            HasArg::No,
            "sets input format to plain text",
        ),
        ArgOption::new(
            "in-jsonl",
            '\0',
            HasArg::No,
            "sets input format to JSONL (experimental)",
        ),
        ArgOption::new(
            "in-binary",
            '\0',
            HasArg::No,
            "sets input format to binary (experimental)",
        ),
        ArgOption::new(
            "out-cg",
            '\0',
            HasArg::No,
            "sets output format to CG (default)",
        ),
        ArgOption::new(
            "out-apertium",
            '\0',
            HasArg::No,
            "sets output format to Apertium",
        ),
        ArgOption::new(
            "out-fst",
            '\0',
            HasArg::No,
            "sets output format to HFST/XFST",
        ),
        ArgOption::new(
            "out-niceline",
            '\0',
            HasArg::No,
            "sets output format to Niceline CG",
        ),
        ArgOption::new(
            "out-plain",
            '\0',
            HasArg::No,
            "sets output format to plain text",
        ),
        ArgOption::new(
            "out-jsonl",
            '\0',
            HasArg::No,
            "sets output format to JSONL (experimental)",
        ),
        ArgOption::new(
            "out-binary",
            '\0',
            HasArg::No,
            "sets output format to binary (experimental)",
        ),
    ]
}

// `options_t options_default = options;` and the other copies. Constructor
// functions (see module NOTE); each returns a fresh clone of the base table.
pub fn options_default() -> OptionsTable {
    options()
}
pub fn options_override() -> OptionsTable {
    options()
}
pub fn grammar_options_default() -> OptionsTable {
    options()
}
pub fn grammar_options_override() -> OptionsTable {
    options()
}
