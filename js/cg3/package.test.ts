import { assertEqual, runTests, test } from "../test/harness.ts";
import { entryOf, packageTests } from "../test/packages.ts";

const dir = new URL("./", import.meta.url);
packageTests(dir, "@divvun/cg3");

// [spec:cg3:req:wasm.typed-surface/test]
test("the entry module exports the API", async () => {
  const names = Object.keys(await entryOf(dir)).sort();
  assertEqual(names.join(" "), "Cg3Error Grammar convert init mwesplit readGrammar setLogLevel setLogger version", "exports");
});

await runTests("cg3/package");
