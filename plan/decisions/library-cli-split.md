---
id [dec:cg3:library-cli-split]
epitome "The command-line tools are their own crate, cg3-cli; the cg3 library keeps everything a grammar's load and run need, including the option language a grammar's CMDARGS are written in."
state @decided
category @existence
scope {
    elements ([arch:cg3:cli-boundary] [arch:cg3:embedding])
    rules (
        [spec:cg3:req:crate-split.library]
        [spec:cg3:req:crate-split.cli]
        [spec:cg3:req:crate-split.no-cli-deps]
        [spec:cg3:req:crate-split.one-version]
        [spec:cg3:req:crate-split.tools-unchanged]
    )
}
author "brendan@necessary.nu"
decided_at "2026-10-29T00:00:00Z"
edges {
    enables ([dec:cg3:wasm-package])
}
alternatives (
    {
        option "One crate, with a default `cli` feature that gates the tools and the dependencies only they use."
        rejected_because "Cargo unifies features across a build, so one dependent that leaves the feature on gives it to every other, and the tools are back in the library's public API. The boundary between a run and a process stays a convention inside one crate, where nothing checks it."
    }
    {
        option "Move the whole option language to the command line, and give the library only a typed run configuration."
        rejected_because "A grammar carries options of its own in `CMDARGS` and `CMDARGS-OVERRIDE`, written in vislcg3's option language. A run that ignores them does not run the grammar as vislcg3 does, so the library needs the parser and the merge anyway, and moving them would leave two copies or an embedded run that is wrong."
    }
    {
        option "Keep one crate, and have the wasm binding depend on it as it is."
        rejected_because "It builds: a dependency's binaries are not built, and sigpipe and tracing-subscriber compile for wasm32. But every embedder then carries the tools in the API it depends on and their dependencies in its build, and nothing stops the library starting to read argv, the environment or the standard streams."
    }
)
consequences {
    accepted (
        "`cargo install cg3` no longer installs the tools; they come from `cg3-cli`."
        "`merge_options` moves from `tools/mod.rs` into the library beside `parse_opts`, and `parse_opts_env`, which reads the environment, moves to `cg3-cli`. The option table, `parse_opts` and `parse_args` stay where they are."
        "The engine goes on reading its configuration from the option table through `set_options`. The library's typed run options are translated into that table, so there is one translation from options to a run."
        "The build script that stamps the build date and Git hash, and the banner constants, move to `cg3-cli`. Both crates take their version from the workspace."
        "`cg3-cli` has a `profiler` feature that turns on the library's, for `vislcg3 --profile`, cg-annotate and cg-merge-annotations."
        "The tests that run a binary move to `cg3-cli`, because Cargo sets `CARGO_BIN_EXE_*` only for binaries in the test's own package. The commit checks name both packages."
        "`[arch:cg3:cli-boundary]` exposes `crates/cg3-cli/**` once the tools have moved there."
    )
}
codifies (
    [spec:cg3:req:crate-split.library]
    [spec:cg3:req:crate-split.cli]
    [spec:cg3:req:crate-split.no-cli-deps]
    [spec:cg3:req:crate-split.one-version]
)
establishes ([arch:cg3:cli-boundary])
---

## Rationale

The port began as a file-for-file translation of the C++, so the tools' `main`
functions, their option tables and their process handling landed in the same
crate as the engine. The crate's public API exposes `cg3::tools`, and anything
that depends on the library builds the dependencies only a process uses. That
went unnoticed while the tools were the only consumer. The wasm package is the
second, and the first that has no process at all.

Where the line falls matters more than the fact of the split. Options look like
a command-line concern, but they are not only that: a grammar can carry them in
`CMDARGS` and `CMDARGS-OVERRIDE`, in the same syntax vislcg3 reads from its
arguments, and vislcg3 merges them with its own before it runs. They are part
of what the grammar says. So the option table, the string tokeniser, the
argument parser and the merge belong to the library, and the run's typed options
are translated into that table, not kept beside it. What only a process has
moves out: an argument vector, an environment, files named on a command line,
standard streams, an exit code, a log subscriber and a signal disposition.

With the line drawn there, the library's dependency graph says what it is. If
the library starts to need a process, it has to gain a dependency the split
forbids, and `[spec:cg3:req:crate-split.no-cli-deps]` turns that into a failing
check.
