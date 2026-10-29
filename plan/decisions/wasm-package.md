---
id [dec:cg3:wasm-package]
epitome "@divvun/cg3 is the library compiled to wasm32-unknown-unknown behind a wasm-bindgen binding of the load-and-run API: one ES module, compiled grammars only, and every apply an independent run."
state @decided
category @existence
scope {
    elements ([arch:cg3:wasm-package] [arch:cg3:embedding])
    rules (
        [spec:cg3:req:embed.load]
        [spec:cg3:req:embed.info]
        [spec:cg3:req:embed.run-options]
        [spec:cg3:req:embed.grammar-options]
        [spec:cg3:req:embed.run]
        [spec:cg3:req:embed.runs-independent]
        [spec:cg3:req:embed.cli-runs-through]
        [spec:cg3:req:wasm.library-builds]
        [spec:cg3:req:wasm.width-independent]
        [spec:cg3:req:wasm.module]
        [spec:cg3:req:wasm.grammar-load]
        [spec:cg3:req:wasm.apply]
        [spec:cg3:req:wasm.errors]
        [spec:cg3:req:wasm.no-process]
        [spec:cg3:req:wasm.parity]
    )
}
author "brendan@necessary.nu"
decided_at "2026-10-29T00:00:00Z"
edges {
    requires ([dec:cg3:library-cli-split] [dec:cg3:grammar-load-phases])
}
alternatives (
    {
        option "Build and pack with wasm-pack."
        rejected_because "wasm-pack names the npm package after the crate, so `@divvun/cg3` would need its `package.json` rewritten anyway, and the layouts it produces are the ones wasm-bindgen produces directly. The wasm-bindgen CLI has to match the crate's wasm-bindgen version either way."
    }
    {
        option "Separate builds for wasm-bindgen's `bundler` and `nodejs` targets."
        rejected_because "A `bundler` build cannot be imported by a browser or by Deno without a bundler, and a `nodejs` build is CommonJS for Node alone. The `web` target's ES module runs in all three once its initialiser accepts the `.wasm` bytes from the caller."
    }
    {
        option "Target wasm32-unknown-emscripten or wasm32-wasip1."
        rejected_because "Emscripten exists to carry C code and a libc, which the library does not need without the profiler. WASI supplies files and an environment that a browser does not have, and the binding needs neither."
    }
    {
        option "Compile textual grammars in the package as well."
        rejected_because "`INCLUDE` resolves files a browser does not have, the parser and its source-quoting diagnostics add to the module, and deployments ship `.cg3b`. A textual grammar is compiled with cg-comp first."
    }
    {
        option "A runner that keeps its state from one apply to the next."
        rejected_because "A run's windows, variables and external processes would carry from one input into the next, so an input's output would depend on what ran before it. vislcg3 gives every input a fresh process. A fresh run costs an overlay over the shared core, not a copy of the grammar."
    }
    {
        option "Take vislcg3 arguments as an array of strings."
        rejected_because "An argument array has no types for TypeScript to check, and it accepts options a run cannot act on, such as file names. Typed fields named after the long options cover every option a run reads, and the grammar's own `CMDARGS` are still merged with them."
    }
)
consequences {
    accepted (
        "`crates/cg3-wasm` is a `cdylib` that depends on the library alone and binds only the load-and-run API."
        "The binding collects a run's warnings with a `tracing` subscriber installed for that run alone, and returns them with its output. Nothing is written to the console."
        "An `EXTERNAL` rule fails where its process would start, with the error `std::process` gives on a target without processes."
        "The binary stream format is not offered: the JavaScript surface takes and returns strings."
    )
    deferred (
        "A runner reused across applies, if a fresh run per input costs too much for per-sentence use. `wasm.parity` records the load time, run time and memory of a real Divvun grammar."
        "A view of a grammar's rules, sets and tags beyond the counts `info()` reports."
        "Binary stream input and output over `Uint8Array`."
    )
}
codifies (
    [spec:cg3:req:embed.runs-independent]
    [spec:cg3:req:wasm.library-builds]
    [spec:cg3:req:wasm.width-independent]
    [spec:cg3:req:wasm.parity]
)
establishes ([arch:cg3:embedding] [arch:cg3:wasm-package])
---

## Rationale

The library already loads a `.cg3b` from a byte slice and runs a grammar from
one in-memory stream to another, so the package is mostly a binding. What it
needs from the library is small: a load from bytes, a run from bytes to bytes,
and a build for a 32-bit target. The flat containers' probe hash is the only
code the 32-bit build rejects, and it computes what the C++ computes once it
works in `u64` and truncates.

The binding is over the library's load-and-run API, not over the engine. That
API is what vislcg3 runs through, so the golden corpus tests the same path the
package takes, and the package's own parity check needs only to show that
compiling to wasm32 changed nothing. Each apply is a fresh run over the shared
core because that is what vislcg3 is: one input, one run, nothing carried over.

One ES module from wasm-bindgen's `web` target covers browsers, Deno and Node,
provided the caller can hand the initialiser the `.wasm` bytes where fetching
the file beside the module does not work. The package reads compiled grammars
only, because that is what is deployed and because a textual grammar's
`INCLUDE`s name files that a browser cannot open.
