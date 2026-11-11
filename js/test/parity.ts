// The parity check: the package against vislcg3 on the golden corpus.
// js/test/parity-fixtures.mjs writes each corpus grammar's .cg3b, its input
// and what vislcg3 writes for them at test-build time; this module starts the
// package as it ships, runs every one through it with the same options and
// compares the output with vislcg3's, byte for byte. It is written once for
// every environment: parity-runtime.ts runs it under Node, Bun and Deno, and
// parity-browser.ts in a browser, on a page and in a Web Worker.

import type * as Cg3 from "../cg3/index.ts";
import type { ApplyOptions, LoadOptions } from "../cg3/index.ts";

type Package = typeof Cg3;

/** Where the check finds the package and the fixtures, as URLs the environment can read. */
export interface ParityArgs {
  module: string;
  /** URL of the directory of built fixtures, ending in a slash. */
  fixtures: string;
}

/** Reads the bytes at a URL: with fetch in a browser, from the file system elsewhere. */
export type ReadBytes = (url: string) => Promise<Uint8Array>;

/** One corpus test as parity.json describes it. */
type Fixture =
  | { name: string; exempt: string }
  | { name: string; args: string[]; compileError: string }
  | { name: string; args: string[]; options: ApplyOptions; load: LoadOptions; nativeFailed: boolean };

/** Every corpus test by outcome, each as its name and what happened. */
export interface ParityResult {
  same: string[];
  exempt: string[];
  differ: string[];
}

/** Where `actual` first differs from `expected`, or null when they are the same bytes. */
function difference(actual: Uint8Array, expected: Uint8Array): string | null {
  let i = 0;
  while (i < actual.length && i < expected.length && actual[i] === expected[i]) i += 1;
  if (i === actual.length && i === expected.length) return null;
  const decoder = new TextDecoder();
  const around = (bytes: Uint8Array) => JSON.stringify(decoder.decode(bytes.subarray(Math.max(0, i - 30), i + 50)));
  return `byte ${i} of ${actual.length} differs from vislcg3's ${expected.length}: ${around(actual)}, vislcg3 ${around(expected)}`;
}

/** The error a call threw, or null when it returned. */
function thrown(f: () => void): unknown {
  try {
    f();
    return null;
  } catch (error) {
    return error;
  }
}

// [spec:cg3:req:wasm.parity+2/test]
// [spec:cg3:req:wasm.load-options/test]
/**
 * Starts the package at `args.module` from the `.wasm` file beside it, as a
 * caller who passes init nothing does, and runs every corpus test in
 * parity.json through it. Throws only when the package or parity.json cannot
 * be loaded; an output that differs, or a run that fails where vislcg3 does
 * not, is a difference.
 */
export async function runParity(args: ParityArgs, read: ReadBytes): Promise<ParityResult> {
  const pkg: Package = await import(args.module);
  await pkg.init();
  pkg.setLogLevel("off");
  const fixture = (name: string) => read(new URL(`parity/${name}`, args.fixtures).href);
  const { tests }: { tests: Fixture[] } = JSON.parse(new TextDecoder().decode(await fixture("parity.json")));
  const result: ParityResult = { same: [], exempt: [], differ: [] };
  for (const test of tests) {
    const { name } = test;
    if ("exempt" in test) {
      result.exempt.push(`${name}: ${test.exempt}`);
      continue;
    }
    if ("compileError" in test) {
      result.differ.push(`${name}: ${test.compileError}`);
      continue;
    }
    const [bytes, input, expected] = await Promise.all(
      [".cg3b", ".input", ".expected"].map((suffix) => fixture(`${name}${suffix}`)),
    );
    if (bytes === undefined || input === undefined || expected === undefined) throw new Error(`${name}: missing fixtures`);
    const ran: { output?: string } = {};
    const failure = thrown(() => {
      const grammar = pkg.readGrammar(bytes, test.load);
      try {
        ran.output = grammar.apply(new TextDecoder().decode(input), test.options).output;
      } finally {
        grammar.free();
      }
    });
    const message = failure instanceof Error ? failure.message : String(failure);
    if (failure instanceof pkg.Cg3Error && failure.code === "invalid-argument" && /unknown field/.test(message)) {
      result.exempt.push(`${name}: an option the package does not offer (${test.args.join(" ")})`);
    } else if (failure !== null) {
      if (test.nativeFailed) result.same.push(name);
      else result.differ.push(`${name}: vislcg3 succeeded, and the package threw ${message}`);
    } else if (test.nativeFailed || ran.output === undefined) {
      result.differ.push(`${name}: vislcg3 failed, and the package succeeded`);
    } else {
      const found = difference(new TextEncoder().encode(ran.output), expected);
      if (found === null) result.same.push(name);
      else result.differ.push(`${name}: ${found}`);
    }
  }
  return result;
}

/**
 * The lines that report `result` under `label`, and whether it passed: no
 * corpus test differs, and at least one was compared. Each exemption is named
 * with its reason.
 */
export function report(label: string, result: ParityResult): { lines: string[]; passed: boolean } {
  const { same, exempt, differ } = result;
  const passed = differ.length === 0 && same.length > 0;
  const counts = `${same.length} corpus tests the same as vislcg3, ${exempt.length} exempt, ${differ.length} different`;
  const lines = [`${passed ? "ok" : "not ok"} - ${label}: ${counts}`];
  for (const line of exempt) lines.push(`# exempt ${line}`);
  for (const line of differ) lines.push(`  DIFFERS ${line}`);
  return { lines, passed };
}
