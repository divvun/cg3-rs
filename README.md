# cg3 (Rust port)

A behavior-oriented Rust port of [**VISL CG-3**](https://edu.visl.dk/cg3.html),
the reference [Constraint Grammar](https://en.wikipedia.org/wiki/Constraint_Grammar)
engine. Constraint Grammar is a rule-based formalism for disambiguating and
annotating morphologically-analysed text: given the alternative readings of each
word (a *cohort*), CG rules `SELECT` / `REMOVE` / `ADD` / `MAP` / `SUBSTITUTE`
readings by contextual tests, and assign dependency and relation structure. CG-3
is used extensively in rule-based language technology — for example throughout
[Divvun](https://divvun.no/)'s grammar checkers and proofing tools, and in
machine translation via [Apertium](https://wiki.apertium.org/wiki/Constraint_Grammar).

This repository contains:

- `crates/cg3/` — the Rust port: the library `cg3` plus six command-line
  binaries (`vislcg3`, `cg-comp`, `cg-proc`, `cg-conv`, `cg-relabel`,
  `cg-mwesplit`), and two SQLite-backed profiling tools (`cg-annotate`,
  `cg-merge-annotations`) behind the optional `profiler` feature.
- `docs/spec/port/` — the behavioral specification (per-symbol `def`/`sem`
  rules) that pins the port to the C++ behavior of
  [upstream CG-3](https://github.com/GrammarSoft/cg3), which served as the
  porting reference.
- `test/` — the upstream conformance corpus (`T_*` grammar/input/expected
  fixtures + the Apertium suite), driven by the Rust harnesses in
  `crates/cg3/tests/`.

## Relationship to the C++ codebase

The upstream C++ tree—checked out alongside this repository as
`../cg3-old-port` in this workspace—is the behavioral reference. This is a
direct port of its grammar model, parser, rule engine, stream applicators, and
CLI behavior. It is fully compatible with CG-3 grammar source and the `.cg3b`
binary ABI. The Rust crate API is distinct from the native `libcg3` C API.

| Area | This Rust port | C++ reference |
|------|----------------|---------------|
| Architecture | Composition around one owned `GrammarApplicator`; a `StreamFormat` strategy models virtual print dispatch. | Deep class hierarchy with virtual multiple inheritance; `FormatConverter` inherits all format applicators over one virtual base. |
| Runtime model | Typed arena IDs, ownership and borrowing, typed flag sets, and `Result` at recoverable API boundaries. | Owning/raw pointers, inheritance, integer bit masks, exceptions, and `CG3Quit` process exits. |
| Text and containers | UTF-8 `String`, Rust `regex`, `serde_json`, and Rust collections or ported flat/sorted containers. | ICU UTF-16 strings and regex, RapidJSON, Boost containers, and STL containers. |
| Build and dependencies | Cargo; the default engine and six CLI tools are pure Rust. SQLite is optional behind `profiler`. | CMake; links ICU/Boost and builds the native `libcg3` surface, with optional SQLite and bindings. |
| Public integrations | Rust crate API and command-line tools, with source and `.cg3b` ABI compatibility. It does not currently ship the native `libcg3` C API, SWIG/Python package, or Emscripten/WASM build. | `libcg3` C API, SWIG Python bindings, and an Emscripten/WASM target in addition to the tools. |
| Binary grammars | Fully byte-compatible with the current `.cg3b` ABI (revision 13898) and reads the main format from revisions 10373–13898; the separate ancient reader is intentionally absent. | Also carries the separate legacy reader for grammars as old as revision 10043. |

The goal is behavioral compatibility, not a redesign of CG-3 semantics. Some
C++ implementation details are intentionally reproduced when observable,
including selected CLI quirks; memory ownership, dispatch, and error plumbing
use Rust-native mechanisms where that does not change behavior.

## Scope

Core engine + command-line tools only, **fully compatible with CG-3 grammar
source and byte-compatible with the current `.cg3b` binary ABI (rev 13898)**.
Out of scope by design: the native `libcg3` C API and its language bindings
(SWIG / Python / WASM), and the C++ legacy `.cg3b` reader used for revisions
10043–10297.

`FormatConverter`, used by both `cg-conv` and `vislcg3`, supports every input
and output arm present in the C++ converter: CG, Apertium, Niceline, plaintext,
FST, JSONL, and binary. Matxin is available through `cg-proc`, but neither the
C++ nor Rust `FormatConverter` has a Matxin switch arm. Consequently the
upstream `cg-conv --out-matxin` quirk is preserved: that option leaves the
default CG output selected.

Known edge differences are concentrated around replacements for ICU and
RapidJSON:

- plaintext tokenization currently recognizes ASCII punctuation rather than
  ICU's full Unicode punctuation categories;
- a few non-ASCII case-folding and combining-mark decisions use Rust standard
  Unicode operations instead of ICU and can differ on unusual inputs;
- JSONL is structurally equivalent, but object key order can differ, and Rust
  strings preserve embedded NUL characters that RapidJSON's C-string calls
  truncate.

## Building

There is no crates.io release; build from a checkout of
[`divvun/cg3-rs`](https://github.com/divvun/cg3-rs):

```sh
cargo build                       # the library + the six default binaries
cargo build --features profiler   # also builds cg-annotate / cg-merge-annotations
cargo nextest run -p cg3          # the full test suite (unit + integration + the
                                  # golden/Apertium conformance corpus)
# or: cargo test -p cg3
```

The `profiler` feature is off by default; it pulls in `rusqlite` (bundled
SQLite, which needs a C toolchain to build) for `vislcg3 --profile` and the two
report tools. The base build is pure Rust.

The conformance corpus (the upstream `runall.pl` sub-tests + the Apertium
`cg-proc` suite) is a native part of the test run — `tests/golden.rs` and the
other integration tests drive the real binaries over `test/` and compare with
the expected fixtures. Comparisons ignore blank-only line differences and, where
necessary, stabilize order-dependent mappings/relations. No Perl or external
harness is required.

## Binaries

| Binary | Purpose |
|--------|---------|
| `vislcg3` | The engine: apply a textual or compiled grammar to a stream of cohorts — disambiguation, mapping, dependency/relation analysis. |
| `cg-comp` | Compile a textual grammar to the binary `.cg3b` form. |
| `cg-proc` | Apertium/Matxin-oriented grammar processor. |
| `cg-conv` | Convert between CG, Niceline, Apertium, FST, plaintext, JSONL, and binary streams. |
| `cg-relabel` | Rewrite set/tag labels in a grammar. |
| `cg-mwesplit` | Split multi-word-expression cohorts into one cohort per component word. |
| `cg-annotate` / `cg-merge-annotations` | Profiling / coverage-annotation tooling (SQLite-backed; requires `--features profiler`). |

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

Ported symbols carry `// [spec:cg3:def:…]` / `// [spec:cg3:sem:…]`
annotations tying the code to rules under `docs/spec/port/`. Implementation and
test coverage are checked with the repository's `nplan` metadata; some covered
rules do not yet have a dedicated matching `…/test` facet.

## License

**GPL-3.0-or-later**, matching upstream CG-3. See [`COPYING`](COPYING).

This is a port. All credit for the Constraint Grammar formalism, the original
design and algorithms, and the C++ implementation goes to the **VISL CG-3**
project — [GrammarSoft ApS](https://grammarsoft.com/) and Tino Didriksen, with
contributions from Kevin Brubeck Unhammer, Francis M. Tyers, and Daniel Swanson
(<https://github.com/GrammarSoft/cg3>). See [`AUTHORS`](AUTHORS).
