# The Python package

`divvun-cg3` puts the library in Python. It is a native extension module, a
pyo3 binding over the load-and-run API, so a run is the library's own code on
the host and not WebAssembly. A Python process has files and can start
processes, so the package loads textual grammars as well as compiled ones, and
it runs `EXTERNAL` rules.

The surface follows `@divvun/cg3` where Python allows: the same operations,
the same options and the same results. It differs where Python has a
convention of its own, for names, the objects that carry options, exceptions
and logging, and it needs no second entry point for compiling, since a native
module pays nothing for code it does not run.

## The module

> [spec:cg3:req:python.module+1]
> The package MUST be published to PyPI as `divvun-cg3`, at a version of its
> own that `crates/cg3-python` carries, from a `python/v<version>` tag naming
> that version, and MUST be imported as `cg3`. `cg3.__version__` MUST be that
> version. One wheel per platform MUST serve CPython 3.11 and
> every later release, through the stable ABI. The package MUST declare
> GPL-3.0-or-later and carry the licence's text.

> [spec:cg3:req:python.typed]
> The package MUST ship type stubs for its whole public API and the `py.typed`
> marker, and no public signature may use `Any`. Results MUST be instances of
> declared classes with typed attributes. Names follow Python conventions:
> snake_case functions, methods, attributes and options, and PascalCase
> classes.

## Grammars and runs

> [spec:cg3:req:python.grammar-load]
> `cg3.read_grammar(data)` MUST load a grammar from the bytes of a `.cg3b`
> through `CompiledGrammar::from_cg3b`. `cg3.parse_grammar(source, name)` MUST
> parse a textual grammar from a string through `CompiledGrammar::from_text`,
> under `name` when one is given. `cg3.load_grammar(path)` MUST read the file
> at `path` and load it as a `.cg3b` when it begins with the `CG3B` magic, as
> vislcg3 decides, and as a textual grammar under `path` otherwise. Each
> returns a `Grammar`, and `grammar.info()` MUST return what
> `CompiledGrammar::info` reports, as a `GrammarInfo`.

> [spec:cg3:req:python.apply+1]
> `grammar.apply(input, options)` MUST run the grammar on a string through
> `CompiledGrammar::run`, with the options a `cg3.RunOptions` gives or with
> none, and return an `ApplyResult`, whose `output` is the output as a string
> and whose `warnings` are every warning and non-fatal error the run logged, in
> order, as strings. `cg3.RunOptions` MUST be a frozen dataclass with a
> keyword-only field for each option a run reads, named after its vislcg3 long
> option in snake_case, with `input_format` and `output_format` standing for
> the `--in-*` and `--out-*` options. It MUST check its fields when it is made,
> and MUST offer every stream format except the binary one. An option it does
> not know MUST raise `TypeError`, as it does for any Python class.

> [spec:cg3:req:python.errors]
> Every failure the library reports MUST be raised as a `cg3.Cg3Error`, a
> subclass of `Exception`: `cg3.GrammarError` for a grammar that does not load
> or parse, and `cg3.RunError` for a run that fails. Its message MUST be the
> library error's, after the source quotation vislcg3 prints for a failed
> parse, or for a failed run the library places in the grammar, without colour.
> An argument of the wrong type or value MUST raise the `TypeError`,
> `ValueError`, `OverflowError` or `OSError` Python raises for it. A failed load
> or run MUST NOT end the process, and a grammar MUST stay usable after a run
> of it fails.

> [spec:cg3:req:python.threads]
> Loading and applying MUST release the GIL while the library works. A
> `Grammar` is immutable, so threads MAY apply one grammar at the same time,
> each apply an independent run.

> [spec:cg3:req:python.logging]
> The library's `tracing` events, from loading a grammar and from running one,
> MUST reach Python's `logging` module when the call returns, in the order they
> happened. Each goes to the logger named after its target with `::` read as
> `.`, such as `cg3.grammar_applicator`, at the level named after its own, and
> a `trace` event at level 5. An event below the effective level of the `cg3`
> logger when the call starts MUST NOT be kept. Whatever the logging setup,
> `apply` still returns its run's warnings.

## Compiling and the other tools

The package does what each command-line tool does, through the library
function the tool calls.

> [spec:cg3:req:python.load-options]
> `read_grammar`, `parse_grammar` and `load_grammar` MUST take a keyword-only
> `options`, a `cg3.LoadOptions` or `None`, and load through the library's
> `LoadOptions`. `cg3.LoadOptions` MUST be a frozen dataclass with a
> keyword-only field for each load option, named after its vislcg3 long option
> in snake_case, and MUST check its fields when it is made.

> [spec:cg3:req:python.includes]
> `parse_grammar` MUST take a keyword-only `files`, a mapping from names to
> `str` or `bytes`. When it is given, every `INCLUDE` MUST resolve through the
> library's in-memory resolver over those files and no others; when it is
> not, through the default resolver, as vislcg3 resolves it.

> [spec:cg3:req:python.write]
> `Grammar.to_cg3b()` MUST return what `CompiledGrammar::to_cg3b` returns, as
> `bytes`, and `Grammar.to_text()` what `CompiledGrammar::to_text` returns, as
> `str`. `Grammar.write_cg3b(path)` MUST write that `.cg3b` to `path` and, when
> the grammar has one, its companion file beside it, as cg-comp does. Writing
> MUST release the GIL while the library works, and MUST NOT change the
> grammar.

> [spec:cg3:req:python.tools]
> `cg3.convert(input, options)`, `cg3.mwesplit(input)`,
> `Grammar.relabel(relabels)` and `Grammar.proc(input, options)` MUST return
> what `cg3::convert`, `cg3::mwesplit`, `CompiledGrammar::relabel` and
> `CompiledGrammar::proc` return for the same arguments: a `str` from the
> three that write a stream, and a new `Grammar` from `relabel`. Their options
> MUST be `cg3.ConvertOptions` and `cg3.ProcOptions`, frozen dataclasses with
> a keyword-only field for each option, named after the tool's long option in
> snake_case, which check their fields when they are made; `ConvertOptions`
> MUST offer every stream format except the binary one. A failed conversion,
> split or `proc` MUST raise `cg3.RunError` and a failed relabel
> `cg3.GrammarError`. Each MUST release the GIL while the library works.

## Parity

> [spec:cg3:req:python.parity]
> For every grammar in the golden corpus, the package MUST produce the output
> vislcg3 produces for the same grammar, input and options, byte for byte, or
> fail where vislcg3 fails. It MUST do so both when it loads the grammar's
> textual source and when it loads the `.cg3b` cg-comp writes for it. A test is
> exempt when its stream is binary or it passes an option the package does not
> offer, such as vislcg3's load-time `--nrules` filters or `--vislcg-compat`;
> the check MUST name each exemption and its reason. `EXTERNAL` rules are not
> exempt. The check MUST run through the built package.

> [spec:cg3:req:python.write-parity]
> For every grammar in the golden corpus, the `.cg3b` the package writes for
> the grammar's textual source MUST equal, byte for byte, the `.cg3b` cg-comp
> writes for it, its companion file MUST equal cg-comp's, and its `to_text`
> MUST equal what vislcg3 `--grammar-only --grammar-out` writes. The check
> MUST run through the built package.

> [spec:cg3:req:python.tools-parity]
> For every input the repository's tests give cg-conv, cg-mwesplit,
> cg-relabel and cg-proc, the package MUST give, byte for byte, the output the
> tool gives for the same input, grammar and options, or fail where it fails. The check MUST run
> through the built package.
