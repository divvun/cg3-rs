// The built package under Node, as a user imports it: node smoke/node.mjs
// GRAMMAR.cg3b INPUT EXPECTED. Exits nonzero unless the output matches.
import { readFile } from "node:fs/promises";
import init, { Grammar } from "../pkg/cg3.js";

const [grammarPath, inputPath, expectedPath] = process.argv.slice(2);
await init({ module_or_path: await readFile(new URL("../pkg/cg3_bg.wasm", import.meta.url)) });

const grammar = Grammar.fromBytes(await readFile(grammarPath));
const { output, warnings } = grammar.apply(await readFile(inputPath, "utf8"), { showEndTags: true });
const lines = (s) => s.split("\n").filter((l) => l.trim() !== "").map((l) => l.trim().split(/\s+/).join(" "));
const expected = lines(await readFile(expectedPath, "utf8"));
const got = lines(output);
grammar.free();

if (JSON.stringify(got) !== JSON.stringify(expected)) {
  console.error("output differs from expected:\n" + output);
  process.exit(1);
}
if (warnings.length !== 0) {
  console.error("unexpected warnings: " + JSON.stringify(warnings));
  process.exit(1);
}
let refused = false;
try {
  Grammar.fromBytes(new Uint8Array([1, 2, 3]));
} catch (e) {
  refused = e instanceof Error;
}
if (!refused) {
  console.error("loading bytes that are no grammar did not throw an Error");
  process.exit(1);
}
console.log("ok");
