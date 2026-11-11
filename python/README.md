# divvun-cg3

[VISL CG-3](https://visl.sdu.dk/cg3.html) for Python. It loads Constraint
Grammars, compiled (`.cg3b`) or textual, and runs them on text, and a run gives
the output `vislcg3` gives for the same grammar, input and options, byte for
byte. It does what the other tools do too: it writes a grammar out as
`cg-comp` does, relabels one as `cg-relabel` does, runs one as `cg-proc` does,
and converts and splits streams as `cg-conv` and `cg-mwesplit` do. It is the
[cg3-rs](https://github.com/divvun/cg3-rs) library, written in Rust, as a
native extension module: one wheel per platform for CPython 3.11 and later.

```sh
pip install divvun-cg3
```

## Running a grammar

```python
import cg3

grammar = cg3.load_grammar("disambiguator.cg3b")

result = grammar.apply('"<word>"\n\t"word" n\n\t"word" v\n')
result.output    # the output, as a string
result.warnings  # every warning and non-fatal error the run logged, in order
```

There are three ways to load a grammar:

- `cg3.load_grammar(path)` reads a file. It loads it as compiled when it
  begins with the `.cg3b` magic, as `vislcg3` decides, and as textual
  otherwise, with relative `INCLUDE`s found beside it.
- `cg3.read_grammar(data)` loads the bytes of a `.cg3b`, as `cg-comp` writes
  it.
- `cg3.parse_grammar(source, name=None)` parses a textual grammar from a
  string. Its diagnostics call it `name`, and its relative `INCLUDE`s resolve
  against the directory of the file `name` names.

`parse_grammar` can also take the files a grammar's `INCLUDE`s read from
memory, as a mapping from names to text or bytes. Then every `INCLUDE` reads
from those files and no others: a relative name is found beside the source
that includes it, else at the top of the names, as it would be on disk.

```python
grammar = cg3.parse_grammar(
    "INCLUDE sets.cg3 ;\nSELECT Nouns ;",
    "grammar.cg3",
    files={"sets.cg3": 'DELIMITERS = "<.>" ;\nLIST Nouns = n ;'},
)
```

```python
grammar = cg3.parse_grammar('DELIMITERS = "<.>" ; SELECT (n) ;')
grammar.apply('"<word>"\n\t"word" n\n\t"word" v\n').output
# '"<word>"\n\t"word" n\n'
```

A grammar is immutable and loaded once. Each `apply` is a run of its own over
it: nothing one run learns reaches the next.

## Options

A run's options are one `cg3.RunOptions`, made once and passed to as many
runs as need it:

```python
options = cg3.RunOptions(trace=True, num_windows=4)
grammar.apply(text, options)

apertium = cg3.RunOptions(input_format="apertium", output_format="apertium")
grammar.apply(text, apertium)
```

Each field is named after its `vislcg3` long option in snake_case:
`--num-windows 4` is `num_windows=4`. An option whose value `vislcg3` makes
optional is `True` without one, as `--trace` is `trace=True`. `input_format`
and `output_format` stand for the `--in-*` and `--out-*` options: `"cg"` (the
default), `"niceline"`, `"apertium"`, `"fst"`, `"plain"` and `"jsonl"`;
`cg3.Format` is their type.

`RunOptions` is a frozen dataclass, checked when it is made: a value of the
wrong type raises `TypeError`, a number outside 0 to 4294967295
`OverflowError`, and a format the package does not offer `ValueError`. An
option it does not know raises `TypeError`, as any Python class does.
`dataclasses.replace(options, trace=False)` makes a variant, and its `repr`
names only the options given. The options a grammar sets with `CMDARGS` apply
as they do in `vislcg3`, and an `EXTERNAL` rule starts its process as
`vislcg3` starts it.

## Writing a grammar

A grammar writes itself out without changing:

```python
grammar = cg3.load_grammar("disambiguator.cg3")

grammar.to_cg3b()                         # the .cg3b cg-comp writes, as bytes
grammar.to_text()                         # what vislcg3 --grammar-out writes
grammar.write_cg3b("disambiguator.cg3b")  # the .cg3b and its companion file
```

`write_cg3b` writes the `.cg3b` and, as `cg-comp` does, the companion file
beside it, `disambiguator.cg3b.cg3src`, which holds the grammar's sources so
that a runtime error in a rule can quote it. A grammar loaded from a `.cg3b`
has no sources, so nothing is written beside it.

## The other tools

`cg3.convert` converts a stream from one format to another, as `cg-conv` does,
and `cg3.mwesplit` splits multi-word cohorts, as `cg-mwesplit` does:

```python
cg3.convert('"<word>"\n\t"word" n\n', cg3.ConvertOptions(output_format="apertium"))
# '^word/word<n>$'

cg3.mwesplit(text)
```

`grammar.proc(input, options)` runs a grammar as `cg-proc` does, on the
Apertium stream unless the options name another, and `grammar.relabel(rules)`
gives the grammar `cg-relabel` makes of it and the relabel rules, as a new
grammar:

```python
grammar.proc("^word/word<n>/word<vblex>$\n", cg3.ProcOptions(trace=True))

rules = cg3.load_grammar("relabel.cg3r")
grammar.relabel(rules).write_cg3b("relabelled.cg3b")
```

`load_grammar` reads the rules as `cg-relabel` reads its rule file, compiled
or textual. `ConvertOptions` and `ProcOptions` are frozen dataclasses like
`RunOptions`, checked when they are made, with a field for each option named
after the tool's long option in snake_case: `cg-conv --sub-delim "#"` is
`sub_delim="#"`, and `cg-proc --wordform-case` is `wordform_case=True`.
`ConvertOptions.input_format` and `output_format` stand for `--in-*` and
`--out-*`, and the input's format is detected from the input, as `cg-conv`
detects it, when `input_format` is left out. `ProcOptions.stream_format` is
`"apertium"` (the default), `"cg"` or `"matxin"`, the formats
`--stream-format` numbers 1, 0 and 2; `cg3.ProcFormat` is their type. The
binary stream format is offered by neither. The options that do nothing in
the tools, `rtl` and cg-proc's `disambiguation` and `null_flush`, do nothing
here.

## Grammar information

`grammar.info()` gives what `vislcg3 -v` reports about a grammar:

```python
grammar.info()
# GrammarInfo(sections=1, templates=0, rules=3, sets=25, tags=31, rules_any=0,
#             has_dependency_rules=False, has_relation_rules=False)
```

## Threads

Loading, applying, writing and the other tools release the GIL while the
library works, so threads can apply one grammar at the same time and run in
parallel:

```python
from concurrent.futures import ThreadPoolExecutor

with ThreadPoolExecutor() as pool:
    outputs = [r.output for r in pool.map(grammar.apply, texts)]
```

## Errors

A failure the library reports raises a `cg3.Cg3Error`:

- `cg3.GrammarError` for a grammar that does not load, parse or relabel. A
  parse failure's message quotes every error in the grammar's source, as
  `vislcg3` prints it.
- `cg3.RunError` for a run, a conversion or a split that fails. The grammar
  stays usable.

An argument of the wrong type or value raises the `TypeError`, `ValueError`,
`OverflowError` or `OSError` Python raises for it, and a file that cannot be
read or written raises what `open()` would.

## Logging

The library's log events, from loading a grammar, running one and the other
tools, go to the standard `logging` module once the call returns, on loggers under `cg3`
named after the Rust module they came from, such as
`cg3.apertium_applicator`. Python's own levels apply, with the library's
trace level at 5. Events below the level of the `cg3` logger at the start of a
call are not kept, so set that logger's level to see more:

```python
import logging

logging.getLogger("cg3").setLevel(logging.DEBUG)
```

Whatever the logging setup, `apply` still returns its run's warnings.

## How it is checked

The suite runs every grammar of cg3-rs's golden corpus through the built
package twice, loaded from its textual source and from the `.cg3b` `cg-comp`
writes for it, and compares each output with `vislcg3`'s byte for byte. All 69
tests match both ways, the `EXTERNAL` rule's included, with the options their
command lines give read as a `LoadOptions` and a `RunOptions`; none is exempt.

It holds the package to the other tools the same way. Every corpus grammar,
loaded from disk and from memory, writes the `.cg3b` and companion file
`cg-comp` writes for it, and its `to_text` is what `vislcg3 --grammar-out`
writes. Every input cg3-rs's tests give `cg-conv`, `cg-mwesplit`, `cg-proc`
and `cg-relabel` gives the tool's bytes through `convert`, `mwesplit`,
`proc` and `relabel`, with the tool's command line read into a
`ConvertOptions` or `ProcOptions`: 255 cases, the JavaScript package's, which
`js/test/tool-fixtures.mjs` lists. Four are exempt, because they ask `cg-conv`
for the binary stream; none differ.

## Types

The package ships type stubs and the `py.typed` marker, and `mypy --strict`
checks them.

## Developing

The package is built from `crates/cg3-python` in the
[cg3-rs](https://github.com/divvun/cg3-rs) repository, with
[maturin](https://www.maturin.rs) and [uv](https://docs.astral.sh/uv/):

```sh
cd python
uv sync --no-install-project
uv run --no-sync maturin develop --uv   # build the extension into .venv
uv run --no-sync pytest                 # the tests, with cg-comp and vislcg3
uv run --no-sync mypy                   # the stubs and the tests
uv run --no-sync maturin build --release
```

The tests build the cg3 tools and the program the corpus's `EXTERNAL` rule
runs with cargo, or take them from the directory `CG3_BIN` names. The tool
parity check writes its fixtures with `node js/test/tool-fixtures.mjs`, or
reads the directory `CG3_TOOL_FIXTURES` names, which that command wrote. They
check the wheel `CG3_WHEEL` names, or build one.

The package's version is the one `crates/cg3-python/Cargo.toml` carries. A
`python/v<version>` tag naming it releases the wheels and the sdist to PyPI.

## Licence

GPL-3.0-or-later. The package carries the licence's text.
