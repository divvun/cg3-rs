# divvun.cg3

[VISL CG-3](https://visl.sdu.dk/cg3.html) for Swift, on iOS and macOS. It
loads compiled Constraint Grammars (`.cg3b`) and runs them on text, and a run
gives the output `vislcg3` gives for the same grammar, input and options, byte
for byte. It converts and splits streams as `cg-conv` and `cg-mwesplit` do,
runs grammars as `cg-proc` does, and compiles and writes them as `cg-comp`
does. It is the [cg3-rs](https://github.com/divvun/cg3-rs) library, written in
Rust, in one static library that both of the package's products link.

| Product | What it does |
|---|---|
| `CG3` | Loads `.cg3b` grammars and runs them; converts and splits streams. |
| `CG3Compile` | Compiles textual grammars to `.cg3b`, writes them as text, relabels them. It re-exports `CG3`. |

An app that links only `CG3` carries no compiler: the linker drops it.

## Installing

The package is published to Divvun's SwiftPM registry, under the scope
`divvun`. Route the scope to it once:

```sh
swift package-registry set --scope divvun https://pahkat-3.giellalt.org/swift/
```

Then depend on it by its identifier:

```swift
.package(id: "divvun.cg3", from: "0.1.0"),
```

```swift
.target(name: "Keyboard", dependencies: [.product(name: "CG3", package: "divvun.cg3")]),
```

It supports iOS 13 and macOS 11 and later, and needs Xcode 16 or later. The
XCFramework holds macOS (arm64 and x86_64), iOS devices (arm64) and the iOS
simulator (arm64, iOS 14 and later). It has no x86_64 simulator slice, so a
build for every simulator, such as `-destination 'generic/platform=iOS
Simulator'`, needs `ARCHS=arm64`.

## Running a grammar

```swift
import CG3

let grammar = try Grammar(cg3b: Data(contentsOf: disambiguatorURL))
let result = try grammar.apply(to: input)
print(result.output)
```

`Grammar(cg3b:options:)` takes a compiled grammar, as `cg-comp` writes it.
`apply(to:options:)` runs it on one input and returns the output and every
warning and non-fatal error the run logged, in order. Each call is a run of
its own, and a run that fails throws without harming the grammar. A grammar
does not change, so threads may apply one grammar at once.

A grammar that selects the reading `wanted`:

```swift
try grammar.apply(to: "\"<word>\"\n\t\"word\" notwanted\n\t\"word\" wanted\n").output
// "<word>"
// 	"word" wanted
```

`grammar.info` reports what `vislcg3`'s verbose summary does: the number of
sections, rules, sets and tags, and whether the grammar has dependency or
relation rules.

## Options

`ApplyOptions` has a property for each option a run reads, named after its
`vislcg3` long option in lowerCamelCase: `--num-windows 4` is
`ApplyOptions(numWindows: 4)`. `input` and `output` stand for the `--in-*` and
`--out-*` options. An option whose value `vislcg3` makes optional takes
`true`, `false` or a string: `ApplyOptions(trace: true)` is `--trace`, and
`ApplyOptions(trace: "R1")` is `--trace=R1`. The options a grammar sets with
`CMDARGS` apply as they do in `vislcg3`.

```swift
let options = ApplyOptions(input: .apertium, output: .apertium)
try grammar.apply(to: "^word/word<notwanted>/word<wanted>$\n", options: options).output
// ^word/word<wanted>$
```

`LoadOptions` holds the options `vislcg3` applies while it loads a grammar:
`nrules`, `nrulesV`, `vislcgCompat` and `prefix`.

## The other tools

```swift
try convert("^w/a<n>+b<v>$\n", options: ConvertOptions(input: .apertium))
// "<w>"
// 	"b" v
// 		"a" n

try mwesplit(cohorts)
try grammar.proc("^word/word<notwanted>/word<wanted>$\n")
// ^word/word<wanted>$
```

`convert(_:options:)` writes what `cg-conv` writes, `mwesplit(_:)` what
`cg-mwesplit` writes, and `grammar.proc(_:options:)` what `cg-proc` writes,
each with options named after the tool's long options.

## Compiling

```swift
import CG3Compile

let compiled = try compileGrammar(source, name: "grammar.cg3", files: [
    "sets.cg3": sets,
    "rules.cg3": rules,
])
let grammar = try Grammar(cg3b: compiled.cg3b)
```

`compileGrammar(_:name:files:options:)` returns the `.cg3b` and the companion
file `cg-comp` writes, and the warnings the parse logged. Its `INCLUDE`s read
only the files it is given, from a dictionary or a closure that takes a name
and returns the file's text:

```swift
let compiled = try compileGrammar(source, name: "grammar.cg3") { name in
    try? String(contentsOf: grammarDirectory.appendingPathComponent(name), encoding: .utf8)
}
```

`grammarText(of:)` returns a `.cg3b` as text, as `vislcg3 --grammar-out` writes
it, and `relabelGrammar(_:with:)` relabels one as `cg-relabel` does.

## Errors

Every error is a `CG3Error`. Its `code` is `invalidArgument`, `invalidGrammar`,
`runFailed`, `compileFailed`, or `panicked` for a bug in the library, and its
`message` is the library's. A compile that fails carries every error the parse
found in `diagnostics`, each with its message, source, line, the text
`vislcg3` prints, and its place as a `Range<String.Index>` in the string you
gave for that source.

```swift
do {
    _ = try compileGrammar(source, name: "broken.cg3")
} catch let error as CG3Error where error.code == .compileFailed {
    for diagnostic in error.diagnostics {
        print(diagnostic.line, diagnostic.message, diagnostic.range.map { source[$0] } ?? "")
    }
}
```

## Logging

The library's log events go to the unified logging system, under the
subsystem `no.divvun.cg3` and a category for each part of the library. Their
messages are private. Set `logLevel` to choose the least level logged
(`.warning` by default, `nil` for none), and `logHandler` to receive every
event as well:

```swift
CG3.logLevel = .info
CG3.logHandler = { event in print(event.level, event.target, event.message) }
```

The settings apply to both products. Whatever they are, `apply` still returns
its run's warnings.

## Linking beside other Rust libraries

The package's static library keeps every symbol but its C interface to
itself, so an app can link it beside other Rust libraries, such as
`divvun.hfst`'s.

## Licence

GPL-3.0-or-later. The package carries the licence's text in `COPYING`.
