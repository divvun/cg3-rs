import CG3
import CG3Compile
import Foundation
import Testing

/// A value in the fixtures' JSON: the options are `@divvun/cg3`'s, written
/// for JavaScript, which takes a flag as `true` and a value as a string or a
/// number.
enum JSONValue: Decodable, Sendable, Equatable {
    case bool(Bool)
    case number(Double)
    case string(String)

    init(from decoder: any Decoder) throws {
        let value = try decoder.singleValueContainer()
        if let flag = try? value.decode(Bool.self) {
            self = .bool(flag)
        } else if let number = try? value.decode(Double.self) {
            self = .number(number)
        } else {
            self = .string(try value.decode(String.self))
        }
    }

    var flag: Bool { self == .bool(true) }

    var text: String? {
        if case let .string(text) = self { return text }
        return nil
    }

    var count: UInt32? {
        if case let .number(number) = self { return UInt32(exactly: number) }
        return nil
    }

    var argument: OptionalArgument {
        switch self {
        case let .bool(on): on ? .on : .off
        case let .number(number): .value(String(Int(number)))
        case let .string(value): .value(value)
        }
    }
}

/// The options a fixture gives, read as the package's: each key the
/// property of the same name. A key the package has no option for is named,
/// as `@divvun/cg3` refuses it.
struct Options<Value> {
    var value: Value
    var unknown: [String] = []

    init(_ value: Value, _ given: [String: JSONValue], _ set: (inout Value, String, JSONValue) -> Bool) {
        self.value = value
        for (key, option) in given.sorted(by: { $0.key < $1.key }) where !set(&self.value, key, option) {
            unknown.append(key)
        }
    }
}

func streamFormat(_ value: JSONValue) -> StreamFormat? {
    value.text.flatMap(StreamFormat.init(rawValue:))
}

func applyOptions(_ given: [String: JSONValue]) -> Options<ApplyOptions> {
    Options(ApplyOptions(), given) { options, key, value in
        switch key {
        case "input": options.input = streamFormat(value)
        case "output": options.output = streamFormat(value)
        case "ordered": options.ordered = value.flag
        case "unsafe": options.unsafe = value.flag
        case "sections": options.sections = value.text
        case "rules": options.rules = value.text
        case "rule": options.rule = value.text
        case "debugRules": options.debugRules = value.text
        case "verbose": options.verbose = value.argument
        case "noMappings": options.noMappings = value.flag
        case "noCorrections": options.noCorrections = value.flag
        case "noBeforeSections": options.noBeforeSections = value.flag
        case "noSections": options.noSections = value.flag
        case "noAfterSections": options.noAfterSections = value.flag
        case "trace": options.trace = value.argument
        case "traceNameOnly": options.traceNameOnly = value.flag
        case "traceNoRemoved": options.traceNoRemoved = value.flag
        case "traceEncl": options.traceEncl = value.flag
        case "deleted": options.deleted = value.flag
        case "singleRun": options.singleRun = value.flag
        case "maxRuns": options.maxRuns = value.count
        case "unicodeTags": options.unicodeTags = value.flag
        case "uniqueTags": options.uniqueTags = value.flag
        case "printIds": options.printIds = value.flag
        case "printDep": options.printDep = value.flag
        case "numWindows": options.numWindows = value.count
        case "alwaysSpan": options.alwaysSpan = value.flag
        case "softLimit": options.softLimit = value.count
        case "hardLimit": options.hardLimit = value.count
        case "textDelimit": options.textDelimit = value.argument
        case "depDelimit": options.depDelimit = value.argument
        case "depAbsolute": options.depAbsolute = value.flag
        case "depOriginal": options.depOriginal = value.flag
        case "depAllowLoops": options.depAllowLoops = value.flag
        case "depNoCrossing": options.depNoCrossing = value.flag
        case "noMagicReadings": options.noMagicReadings = value.flag
        case "noPassOrigin": options.noPassOrigin = value.flag
        case "splitMappings": options.splitMappings = value.flag
        case "showEndTags": options.showEndTags = value.flag
        case "noBreak": options.noBreak = value.flag
        default: return false
        }
        return true
    }
}

func loadOptions(_ given: [String: JSONValue]) -> Options<LoadOptions> {
    Options(LoadOptions(), given) { options, key, value in
        switch key {
        case "nrules": options.nrules = value.text
        case "nrulesV": options.nrulesV = value.text
        case "vislcgCompat": options.vislcgCompat = value.flag
        case "prefix": options.prefix = value.text?.unicodeScalars.first
        default: return false
        }
        return true
    }
}

func convertOptions(_ given: [String: JSONValue]) -> Options<ConvertOptions> {
    Options(ConvertOptions(), given) { options, key, value in
        switch key {
        case "input": options.input = streamFormat(value)
        case "output": options.output = streamFormat(value)
        case "inAuto": options.inAuto = value.flag
        case "addTags": options.addTags = value.flag
        case "prefix": options.prefix = value.text?.unicodeScalars.first
        case "wfactor": if case let .number(factor) = value { options.wfactor = factor }
        case "wtag": options.wtag = value.text
        case "subDelim": options.subDelim = value.text
        case "rtl": options.rtl = value.flag
        case "ltr": options.ltr = value.flag
        case "ordered": options.ordered = value.flag
        case "parseDep": options.parseDep = value.flag
        // `true` is the option without a value, which cg-conv reads as 10.
        case "depDelimit": options.depDelimit = value.flag ? 10 : value.count
        case "unicodeTags": options.unicodeTags = value.flag
        case "deleted": options.deleted = value.flag
        case "noBreak": options.noBreak = value.flag
        default: return false
        }
        return true
    }
}

func procOptions(_ given: [String: JSONValue]) -> Options<ProcOptions> {
    Options(ProcOptions(), given) { options, key, value in
        switch key {
        case "streamFormat": options.streamFormat = value.text.flatMap(ProcStreamFormat.init(rawValue:)) ?? .apertium
        case "sections": options.sections = value.count
        case "rule": options.rule = value.text
        case "trace": options.trace = value.flag
        case "wordformCase": options.wordformCase = value.flag
        case "noWordForms": options.noWordForms = value.flag
        case "generation": options.generation = value.flag
        case "first": options.first = value.flag
        case "disambiguation": options.disambiguation = value.flag
        case "nullFlush": options.nullFlush = value.flag
        default: return false
        }
        return true
    }
}

/// Where `actual` first differs from `expected`, or `nil` when they are the
/// same bytes.
func difference(_ actual: Data, _ expected: Data) -> String? {
    if actual == expected { return nil }
    let at = zip(actual, expected).prefix { $0 == $1 }.count
    let around = { (bytes: Data) in String(decoding: bytes.dropFirst(max(0, at - 30)).prefix(80), as: UTF8.self) }
    return "byte \(at) of \(actual.count) differs from \(expected.count): \(around(actual).debugDescription), expected \(around(expected).debugDescription)"
}

/// What a call gave: its bytes, or the error it threw.
enum Outcome {
    case wrote(Data)
    case threw(any Error)

    init(_ body: () throws -> Data) {
        do { self = .wrote(try body()) } catch { self = .threw(error) }
    }

    /// Whether this is what the tool did: the same bytes, or a `CG3Error`
    /// where it failed.
    func check(failed: Bool, expected: Data?, _ label: String) {
        switch self {
        case let .threw(error):
            #expect(error is CG3Error, "\(label): threw \(error)")
            #expect(failed, "\(label): the tool succeeded, and the package threw \(error)")
        case let .wrote(bytes):
            #expect(!failed, "\(label): the tool failed, and the package succeeded")
            if let expected, !failed {
                let found = difference(bytes, expected)
                #expect(found == nil, "\(label): \(found ?? "")")
            }
        }
    }
}

/// One corpus test as `parity.json` describes it.
struct CorpusCase: Decodable, Sendable, CustomTestStringConvertible {
    let name: String
    let exempt: String?
    let args: [String]?
    let options: [String: JSONValue]?
    let load: [String: JSONValue]?
    let nativeFailed: Bool?
    let compileError: String?

    var testDescription: String { name }

    /// Why the case is exempt: as `parity.json` names it, or an option the
    /// package does not offer, as `@divvun/cg3` names it.
    var exemption: String? {
        if let exempt { return exempt }
        let unknown = applyOptions(options ?? [:]).unknown + loadOptions(load ?? [:]).unknown
        if unknown.isEmpty { return nil }
        return "an option the package does not offer (\((args ?? []).joined(separator: " ")))"
    }

    static let all: [CorpusCase] = {
        guard let data = try? Fixtures.data("parity/parity.json"),
            let manifest = try? JSONDecoder().decode([String: [CorpusCase]].self, from: data)
        else { return [] }
        return manifest["tests"] ?? []
    }()
}

/// `CG3` against vislcg3 on the golden corpus: each corpus grammar compiled
/// to `.cg3b` by cg-comp, its input, and what vislcg3 wrote for them, as
/// `js/test/fixtures.mjs` writes them for `@divvun/cg3`.
struct CorpusParityTests {
    // [spec:cg3:req:swift.parity/test]
    @Test(arguments: CorpusCase.all.filter { $0.exemption == nil })
    func cg3GivesVislcg3sBytes(_ test: CorpusCase) throws {
        #expect(test.compileError == nil, "cg-comp failed: \(test.compileError ?? "")")
        let cg3b = try Fixtures.data("parity/\(test.name).cg3b")
        let input = String(decoding: try Fixtures.data("parity/\(test.name).input"), as: UTF8.self)
        let expected = try Fixtures.data("parity/\(test.name).expected")
        let outcome = Outcome {
            let grammar = try Grammar(cg3b: cg3b, options: loadOptions(test.load ?? [:]).value)
            return Data(try grammar.apply(to: input, options: applyOptions(test.options ?? [:]).value).output.utf8)
        }
        outcome.check(failed: test.nativeFailed ?? false, expected: expected, test.name)
    }

    // [spec:cg3:req:swift.parity/test]
    @Test func theCorpusIsThereAndTheExemptionsAreTheNamedOnes() {
        let exempt = CorpusCase.all.compactMap { test in test.exemption.map { "\(test.name): \($0)" } }
        for line in exempt { print("exempt \(line)") }
        #expect(CorpusCase.all.count > 60, "parity.json lists the corpus")
        #expect(exempt == ["T_External: an EXTERNAL rule, whose process a target without processes cannot start"])
    }
}

/// A corpus grammar as `tools.json` describes it.
struct CompileCase: Decodable, Sendable, CustomTestStringConvertible {
    let name: String
    let source: String
    let files: [String: String]
    let failed: Bool
    let exempt: String?
    let cg3b: String?
    let companion: String?
    let text: String?

    var testDescription: String { name }
}

/// One tool's case as `tools.json` describes it.
struct ToolCase: Decodable, Sendable, CustomTestStringConvertible {
    let tool: String?
    let label: String
    let exempt: String?
    let failed: Bool?
    let expected: String?
    let input: String?
    let grammar: String?
    let rules: String?
    let options: [String: JSONValue]?

    var testDescription: String { label }
}

struct ToolManifest: Decodable {
    let compile: [CompileCase]
    let tools: [ToolCase]

    static let shared: ToolManifest? = {
        guard let data = try? Fixtures.data("tools/tools.json") else { return nil }
        return try? JSONDecoder().decode(ToolManifest.self, from: data)
    }()
}

/// `CG3Compile` against cg-comp and vislcg3 `--grammar-out`, and `CG3`
/// against cg-conv, cg-mwesplit and cg-proc, and `CG3Compile` against
/// cg-relabel, on the cases `js/test/tool-fixtures.mjs` writes.
struct ToolParityTests {
    // [spec:cg3:req:swift.compile-parity/test]
    @Test(arguments: (ToolManifest.shared?.compile ?? []).filter { $0.exempt == nil })
    func compilingWritesWhatCGCompAndGrammarOutWrite(_ grammar: CompileCase) throws {
        let label = "cg-comp \(grammar.name)/grammar.cg3"
        let source = try Fixtures.text("tools/\(grammar.source)")
        var files: [String: String] = [:]
        for (name, path) in grammar.files {
            files[name] = try Fixtures.text("tools/\(path)")
        }
        let expected = try grammar.cg3b.map { try Fixtures.data("tools/\($0)") }
        var result: CompileResult?
        Outcome {
            let compiled = try compileGrammar(source, name: "grammar.cg3", files: files)
            result = compiled
            return compiled.cg3b
        }.check(failed: grammar.failed, expected: expected, label)
        guard let result, !grammar.failed else { return }
        let companion = try grammar.companion.map { try Fixtures.data("tools/\($0)") }
        #expect(result.companion == companion, "\(label): its companion")
        let text = try grammar.text.map { try Fixtures.data("tools/\($0)") }
        Outcome { Data(try grammarText(of: result.cg3b).utf8) }
            .check(failed: false, expected: text, "vislcg3 --grammar-only --grammar-out of \(grammar.name)")
    }

    // [spec:cg3:req:swift.compile-parity/test]
    @Test(arguments: (ToolManifest.shared?.tools ?? []).filter { $0.exempt == nil })
    func eachToolsCaseGivesTheToolsBytes(_ test: ToolCase) throws {
        let expected = try test.expected.map { try Fixtures.data("tools/\($0)") }
        let input = try test.input.map { String(decoding: try Fixtures.data("tools/\($0)"), as: UTF8.self) } ?? ""
        let given = test.options ?? [:]
        let outcome: Outcome
        switch test.tool {
        case "cg-conv":
            let options = convertOptions(given)
            #expect(options.unknown.isEmpty, "\(test.label): \(options.unknown)")
            outcome = Outcome { Data(try convert(input, options: options.value).utf8) }
        case "cg-mwesplit":
            outcome = Outcome { Data(try mwesplit(input).utf8) }
        case "cg-proc":
            let options = procOptions(given)
            #expect(options.unknown.isEmpty, "\(test.label): \(options.unknown)")
            let cg3b = try Fixtures.data("tools/\(test.grammar ?? "")")
            outcome = Outcome { Data(try Grammar(cg3b: cg3b).proc(input, options: options.value).utf8) }
        case "cg-relabel":
            let cg3b = try Fixtures.data("tools/\(test.grammar ?? "")")
            let rules = try Fixtures.data("tools/\(test.rules ?? "")")
            outcome = Outcome { try relabelGrammar(cg3b, with: rules) }
        default:
            Issue.record("\(test.label): a tool the check does not know")
            return
        }
        outcome.check(failed: test.failed ?? false, expected: expected, test.label)
    }

    // [spec:cg3:req:swift.compile-parity/test]
    @Test func theCasesAreThereAndTheExemptionsAreTheNamedOnes() throws {
        let manifest = try #require(ToolManifest.shared, "tools/tools.json is in the fixtures")
        let reasons = Set(manifest.compile.compactMap(\.exempt) + manifest.tools.compactMap(\.exempt))
        for line in manifest.compile.compactMap({ c in c.exempt.map { "cg-comp \(c.name): \($0)" } }) { print("exempt \(line)") }
        for line in manifest.tools.compactMap({ t in t.exempt.map { "\(t.label): \($0)" } }) { print("exempt \(line)") }
        #expect(manifest.compile.count > 60)
        #expect(manifest.tools.count > 200)
        #expect(
            reasons.isSubset(of: [
                "a binary stream, which a string cannot carry",
                "an input that is not UTF-8, which a string cannot carry",
                "a source that is not UTF-8, which a string cannot carry",
            ]), "\(reasons)")
    }
}
