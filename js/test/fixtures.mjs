// Builds the fixtures the JavaScript suite reads, with this checkout's cg3
// tools. They are made at test-build time rather than committed, so the bytes
// a test compares against are what the checkout's own tools write. Only
// `node js/build.mjs --test` calls this.

import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { buildParityFixtures } from "./parity-fixtures.mjs";

/** Builds the cg3 tools with cargo and returns the directory they are in. */
export function buildTools(root, run) {
  run("cargo", ["build", "-p", "cg3-cli"]);
  return join(root, "target", "debug");
}

function compile(bin, dir, out) {
  const compiled = spawnSync(join(bin, "cg-comp"), ["grammar.cg3", out], { cwd: dir, encoding: "utf8" });
  if (compiled.status !== 0) throw new Error(`cg-comp failed in ${dir}: ${compiled.error ?? compiled.stderr}`);
}

/**
 * Writes every fixture to `out`, with the tools in `bin` and the golden corpus
 * in `corpus`: contract/ holds the grammars contract.ts runs, parity/ the
 * parity check's.
 */
export function buildFixtures(bin, corpus, out) {
  rmSync(out, { recursive: true, force: true });
  const contract = join(out, "contract");
  mkdirSync(contract, { recursive: true });
  compile(bin, join(corpus, "T_Select"), join(contract, "select.cg3b"));
  copyFileSync(join(corpus, "T_Select", "input.txt"), join(contract, "select.input"));
  compile(bin, join(corpus, "T_External"), join(contract, "external.cg3b"));
  const count = buildParityFixtures(bin, corpus, join(out, "parity"));
  console.log(`fixtures: the contract grammars, and ${count} corpus tests for the parity check`);
}
