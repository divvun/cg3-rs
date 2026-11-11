import { assert, assertEqual, runTests, test } from "../test/harness.ts";
import { entryOf, packageTests } from "../test/packages.ts";

const dir = new URL("./", import.meta.url);
packageTests(dir, "@divvun/cg3");

// [spec:cg3:req:wasm.typed-surface/test]
test("the entry module exports the API", async () => {
  const names = Object.keys(await entryOf(dir)).sort();
  assertEqual(names.join(" "), "Cg3Error Grammar convert init mwesplit readGrammar setLogLevel setLogger version", "exports");
});

// [spec:cg3:req:wasm.typed-surface/test]
// [spec:cg3:req:wasm.compile/test]
test("the compile entry module exports its API, and the same Cg3Error", async () => {
  const compile = await entryOf(dir, "./compile");
  const names = Object.keys(compile).sort();
  assertEqual(names.join(" "), "Cg3Error compileGrammar init relabel setLogLevel setLogger toText version", "exports");
  assert(compile.Cg3Error === (await entryOf(dir)).Cg3Error, "one Cg3Error class for both entry points");
});

await runTests("cg3/package");
