// What a grammar costs in the package, beside what it costs vislcg3:
//
//   node crates/cg3-wasm/parity/bench.mjs VISLCG3 GRAMMAR.cg3b INPUT
//
// Prints the .wasm and .cg3b sizes, the package's load time and the memory
// the module holds once the grammar is loaded and after a run, and each run's
// wall time, and fails unless both runs write the same bytes.
import { spawnSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";
import { performance } from "node:perf_hooks";
import init, { Grammar } from "../pkg/cg3.js";

const [vislcg3, grammarPath, inputPath] = process.argv.slice(2);
if (!vislcg3 || !grammarPath || !inputPath) {
  console.error("usage: bench.mjs VISLCG3 GRAMMAR.cg3b INPUT");
  process.exit(2);
}
const mib = (bytes) => `${(bytes / 1048576).toFixed(1)} MiB`;
const ms = (t) => `${t.toFixed(0)} ms`;
const wasmFile = new URL("../pkg/cg3_bg.wasm", import.meta.url);

let t = performance.now();
const wasm = await init({ module_or_path: readFileSync(wasmFile) });
const initMs = performance.now() - t;
const bytes = readFileSync(grammarPath);
const input = readFileSync(inputPath, "utf8");

t = performance.now();
const grammar = Grammar.fromBytes(bytes);
const loadMs = performance.now() - t;
const loaded = wasm.memory.buffer.byteLength;

t = performance.now();
const { output, warnings } = grammar.apply(input);
const applyMs = performance.now() - t;
const afterRun = wasm.memory.buffer.byteLength;
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

console.log(`module      ${mib(statSync(wasmFile).size)}, instantiated in ${ms(initMs)}`);
console.log(`grammar     ${mib(bytes.length)}: ${info.rules} rules, ${info.sets} sets, ${info.tags} tags`);
console.log(`load        ${ms(loadMs)}; module memory ${mib(loaded)} loaded, ${mib(afterRun)} after the run`);
console.log(`run         package ${ms(applyMs)}, vislcg3 ${ms(nativeMs)} (process included); ${warnings.length} warnings`);
console.log(`output      ${same ? "the same bytes as vislcg3" : "DIFFERS from vislcg3"}`);
process.exit(same ? 0 : 1);
