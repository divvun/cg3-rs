import Foundation
// [spec:cg3:req:swift.surface]
// The generated bindings are imported internally in every file, so nothing
// they declare is public in CG3.
internal import CG3Bindings

// [spec:cg3:req:swift.concurrency]
// [spec:cg3:req:swift.grammar-load]
/// A compiled grammar, loaded once and applied any number of times. It does
/// not change, so threads may apply one grammar at once: each apply is a run
/// of its own.
public final class Grammar: Sendable {
    private let inner: CG3Bindings.Grammar

    // [spec:cg3:req:swift.grammar-load]
    // [spec:cg3:req:swift.load-options]
    /// Loads a compiled grammar from the bytes of a `.cg3b`, as cg-comp
    /// writes it, under the load options `options` gives.
    ///
    /// - Throws: `CG3Error` with code `invalidGrammar` for bytes that are not
    ///   a `.cg3b` the library reads, and `invalidArgument` for a rule filter
    ///   that does not compile.
    public init(cg3b: Data, options: LoadOptions = LoadOptions()) throws {
        inner = try calling { try CG3Bindings.Grammar(cg3b: cg3b, options: options.bindings) }
    }

    // [spec:cg3:req:swift.grammar-load]
    /// What vislcg3's verbose summary reports about the grammar.
    public var info: GrammarInfo {
        GrammarInfo(inner.info())
    }

    // [spec:cg3:req:swift.apply]
    // [spec:cg3:req:swift.errors]
    /// Runs the grammar on `input`, as vislcg3 runs it on one standard input,
    /// and returns the output with every warning and non-fatal error the run
    /// logged. Each call is a run of its own.
    ///
    /// - Throws: `CG3Error` with code `runFailed` for a run the library
    ///   reports as failed; the grammar stays usable.
    public func apply(to input: String, options: ApplyOptions = ApplyOptions()) throws -> ApplyResult {
        let applied = try calling { try inner.apply(input: input, options: options.bindings) }
        return ApplyResult(output: applied.output, warnings: applied.warnings)
    }

    // [spec:cg3:req:swift.tools]
    /// Runs the grammar on `input` as cg-proc does, and returns what cg-proc
    /// writes. The options the grammar sets with `CMDARGS` apply as they do
    /// in cg-proc.
    ///
    /// - Throws: `CG3Error` with code `runFailed` for a run the library
    ///   reports as failed; the grammar stays usable.
    public func proc(_ input: String, options: ProcOptions = ProcOptions()) throws -> String {
        try calling { try inner.proc(input: input, options: options.bindings) }
    }
}

// [spec:cg3:req:swift.grammar-load]
/// What vislcg3's verbose summary reports about a loaded grammar.
public struct GrammarInfo: Sendable, Hashable {
    /// The grammar's sections.
    public let sections: Int
    /// Its templates.
    public let templates: Int
    /// Its rules.
    public let rules: Int
    /// Its sets.
    public let sets: Int
    /// Its tags.
    public let tags: Int
    /// The rules no tag index can skip.
    public let rulesAny: Int
    /// Whether it has dependency rules.
    public let hasDependencyRules: Bool
    /// Whether it has relation rules.
    public let hasRelationRules: Bool

    init(_ info: CG3Bindings.GrammarInfo) {
        sections = Int(info.sections)
        templates = Int(info.templates)
        rules = Int(info.rules)
        sets = Int(info.sets)
        tags = Int(info.tags)
        rulesAny = Int(info.rulesAny)
        hasDependencyRules = info.hasDependencyRules
        hasRelationRules = info.hasRelationRules
    }
}

// [spec:cg3:req:swift.apply]
/// A run's output, and every warning and non-fatal error it logged, in order.
public struct ApplyResult: Sendable, Hashable {
    /// What the run wrote.
    public let output: String
    /// Every warning and non-fatal error the run logged, in order, whatever
    /// `logLevel` is.
    public let warnings: [String]
}
