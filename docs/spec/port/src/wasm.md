# The wasm package

`@divvun/cg3` puts the library in a JavaScript runtime: a browser, a Web
Worker, Deno, Node or Bun. It is the library compiled to
`wasm32-unknown-unknown` with a thin binding over the library's API, and it
has two entry points. `@divvun/cg3` loads compiled grammars and runs them,
which is all a deployment needs, so its module carries no parser and no
writer. `@divvun/cg3/compile` compiles textual grammars and writes them, from a
`.wasm` of its own that only a caller who compiles loads. A browser has no
files, so the files a grammar's `INCLUDE`s name come from the caller.

wasm32 has a 32-bit `usize`. Where the port computes in `usize` what the C++
computes in `size_t`, a 32-bit build has to produce what a 64-bit one does.

## The library on 32-bit targets

> [spec:cg3:req:wasm.library-builds]
> The library MUST build for `wasm32-unknown-unknown` with its default features,
> and CI MUST check that build.

> [spec:cg3:req:wasm.width-independent]
> An output, an error or a `.cg3b` byte MUST NOT depend on the width of
> `usize`. Where the C++ computes in `size_t` from 64-bit constants, the port
> MUST compute in `u64` and truncate to `usize`, which is the value a 32-bit
> `size_t` holds after the same C++ expression.

## The JavaScript surface

The package is a TypeScript wrapper over the wasm-bindgen glue, as
`@divvun/hfst` and `@divvun/fst` are, so the three start, fail, free and log
alike.

> [spec:cg3:req:wasm.module+3]
> The package MUST be published to npm as `@divvun/cg3`, at a version of its
> own that `crates/cg3-wasm` carries, from a `wasm/v<version>` tag naming that
> version, and `version()` MUST return it from either entry point. It has two
> entry points, `@divvun/cg3` and `@divvun/cg3/compile`. Each is one ES module
> with TypeScript declarations and a `.wasm` file of its own beside it, and
> importing or starting one MUST NOT fetch or compile the other's `.wasm`. Both
> MUST load unchanged in browsers, Web Workers, Deno, Node 20 or later, and
> Bun. The package MUST declare GPL-3.0-or-later and carry the licence's text.

> [spec:cg3:req:wasm.read-only-core]
> The module behind `@divvun/cg3` MUST NOT link the textual grammar parser or
> the grammar writers. A build check MUST fail when code from either is
> reachable from that module's exports.

> [spec:cg3:req:wasm.init]
> Importing the package MUST NOT fetch or compile its `.wasm` file. The caller
> starts the package with one async `init`, which accepts a URL, a `Response`,
> the module's bytes or a compiled `WebAssembly.Module`, and with no argument
> loads the `.wasm` file beside the package in every runtime, Node included.
> Calling `init` again MUST return the same promise. A call to anything else
> before `init` has resolved MUST throw a `Cg3Error` with code
> `not-initialised`.

> [spec:cg3:req:wasm.grammar-load+1]
> `readGrammar(bytes)` MUST load a grammar from a `Uint8Array` through
> `CompiledGrammar::from_cg3b` and return a `Grammar`. `grammar.info()` MUST
> return what `CompiledGrammar::info` reports, as a plain object.

> [spec:cg3:req:wasm.apply+1]
> `grammar.apply(input, options)` MUST run the grammar on a string through
> `CompiledGrammar::run` and return the output as a string, with every warning
> and non-fatal error the run logged, in order, as strings. `options` MUST name
> each run option after its vislcg3 long option in camelCase, MUST refuse an
> option it does not know, and MUST offer every stream format except the binary
> one.

> [spec:cg3:req:wasm.errors+1]
> Every error the package raises MUST be a `Cg3Error`, a subclass of `Error`
> carrying a stable string `code` and, for an error the library reports, the
> library error's message. The codes are `not-initialised`, `init-failed`,
> `poisoned`, `freed`, `invalid-argument` (an argument or run option the
> package cannot use), `invalid-grammar` (bytes that are not a `.cg3b` the
> library reads) and `run-failed` (a run the library reports as failed). A load
> or run that fails MUST NOT trap, and a grammar MUST stay usable after a run of
> it fails. Should the module trap, the package MUST rethrow the trap as
> `poisoned`, and every later call into that module instance MUST throw
> `poisoned` without running, because its memory can no longer be trusted.

> [spec:cg3:req:wasm.handle-lifetime]
> A `Grammar` MUST release its WebAssembly memory when its `free()` or
> `[Symbol.dispose]()` method is called, so `using` declarations work. A
> grammar the caller drops without freeing SHOULD be released through a
> `FinalizationRegistry`. Any call on a freed grammar MUST throw a `Cg3Error`
> with code `freed`. Freeing twice MUST do nothing.

> [spec:cg3:req:wasm.typed-surface]
> The package MUST ship TypeScript declarations for its whole public API, and
> no exported signature may use `any`. Results MUST be plain objects, arrays
> and strings with declared shapes. Names follow JavaScript conventions:
> camelCase functions, methods and options, PascalCase classes.

> [spec:cg3:req:wasm.logging]
> The library's `tracing` events, from loading a grammar and from running one,
> MUST reach a logger the caller can replace with `setLogger`, each with its
> level, target and message. The default logger writes warnings and errors to
> the console and drops lower levels. `setLogLevel` sets the least level sent,
> and `off` sends nothing. Whatever the logger, `apply` still returns its run's
> warnings.

> [spec:cg3:req:wasm.no-process]
> An `EXTERNAL` rule MUST fail its run with the error a process that cannot start
> gives, naming the rule's line.

> [spec:cg3:req:wasm.parity+2]
> For every grammar in the golden corpus, compiled to `.cg3b`, the package MUST
> produce the output vislcg3 produces for the same `.cg3b`, input and options,
> byte for byte, or fail where vislcg3 fails. A test is exempt when its grammar
> runs an `EXTERNAL` rule, its stream is binary, or it passes an option the
> package does not offer, such as vislcg3's load-time `--nrules` filters or
> `--vislcg-compat`; the check MUST name each exemption and its reason. The
> check MUST run under Node, Bun and Deno, and in a headless Chromium on a page
> and in a Web Worker, and CI MUST run it.

## Load options and the other tools

> [spec:cg3:req:wasm.load-options]
> `readGrammar(bytes, options)` MUST load through the library's
> `LoadOptions`, with `options` naming each load option after its vislcg3 long
> option in camelCase, and MUST refuse an option it does not know.

> [spec:cg3:req:wasm.tools]
> The `@divvun/cg3` entry MUST offer `convert(input, options)` and
> `mwesplit(input)`, which return as strings what `cg3::convert` and
> `cg3::mwesplit` return, and `grammar.proc(input, options)`, which returns
> what `CompiledGrammar::proc` returns. Their options MUST name each option of
> cg-conv and of cg-proc after its long option in camelCase and MUST refuse an
> option they do not know, and `convert` MUST offer every stream format except
> the binary one. A failure MUST throw a `Cg3Error` with code `run-failed`.

## The compile entry point

> [spec:cg3:req:wasm.compile]
> `@divvun/cg3/compile` MUST start as `wasm.init` says of the package, with an
> `init` of its own that by default loads its own `.wasm`.
> `compileGrammar(source, options)` MUST compile a textual grammar from a
> string through `CompiledGrammar::from_text` and return the `.cg3b` bytes
> `to_cg3b` returns, as a `Uint8Array` that `readGrammar` loads, with the
> companion file's bytes or `null`, and the warnings the parse logged, in
> order, as strings. `options` names the source, the files its includes may
> read and the load options. `toText(bytes)` MUST return what `to_text` returns
> for the grammar a `.cg3b` holds, and `relabel(bytes, relabels)` the `.cg3b`
> bytes of the grammar `CompiledGrammar::relabel` makes from two. Grammars pass
> between the two entry points as `.cg3b` bytes.

> [spec:cg3:req:wasm.compile-includes]
> An `INCLUDE` in a grammar `compileGrammar` compiles MUST resolve only from
> the files its options give, through the library's in-memory resolver. Those
> files MUST be accepted as a record or `Map` from names to strings or
> `Uint8Array`s, or as a function that takes a resolved name and returns the
> file's text or bytes, or `undefined` for a file it does not have. A name
> they do not supply MUST fail the compile and name the file.

> [spec:cg3:req:wasm.compile-errors]
> A compile that fails MUST throw a `Cg3Error` with code `compile-failed`,
> whose `diagnostics` hold every error the parse found, each with its message,
> the name of its source, its line and its text as vislcg3 prints it, without
> colour. A failure that is not a compile follows `wasm.errors`. A trap in one
> entry point's module MUST poison that module only, and each entry point's
> objects MUST follow `wasm.handle-lifetime`.

> [spec:cg3:req:wasm.compile-parity]
> For every grammar in the golden corpus, `compileGrammar`, given the files
> the grammar includes, MUST return, byte for byte, the `.cg3b` and the
> companion file cg-comp writes, and `toText` of that `.cg3b` MUST equal what
> vislcg3 `--grammar-only --grammar-out` writes for it. For every input the
> repository's tests give cg-conv, cg-mwesplit, cg-relabel and cg-proc in a
> stream that is not binary, the package MUST give, byte for byte, the tool's
> output. The check MUST run
> under Node, Bun and Deno, and in a headless Chromium, and CI MUST run it.
