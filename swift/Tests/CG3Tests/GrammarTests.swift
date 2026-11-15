import CG3
import Foundation
import Testing

/// A grammar's load, its runs and its failures, against the contract
/// grammars `js/test/fixtures.mjs` writes.
struct GrammarTests {
    let select: Data
    let external: Data
    let input: String

    init() throws {
        select = try Fixtures.data("contract/select.cg3b")
        external = try Fixtures.data("contract/external.cg3b")
        input = try Fixtures.text("contract/select.input")
    }

    // [spec:cg3:req:swift.grammar-load/test]
    @Test func aGrammarLoadsFromACG3BAndInfoIsTheLibrarysSummary() throws {
        let grammar = try Grammar(cg3b: select)
        let info = grammar.info
        #expect(info.sections == 1)
        #expect(info.templates == 0)
        #expect(info.rules == 4)
        #expect(info.sets == 7)
        #expect(info.tags == 30)
        #expect(info.rulesAny == 0)
        #expect(!info.hasDependencyRules)
        #expect(!info.hasRelationRules)
        let applied = try grammar.apply(to: input)
        #expect(!applied.output.isEmpty)
        #expect(applied.warnings.isEmpty)
    }

    // [spec:cg3:req:swift.apply/test]
    @Test func applyRunsTheGrammarAsVislcg3Does() throws {
        let grammar = try Grammar(cg3b: select)
        let word = "\"<word>\"\n\t\"word\" notwanted\n\t\"word\" wanted\n"
        #expect(try grammar.apply(to: word).output == "\"<word>\"\n\t\"word\" wanted\n")
        let traced = try grammar.apply(to: word, options: ApplyOptions(trace: true))
        #expect(traced.output.contains("SELECT:11"))
        let apertium = try grammar.apply(
            to: "^word/word<notwanted>/word<wanted>$\n", options: ApplyOptions(input: .apertium, output: .apertium))
        #expect(apertium.output == "^word/word<wanted>$\n")
    }

    // [spec:cg3:req:swift.apply/test]
    @Test func anOptionalArgumentIsOffOnOrAValue() {
        let off: OptionalArgument = false
        let on: OptionalArgument = true
        let value: OptionalArgument = "R1"
        #expect(off == .off)
        #expect(on == .on)
        #expect(value == .value("R1"))
        #expect(ApplyOptions().trace == .off)
    }

    // [spec:cg3:req:swift.errors/test]
    @Test(arguments: [Data([1, 2, 3]), Data()])
    func bytesThatAreNoCG3BThrowInvalidGrammar(bytes: Data) throws {
        let error = try #require(throws: CG3Error.self) { try Grammar(cg3b: bytes) }
        #expect(error.code == .invalidGrammar)
        #expect(!error.message.isEmpty)
        #expect(error.diagnostics.isEmpty)
    }

    // [spec:cg3:req:swift.errors/test]
    @Test func halfACG3BThrowsInvalidGrammar() throws {
        let half = select.prefix(select.count / 2)
        let error = try #require(throws: CG3Error.self) { try Grammar(cg3b: half) }
        #expect(error.code == .invalidGrammar)
    }

    // [spec:cg3:req:swift.load-options/test]
    @Test func aRuleFilterThatDoesNotCompileThrowsInvalidArgument() throws {
        let error = try #require(throws: CG3Error.self) {
            try Grammar(cg3b: select, options: LoadOptions(nrules: "("))
        }
        #expect(error.code == .invalidArgument)
        #expect(error.message.contains("--nrules"))
    }

    // [spec:cg3:req:swift.load-options/test]
    @Test func loadOptionsFilterTheRulesAsVislcg3Does() throws {
        let word = "\"<word>\"\n\t\"word\" notwanted\n\t\"word\" wanted\n"
        let filtered = try Grammar(cg3b: select, options: LoadOptions(nrules: "^nothing-is-called-this$"))
        #expect(try filtered.apply(to: word).output == word, "no rule is kept, so both readings stay")
        let mismatch = try #require(throws: CG3Error.self) {
            try Grammar(cg3b: select, options: LoadOptions(prefix: "%"))
        }
        #expect(mismatch.code == .invalidGrammar)
    }

    // [spec:cg3:req:swift.errors/test]
    // [spec:cg3:req:swift.external/test]
    @Test func aRunTheLibraryFailsThrowsRunFailedAndTheGrammarStaysUsable() throws {
        let grammar = try Grammar(cg3b: external)
        for _ in 0..<2 {
            let error = try #require(throws: CG3Error.self) { try grammar.apply(to: input) }
            #expect(error.code == .runFailed)
            // macOS starts the rule's shell, which finds no program and
            // answers with what the protocol does not expect; iOS starts
            // nothing.
            #expect(error.message.contains("EXTERNAL"), "\(error.message)")
            #if os(iOS) && !targetEnvironment(simulator)
                #expect(error.message.contains("could not be started"), "\(error.message)")
            #endif
        }
        let procError = try #require(throws: CG3Error.self) {
            try grammar.proc(input, options: ProcOptions(streamFormat: .cg))
        }
        #expect(procError.code == .runFailed)
        #expect(try !Grammar(cg3b: select).apply(to: input).output.isEmpty)
    }

    // [spec:cg3:req:swift.tools/test]
    @Test func convertMwesplitAndProcWriteWhatTheToolsWrite() throws {
        let apertium = "^w/a<n>+b<v>$\n"
        #expect(try convert(apertium, options: ConvertOptions(input: .apertium)) == "\"<w>\"\n\t\"b\" v\n\t\t\"a\" n\n\n")
        #expect(
            try convert(apertium, options: ConvertOptions(input: .apertium, ltr: true))
                == "\"<w>\"\n\t\"a\" n\n\t\t\"b\" v\n\n")
        let multiword = "\"<dalle go>\"\n\t\"go\" CS <W:0> \"<go>\"\n\t\t\"dalle\" Adv <W:0> \"<dalle >\"\n"
        #expect(try mwesplit(multiword) == "\"<dalle>\"\n\t\"dalle\" Adv <W:0>\n: \n\"<go>\"\n\t\"go\" CS <W:0>\n\n")
        let grammar = try Grammar(cg3b: select)
        let word = "\"<word>\"\n\t\"word\" notwanted\n\t\"word\" wanted\n"
        #expect(try grammar.proc(word, options: ProcOptions(streamFormat: .cg)) == "\"<word>\"\n\t\"word\" wanted\n")
        let lexical = "^word/word<notwanted>/word<wanted>$\n"
        #expect(try grammar.proc(lexical) == "^word/word<wanted>$\n")
        #expect(
            try grammar.proc(lexical, options: ProcOptions(trace: true))
                == "^word/word<wanted><SELECT:11>/\u{00AC}word<notwanted><SELECT:11>$\n")
    }

    // [spec:cg3:req:swift.tools/test]
    // [spec:cg3:req:swift.errors/test]
    @Test func aToolTheLibraryFailsThrowsRunFailed() throws {
        let reserved = "{\"w\": \"a\", \"rs\": [{\"l\": \"a\", \"ts\": [\"n\"]}], \"ds\": 4294967295}\n"
        let error = try #require(throws: CG3Error.self) { try convert(reserved, options: ConvertOptions(input: .jsonl)) }
        #expect(error.code == .runFailed)
        #expect(!error.message.isEmpty)
    }

    // [spec:cg3:req:swift.concurrency/test]
    @Test func threadsApplyOneGrammarAtOnceEachARunOfItsOwn() async throws {
        let grammar = try Grammar(cg3b: select)
        let expected = try grammar.apply(to: input).output
        let outputs = try await withThrowingTaskGroup(of: String.self) { group in
            for _ in 0..<16 {
                group.addTask { try grammar.apply(to: input).output }
            }
            return try await group.reduce(into: []) { $0.append($1) }
        }
        #expect(outputs.count == 16)
        #expect(outputs.allSatisfy { $0 == expected })
    }
}

/// The codes `CG3Error` carries.
struct ErrorTests {
    // [spec:cg3:req:swift.errors/test]
    @Test func theCodesAreTheJavaScriptPackagesAndAPanicIsPanicked() throws {
        let errorsTS = try String(
            contentsOf: Fixtures.root.appendingPathComponent("js/cg3/errors.ts"), encoding: .utf8)
        for code in CG3Error.Code.allCases where code != .panicked {
            #expect(errorsTS.contains("\"\(code.rawValue)\""), "js/cg3/errors.ts lists \(code.rawValue)")
        }
        #expect(CG3Error.Code.panicked.rawValue == "panicked")
    }

    @Test func anErrorDescribesItselfWithItsCode() throws {
        let error = try #require(throws: CG3Error.self) { try Grammar(cg3b: Data([0])) }
        #expect(error.description.hasPrefix("invalid-grammar: "))
        #expect(error.localizedDescription == error.message)
    }

    // [spec:cg3:req:swift.package/test]
    @Test func theVersionIsTheOneCratesCG3SwiftCarries() throws {
        let manifest = try String(
            contentsOf: Fixtures.root.appendingPathComponent("crates/cg3-swift/Cargo.toml"), encoding: .utf8)
        #expect(manifest.contains("\nversion = \"\(version)\"\n"))
    }
}
