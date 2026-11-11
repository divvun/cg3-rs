// A trap in one entry point's module, with both started: the module that
// trapped is poisoned, and the other runs on. `variant` names the entry point
// that panics.

import type * as CompileTesting from "../cg3/compile-testing.ts";
import type * as Testing from "../cg3/testing.ts";
import { assert, assertEqual, readBytes, runTests, scenarioArgs, test, thrownCode } from "./harness.ts";

const args = scenarioArgs();
const compile: typeof CompileTesting = await import(args.entries.compile.module);
const main: typeof Testing = await import(args.entries.main.module);
await compile.init(await readBytes(args.entries.compile.wasm));
await main.init(await readBytes(args.entries.main.wasm));
const select = await readBytes(new URL("contract/select.cg3b", args.fixtures).href);
const source = new TextDecoder().decode(await readBytes(new URL("contract/select.cg3", args.fixtures).href));

const trapped = args.variant === "compile" ? compile : main;
const survivor = args.variant === "compile" ? main : compile;

// [spec:cg3:req:wasm.compile-errors/test]
test(`a panic in the ${args.variant} module poisons that module only`, () => {
  assertEqual(thrownCode(trapped.Cg3Error, () => trapped.testing.panic("boom")), "poisoned", "the panic");
  assertEqual(thrownCode(trapped.Cg3Error, () => trapped.version()), "poisoned", "a later call into it");
  assertEqual(survivor.version(), args.version, "the other entry point's module");
});

// [spec:cg3:req:wasm.compile-errors/test]
test("the other entry point still compiles or runs", () => {
  if (args.variant === "compile") {
    assertEqual(thrownCode(compile.Cg3Error, () => compile.compileGrammar(source)), "poisoned", "compileGrammar");
    using grammar = main.readGrammar(select);
    assert(grammar.info().rules > 0, "the main module loads a grammar");
  } else {
    assertEqual(thrownCode(main.Cg3Error, () => main.readGrammar(select)), "poisoned", "readGrammar");
    assert(compile.compileGrammar(source).cg3b.length > 0, "the compile module compiles");
  }
});

await runTests(args.label);
