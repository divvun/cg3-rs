# The wasm package

`@divvun/cg3` puts the library in a JavaScript runtime: a browser, Deno or
Node. It is the library compiled to `wasm32-unknown-unknown` with a thin binding
over the load-and-run API, and it reads compiled grammars only. A `.cg3b` is
what deployments ship, and a textual grammar's `INCLUDE`s name files a browser
does not have.

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

> [spec:cg3:req:wasm.module]
> The package MUST be published to npm as `@divvun/cg3`, at the version of the
> library it was built from, as one ES module with TypeScript declarations and
> its `.wasm` file. It MUST load in browsers, in Deno and in Node's long-term
> support releases, and its initialiser MUST accept the `.wasm` bytes from the
> caller in place of fetching the file beside the module.

> [spec:cg3:req:wasm.grammar-load]
> `Grammar.fromBytes` MUST load a grammar from a `Uint8Array` through
> `CompiledGrammar::from_cg3b`. `grammar.info()` MUST return what
> `CompiledGrammar::info` reports, and `grammar.free()` MUST release the
> grammar's memory.

> [spec:cg3:req:wasm.apply+1]
> `grammar.apply(input, options)` MUST run the grammar on a string through
> `CompiledGrammar::run` and return the output as a string, with every warning
> and non-fatal error the run logged, in order, as strings. `options` MUST name
> each run option after its vislcg3 long option in camelCase, MUST refuse an
> option it does not know, and MUST offer every stream format except the binary
> one.

> [spec:cg3:req:wasm.errors]
> A load or run that fails MUST throw an `Error` whose message is the library
> error's message. It MUST NOT trap, and a grammar MUST stay usable after a run
> of it fails.

> [spec:cg3:req:wasm.no-process]
> An `EXTERNAL` rule MUST fail its run with the error a process that cannot start
> gives, naming the rule's line.

> [spec:cg3:req:wasm.parity]
> For every grammar in the golden corpus, compiled to `.cg3b`, the package MUST
> produce the output vislcg3 produces for the same `.cg3b`, input and options,
> byte for byte. Tests whose grammar runs an `EXTERNAL` rule or whose stream is
> binary are exempt. CI MUST run this check.
