# cg3 (Rust port)

An idiomatic Rust re-implementation of [**VISL CG-3**](https://edu.visl.dk/cg3.html),
the reference [Constraint Grammar](https://en.wikipedia.org/wiki/Constraint_Grammar)
engine. Constraint Grammar is a rule-based formalism for disambiguating and
annotating morphologically-analysed text: given the alternative readings of each
word (a *cohort*), CG rules `SELECT` / `REMOVE` / `ADD` / `MAP` / `SUBSTITUTE`
readings by contextual tests, and assign dependency and relation structure. CG-3
is used extensively in rule-based language technology — for example throughout
[Divvun](https://divvun.no/)'s grammar checkers and proofing tools, and in
machine translation via [Apertium](https://wiki.apertium.org/wiki/Constraint_Grammar).

This repository contains:

- `crates/cg3/` — the Rust port: the library `cg3` plus eight command-line
  binaries (`vislcg3`, `cg-comp`, `cg-proc`, `cg-conv`, `cg-relabel`,
  `cg-mwesplit`, `cg-annotate`, `cg-merge-annotations`).
- `docs/spec/port/` — the behavioral specification (per-symbol `def`/`sem`
  rules) that pins the port to the C++ behavior of
  [upstream CG-3](https://github.com/GrammarSoft/cg3), which served as the
  porting reference.
- `test/` — the upstream conformance corpus (`T_*` fixtures, `runall.pl`, and
  the Apertium suite), reproduced byte-for-byte by the port.

## Scope

Core engine + command-line tools only, and **byte-compatible with the current
`.cg3b` binary grammar format (rev 13898)**. Out of scope by design: the
`libcg3` C API and its language bindings (SWIG / Python / WASM), and the legacy
pre-13898 `.cg3b` reader.

## Status

The port is complete, built in four waves:

1. **Markup** — a per-symbol behavioral spec (`def` + `sem` rules) extracted
   from the C++, under `docs/spec/port/`.
2. **Literal port** — every C++ symbol translated 1:1 (bug-for-bug) into Rust,
   the quirks reproduced and flagged.
3. **Tests** — **159 tests** pinning the spec (unit + integration + a golden
   harness), plus the full upstream corpus: `test/runall.pl` (69 grammars ×
   five sub-tests each) and the Apertium suite, reproduced byte-identically.
4. **Idiomatization** — the literal port reshaped into native Rust — `Result`
   errors, typed identity newtypes (`TagHash` / `SetNumber` / `GlobalNumber`),
   `Option` in place of zero/`MAX` sentinels, `bitflags` sets, trait composition
   instead of the C++ virtual applicators, native UTF-8 strings, and no global
   mutable state — with every spec rule still covered and all tests green.

All 796 symbols clear all four wave gates; the `.cg3b` grammar/stream and JSONL
wire formats and the tag-hash ordering are byte-identical to upstream.

## Building

There is no crates.io release; build from a checkout of
[`divvun/cg3-rs`](https://github.com/divvun/cg3-rs):

```sh
cargo build                 # the library + all eight binaries
cargo nextest run -p cg3    # the full test suite (159 tests)
# or: cargo test -p cg3
```

The `test/runall.pl` / `test/Apertium` conformance harnesses additionally expect
`build/src/<tool>` symlinks pointing at `target/debug/<tool>` (as upstream's does).

## Binaries

| Binary | Purpose |
|--------|---------|
| `vislcg3` | The engine: apply a textual or compiled grammar to a stream of cohorts — disambiguation, mapping, dependency/relation analysis. |
| `cg-comp` | Compile a textual grammar to the binary `.cg3b` form. |
| `cg-proc` | Apertium-style processor (reads/writes the FST stream format). |
| `cg-conv` | Convert between stream formats (CG, Niceline, Apertium, Matxin, FST, plain, JSONL, binary). |
| `cg-relabel` | Rewrite set/tag labels in a grammar. |
| `cg-mwesplit` | Split multi-word-expression cohorts into one cohort per component word. |
| `cg-annotate` / `cg-merge-annotations` | Profiling / coverage-annotation tooling. |

### Example

```sh
$ cargo build
$ ./target/debug/cg-comp test/T_Select/grammar.cg3 grammar.cg3b
$ ./target/debug/vislcg3 -g grammar.cg3b < test/T_Select/input.txt
```

A CG stream is a sequence of cohorts (`"<word>"`), each followed by tab-indented
readings (`"baseform" tags…`); rules pick, drop, add and rewrite readings by
their tags and context. Given

```
"<word>"
	"word" notwanted
	"word" wanted
```

and a grammar containing `SELECT (wanted) ;`, `vislcg3` keeps only the matching
reading:

```
"<word>"
	"word" wanted
```

## Module map

The port mirrors the C++ source file-for-file:

- **Core model** — `grammar`, `rule`, `set`, `tag`, `cohort`, `reading`,
  `window`, `single_window`, `contextual_test`, `cohort_iterator`.
- **Grammar I/O** — `textual_parser` (the `.cg3` compiler), `binary_grammar`
  (`.cg3b` read/write), `grammar_writer`.
- **Engine** — `grammar_applicator` (`run_rules`, `run_grammar`,
  `run_contextual_test`, `match_set`, `reflow`, `context`), `profiler`,
  `relabeller`.
- **Stream formats** — eight applicators (CG, Niceline, Apertium, Matxin, FST,
  Plaintext, JSONL, Binary) behind a `StreamFormat` trait, plus
  `format_converter`.
- **CLI** — the eight `src/bin/` tools and `options` parsing.

See `cargo doc -p cg3 --open` for the full surface.

## Specification

Every ported symbol carries `// [spec:cg3:def:…]` / `// [spec:cg3:sem:…]`
annotations tying the code to a rule under `docs/spec/port/`, and every rule is
verified by a test carrying the matching `…/test` facet.

## License

**GPL-3.0-or-later**, matching upstream CG-3. See [`COPYING`](COPYING).

This is a port. All credit for the Constraint Grammar formalism, the original
design and algorithms, and the C++ implementation goes to the **VISL CG-3**
project — [GrammarSoft ApS](https://grammarsoft.com/) and Tino Didriksen, with
contributions from Kevin Brubeck Unhammer, Francis M. Tyers, and Daniel Swanson
(<https://github.com/GrammarSoft/cg3>). See [`AUTHORS`](AUTHORS).
