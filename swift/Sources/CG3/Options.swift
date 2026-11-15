internal import CG3Bindings

// [spec:cg3:req:swift.apply]
// [spec:cg3:req:swift.surface]
/// A stream format, as vislcg3's `--in-*` and `--out-*` options name it. The
/// binary format is left out: a string cannot carry it.
public enum StreamFormat: String, Sendable, Hashable, CaseIterable {
    /// VISL CG, `--in-cg` and vislcg3's output when none is named.
    case cg
    /// Niceline CG, `--in-niceline` and `--out-niceline`.
    case niceline
    /// Apertium's stream format, `--in-apertium` and `--out-apertium`.
    case apertium
    /// HFST/XFST output, `--in-fst` and `--out-fst`.
    case fst
    /// Plain text, `--in-plain` and `--out-plain`.
    case plain
    /// JSON lines, `--in-jsonl` and `--out-jsonl`.
    case jsonl
}

// [spec:cg3:req:swift.apply]
/// The value of an option whose value vislcg3 makes optional, such as
/// `--trace`: off, given without a value, or given with one. `false`, `true`
/// and a string literal stand for each.
public enum OptionalArgument: Sendable, Hashable, ExpressibleByBooleanLiteral, ExpressibleByStringLiteral {
    /// The option is left off the command line.
    case off
    /// The option is given without a value, as `--trace` is.
    case on
    /// The option is given with this value, as `--trace=R1` is.
    case value(String)

    /// `on` for `true`, and `off` for `false`.
    public init(booleanLiteral value: Bool) {
        self = value ? .on : .off
    }

    /// `value(_:)` of the string, so that `trace: "R1"` gives `--trace=R1`.
    public init(stringLiteral value: String) {
        self = .value(value)
    }

    /// The option as the library takes it: `nil` off, and an empty value for
    /// one given without a value.
    var given: String? {
        switch self {
        case .off: nil
        case .on: ""
        case let .value(value): value
        }
    }
}

// [spec:cg3:req:swift.load-options]
/// The options vislcg3 applies to a grammar while it loads it, each named
/// after its vislcg3 long option in lowerCamelCase: `--nrules-v X` is
/// `nrulesV: "X"`. A `--prefix` in the grammar's `CMDARGS` applies as it does
/// in vislcg3.
public struct LoadOptions: Sendable, Hashable {
    /// Keep only the rules whose names this pattern matches.
    public var nrules: String?
    /// Leave out the rules whose names this pattern matches.
    public var nrulesV: String?
    /// Read a textual grammar as VISLCG2 did; a `.cg3b` reads the same either
    /// way.
    public var vislcgCompat: Bool
    /// The mapping prefix, which a `.cg3b` must have been compiled with.
    public var prefix: Unicode.Scalar?

    /// The options given, and every other left off.
    public init(nrules: String? = nil, nrulesV: String? = nil, vislcgCompat: Bool = false, prefix: Unicode.Scalar? = nil) {
        self.nrules = nrules
        self.nrulesV = nrulesV
        self.vislcgCompat = vislcgCompat
        self.prefix = prefix
    }

    var bindings: CG3Bindings.LoadOptions {
        CG3Bindings.LoadOptions(
            nrules: nrules, nrulesV: nrulesV, vislcgCompat: vislcgCompat, prefix: prefix.map(String.init))
    }
}

// [spec:cg3:req:swift.apply]
/// The options a run reads, each named after its vislcg3 long option in
/// lowerCamelCase: `--num-windows 4` is `numWindows: 4`. `input` and `output`
/// stand for the `--in-*` and `--out-*` options. The options a grammar sets
/// with `CMDARGS` apply as they do in vislcg3.
public struct ApplyOptions: Sendable, Hashable {
    /// The input stream's format, an `--in-*` option; CG when `nil`.
    public var input: StreamFormat?
    /// The output stream's format, an `--out-*` option; CG when `nil`.
    public var output: StreamFormat?
    /// `--ordered`: ordered matching, which vislcg3 says it will allow in future.
    public var ordered: Bool
    /// `--unsafe`: allows the removal of all readings in a cohort, even the last one.
    public var `unsafe`: Bool
    /// `--sections`: number or ranges of sections to run; all sections when `nil`.
    public var sections: String?
    /// `--rules`: number or ranges of rules to run; all rules when `nil`.
    public var rules: String?
    /// `--rule`: a name or number of a single rule to run.
    public var rule: String?
    /// `--debug-rules`: number or ranges of rules to debug.
    public var debugRules: String?
    /// `--verbose`: increases verbosity, optionally to a level.
    public var verbose: OptionalArgument
    /// `--no-mappings`: disables all MAP, ADD and REPLACE rules.
    public var noMappings: Bool
    /// `--no-corrections`: disables all SUBSTITUTE and APPEND rules.
    public var noCorrections: Bool
    /// `--no-before-sections`: disables all rules in BEFORE-SECTIONS parts.
    public var noBeforeSections: Bool
    /// `--no-sections`: disables all rules in SECTION parts.
    public var noSections: Bool
    /// `--no-after-sections`: disables all rules in AFTER-SECTIONS parts.
    public var noAfterSections: Bool
    /// `--trace`: prints which rules did what alongside the output; a value names where to stop.
    public var trace: OptionalArgument
    /// `--trace-name-only`: omits the line number of a named rule from the trace; implies `trace`.
    public var traceNameOnly: Bool
    /// `--trace-no-removed`: does not print removed readings; implies `trace`.
    public var traceNoRemoved: Bool
    /// `--trace-encl`: traces which enclosure pass is going on; implies `trace`.
    public var traceEncl: Bool
    /// `--deleted`: reads deleted readings as such, instead of as text.
    public var deleted: Bool
    /// `--single-run`: runs each section only once, as `maxRuns: 1` does.
    public var singleRun: Bool
    /// `--max-runs`: runs each section at most this many times; unlimited when `nil` or 0.
    public var maxRuns: UInt32?
    /// `--unicode-tags`: writes Unicode code points for things like `->`.
    public var unicodeTags: Bool
    /// `--unique-tags`: writes each tag only once per reading.
    public var uniqueTags: Bool
    /// `--print-ids`: always writes IDs.
    public var printIds: Bool
    /// `--print-dep`: always writes dependencies.
    public var printDep: Bool
    /// `--num-windows`: how many windows to keep in the before and ahead buffers; 2 when `nil`.
    public var numWindows: UInt32?
    /// `--always-span`: makes scanning tests always span window boundaries.
    public var alwaysSpan: Bool
    /// `--soft-limit`: the number of cohorts after which the SOFT-DELIMITERS apply; 300 when `nil`.
    public var softLimit: UInt32?
    /// `--hard-limit`: the number of cohorts after which a window is cut; 500 when `nil`.
    public var hardLimit: UInt32?
    /// `--text-delimit`: delimits windows on text that is not CG, so it is attached to no cohort; a value is the pattern.
    public var textDelimit: OptionalArgument
    /// `--dep-delimit`: delimits windows by dependency rather than DELIMITERS; a value is the limit, 10 without one.
    public var depDelimit: OptionalArgument
    /// `--dep-absolute`: writes absolute cohort numbers rather than relative ones.
    public var depAbsolute: Bool
    /// `--dep-original`: writes the input's dependency tag even when it is no longer valid.
    public var depOriginal: Bool
    /// `--dep-allow-loops`: allows circular dependencies.
    public var depAllowLoops: Bool
    /// `--dep-no-crossing`: prevents dependencies whose branches would cross.
    public var depNoCrossing: Bool
    /// `--no-magic-readings`: runs no rule on magic readings.
    public var noMagicReadings: Bool
    /// `--no-pass-origin`: keeps scanning tests from passing the point of origin.
    public var noPassOrigin: Bool
    /// `--split-mappings`: keeps mapped readings apart in the output.
    public var splitMappings: Bool
    /// `--show-end-tags`: lets the `<<<` tags appear in the output.
    public var showEndTags: Bool
    /// `--no-break`: writes no extra whitespace.
    public var noBreak: Bool

    /// The options given, and every other left off.
    public init(
        input: StreamFormat? = nil,
        output: StreamFormat? = nil,
        ordered: Bool = false,
        unsafe: Bool = false,
        sections: String? = nil,
        rules: String? = nil,
        rule: String? = nil,
        debugRules: String? = nil,
        verbose: OptionalArgument = .off,
        noMappings: Bool = false,
        noCorrections: Bool = false,
        noBeforeSections: Bool = false,
        noSections: Bool = false,
        noAfterSections: Bool = false,
        trace: OptionalArgument = .off,
        traceNameOnly: Bool = false,
        traceNoRemoved: Bool = false,
        traceEncl: Bool = false,
        deleted: Bool = false,
        singleRun: Bool = false,
        maxRuns: UInt32? = nil,
        unicodeTags: Bool = false,
        uniqueTags: Bool = false,
        printIds: Bool = false,
        printDep: Bool = false,
        numWindows: UInt32? = nil,
        alwaysSpan: Bool = false,
        softLimit: UInt32? = nil,
        hardLimit: UInt32? = nil,
        textDelimit: OptionalArgument = .off,
        depDelimit: OptionalArgument = .off,
        depAbsolute: Bool = false,
        depOriginal: Bool = false,
        depAllowLoops: Bool = false,
        depNoCrossing: Bool = false,
        noMagicReadings: Bool = false,
        noPassOrigin: Bool = false,
        splitMappings: Bool = false,
        showEndTags: Bool = false,
        noBreak: Bool = false
    ) {
        self.input = input
        self.output = output
        self.ordered = ordered
        self.unsafe = `unsafe`
        self.sections = sections
        self.rules = rules
        self.rule = rule
        self.debugRules = debugRules
        self.verbose = verbose
        self.noMappings = noMappings
        self.noCorrections = noCorrections
        self.noBeforeSections = noBeforeSections
        self.noSections = noSections
        self.noAfterSections = noAfterSections
        self.trace = trace
        self.traceNameOnly = traceNameOnly
        self.traceNoRemoved = traceNoRemoved
        self.traceEncl = traceEncl
        self.deleted = deleted
        self.singleRun = singleRun
        self.maxRuns = maxRuns
        self.unicodeTags = unicodeTags
        self.uniqueTags = uniqueTags
        self.printIds = printIds
        self.printDep = printDep
        self.numWindows = numWindows
        self.alwaysSpan = alwaysSpan
        self.softLimit = softLimit
        self.hardLimit = hardLimit
        self.textDelimit = textDelimit
        self.depDelimit = depDelimit
        self.depAbsolute = depAbsolute
        self.depOriginal = depOriginal
        self.depAllowLoops = depAllowLoops
        self.depNoCrossing = depNoCrossing
        self.noMagicReadings = noMagicReadings
        self.noPassOrigin = noPassOrigin
        self.splitMappings = splitMappings
        self.showEndTags = showEndTags
        self.noBreak = noBreak
    }

    var bindings: CG3Bindings.ApplyOptions {
        CG3Bindings.ApplyOptions(
            input: input.map(\.bindings), output: output.map(\.bindings), ordered: ordered,
            unsafeMode: self.unsafe, sections: sections, rules: rules, rule: rule, debugRules: debugRules,
            verbose: verbose.given, noMappings: noMappings, noCorrections: noCorrections,
            noBeforeSections: noBeforeSections, noSections: noSections, noAfterSections: noAfterSections,
            trace: trace.given, traceNameOnly: traceNameOnly, traceNoRemoved: traceNoRemoved,
            traceEncl: traceEncl, deleted: deleted, singleRun: singleRun, maxRuns: maxRuns,
            unicodeTags: unicodeTags, uniqueTags: uniqueTags, printIds: printIds, printDep: printDep,
            numWindows: numWindows, alwaysSpan: alwaysSpan, softLimit: softLimit, hardLimit: hardLimit,
            textDelimit: textDelimit.given, depDelimit: depDelimit.given, depAbsolute: depAbsolute,
            depOriginal: depOriginal, depAllowLoops: depAllowLoops, depNoCrossing: depNoCrossing,
            noMagicReadings: noMagicReadings, noPassOrigin: noPassOrigin, splitMappings: splitMappings,
            showEndTags: showEndTags, noBreak: noBreak)
    }
}

extension StreamFormat {
    var bindings: CG3Bindings.StreamFormat {
        switch self {
        case .cg: .cg
        case .niceline: .niceline
        case .apertium: .apertium
        case .fst: .fst
        case .plain: .plain
        case .jsonl: .jsonl
        }
    }
}
