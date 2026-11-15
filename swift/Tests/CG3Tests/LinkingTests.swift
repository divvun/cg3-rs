import CG3
import CG3Compile
import Foundation
import LinkProbe
import Testing

/// CG3 and CG3Compile in one binary with a second Rust library, built on its
/// own by the same toolchain with the same uniffi and tracing, with LTO and
/// without the prelink, so std's symbols stay global in it.
struct LinkingTests {
    // [spec:cg3:req:swift.linking/test]
    @Test func cg3LinksBesideAnotherRustLibraryAndEachKeepsItsOwnState() throws {
        #expect(LinkProbe.wordCounts(text: "a b a") == ["a=2", "b=1"])
        let tally = Tally(words: ["x"])
        #expect(tally.add(word: "y") == 2)

        let compiled = try compileGrammar("DELIMITERS = \"<.>\" ;\nREMOVE (v) ;\n")
        let grammar = try Grammar(cg3b: compiled.cg3b)
        let input = "\"<w>\"\n\t\"w\" n\n\t\"w\" v\n"
        #expect(try grammar.apply(to: input).output == "\"<w>\"\n\t\"w\" n\n")

        // Each library dispatches to its own tracing: a run of cg3 that logs
        // a warning reaches nothing of the probe's.
        let seen = LinkProbe.eventsSeen()
        let warned = try grammar.apply(to: "word$ ^word/word<n>$\n", options: ApplyOptions(input: .apertium, output: .apertium))
        #expect(warned.warnings.count == 1)
        #expect(LinkProbe.eventsSeen() == seen)
        _ = LinkProbe.wordCounts(text: "c")
        #expect(LinkProbe.eventsSeen() == seen + 1)
    }

    // [spec:cg3:req:swift.linking/test]
    @Test func aPanicUnwindsInTheLibraryItHappensIn() throws {
        #expect(throws: (any Error).self) { try LinkProbe.panics(message: "unwound") }
        #expect(LinkProbe.wordCounts(text: "after") == ["after=1"])
        let error = try #require(throws: CG3Error.self) { try Grammar(cg3b: Data([1, 2, 3])) }
        #expect(error.code == .invalidGrammar)
    }
}
