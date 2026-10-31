// What a grammar costs in the package, beside what it costs vislcg3:
//
//   node js/build.mjs && node js/bench.mjs VISLCG3 GRAMMAR.cg3b INPUT
//
// Prints the .wasm and .cg3b sizes, the package's start and load times and
// the memory the module holds once the grammar is loaded and after a run, and
// each run's wall time, and fails unless both runs write the same bytes.

import { spawnSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { init, readGrammar, setLogLevel } from "./cg3/dist/index.js";
import initGlue from "./cg3/dist/pkg/cg3_wasm.js";

const [vislcg3, grammarPath, inputPath] = process.argv.slice(2);
if (!vislcg3 || !grammarPath || !inputPath) {
  console.error("usage: bench.mjs VISLCG3 GRAMMAR.cg3b INPUT");
  process.exit(2);
}
const mib = (bytes) => `${(bytes / 1048576).toFixed(1)} MiB`;
const ms = (t) => `${t.toFixed(0)} ms`;
const wasmFile = new URL("./cg3/dist/pkg/cg3_wasm_bg.wasm", import.meta.url);

// The wrapper does not expose the module's memory. Instantiating the glue
// first gets it; the wrapper's init then finds the instance already made.
let t = performance.now();
const { memory } = await initGlue({ module_or_path: readFileSync(wasmFile) });
await init();
const initMs = performance.now() - t;
setLogLevel("off");
const bytes = readFileSync(grammarPath);
const input = readFileSync(inputPath, "utf8");

t = performance.now();
const grammar = readGrammar(bytes);
const loadMs = performance.now() - t;
const loaded = memory.buffer.byteLength;

t = performance.now();
const { output, warnings } = grammar.apply(input);
const applyMs = performance.now() - t;
const afterRun = memory.buffer.byteLength;
const info = grammar.info();
grammar.free();

t = performance.now();
const native = spawnSync(vislcg3, ["-g", grammarPath, "-I", inputPath], { maxBuffer: 1 << 30 });
const nativeMs = performance.now() - t;
if (native.status !== 0) {
  console.error(`vislcg3 failed: ${native.stderr}`);
  process.exit(1);
}
const same = Buffer.compare(Buffer.from(output, "utf8"), native.stdout) === 0;

console.log(`module      ${mib(statSync(wasmFile).size)}, started in ${ms(initMs)}`);
console.log(`grammar     ${mib(bytes.length)}: ${info.rules} rules, ${info.sets} sets, ${info.tags} tags`);
console.log(`load        ${ms(loadMs)}; module memory ${mib(loaded)} loaded, ${mib(afterRun)} after the run`);
console.log(`run         package ${ms(applyMs)}, vislcg3 ${ms(nativeMs)} (process included); ${warnings.length} warnings`);
console.log(`output      ${same ? "the same bytes as vislcg3" : "DIFFERS from vislcg3"}`);
process.exit(same ? 0 : 1);
