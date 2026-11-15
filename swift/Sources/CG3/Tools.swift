internal import CG3Bindings

// [spec:cg3:req:swift.package]
/// The package's version. The package is versioned on its own, apart from
/// the cg3 library it carries.
public var version: String {
    CG3Bindings.version()
}

// [spec:cg3:req:swift.tools]
/// Converts `input` from one stream format to another, as cg-conv does, and
/// returns what cg-conv writes.
///
/// - Throws: `CG3Error` with code `runFailed` for a conversion the library
///   reports as failed, and `invalidArgument` for a `prefix` it cannot use.
public func convert(_ input: String, options: ConvertOptions = ConvertOptions()) throws -> String {
    try calling { try CG3Bindings.convert(input: input, options: options.bindings) }
}

// [spec:cg3:req:swift.tools]
/// Splits each cohort whose wordform tags give its words into one cohort per
/// word, as cg-mwesplit does, and returns what cg-mwesplit writes.
///
/// - Throws: `CG3Error` with code `runFailed` for a split the library reports
///   as failed.
public func mwesplit(_ input: String) throws -> String {
    try calling { try CG3Bindings.mwesplit(input: input) }
}

// [spec:cg3:req:swift.tools]
/// cg-conv's options, each named after its long option in lowerCamelCase:
/// `--sub-delim "#"` is `subDelim: "#"`. `input` and `output` stand for the
/// `--in-*` and `--out-*` options.
public struct ConvertOptions: Sendable, Hashable {
    /// The input stream's format; detected from the input when `nil`, as
    /// cg-conv does.
    public var input: StreamFormat?
    /// Detect the input's format even where `input` or `addTags` names one.
    public var inAuto: Bool
    /// Give each word of plain text a minimal reading. It reads plain text
    /// unless `input` names CG, Niceline, Apertium or FST.
    public var addTags: Bool
    /// The output stream's format; CG when `nil`.
    public var output: StreamFormat?
    /// The mapping prefix; `@` when `nil`.
    public var prefix: Unicode.Scalar?
    /// What FST weights are multiplied by; 1 when `nil`.
    public var wfactor: Double?
    /// The tag FST weights are written as; `W` when `nil`.
    public var wtag: String?
    /// The characters that separate FST sub-readings; `#` when `nil`.
    public var subDelim: String?
    /// Sub-readings run right to left: cg-conv's default, so it has no
    /// effect.
    public var rtl: Bool
    /// Sub-readings run left to right.
    public var ltr: Bool
    /// `--ordered`: tag order matters.
    public var ordered: Bool
    /// `--parse-dep`: reads dependency tags as dependencies rather than as plain tags.
    public var parseDep: Bool
    /// Delimit windows by dependency, after this many; cg-conv takes the
    /// option without a value as 10.
    public var depDelimit: UInt32?
    /// `--unicode-tags`: writes Unicode code points for things like `->`.
    public var unicodeTags: Bool
    /// `--deleted`: reads deleted readings as such, instead of as text.
    public var deleted: Bool
    /// `--no-break`: writes no extra whitespace.
    public var noBreak: Bool

    /// The options given, and every other left off.
    public init(
        input: StreamFormat? = nil,
        inAuto: Bool = false,
        addTags: Bool = false,
        output: StreamFormat? = nil,
        prefix: Unicode.Scalar? = nil,
        wfactor: Double? = nil,
        wtag: String? = nil,
        subDelim: String? = nil,
        rtl: Bool = false,
        ltr: Bool = false,
        ordered: Bool = false,
        parseDep: Bool = false,
        depDelimit: UInt32? = nil,
        unicodeTags: Bool = false,
        deleted: Bool = false,
        noBreak: Bool = false
    ) {
        self.input = input
        self.inAuto = inAuto
        self.addTags = addTags
        self.output = output
        self.prefix = prefix
        self.wfactor = wfactor
        self.wtag = wtag
        self.subDelim = subDelim
        self.rtl = rtl
        self.ltr = ltr
        self.ordered = ordered
        self.parseDep = parseDep
        self.depDelimit = depDelimit
        self.unicodeTags = unicodeTags
        self.deleted = deleted
        self.noBreak = noBreak
    }

    var bindings: CG3Bindings.ConvertOptions {
        CG3Bindings.ConvertOptions(
            input: input.map(\.bindings), inAuto: inAuto, addTags: addTags, output: output.map(\.bindings),
            prefix: prefix.map(String.init), wfactor: wfactor, wtag: wtag, subDelim: subDelim, ltr: ltr,
            ordered: ordered, parseDep: parseDep, depDelimit: depDelimit, unicodeTags: unicodeTags,
            deleted: deleted, noBreak: noBreak)
    }
}

// [spec:cg3:req:swift.tools]
/// A stream format cg-proc reads and writes, which `--stream-format` numbers
/// 0, 1 and 2. The binary format, 3, is left out: a string cannot carry it.
public enum ProcStreamFormat: String, Sendable, Hashable, CaseIterable {
    /// VISL CG, `--stream-format 0`.
    case cg
    /// Apertium's stream format, `--stream-format 1`, cg-proc's default.
    case apertium
    /// Matxin's, `--stream-format 2`.
    case matxin
}

// [spec:cg3:req:swift.tools]
/// cg-proc's options, each named after its long option in lowerCamelCase:
/// `--wordform-case` is `wordformCase: true`.
public struct ProcOptions: Sendable, Hashable {
    /// The stream format.
    public var streamFormat: ProcStreamFormat
    /// Run only the first this many sections.
    public var sections: UInt32?
    /// Run only the rules of this name.
    public var rule: String?
    /// `--trace`: prints which rules did what alongside the output.
    public var trace: Bool
    /// Give each baseform the case of its wordform.
    public var wordformCase: Bool
    /// Print no cohort's wordform.
    public var noWordForms: Bool
    /// No `^`/`$` around lexical units, and readings as surface forms.
    public var generation: Bool
    /// Print only the first of the readings left.
    public var first: Bool
    /// Disambiguate: the only thing cg-proc does, so it has no effect.
    public var disambiguation: Bool
    /// Flush on a null byte: cg-proc always does, so it has no effect.
    public var nullFlush: Bool

    /// The options given, and every other left off.
    public init(
        streamFormat: ProcStreamFormat = .apertium,
        sections: UInt32? = nil,
        rule: String? = nil,
        trace: Bool = false,
        wordformCase: Bool = false,
        noWordForms: Bool = false,
        generation: Bool = false,
        first: Bool = false,
        disambiguation: Bool = false,
        nullFlush: Bool = false
    ) {
        self.streamFormat = streamFormat
        self.sections = sections
        self.rule = rule
        self.trace = trace
        self.wordformCase = wordformCase
        self.noWordForms = noWordForms
        self.generation = generation
        self.first = first
        self.disambiguation = disambiguation
        self.nullFlush = nullFlush
    }

    var bindings: CG3Bindings.ProcRunOptions {
        let format: CG3Bindings.ProcStreamFormat =
            switch streamFormat {
            case .cg: .cg
            case .apertium: .apertium
            case .matxin: .matxin
            }
        return CG3Bindings.ProcRunOptions(
            streamFormat: format, sections: sections, rule: rule, trace: trace, wordformCase: wordformCase,
            noWordForms: noWordForms, generation: generation, first: first)
    }
}
