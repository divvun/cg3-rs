# The library and the command line

The C++ builds one library and links each tool against it, but its `main`
functions, option tables and process handling all live in the same source tree,
and the port copied that into one crate. The crate's public API therefore
includes the tools, and anything that depends on it depends on what only a
process needs. The port splits them: `cg3` is the library, and `cg3-cli` holds
the command-line tools.

The line between them is not "options belong to the command line". A grammar
carries options of its own, in its `CMDARGS` and `CMDARGS-OVERRIDE` statements,
written in the same language as vislcg3's command line, and a run that ignores
them does not run the grammar as vislcg3 does. That option language belongs to
the grammar, so it stays in the library. What moves is what only a process has:
an argument vector, an environment, files named on a command line, standard
streams and an exit code.

> [spec:cg3:req:crate-split.library]
> The `cg3` crate MUST hold everything loading, compiling, writing and running a
> grammar needs. That includes the option language a grammar's `CMDARGS` and
> `CMDARGS-OVERRIDE` are written in: the option table, the tokeniser and argument
> parser that read an option string into it, the merge of one option table onto
> another, and the translation of options into a run's configuration. The
> library's public API MUST NOT include the command-line tools.

> [spec:cg3:req:crate-split.cli]
> The `cg3-cli` crate MUST hold the command-line tools and what only a process
> needs: the argument vector, the option tables read from the environment
> (`CG3_DEFAULT`, `CG3_OVERRIDE`, `CG3_CONV_DEFAULT`, `CG3_CONV_OVERRIDE`),
> cg-conv's own option table, files named on the command line, the standard
> streams, exit codes, version banners, the stderr log subscriber and the
> SIGPIPE disposition.

> [spec:cg3:req:crate-split.no-cli-deps]
> The library MUST NOT depend on `cg3-cli`, `sigpipe` or `tracing-subscriber`,
> with any combination of its features.

> [spec:cg3:req:crate-split.one-version]
> The two crates MUST carry one version, and every tool's banner MUST report it.

> [spec:cg3:req:crate-split.tools-unchanged]
> Every tool MUST keep its name, arguments, output, diagnostics and exit codes.
> The golden corpus and every test that runs a binary MUST pass, run from
> `cg3-cli`, with their expectations unchanged.
