import Foundation
internal import CG3Bindings

// [spec:cg3:req:swift.errors]
/// An error from the package. `code` says what went wrong, in a form code can
/// test; `message` says it in the library's words.
public struct CG3Error: Error, Sendable, Hashable, CustomStringConvertible, LocalizedError {
    /// What went wrong. The raw values are `@divvun/cg3`'s codes, which do not
    /// change between releases.
    public enum Code: String, Sendable, Hashable, CaseIterable {
        /// An argument or option the library cannot use, such as a rule
        /// filter that does not compile.
        case invalidArgument = "invalid-argument"
        /// Bytes that are not a `.cg3b` the library reads.
        case invalidGrammar = "invalid-grammar"
        /// A run, conversion or split the library reports as failed, such as
        /// one whose `EXTERNAL` process cannot start. The grammar stays
        /// usable.
        case runFailed = "run-failed"
        /// A textual grammar `CG3Compile` could not compile; `diagnostics`
        /// holds every error the parse found.
        case compileFailed = "compile-failed"
        /// The library panicked, which is a bug in it. The call failed and
        /// changed nothing.
        case panicked
    }

    /// What went wrong.
    public let code: Code
    /// What went wrong, in the library's words.
    public let message: String
    /// Every error a failed compile's parse found, for `compileFailed`; empty
    /// for any other code.
    public let diagnostics: [Diagnostic]

    package init(code: Code, message: String, diagnostics: [Diagnostic] = []) {
        self.code = code
        self.message = message
        self.diagnostics = diagnostics
    }

    /// The code and the message.
    public var description: String {
        "\(code.rawValue): \(message)"
    }

    /// The message.
    public var errorDescription: String? {
        message
    }

    // [spec:cg3:req:swift.errors]
    /// The error for anything the generated bindings threw: `panicked` for
    /// what UniFFI throws when the library panics, and for any other error
    /// they throw, which only a bug can make.
    package static func panicked(_ error: any Error) -> CG3Error {
        let message = (error as? LocalizedError)?.errorDescription ?? String(describing: error)
        return CG3Error(code: .panicked, message: message)
    }
}

// [spec:cg3:req:swift.compile-errors]
/// One error a failed parse found.
public struct Diagnostic: Sendable, Hashable {
    /// What went wrong, in the library's words.
    public let message: String
    /// The name of the source it is in: the compiled source's, or an
    /// included file's as the `INCLUDE` resolved it.
    public let source: String
    /// The line it is on, from 1.
    public let line: Int
    /// The error as vislcg3 prints it, quoting the grammar, without colour.
    public let text: String
    /// The name the caller gave the included file it is in, or `nil` when it
    /// is in the compiled source itself.
    public let file: String?
    /// Where it sits in the string the caller gave for its source, or `nil`
    /// for an error with no place.
    public let range: Range<String.Index>?
    /// The same place as UTF-8 byte offsets into that string.
    public let utf8Range: Range<Int>?

    package init(
        message: String, source: String, line: Int, text: String, file: String?,
        range: Range<String.Index>?, utf8Range: Range<Int>?
    ) {
        self.message = message
        self.source = source
        self.line = line
        self.text = text
        self.file = file
        self.range = range
        self.utf8Range = utf8Range
    }
}

/// Runs `body`, a call into the generated bindings, with logging installed,
/// and throws what it throws as a `CG3Error`.
func calling<T>(_ body: () throws -> T) throws -> T {
    Logging.install()
    do {
        return try body()
    } catch let error as CG3Bindings.Cg3Error {
        throw CG3Error(error)
    } catch {
        throw CG3Error.panicked(error)
    }
}

extension CG3Error {
    init(_ error: CG3Bindings.Cg3Error) {
        switch error {
        case let .InvalidArgument(message): self.init(code: .invalidArgument, message: message)
        case let .InvalidGrammar(message): self.init(code: .invalidGrammar, message: message)
        case let .RunFailed(message): self.init(code: .runFailed, message: message)
        }
    }
}
