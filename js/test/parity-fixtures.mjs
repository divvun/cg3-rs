// The parity check's fixtures: every golden-corpus grammar under test/T_*
// compiled to .cg3b by this checkout's cg-comp, with the input and what this
// checkout's vislcg3 writes for that .cg3b, input and args.txt. The check
// (js/test/parity.ts) runs each through the package and compares the bytes.
// A browser cannot start vislcg3, so the native side is done here, at
// test-build time, and parity.json carries it to every runtime.

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// vislcg3's short options, and the long ones that take a value, as its option
// table declares them. A value for an optional one must be attached (`--trace=2`).
const SHORT = {
  u: "unsafe",
  s: "sections",
  v: "verbose",
  t: "trace",
  T: "text-delimit",
  D: "dep-delimit",
  o: "no-pass-origin",
  e: "show-end-tags",
  2: "vislcg-compat",
};
const REQUIRED = new Set([
  "sections",
  "rules",
  "rule",
  "debug-rules",
  "max-runs",
  "num-windows",
  "soft-limit",
  "hard-limit",
  "nrules",
  "nrules-v",
  "prefix",
]);
const NUMERIC = new Set(["max-runs", "num-windows", "soft-limit", "hard-limit"]);
// The options vislcg3 applies while it loads a grammar, which readGrammar takes.
const LOAD = new Set(["nrules", "nrules-v", "vislcg-compat", "prefix"]);
const BINARY = new Set(["in-binary", "out-binary"]);
const camel = (long) => long.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());

/**
 * The ApplyOptions and LoadOptions an args.txt names, or the reason it names
 * none. An option the package does not offer still becomes a key of the
 * ApplyOptions here: the package refusing it is what makes the test exempt.
 */
export function toOptions(args) {
  const options = {};
  const load = {};
  for (let i = 0; i < args.length; i += 1) {
    const [flag, attached] = args[i].split(/=(.*)/s, 2);
    let long;
    if (flag.startsWith("--")) long = flag.slice(2);
    else if (/^-.$/.test(flag) && SHORT[flag[1]] !== undefined) long = SHORT[flag[1]];
    else return { exempt: `an argument the check cannot read (${args[i]})` };
    if (BINARY.has(long)) return { exempt: "a binary stream, which a string cannot carry" };
    if (long.startsWith("in-") || long.startsWith("out-")) {
      options[long.startsWith("in-") ? "input" : "output"] = long.slice(long.indexOf("-") + 1);
      continue;
    }
    let value = attached;
    if (value === undefined && REQUIRED.has(long)) {
      i += 1;
      value = args[i];
    }
    const given = LOAD.has(long) ? load : options;
    given[camel(long)] = value === undefined ? true : NUMERIC.has(long) ? Number(value) : value;
  }
  return { options, load };
}

/**
 * Writes the fixtures for every corpus test in `corpus` to `out`, with the
 * cg-comp and vislcg3 in `bin`, and parity.json listing them in name order.
 */
export function buildParityFixtures(bin, corpus, out) {
  mkdirSync(out, { recursive: true });
  const tests = [];
  const names = readdirSync(corpus)
    .filter((name) => name.startsWith("T_"))
    .sort();
  for (const name of names) {
    const dir = join(corpus, name);
    if (!existsSync(join(dir, "grammar.cg3")) || !existsSync(join(dir, "input.txt"))) continue;
    if (/^\s*EXTERNAL\b/m.test(readFileSync(join(dir, "grammar.cg3"), "utf8"))) {
      tests.push({ name, exempt: "an EXTERNAL rule, whose process a target without processes cannot start" });
      continue;
    }
    const argsFile = join(dir, "args.txt");
    const args = existsSync(argsFile) ? readFileSync(argsFile, "utf8").split(/\s+/).filter(Boolean) : [];
    const { options, load, exempt } = toOptions(args);
    if (exempt !== undefined) {
      tests.push({ name, exempt });
      continue;
    }
    const cg3b = join(out, `${name}.cg3b`);
    const compiled = spawnSync(join(bin, "cg-comp"), ["grammar.cg3", cg3b], { cwd: dir, encoding: "utf8" });
    if (compiled.status !== 0) {
      tests.push({ name, args, compileError: `cg-comp exited ${compiled.status}: ${compiled.error ?? compiled.stderr}` });
      continue;
    }
    copyFileSync(join(dir, "input.txt"), join(out, `${name}.input`));
    const native = spawnSync(join(bin, "vislcg3"), [...args, "-g", cg3b, "-I", "input.txt"], {
      cwd: dir,
      maxBuffer: 1 << 28,
    });
    if (native.error !== undefined) throw native.error;
    writeFileSync(join(out, `${name}.expected`), native.stdout);
    tests.push({ name, args, options, load, nativeFailed: native.status !== 0 });
  }
  writeFileSync(join(out, "parity.json"), `${JSON.stringify({ tests }, null, 1)}\n`);
  return tests.length;
}
