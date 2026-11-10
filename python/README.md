# divvun-cg3

[VISL CG-3](https://visl.sdu.dk/cg3.html) for Python. It loads Constraint
Grammars, compiled (`.cg3b`) or textual, and runs them on text, and a run gives
the output `vislcg3` gives for the same grammar, input and options, byte for
byte. It is the [cg3-rs](https://github.com/divvun/cg3-rs) library, written in
Rust, as a native extension module: one wheel per platform for CPython 3.11
and later.

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

```python
grammar = cg3.parse_grammar('DELIMITERS = "<.>" ; SELECT (n) ;')
grammar.apply('"<word>"\n\t"word" n\n\t"word" v\n').output
# '"<word>"\n\t"word" n\n'
```

A grammar is immutable and loaded once. Each `apply` is a run of its own over
it: nothing one run learns reaches the next.

## Options

`apply` takes each run option as a keyword argument named after its `vislcg3`
long option in snake_case: `--num-windows 4` is `num_windows=4`. An option
whose value `vislcg3` makes optional is `True` without one, as `--trace` is
`trace=True`. `None` leaves an option off, as not passing it does.

```python
grammar.apply(text, trace=True, num_windows=4)
grammar.apply(text, input_format="apertium", output_format="apertium")
```

`input_format` and `output_format` stand for the `--in-*` and `--out-*`
options: `"cg"` (the default), `"niceline"`, `"apertium"`, `"fst"`, `"plain"`
and `"jsonl"`; `cg3.Format` is their type. An option `apply` does not know
raises `TypeError`, as it would for any Python function. The options a
grammar sets with `CMDARGS` apply as they do in `vislcg3`, and an `EXTERNAL`
rule starts its process as `vislcg3` starts it.

## Grammar information

`grammar.info()` gives what `vislcg3 -v` reports about a grammar:

```python
grammar.info()
# GrammarInfo(sections=1, templates=0, rules=3, sets=25, tags=31, rules_any=0,
#             has_dependency_rules=False, has_relation_rules=False)
```

## Threads

Loading and applying release the GIL while the library works, so threads can
apply one grammar at the same time and run in parallel:

```python
from concurrent.futures import ThreadPoolExecutor

with ThreadPoolExecutor() as pool:
    outputs = [r.output for r in pool.map(grammar.apply, texts)]
```

## Errors

A failure the library reports raises a `cg3.Cg3Error`:

- `cg3.GrammarError` for a grammar that does not load or parse. A parse
  failure's message quotes every error in the grammar's source, as `vislcg3`
  prints it.
- `cg3.RunError` for a run that fails. The grammar stays usable.

An argument of the wrong type or value raises the `TypeError`, `ValueError`,
`OverflowError` or `OSError` Python raises for it, and a file that cannot be
read raises what `open()` would.

## Logging

The library's log events, from loading a grammar and from running one, go to
the standard `logging` module once the call returns, on loggers under `cg3`
named after the Rust module they came from, such as
`cg3.apertium_applicator`. Python's own levels apply, with the library's
trace level at 5. Events below the level of the `cg3` logger at the start of a
call are not kept, so set that logger's level to see more:

```python
import logging

logging.getLogger("cg3").setLevel(logging.DEBUG)
```

Whatever the logging setup, `apply` still returns its run's warnings.

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

The tests build `cg-comp` and `vislcg3` with cargo, or take them from the
directory `CG3_BIN` names.

## Licence

GPL-3.0-or-later. The package carries the licence's text.
