---
id [dec:cg3:tool-surface]
epitome "The packages do what the command-line tools do, other than profiling: the library compiles, writes, converts, splits, relabels and runs as cg-proc through functions the tools call, divvun-cg3 binds all of it, and @divvun/cg3 compiles and writes through a second entry point, @divvun/cg3/compile, whose .wasm only a caller who compiles loads."
state @decided
category @existence
scope {
    elements ([arch:cg3:embedding] [arch:cg3:python-package] [arch:cg3:wasm-package])
    rules (
        [spec:cg3:req:embed.load-options]
        [spec:cg3:req:embed.include-resolver]
        [spec:cg3:req:embed.write-binary]
        [spec:cg3:req:embed.write-text]
        [spec:cg3:req:embed.cli-compiles-through]
        [spec:cg3:req:embed.convert]
        [spec:cg3:req:embed.mwesplit]
        [spec:cg3:req:embed.relabel]
        [spec:cg3:req:embed.proc]
        [spec:cg3:req:python.load-options]
        [spec:cg3:req:python.includes]
        [spec:cg3:req:python.write]
        [spec:cg3:req:python.tools]
        [spec:cg3:req:python.write-parity]
        [spec:cg3:req:python.tools-parity]
        [spec:cg3:req:wasm.module]
        [spec:cg3:req:wasm.read-only-core]
        [spec:cg3:req:wasm.load-options]
        [spec:cg3:req:wasm.tools]
        [spec:cg3:req:wasm.compile]
        [spec:cg3:req:wasm.compile-includes]
        [spec:cg3:req:wasm.compile-errors]
        [spec:cg3:req:wasm.compile-parity]
    )
}
author "brendan@necessary.nu"
decided_at "2026-11-11T00:00:00Z"
edges {
    requires ([dec:cg3:library-cli-split] [dec:cg3:wasm-package] [dec:cg3:python-package])
}
alternatives (
    {
        option "Compile and write in the main npm entry point."
        rejected_because "The parser, its diagnostics and the writers would be in the module every page that only runs a grammar loads. A second entry point with its own `.wasm` costs those pages nothing."
    }
    {
        option "Publish compiling as a separate npm package, such as `@divvun/cg3-compile`."
        rejected_because "Two packages carry two versions, which a user has to keep in step so that one's `.cg3b` loads in the other. An entry point of the same package has its version by construction."
    }
    {
        option "Let the compile entry point hand back a `Grammar` the main entry point runs."
        rejected_because "The two entry points are two WebAssembly instances with separate memories, so an object of one cannot live in the other. A `.cg3b` is the form a grammar already travels in, and loading one is fast."
    }
    {
        option "Writers that consume the grammar, as the C++ writers change the one they write."
        rejected_because "A `CompiledGrammar` is shared by every run of it and, in Python, by threads. A writer that needs the only reference fails whenever anything else holds one, which is what vislcg3's `Arc::try_unwrap` does today. Writers that read the grammar and keep their changes to themselves work on any handle."
    }
    {
        option "Keep `INCLUDE` on the filesystem and give the compile entry point WASI's files."
        rejected_because "A browser has no WASI, and a host that has files can hand them over. A resolver the caller supplies serves a browser from memory and Python and the tools from disk through one seam."
    }
    {
        option "Bind each tool's command line, taking its arguments as strings."
        rejected_because "An argument list has no types for a checker, and it carries what only a process has: file names, redirections, `--help`. Typed options over bytes are what the packages already do for runs, and each tool becomes a process around the function its package binds."
    }
    {
        option "Offer profiling: `--profile`, cg-annotate and cg-merge-annotations."
        rejected_because "The profiler writes an SQLite database through bundled C, behind a feature off by default, and its reports are directories of HTML. It is a tool for developing a grammar on a machine that has vislcg3, not something a package that runs or compiles grammars needs."
    }
)
consequences {
    accepted (
        "cg-comp's flow, cg-conv's option mapping, cg-relabel's loading and cg-proc's setup move from `cg3-cli` into the library, as `crate-split.library` already requires, and each tool calls what its package binds."
        "The writers leave the grammar unchanged, so they work from a shared `CompiledGrammar`."
        "A grammar loaded from text keeps its sources in memory, so an error that quotes the grammar reads no file at run time."
        "`@divvun/cg3` gains a second entry point and a second `.wasm`, built by the same build script and shipped in the same package."
        "`--prefix` in a grammar's `CMDARGS` applies through the library as it does for vislcg3, where the library ignores it today."
    )
    deferred (
        "The binary stream format in either package."
        "`--dump-ast`, `--show-tags`, `--show-unused-sets` and `--show-set-hashes`, whose reports the port does not produce."
    )
}
codifies (
    [spec:cg3:req:embed.write-binary]
    [spec:cg3:req:embed.include-resolver]
    [spec:cg3:req:wasm.read-only-core]
)
---

## Rationale

Each command-line tool is a function over bytes with a process around it. The
packages already bind vislcg3 that way, through the load-and-run API, and the
golden corpus checks the binding because vislcg3 runs through the same API.
Giving cg-comp, cg-conv, cg-mwesplit, cg-relabel and cg-proc the same shape
makes the library hold the whole of each tool, which `crate-split.library`
asks for, and lets the corpus check the packages' versions of them too.

Two things stand in the way in the library. The writers change the grammar
they write, so they need the only reference to it, and a shared
`CompiledGrammar` never gives them one. And `INCLUDE` opens files directly,
which a browser cannot. The writers therefore keep their changes to
themselves, and includes go through a resolver: the default reads the disk as
vislcg3 does, and one over named files in memory serves a host without files.

Python pays nothing for code it does not call, so its package has one module.
The npm package does pay, in bytes every page downloads, so compiling is a
second entry point with a `.wasm` of its own, and the main entry point stays
the read-only module a deployment needs, with a build check that keeps it
that way.
