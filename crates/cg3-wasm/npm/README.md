# @divvun/cg3

[VISL CG-3](https://visl.sdu.dk/cg3.html) for JavaScript: load a compiled
Constraint Grammar (`.cg3b`) and run it in a browser, in Deno or in Node. It is
the [cg3-rs](https://github.com/divvun/cg3-rs) library compiled to WebAssembly,
and a run gives the output `vislcg3` gives for the same grammar, input and
options.

```js
import init, { Grammar } from "@divvun/cg3";

// Browsers and Deno fetch the .wasm file that sits beside the module.
await init();

const bytes = new Uint8Array(await (await fetch("grammar.cg3b")).arrayBuffer());
const grammar = Grammar.fromBytes(bytes);

const { output, warnings } = grammar.apply(input, { trace: true });

grammar.free();
```

In Node, which cannot fetch a file, pass the module's bytes to `init`:

```js
import { readFile } from "node:fs/promises";
import init, { Grammar } from "@divvun/cg3";

await init({
  module_or_path: await readFile(new URL(import.meta.resolve("@divvun/cg3/cg3_bg.wasm"))),
});
```

## The API

- `Grammar.fromBytes(bytes)` loads a `.cg3b` (as `cg-comp` writes it). It
  throws an `Error` carrying the reason when the bytes are not a grammar.
- `grammar.apply(input, options?)` runs the grammar on one input and returns
  `{ output, warnings }`: the output as a string, and every warning and
  non-fatal error the run logged. A run that fails throws; the grammar stays
  usable. Each call is a run of its own.
- `grammar.info()` returns what `vislcg3 -v` reports: the numbers of sections,
  templates, rules, sets and tags, and whether the grammar has dependency or
  relation rules.
- `grammar.free()` releases the grammar's memory.

`options` names each run option after its `vislcg3` long option in camelCase:
`--num-windows 4` is `{ numWindows: 4 }`, `--trace` is `{ trace: true }`. The
`input` and `output` formats are `"cg"` (the default), `"niceline"`,
`"apertium"`, `"fst"`, `"plain"` and `"jsonl"`. An option the package does not
know is an error. The options a grammar sets with `CMDARGS` apply as they do
in `vislcg3`.

Textual grammars are not read: compile them with `cg-comp` first. `EXTERNAL`
rules fail their run, since there is no process to start.

## Licence

GPL-3.0-or-later.
