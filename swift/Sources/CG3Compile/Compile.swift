import Foundation
// [spec:cg3:req:swift.surface]
// CG3's types are this product's too: a compile takes LoadOptions and throws
// CG3Error. The generated bindings are imported internally in every file, so
// nothing they declare is public.
@_exported public import CG3
internal import CG3CompileBindings

// [spec:cg3:req:swift.compile]
/// A compiled grammar, as cg-comp writes it, and what its parse logged.
public struct CompileResult: Sendable, Hashable {
    /// The `.cg3b`, which `Grammar(cg3b:)` loads.
    public let cg3b: Data
    /// The companion file cg-comp writes beside the `.cg3b`, with the
    /// grammar's sources and where each rule was written in them.
    public let companion: Data?
    /// Every warning and error the parse logged, in order.
    public let warnings: [String]

    init(_ compiled: CG3CompileBindings.CompileResult) {
        cg3b = compiled.cg3b
        companion = compiled.companion
        warnings = compiled.warnings
    }
}

// [spec:cg3:req:swift.compile]
// [spec:cg3:req:swift.compile-includes]
/// Compiles the textual grammar `source` and writes it as cg-comp does.
///
/// - Parameters:
///   - source: The grammar's text.
///   - name: What diagnostics and the companion file call the source, and
///     what its relative `INCLUDE`s are resolved against;
///     `<utf8-memory>` when `nil`. Give the name cg-comp was given to get the
///     companion file it writes.
///   - files: The files the source's `INCLUDE`s may read, and no others, by
///     name. A name is resolved as vislcg3 resolves it on disk: beside the
///     source that includes it, else at the top of the names, with `./`,
///     `..` and repeated slashes read as a path reads them.
///   - options: The load options vislcg3 applies while it loads a grammar.
/// - Throws: `CG3Error` with code `compileFailed` for a grammar the library
///   will not compile, with every error the parse found in `diagnostics`; an
///   `INCLUDE` of a file `files` does not give is one of them, naming the
///   file. `invalidArgument` for a load option it cannot use.
public func compileGrammar(
    _ source: String, name: String? = nil, files: [String: String] = [:], options: CG3.LoadOptions = CG3.LoadOptions()
) throws -> CompileResult {
    let options = options.compile
    let compiled = try compiling(source, given: { files }) {
        try CG3CompileBindings.compileGrammar(source: source, name: name, files: files, options: options)
    }
    return CompileResult(compiled)
}

// [spec:cg3:req:swift.compile]
// [spec:cg3:req:swift.compile-includes]
/// Compiles the textual grammar `source` and writes it as cg-comp does, with
/// each file its `INCLUDE`s read asked of `files`.
///
/// - Parameters:
///   - source: The grammar's text.
///   - name: What diagnostics and the companion file call the source, and
///     what its relative `INCLUDE`s are resolved against;
///     `<utf8-memory>` when `nil`.
///   - files: Takes a name, resolved as vislcg3 resolves it on disk, and
///     returns the file's text, or `nil` for a file it does not have. It is
///     called while the compile runs, on the calling thread, and asked for
///     each file once.
///   - options: The load options vislcg3 applies while it loads a grammar.
/// - Throws: `CG3Error`, as `compileGrammar(_:name:files:options:)` with a
///   dictionary does.
public func compileGrammar(
    _ source: String, name: String? = nil, files: (String) -> String?, options: CG3.LoadOptions = CG3.LoadOptions()
) throws -> CompileResult {
    let options = options.compile
    return try withoutActuallyEscaping(files) { (files: @escaping (String) -> String?) throws -> CompileResult in
        let asked = Asked(files)
        let compiled = try compiling(source, given: { asked.given }) {
            try CG3CompileBindings.compileGrammarAsking(source: source, name: name, files: asked, options: options)
        }
        return CompileResult(compiled)
    }
}

// [spec:cg3:req:swift.compile]
/// The grammar in the `.cg3b` `cg3b` as text, as vislcg3 `--grammar-out`
/// writes it.
///
/// - Throws: `CG3Error` with code `invalidGrammar` for bytes that are not a
///   `.cg3b` the library reads.
public func grammarText(of cg3b: Data) throws -> String {
    try compiling(nil, given: { [:] }) { try CG3CompileBindings.grammarText(cg3b: cg3b) }
}

// [spec:cg3:req:swift.compile]
/// The `.cg3b` of the grammar in `cg3b` relabelled by the relabel rules
/// `relabels`, as cg-relabel writes it.
///
/// - Throws: `CG3Error` with code `compileFailed` for rules that do not
///   compile, with their diagnostics, and `invalidGrammar` for bytes that are
///   no `.cg3b` the library reads or rules it cannot relabel with.
public func relabelGrammar(_ cg3b: Data, with relabels: String) throws -> Data {
    try compiling(relabels, given: { [:] }) { try CG3CompileBindings.relabel(cg3b: cg3b, relabels: Data(relabels.utf8)) }
}

// [spec:cg3:req:swift.compile]
/// The `.cg3b` of the grammar in `cg3b` relabelled by the relabel rules in
/// `relabels`, read as cg-relabel reads its rule file: a `.cg3b`, or a
/// textual grammar in UTF-8. A diagnostic of rules given as bytes has no
/// `range`, since there is no string for it to lie in, but has its
/// `utf8Range`.
///
/// - Throws: `CG3Error` with code `compileFailed` for textual rules that do
///   not compile, with their diagnostics, and `invalidGrammar` for bytes or
///   rules the library cannot relabel with.
public func relabelGrammar(_ cg3b: Data, with relabels: Data) throws -> Data {
    try compiling(nil, given: { [:] }) { try CG3CompileBindings.relabel(cg3b: cg3b, relabels: relabels) }
}

/// What a closure gave for each name it was asked for, kept so a diagnostic
/// can lie in the string it gave. It lives only for the call it is made for,
/// on the calling thread.
final class Asked: CG3CompileBindings.IncludeFiles, @unchecked Sendable {
    private let files: (String) -> String?
    private(set) var given: [String: String] = [:]

    init(_ files: @escaping (String) -> String?) {
        self.files = files
    }

    func read(name: String) -> String? {
        let text = files(name)
        if let text { given[name] = text }
        return text
    }
}

// [spec:cg3:req:swift.compile-errors]
/// Runs `body`, a call into the generated bindings, with logging installed,
/// and throws what it throws as a `CG3Error`, placing each diagnostic in
/// `source`, the string the call compiled, or in the file of `given` it
/// names. `given` is read when the call has failed, since a closure's files
/// are given while it runs.
func compiling<T>(_ source: String?, given: () -> [String: String], _ body: () throws -> T) throws -> T {
    Logging.install()
    do {
        return try body()
    } catch let error as CG3CompileBindings.CompileError {
        switch error {
        case let .InvalidArgument(message):
            throw CG3Error(code: .invalidArgument, message: message)
        case let .InvalidGrammar(message):
            throw CG3Error(code: .invalidGrammar, message: message)
        case let .CompileFailed(message, diagnostics):
            let files = given()
            let placed = diagnostics.map { diagnostic in
                let text = diagnostic.file.map { files[$0] } ?? source
                return CG3.Diagnostic(diagnostic, in: text)
            }
            throw CG3Error(code: .compileFailed, message: message, diagnostics: placed)
        }
    } catch {
        throw CG3Error.panicked(error)
    }
}

extension CG3.Diagnostic {
    // [spec:cg3:req:swift.compile-errors]
    /// The diagnostic the library reported, with its UTF-8 span read as a
    /// range of `text`, the string the caller gave for its source, when there
    /// is one and the span lies on its scalars.
    init(_ diagnostic: CG3CompileBindings.Diagnostic, in text: String?) {
        let utf8 = diagnostic.span.map { Int($0.start)..<Int($0.end) }
        self.init(
            message: diagnostic.message, source: diagnostic.source, line: Int(diagnostic.line),
            text: diagnostic.text, file: diagnostic.file, range: utf8.flatMap { text?.range(utf8: $0) },
            utf8Range: utf8)
    }
}

extension String {
    /// The range of the characters the UTF-8 byte offsets `offsets` bound,
    /// or `nil` when they lie past the end or inside a scalar.
    func range(utf8 offsets: Range<Int>) -> Range<String.Index>? {
        let view = utf8
        guard offsets.upperBound <= view.count,
            let start = view.index(view.startIndex, offsetBy: offsets.lowerBound, limitedBy: view.endIndex),
            let end = view.index(view.startIndex, offsetBy: offsets.upperBound, limitedBy: view.endIndex),
            start.samePosition(in: unicodeScalars) != nil, end.samePosition(in: unicodeScalars) != nil
        else { return nil }
        return start..<end
    }
}

extension CG3.LoadOptions {
    var compile: CG3CompileBindings.LoadOptions {
        CG3CompileBindings.LoadOptions(
            nrules: nrules, nrulesV: nrulesV, vislcgCompat: vislcgCompat, prefix: prefix.map(String.init))
    }
}
