// The package runs every corpus grammar as vislcg3 does, byte for byte:
//
//   node crates/cg3-wasm/parity/parity.mjs BIN_DIR CORPUS_DIR
//
// BIN_DIR holds the native cg-comp and vislcg3, CORPUS_DIR the test/T_*
// directories. Each grammar is compiled to .cg3b by cg-comp, run by vislcg3
// with its args.txt, and run by the built package (crates/cg3-wasm/pkg) with
// the same options; the two outputs must be the same bytes, or both runs must
// fail. A test is exempt, with its reason printed, when its grammar runs an
// EXTERNAL rule, its stream is binary, or it passes an option the package does
// not offer. Exits nonzero on any difference.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import init, { Grammar } from "../pkg/cg3.js";

const [binArg, corpusArg] = process.argv.slice(2);
if (!binArg || !corpusArg) {
  console.error("usage: parity.mjs BIN_DIR CORPUS_DIR");
  process.exit(2);
}
const [binDir, corpusDir] = [resolve(binArg), resolve(corpusArg)];
await init({ module_or_path: readFileSync(new URL("../pkg/cg3_bg.wasm", import.meta.url)) });

// vislcg3's short options, and the long ones that take a value, as its option
// table declares them. A value for an optional one must be attached (`--trace=2`).
const SHORT = { u: "unsafe", s: "sections", v: "verbose", t: "trace", T: "text-delimit", D: "dep-delimit", o: "no-pass-origin", e: "show-end-tags", "2": "vislcg-compat" };
const REQUIRED = new Set(["sections", "rules", "rule", "debug-rules", "max-runs", "num-windows", "soft-limit", "hard-limit", "nrules", "nrules-v", "prefix"]);
const NUMERIC = new Set(["max-runs", "num-windows", "soft-limit", "hard-limit"]);
const BINARY = new Set(["in-binary", "out-binary"]);
const camel = (long) => long.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());

/** The ApplyOptions an args.txt names, or the reason it names none. */
function toOptions(args) {
  const options = {};
  for (let i = 0; i < args.length; i++) {
    let [flag, attached] = args[i].split(/=(.*)/s, 2);
    let long;
    if (flag.startsWith("--")) long = flag.slice(2);
    else if (/^-.$/.test(flag) && SHORT[flag[1]]) long = SHORT[flag[1]];
    else return { exempt: `argument ${args[i]}` };
    if (BINARY.has(long)) return { exempt: "binary stream" };
    if (long.startsWith("in-") || long.startsWith("out-")) {
      options[long.startsWith("in-") ? "input" : "output"] = long.slice(long.indexOf("-") + 1);
      continue;
    }
    let value = attached;
    if (value === undefined && REQUIRED.has(long)) value = args[++i];
    options[camel(long)] = value === undefined ? true : NUMERIC.has(long) ? Number(value) : value;
  }
  return { options };
}

const work = mkdtempSync(join(tmpdir(), "cg3-parity-"));
const results = { same: [], exempt: [], differ: [] };
const dirs = readdirSync(corpusDir).filter((d) => d.startsWith("T_")).sort();
for (const name of dirs) {
  const dir = join(corpusDir, name);
  if (!existsSync(join(dir, "grammar.cg3")) || !existsSync(join(dir, "input.txt"))) continue;
  const source = readFileSync(join(dir, "grammar.cg3"), "utf8");
  if (/^\s*EXTERNAL\b/m.test(source)) {
    results.exempt.push(`${name}: EXTERNAL rule`);
    continue;
  }
  const argsFile = join(dir, "args.txt");
  const args = existsSync(argsFile) ? readFileSync(argsFile, "utf8").split(/\s+/).filter(Boolean) : [];
  const { options, exempt } = toOptions(args);
  if (exempt) {
    results.exempt.push(`${name}: ${exempt}`);
    continue;
  }

  const cg3b = join(work, `${name}.cg3b`);
  const comp = spawnSync(join(binDir, "cg-comp"), ["grammar.cg3", cg3b], { cwd: dir });
  if (comp.status !== 0) {
    results.differ.push(`${name}: cg-comp failed: ${comp.error ?? comp.stderr}`);
    continue;
  }
  const native = spawnSync(join(binDir, "vislcg3"), [...args, "-g", cg3b, "-I", "input.txt"], { cwd: dir, maxBuffer: 1 << 28 });

  let output, failure;
  try {
    const grammar = Grammar.fromBytes(readFileSync(cg3b));
    try {
      output = grammar.apply(readFileSync(join(dir, "input.txt"), "utf8"), options).output;
    } finally {
      grammar.free();
    }
  } catch (e) {
    failure = e;
  }
  if (failure && /unknown field/.test(String(failure.message))) {
    results.exempt.push(`${name}: an option the package does not offer (${args.join(" ")})`);
  } else if (native.status !== 0 && failure) {
    results.same.push(name);
  } else if (native.status !== 0 || failure) {
    results.differ.push(`${name}: vislcg3 exited ${native.status}; the package ${failure ? `threw ${failure.message}` : "succeeded"}`);
  } else if (Buffer.compare(Buffer.from(output, "utf8"), native.stdout) === 0) {
    results.same.push(name);
  } else {
    results.differ.push(`${name}: outputs differ`);
  }
}
rmSync(work, { recursive: true, force: true });

console.log(`${results.same.length} the same, ${results.exempt.length} exempt, ${results.differ.length} different`);
for (const e of results.exempt) console.log(`  exempt  ${e}`);
for (const d of results.differ) console.log(`  DIFFERS ${d}`);
process.exit(results.differ.length === 0 && results.same.length > 0 ? 0 : 1);
