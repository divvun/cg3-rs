# The Swift package

`divvun.cg3` puts the library in Swift, on iOS and macOS. It is one static
library, the library and a UniFFI binding of it, in an XCFramework, with a
thin Swift layer over the generated code. It has two products. `CG3` loads
compiled grammars and runs them, as `@divvun/cg3`'s main entry point does.
`CG3Compile` compiles textual grammars and writes them, as
`@divvun/cg3/compile` does. Both link the one static library, and an app
that uses only `CG3` carries no compiler: the linker drops it.

An app may link this package beside other Rust libraries, such as `HFST`.
Each Rust static library carries its own copy of Rust's standard library,
so this one keeps every symbol but its C interface to itself.

## The package

> [spec:cg3:req:swift.package]
> The package MUST be published through pahkat's SwiftPM registry with the
> identifier `divvun.cg3`, at a version of its own that `crates/cg3-swift`
> carries, from a `swift/v<version>` tag naming that version. `version` MUST
> return it from `CG3`. Each version's source archive, as
> `swift package archive-source` makes it, MUST hold `Package.swift`, the
> Swift sources and the XCFramework, which the manifest names with
> `.binaryTarget(name:path:)`. Beside it is a `package-metadata.json` with
> `author`, `description`, `licenseURL`, `readmeURL` and `repositoryURLs`.
> The package MUST declare GPL-3.0-or-later and carry the licence's text.

> [spec:cg3:req:swift.platforms]
> The XCFramework MUST hold a static library for macOS, arm64 and x86_64 in
> one fat slice; for iOS devices, arm64; and for the iOS simulator, arm64.
> The package MUST declare iOS 13 and macOS 11 as its least versions. The
> simulator slice is built for iOS 14, the least version Rust's
> `aarch64-apple-ios-sim` supports.

> [spec:cg3:req:swift.one-library]
> Both products MUST link one static library, which holds the UniFFI glue of
> both. An app that uses both carries the library once.

> [spec:cg3:req:swift.linking]
> The static library MUST define as global symbols only its C interface,
> whose names begin with `uniffi_cg3_swift` or `ffi_cg3_swift`, so that it
> links into one binary with any other static library, another Rust one
> included. A build check MUST fail when it defines any other global symbol.
> A test MUST link `CG3` and `CG3Compile` into one binary with a second Rust
> static library, built on its own by the same toolchain, and call all three
> on the iOS simulator.

> [spec:cg3:req:swift.read-only-core]
> A binary that links only `CG3` MUST NOT carry the textual grammar parser or
> the grammar writers. A build check MUST link a binary that calls every
> function of `CG3`'s glue and fail when the parser's or the writers' entry
> functions are in it, and MUST find them in one that calls `CG3Compile`'s.

## The Swift surface

The surface is `@divvun/cg3`'s: the same operations, options and results.
Names follow the Swift API Design Guidelines, and the shapes follow Swift's
conventions for options, errors and logging.

> [spec:cg3:req:swift.surface]
> The public API of both products MUST follow the Swift API Design
> Guidelines: lowerCamelCase functions, properties, options and cases;
> UpperCamelCase types; argument labels that read as a phrase. Every public
> declaration MUST have a documentation comment. No declaration of the
> generated bindings may be public in either product.

> [spec:cg3:req:swift.grammar-load]
> `Grammar(cg3b:options:)` MUST load a grammar from the bytes of a `.cg3b`,
> as `Data`, through `CompiledGrammar::from_cg3b`. `grammar.info` MUST be
> what `CompiledGrammar::info` reports, as a `GrammarInfo`.

> [spec:cg3:req:swift.load-options]
> `LoadOptions` MUST have a property for each load option, named after its
> vislcg3 long option in lowerCamelCase, with `prefix` a `Unicode.Scalar`. A
> load MUST go through the library's `LoadOptions`. A rule filter that does
> not compile as vislcg3 compiles it MUST throw `invalidArgument`.

> [spec:cg3:req:swift.apply]
> `grammar.apply(to:options:)` MUST run the grammar on a string through
> `CompiledGrammar::run` and return an `ApplyResult`: the output as a
> string, and every warning and non-fatal error the run logged, in order, as
> strings. `ApplyOptions` MUST have a property for each option a run reads,
> named after its vislcg3 long option in lowerCamelCase, with `input` and
> `output` standing for the `--in-*` and `--out-*` options. It MUST offer
> every stream format except the binary one. An option whose value vislcg3
> makes optional takes `true`, `false` or a string.

> [spec:cg3:req:swift.tools]
> `CG3` MUST offer `convert(_:options:)` and `mwesplit(_:)`, which return as
> strings what `cg3::convert` and `cg3::mwesplit` return, and
> `grammar.proc(_:options:)`, which returns what `CompiledGrammar::proc`
> returns. `ConvertOptions` and `ProcOptions` MUST have a property for each
> option of cg-conv and of cg-proc, named after its long option in
> lowerCamelCase. `ConvertOptions` MUST offer every stream format except the
> binary one. A failure MUST throw `runFailed`.

> [spec:cg3:req:swift.errors]
> Every failure MUST be thrown as a `CG3Error`, with a `code`, the message
> and, for `compileFailed`, the diagnostics. The codes are those of
> `@divvun/cg3` that a native library can raise: `invalidArgument`,
> `invalidGrammar`, `runFailed` and `compileFailed`, whose raw values are
> the JavaScript codes. A panic in the library is caught and thrown as
> `panicked`. A failed load or run MUST NOT end the process, and a grammar
> MUST stay usable after a run of it fails.

> [spec:cg3:req:swift.concurrency]
> Every public type MUST be `Sendable`. A `Grammar` is a final class that
> does not change, so threads MAY apply one grammar at once, each apply a
> run of its own, without a lock. The logging settings MUST be guarded by a
> lock. Nothing else in either product changes.

> [spec:cg3:req:swift.logging]
> The library's `tracing` events MUST reach the unified logging system as
> they happen, under the subsystem `no.divvun.cg3` and the event's target as
> the category, through `os_log`: `os.Logger` needs iOS 14. An error or a
> warning is logged as `error`, info as `info`, and debug and trace as
> `debug`. The message is private, as a keyboard's input is. When a
> `logHandler` is set, each event MUST reach it as well, as a `LogEvent`
> with its level, target and message. `logLevel` sets the least level
> sent, `warning` by default, and `nil` sends nothing. The settings apply to
> both products. Whatever the settings, `apply` still returns its run's
> warnings.

> [spec:cg3:req:swift.external]
> An `EXTERNAL` rule MUST start its process where the platform lets an app
> start one, as on macOS. Where it does not, as on iOS, the run MUST fail
> with `runFailed` and the error starting the process gives.

## Compiling

> [spec:cg3:req:swift.compile]
> `compileGrammar(_:name:files:options:)` in `CG3Compile` MUST compile a
> textual grammar from a string through `CompiledGrammar::from_text` and
> return a `CompileResult`: the `.cg3b` that `to_cg3b` returns, as `Data`
> that `Grammar(cg3b:)` loads; the companion file's bytes, or `nil`; and the
> warnings the parse logged, in order, as strings. `grammarText(of:)` MUST
> return what `to_text` returns for the grammar a `.cg3b` holds, and
> `relabelGrammar(_:with:)` the `.cg3b` of the grammar
> `CompiledGrammar::relabel` makes from a `.cg3b` and relabel rules, given
> as a string or as the bytes cg-relabel reads. Grammars pass between the
> products as `.cg3b` bytes, as they do in `@divvun/cg3`.

> [spec:cg3:req:swift.compile-includes]
> An `INCLUDE` in a grammar `compileGrammar` compiles MUST resolve only from
> the files its `files` argument gives, through the library's in-memory
> resolver. `files` MUST be accepted as a `[String: String]` from names to
> text, or as a closure `(String) -> String?` that takes a resolved name and
> returns the file's text, or `nil` for a file it does not have. The closure
> is asked for each file once. A name neither gives MUST fail the compile
> and name the file.

> [spec:cg3:req:swift.compile-errors]
> A compile that fails MUST throw `compileFailed`, whose diagnostics hold
> every error the parse found. Each has its message; the name of its source;
> its line; its text as vislcg3 prints it, without colour; and `file`, the
> name the caller gave the included file it is in, or `nil` for the
> compiled source. Its `range` MUST be a `Range<String.Index>` into the
> string the caller gave for that source, and its `utf8Range` the same
> place as UTF-8 byte offsets; both are `nil` for an error with no place.

## Parity

> [spec:cg3:req:swift.parity]
> For every grammar in the golden corpus, compiled to `.cg3b`, `CG3` MUST
> produce the output vislcg3 produces for the same `.cg3b`, input and
> options, byte for byte, or fail where vislcg3 fails. The cases and their
> exemptions are those `js/test/fixtures.mjs` writes for `@divvun/cg3`, and
> the check MUST name each exemption and its reason. The check MUST run with
> `swift test` on macOS and with `xcodebuild test` on an iOS simulator, and
> CI MUST run it.

> [spec:cg3:req:swift.compile-parity]
> For every grammar in the golden corpus, `compileGrammar`, given the files
> the grammar includes, MUST return, byte for byte, the `.cg3b` and the
> companion file cg-comp writes, and `grammarText` of that `.cg3b` MUST
> equal what vislcg3 `--grammar-only --grammar-out` writes for it. For every
> input the repository's tests give cg-conv, cg-mwesplit, cg-relabel and
> cg-proc in a stream that is not binary, the package MUST give, byte for
> byte, the tool's output. The cases are those `js/test/tool-fixtures.mjs`
> writes. The check MUST run where `swift.parity` runs.
