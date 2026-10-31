// The grammar wrapper, against a fake glue.

import { FakeGlue } from "../test/fake-glue.ts";
import { assert, assertEqual, runTests, test, thrownCode } from "../test/harness.ts";
import { Core } from "./core.ts";
import { Cg3Error } from "./errors.ts";
import { type ApplyOptions, grammarReader } from "./grammar.ts";

async function started() {
  const glue = new FakeGlue();
  const core = new Core(glue, new URL("http://127.0.0.1:9/never.wasm"));
  await core.init(new Uint8Array(0));
  return { glue, ...grammarReader(core) };
}

// [spec:cg3:req:wasm.grammar-load+1/test]
test("readGrammar hands the bytes to the module, and info is the plain object it builds", async () => {
  const { glue, readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1, 2, 3]));
  const info = grammar.info();
  assertEqual(Object.getPrototypeOf(info), Object.prototype, "a plain object");
  assertEqual(
    JSON.stringify(info),
    '{"sections":2,"templates":0,"rules":3,"sets":4,"tags":5,"rulesAny":1,"hasDependencyRules":false,"hasRelationRules":true}',
    "info",
  );
  assertEqual(glue.made.length, 1, "grammars made");
});

// [spec:cg3:req:wasm.grammar-load+1/test]
// [spec:cg3:req:wasm.errors+1/test]
test("bytes the module refuses throw invalid-grammar, and bytes that are not a Uint8Array invalid-argument", async () => {
  const { glue, readGrammar } = await started();
  assertEqual(thrownCode(Cg3Error, () => readGrammar(new Uint8Array([0]))), "invalid-grammar", "no grammar");
  const before = glue.entered;
  for (const notBytes of [[1, 2, 3], "grammar", new ArrayBuffer(3), null]) {
    const code = thrownCode(Cg3Error, () => readGrammar(notBytes as unknown as Uint8Array));
    assertEqual(code, "invalid-argument", String(notBytes));
  }
  assertEqual(glue.entered, before, "calls into the module");
});

// [spec:cg3:req:wasm.errors+1/test]
test("apply sends the options as JSON and returns what the module returns", async () => {
  const { glue, readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1]));
  const options: ApplyOptions = { trace: true, numWindows: 4, input: "apertium", output: "cg" };
  const result = grammar.apply("text", options);
  assertEqual(result.output, "text", "output");
  assertEqual(result.warnings.length, 0, "warnings");
  grammar.apply("again");
  const runs = glue.made[0]?.runs.map(([input, json]) => `${input} ${json}`).join("; ");
  assertEqual(
    runs,
    'text {"trace":true,"numWindows":4,"input":"apertium","output":"cg"}; again {}',
    "runs",
  );
});

// [spec:cg3:req:wasm.errors+1/test]
test("what apply cannot use throws invalid-argument without entering the module", async () => {
  const { glue, readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1]));
  const before = glue.entered;
  const bad: [string, () => unknown][] = [
    ["a number for input", () => grammar.apply(3 as unknown as string)],
    ["null options", () => grammar.apply("x", null as unknown as ApplyOptions)],
    ["an array of options", () => grammar.apply("x", [] as unknown as ApplyOptions)],
    ["a string of options", () => grammar.apply("x", "trace" as unknown as ApplyOptions)],
    ["a BigInt option", () => grammar.apply("x", { maxRuns: 3n } as unknown as ApplyOptions)],
  ];
  for (const [what, call] of bad) {
    assertEqual(thrownCode(Cg3Error, call), "invalid-argument", what);
  }
  assertEqual(glue.entered, before, "calls into the module");
});

// [spec:cg3:req:wasm.errors+1/test]
test("a failed run throws run-failed with the library's message, and the grammar stays usable", async () => {
  const { glue, readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1]));
  const made = glue.made[0];
  assert(made !== undefined, "the glue made a grammar");
  made.failure = "EXTERNAL on line 3 could not be started";
  let error: unknown = null;
  try {
    grammar.apply("x");
  } catch (thrown) {
    error = thrown;
  }
  assert(error instanceof Cg3Error && error.code === "run-failed", "run-failed");
  assertEqual(error.message, "EXTERNAL on line 3 could not be started", "message");
  made.failure = null;
  assertEqual(grammar.apply("y").output, "y", "the next run");
});

// [spec:cg3:req:wasm.handle-lifetime/test]
test("a freed grammar throws freed from info and apply, and is freed once", async () => {
  const { glue, readGrammar } = await started();
  const grammar = readGrammar(new Uint8Array([1]));
  grammar.free();
  grammar.free();
  assertEqual(glue.freed.join(), "grammar 0", "freed");
  assertEqual(thrownCode(Cg3Error, () => grammar.info()), "freed", "info");
  assertEqual(thrownCode(Cg3Error, () => grammar.apply("x")), "freed", "apply");
});

await runTests("cg3/grammar");
