---
id [dec:cg3:python-package]
epitome "divvun-cg3 is a pyo3 extension module over the load-and-run API, imported as cg3: compiled and textual grammars, keyword-only run options, every apply an independent run with the GIL released."
state @decided
category @existence
scope {
    elements ([arch:cg3:python-package] [arch:cg3:embedding])
    rules (
        [spec:cg3:req:embed.load-text]
        [spec:cg3:req:python.module]
        [spec:cg3:req:python.typed]
        [spec:cg3:req:python.grammar-load]
        [spec:cg3:req:python.apply]
        [spec:cg3:req:python.errors]
        [spec:cg3:req:python.threads]
        [spec:cg3:req:python.logging]
        [spec:cg3:req:python.parity]
    )
}
author "brendan@necessary.nu"
decided_at "2026-11-10T00:00:00Z"
edges {
    requires ([dec:cg3:library-cli-split] [dec:cg3:wasm-package])
}
alternatives (
    {
        option "Run the wasm build from Python through a WebAssembly runtime such as wasmtime."
        rejected_because "It would carry a second runtime into every install and run the library slower than native, and it would keep the limits of a target with no files or processes: no textual grammars, no `EXTERNAL` rules."
    }
    {
        option "Port upstream's SWIG binding, `constraint_grammar` and its `cg-proc` wrapper."
        rejected_because "It wraps cg-proc's command line rather than a load-and-run API, and its stream parser reads the binary stream format alone. The port's API is the one vislcg3 and the wasm package already run through."
    }
    {
        option "Publish to PyPI as `cg3`."
        rejected_because "The name is taken on PyPI by a project that is not this one."
    }
    {
        option "Take the run options as a dictionary, as the JavaScript package takes an object."
        rejected_because "Keyword-only parameters give the check of an unknown option to Python itself, with the `TypeError` every Python function raises, and they make the signature something `help()` and type checkers can read."
    }
    {
        option "Raise one exception class carrying a code, as `Cg3Error` does in JavaScript."
        rejected_because "Python code tells failures apart with `except` clauses on classes. The wrapper-side codes of the JavaScript package (`not-initialised`, `freed`, `poisoned`) have no counterpart in a native module whose objects Python's reference counting frees."
    }
    {
        option "Send each `tracing` event to Python's `logging` as it happens."
        rejected_because "A run holds no GIL, so every event would have to take it back, and a handler would run in the middle of a run. Events are kept for the call and handed over when it returns, below a level read from the `cg3` logger when the call starts."
    }
)
consequences {
    accepted (
        "`crates/cg3-python` is a `cdylib` that depends on the library alone and binds the load-and-run API. maturin builds it from `python/pyproject.toml` as `cg3._cg3`, under a `cg3` package that re-exports it."
        "The library gains `CompiledGrammar::from_text`, so a textual grammar loads through the API vislcg3 and both packages use."
        "One abi3 wheel per platform covers CPython 3.11 and later; free-threaded CPython cannot load it."
        "The binary stream format is not offered: the surface takes and returns strings, as the JavaScript one does."
    )
    deferred (
        "Building wheels for every platform in CI and publishing them to PyPI on a release tag."
        "Binary stream input and output over `bytes`."
        "An abi3t wheel for free-threaded CPython."
    )
}
codifies (
    [spec:cg3:req:embed.load-text]
    [spec:cg3:req:python.threads]
    [spec:cg3:req:python.parity]
)
establishes ([arch:cg3:python-package])
---

## Rationale

The library's load-and-run API is what vislcg3 runs every grammar through and
what the wasm package binds, so a Python binding over it inherits the golden
corpus as its test. A native module is the smaller step from there: the library
already builds for every host the tools ship on, and Python's packaging handles
one compiled extension per platform well.

A native host has what a browser lacks. It can read the files a textual
grammar's `INCLUDE`s name, so the package loads textual grammars through a new
`CompiledGrammar::from_text` beside `from_cg3b`. It can start processes, so an
`EXTERNAL` rule runs as it does under vislcg3.

Runs of one grammar are independent and share its core without copying, so
the binding releases the GIL for the length of a load or a run. Several
threads can apply one grammar at once, and the cost of each is an overlay over
the core.

The surface keeps the JavaScript package's shape and takes Python's
conventions where they differ: snake_case names, keyword-only options that
Python itself checks, exception classes, and the standard `logging` module.
