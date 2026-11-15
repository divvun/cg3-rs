import CG3Compile
import Foundation
import Testing

/// Compiling, the writers and compile errors, against the contract grammars
/// `js/test/fixtures.mjs` writes with cg-comp.
struct CompileTests {
    let select: String
    let selectCG3B: Data
    let selectCompanion: Data
    let include: String
    let sets: String
    let rules: String
    let includeCG3B: Data
    let includeCompanion: Data
    let input: String

    init() throws {
        select = try Fixtures.text("contract/select.cg3")
        selectCG3B = try Fixtures.data("contract/select.cg3b")
        selectCompanion = try Fixtures.data("contract/select.cg3b.cg3src")
        include = try Fixtures.text("contract/include-grammar.cg3")
        sets = try Fixtures.text("contract/include-sets.cg3")
        rules = try Fixtures.text("contract/include-rules.cg3")
        includeCG3B = try Fixtures.data("contract/include.cg3b")
        includeCompanion = try Fixtures.data("contract/include.cg3b.cg3src")
        input = try Fixtures.text("contract/select.input")
    }

    // [spec:cg3:req:swift.compile/test]
    @Test func compileGrammarWritesWhatCGCompWritesWhichGrammarLoads() throws {
        let result = try compileGrammar(select, name: "grammar.cg3")
        #expect(result.cg3b == selectCG3B)
        #expect(result.companion == selectCompanion)
        #expect(result.warnings.isEmpty)
        let compiled = try Grammar(cg3b: result.cg3b)
        #expect(try compiled.apply(to: input) == Grammar(cg3b: selectCG3B).apply(to: input))
    }

    // [spec:cg3:req:swift.compile-includes/test]
    @Test func includesFromADictionaryOrAClosureCompileAsCGCompCompilesThemFromDisk() throws {
        for files in [["sets.cg3": sets, "rules.cg3": rules], ["./sets.cg3": sets, "rules.cg3": rules]] {
            let result = try compileGrammar(include, name: "grammar.cg3", files: files)
            #expect(result.cg3b == includeCG3B)
            #expect(result.companion == includeCompanion)
        }
        var asked: [String] = []
        let result = try compileGrammar(include, name: "grammar.cg3") { name in
            asked.append(name)
            return ["sets.cg3": sets, "rules.cg3": rules][name]
        }
        #expect(result.cg3b == includeCG3B)
        #expect(result.companion == includeCompanion)
        #expect(asked == ["sets.cg3", "rules.cg3"])
    }

    // [spec:cg3:req:swift.compile-includes/test]
    // [spec:cg3:req:swift.compile-errors/test]
    @Test func anIncludeTheFilesDoNotGiveFailsTheCompileNamingTheFile() throws {
        let cases: [(String, () throws -> CompileResult)] = [
            ("sets.cg3", { try compileGrammar(include, name: "grammar.cg3") }),
            ("rules.cg3", { try compileGrammar(include, name: "grammar.cg3", files: ["sets.cg3": sets]) }),
            ("sets.cg3", { try compileGrammar(include, name: "grammar.cg3", files: { _ in nil }) }),
        ]
        for (missing, compile) in cases {
            let error = try #require(throws: CG3Error.self) { try compile() }
            #expect(error.code == .compileFailed)
            #expect(error.message.contains(missing), "\(error.message)")
            #expect(error.diagnostics.contains { $0.text.contains(missing) })
        }
    }

    // [spec:cg3:req:swift.compile-errors/test]
    @Test func aFailedCompileCarriesEveryErrorPlacedInTheCallersString() throws {
        let source = "DELIMITERS = \"<.>\" ;\n# \u{1F600}\nSELECT (a ;\nLIST X = ;\n"
        let error = try #require(throws: CG3Error.self) { try compileGrammar(source, name: "broken.cg3") }
        #expect(error.code == .compileFailed)
        #expect(error.diagnostics.count >= 2)
        for diagnostic in error.diagnostics {
            #expect(diagnostic.source == "broken.cg3")
            #expect(diagnostic.file == nil)
            #expect(!diagnostic.text.contains("\u{1B}"), "no colour")
            #expect(diagnostic.text.contains(diagnostic.message))
            #expect(error.message.contains(diagnostic.message))
        }
        let first = try #require(error.diagnostics.first)
        #expect(first.line == 3)
        let range = try #require(first.range)
        let utf8 = try #require(first.utf8Range)
        let marked = source[range].trimmingCharacters(in: .whitespaces)
        #expect(!marked.isEmpty && source.split(separator: "\n")[2].contains(marked), "\(marked)")
        #expect(source.utf8.distance(from: source.startIndex, to: range.lowerBound) == utf8.lowerBound)
        #expect(source.utf8.distance(from: source.startIndex, to: range.upperBound) == utf8.upperBound)
    }

    // [spec:cg3:req:swift.compile-errors/test]
    @Test func aDiagnosticInAnIncludedFileLiesInTheStringGivenForIt() throws {
        let broken = "LIST N = n ;\nSELECT (a ;\n"
        let dictionary = try #require(throws: CG3Error.self) {
            try compileGrammar("INCLUDE ./lists/sets.cg3 ;\n", name: "grammar.cg3", files: ["lists//sets.cg3": broken])
        }
        let asked = try #require(throws: CG3Error.self) {
            try compileGrammar("INCLUDE ./lists/sets.cg3 ;\n", name: "grammar.cg3") { $0 == "lists/sets.cg3" ? broken : nil }
        }
        for (error, file) in [(dictionary, "lists//sets.cg3"), (asked, "lists/sets.cg3")] {
            #expect(error.code == .compileFailed)
            let diagnostic = try #require(error.diagnostics.first { $0.range != nil })
            #expect(diagnostic.file == file)
            #expect(diagnostic.line == 2)
            let range = try #require(diagnostic.range)
            #expect(broken[range].contains("SELECT") || broken.split(separator: "\n")[1].contains(broken[range]))
        }
    }

    // [spec:cg3:req:swift.compile/test]
    // [spec:cg3:req:swift.load-options/test]
    @Test func compileGrammarReturnsTheParsesWarningsAndTakesTheLoadOptions() throws {
        let warned = try compileGrammar("DELIMITERS = \"<.>\" ;\nSELECT: (a) ;\nREMOVE: (b) ;\n")
        #expect(warned.warnings.count == 2)
        #expect(warned.warnings.allSatisfy { $0.contains("Rule had : but no name") })
        let mapped = "DELIMITERS = \"<.>\" ;\nMAP (%x) (a) ;\n"
        let cg3b = try compileGrammar(mapped, options: LoadOptions(prefix: "%")).cg3b
        #expect(try Grammar(cg3b: cg3b, options: LoadOptions(prefix: "%")).info.rules == 1)
        let other = try #require(throws: CG3Error.self) { try Grammar(cg3b: cg3b, options: LoadOptions(prefix: "@")) }
        #expect(other.code == .invalidGrammar)
        let filter = try #require(throws: CG3Error.self) { try compileGrammar(mapped, options: LoadOptions(nrulesV: "(")) }
        #expect(filter.code == .invalidArgument)
    }

    // [spec:cg3:req:swift.compile/test]
    @Test func grammarTextWritesACG3BAsTextThatCompilesToAGrammarThatRunsAsItDoes() throws {
        let written = try grammarText(of: selectCG3B)
        #expect(written.contains("SELECT"))
        let again = try Grammar(cg3b: compileGrammar(written).cg3b)
        #expect(try again.apply(to: input) == Grammar(cg3b: selectCG3B).apply(to: input))
        let error = try #require(throws: CG3Error.self) { try grammarText(of: Data([1, 2, 3])) }
        #expect(error.code == .invalidGrammar)
    }

    // [spec:cg3:req:swift.compile/test]
    @Test func relabelTakesItsRulesAsTextOrAsACG3B() throws {
        let rules = "LIST notwanted = notwanted ;\nMAP (wanted) (notwanted) ;\n"
        let fromText = try relabelGrammar(selectCG3B, with: rules)
        #expect(try relabelGrammar(selectCG3B, with: compileGrammar(rules).cg3b) == fromText)
        #expect(try relabelGrammar(selectCG3B, with: Data(rules.utf8)) == fromText)
        #expect(try Grammar(cg3b: fromText).info.rules > 0)
        let broken = "MAP (a ;"
        let error = try #require(throws: CG3Error.self) { try relabelGrammar(selectCG3B, with: broken) }
        #expect(error.code == .compileFailed)
        #expect(!error.diagnostics.isEmpty)
        #expect(error.diagnostics.contains { $0.range.map { broken[$0].isEmpty == false } ?? false })
        let bytes = try #require(throws: CG3Error.self) { try relabelGrammar(selectCG3B, with: Data(broken.utf8)) }
        #expect(bytes.diagnostics.allSatisfy { $0.range == nil })
        let none = try #require(throws: CG3Error.self) { try relabelGrammar(Data([1]), with: rules) }
        #expect(none.code == .invalidGrammar)
    }
}
