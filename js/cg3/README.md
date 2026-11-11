# @divvun/cg3

[VISL CG-3](https://visl.sdu.dk/cg3.html) for JavaScript. It loads compiled
Constraint Grammars (`.cg3b`) and runs them on text, and a run gives the
output `vislcg3` gives for the same grammar, input and options, byte for
byte. It converts and splits streams as `cg-conv` and `cg-mwesplit` do, and
runs grammars as `cg-proc` does. It is the [cg3-rs](https://github.com/divvun/cg3-rs) library, written
in Rust and built as WebAssembly. It runs in browsers, Web Workers, Deno,
Node 20 and later, and Bun, as one ES module and one `.wasm` file.

```sh
npm install @divvun/cg3
```

## Running a grammar

```js
import { init, readGrammar } from "@divvun/cg3";

await init();

const response = await fetch("/cg3/disambiguator.cg3b");
const grammar = readGrammar(new Uint8Array(await response.arrayBuffer()));

const { output, warnings } = grammar.apply(input);

grammar.free();
```

`readGrammar` takes a compiled grammar, as `cg-comp` writes it. It does not
read textual grammars: their `INCLUDE`s name files a browser does not have,
so compile them first. `apply(input, options?)` runs the grammar on one input
and returns `{ output, warnings }`: the output as a string, and every warning
and non-fatal error the run logged, in order. Each call is a run of its own,
and a run that fails throws without harming the grammar.

lang-kal's disambiguator, given "Inuit nunaat." as
[@divvun/hfst](https://www.npmjs.com/package/@divvun/hfst)'s tokeniser
analyses it, keeps one of the twelve readings of "Inuit" and one of the five
of "nunaat":

```js
import { readTokeniser } from "@divvun/hfst"; // after its own init()

const input = readTokeniser(pmhfstBytes).format("Inuit nunaat.", "giella-cg");
grammar.apply(input).output;
// "<Inuit>"
// 	"inuk" Gram/str N Rel Pl <W:0.0>
// :
// "<nunaat>"
// 	"nuna" N Abs Sg 3PlPoss <W:0.0>
// "<.>"
// 	"." CLB <W:0.0>
```

## Options

`options` names each run option after its `vislcg3` long option, in
camelCase: `--num-windows 4` is `{ numWindows: 4 }` and `--trace` is
`{ trace: true }`. With `trace`, each removed reading stays in the output,
commented out, with the rule that removed it:

```js
grammar.apply(input, { trace: true }).output;
// "<Inuit>"
// 	"inuk" Gram/str N Rel Pl <W:0.0>
// ;	"Inuit" Sem/Hum Prop Abs Pl <W:0.0> REMOVE:3212:tidlig0665
// ;	"Inuit" Sem/Hum Prop Rel Pl <W:0.0> REMOVE:3212:tidlig0665
// ...
```

The `input` and `output` formats are `"cg"` (the default), `"niceline"`,
`"apertium"`, `"fst"`, `"plain"` and `"jsonl"`. An option the package does not
know throws `Cg3Error` with code `invalid-argument`, and the message lists the
ones it does. The options a grammar sets with `CMDARGS` apply as they do in
`vislcg3`. An `EXTERNAL` rule fails its run, since there is no process to
start.

## The other tools

`convert(input, options?)` converts a stream from one format to another and
returns what `cg-conv` writes, `mwesplit(input)` splits each multi-word
cohort into one cohort per word as `cg-mwesplit` does, and
`grammar.proc(input, options?)` runs a grammar as `cg-proc` does, on an
Apertium stream unless `streamFormat` says otherwise:

```js
import { convert, mwesplit } from "@divvun/cg3";

convert("^w/a<n>+b<v>$\n", { input: "apertium", ltr: true });
// "<w>"
// 	"a" n
// 		"b" v

grammar.proc("^word/word<notwanted>/word<wanted>$\n", { trace: true });
// ^word/word<wanted><SELECT:11>/¬word<notwanted><SELECT:11>$
```

Their options are named after the tools' long options, in camelCase:
`--sub-delim "#"` is `{ subDelim: "#" }` and `--wordform-case` is
`{ wordformCase: true }`. `convert` reads and writes every format `apply`
does, and detects the input's format when `input` is left out, as `cg-conv`
does. `streamFormat` is `"apertium"` (the default), `"cg"` or `"matxin"`. A
conversion or run the library fails throws `Cg3Error` with code `run-failed`,
and an option the package does not know throws `invalid-argument`.

## Grammar information

`grammar.info()` gives what `vislcg3 -v` reports about a grammar:

```js
grammar.info();
// { sections: 6, templates: 0, rules: 5070, sets: 11233, tags: 11285,
//   rulesAny: 1672, hasDependencyRules: false, hasRelationRules: false }
```

## Starting the module

`init()` loads the `.wasm` file that ships beside the package, in every
runtime, Node included. Call it once and wait for it before anything else;
calling it again returns the same promise. It also takes a URL, a `Response`,
the module's bytes or a compiled `WebAssembly.Module`, for bundlers and for
workers that are handed one. `version()` gives the package's version. The
package is versioned on its own, apart from the cg3 library inside it.

## Memory

A grammar holds WebAssembly memory. Call `free()` when done with it, or
declare it with `using`:

```js
{
  using grammar = readGrammar(bytes);
  grammar.apply(input);
} // freed here
```

A grammar dropped without `free()` is released after it is garbage collected.
A call on a freed grammar throws `Cg3Error` with code `freed`.

## Errors

Every error is a `Cg3Error` with a `code` that code can test:
`not-initialised`, `init-failed`, `poisoned`, `freed`, `invalid-argument`,
`invalid-grammar` (bytes that are not a `.cg3b`) and `run-failed`. A panic
inside the module traps it. The instance is poisoned from then on and every
call throws `poisoned`, because its memory can no longer be trusted. Load the
page or process again to recover.

## Logging

The library's log events, from loading a grammar and from running one, go to
the console, warnings and errors only by default. `setLogger((event) => ...)`
takes them instead, each with its `level`, `target` and `message`.
`setLogLevel("debug")` changes how much is sent, and `setLogLevel("off")`
sends nothing. Whatever the logger, `apply` still returns its run's warnings.

## How it is checked

The suite runs every grammar of cg3-rs's golden corpus through the package,
under Node, Bun, Deno and a headless Chromium, on a page and in a Web Worker,
and compares the output with `vislcg3`'s byte for byte. 65 of the 68 tests
match. Three are exempt, because they run an `EXTERNAL` rule or pass an option
the package does not offer (`--nrules`, `--vislcg-compat`); none differ.

## Developing

The package is built from `crates/cg3-wasm` in the
[cg3-rs](https://github.com/divvun/cg3-rs) repository:

```sh
node js/build.mjs           # js/cg3/dist, ready for npm publish from js/cg3
node js/build.mjs --test    # and the test build, the tools and the fixtures
node js/test/run.mjs --require-all
```

The build needs the `wasm32-unknown-unknown` target, the `wasm-bindgen` CLI
at the version `crates/cg3-wasm` pins, `wasm-opt` and `npx`.

The package's version is the one `crates/cg3-wasm/Cargo.toml` and
`js/cg3/package.json` both carry. A `wasm/v<version>` tag naming it releases
the package to npm.

## Licence

GPL-3.0-or-later. The package carries the licence's text in `dist/COPYING`.
