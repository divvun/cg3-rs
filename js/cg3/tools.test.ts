// convert, mwesplit and grammar.proc, against a fake glue.

import { FakeGlue } from "../test/fake-glue.ts";
import { assert, assertEqual, runTests, test, thrownCode } from "../test/harness.ts";
import { Core } from "./core.ts";
import { Cg3Error } from "./errors.ts";
import { grammarReader } from "./grammar.ts";
import { type ConvertOptions, type ProcOptions, tools } from "./tools.ts";

async function started() {
  const glue = new FakeGlue();
  const core = new Core(glue, new URL("http://127.0.0.1:9/never.wasm"));
  await core.init(new Uint8Array(0));
  return { glue, ...tools(core), ...grammarReader(core) };
}

// [spec:cg3:req:wasm.tools/test]
test("convert and mwesplit hand the input and the options' JSON to the module, and return what it writes", async () => {
  const { glue, convert, mwesplit } = await started();
  const options: ConvertOptions = { input: "apertium", output: "jsonl", subDelim: "#", depDelimit: true, ltr: true };
  assertEqual(convert("a", options), "a", "convert");
  assertEqual(convert("b"), "b", "convert without options");
  assertEqual(mwesplit("c"), "c", "mwesplit");
  assertEqual(
    glue.conversions.join("; "),
    'convert a {"input":"apertium","output":"jsonl","subDelim":"#","depDelimit":true,"ltr":true}; convert b {}; mwesplit c',
    "calls",
  );
});

// [spec:cg3:req:wasm.tools/test]
test("grammar.proc hands the input and the options' JSON to the module, and returns what it writes", async () => {
  const { glue, readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1]));
  const options: ProcOptions = { streamFormat: "matxin", sections: 2, wordformCase: true };
  assertEqual(grammar.proc("x", options), "x", "proc");
  assertEqual(grammar.proc("y"), "y", "proc without options");
  const runs = glue.made[0]?.runs.map(([input, json]) => `${input} ${json}`).join("; ");
  assertEqual(runs, 'x {"streamFormat":"matxin","sections":2,"wordformCase":true}; y {}', "runs");
});

// [spec:cg3:req:wasm.tools/test]
// [spec:cg3:req:wasm.errors+1/test]
test("what the tools cannot use throws invalid-argument without entering the module", async () => {
  const { glue, convert, mwesplit, readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1]));
  const before = glue.entered;
  const bad: [string, () => unknown][] = [
    ["a number to convert", () => convert(3 as unknown as string)],
    ["bytes to split", () => mwesplit(new Uint8Array(1) as unknown as string)],
    ["null to proc", () => grammar.proc(null as unknown as string)],
    ["an array of convert options", () => convert("x", [] as unknown as ConvertOptions)],
    ["a string of proc options", () => grammar.proc("x", "trace" as unknown as ProcOptions)],
    ["a BigInt option", () => convert("x", { depDelimit: 3n } as unknown as ConvertOptions)],
  ];
  for (const [what, call] of bad) {
    assertEqual(thrownCode(Cg3Error, call), "invalid-argument", what);
  }
  assertEqual(glue.entered, before, "calls into the module");
});

// [spec:cg3:req:wasm.tools/test]
// [spec:cg3:req:wasm.errors+1/test]
test("a failure the library reports throws run-failed with its message, and the grammar stays usable", async () => {
  const { glue, convert, mwesplit, readGrammar } = await started();
  assertEqual(thrownCode(Cg3Error, () => convert("fail")), "run-failed", "convert");
  assertEqual(thrownCode(Cg3Error, () => mwesplit("fail")), "run-failed", "mwesplit");
  const grammar = readGrammar(new Uint8Array([1]));
  const made = glue.made[0];
  assert(made !== undefined, "the glue made a grammar");
  made.failure = "EXTERNAL on line 3 could not be started";
  assertEqual(thrownCode(Cg3Error, () => grammar.proc("x")), "run-failed", "proc");
  made.failure = null;
  assertEqual(grammar.proc("y"), "y", "the next run");
});

// [spec:cg3:req:wasm.handle-lifetime/test]
test("a freed grammar throws freed from proc", async () => {
  const { readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1]));
  grammar.free();
  assertEqual(thrownCode(Cg3Error, () => grammar.proc("x")), "freed", "proc");
});

// [spec:cg3:req:wasm.init/test]
test("the tools throw not-initialised before init", () => {
  const glue = new FakeGlue();
  const { convert, mwesplit } = tools(new Core(glue, new URL("http://127.0.0.1:9/never.wasm")));
  assertEqual(thrownCode(Cg3Error, () => convert("x")), "not-initialised", "convert");
  assertEqual(thrownCode(Cg3Error, () => mwesplit("x")), "not-initialised", "mwesplit");
  assertEqual(glue.entered, 0, "calls into the module");
});

await runTests("cg3/tools");
