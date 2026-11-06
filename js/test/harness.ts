// A small test harness for Node, Bun and Deno. Nothing here has node: types,
// so deno check can check the suite without @types/node. Each scenario runs in
// a process of its own, because a module instance, once started, stays
// started for the life of the process.

import type * as Entry from "../cg3/index.ts";

/** What the package exports. */
export type Package = typeof Entry;

/** What `run.mjs` hands each scenario process, as JSON in its first argument. */
export interface ScenarioArgs {
  /** URL of the package entry module under test. */
  module: string;
  /** `file:` URL of the package's `.wasm` file. */
  wasm: string;
  /** `http:` URL the runner serves the same `.wasm` file at. */
  http: string;
  package: string;
  /** The version the package declares. */
  version: string;
  /** `file:` URL of the directory of fixtures, ending in a slash. */
  fixtures: string;
  /** Which variant of the scenario to run. */
  variant: string;
  /** Names the package, scenario and variant in the test output. */
  label: string;
}

const deno: unknown = Reflect.get(globalThis, "Deno");

/** The runtime this process is, by the global each one alone defines. */
export const runtime = deno !== undefined ? "deno" : Reflect.has(globalThis, "Bun") ? "bun" : "node";

/** Deno hands a script its own arguments; Node and Bun put the runtime and script paths first. */
export function scenarioArgs(): ScenarioArgs {
  const argv: unknown =
    typeof deno === "object" && deno !== null
      ? Reflect.get(deno, "args")
      : Reflect.get(Reflect.get(globalThis, "process"), "argv");
  const own = runtime === "deno" ? argv : Array.isArray(argv) ? argv.slice(2) : undefined;
  if (!Array.isArray(own) || typeof own[0] !== "string") {
    throw new Error("a scenario takes its arguments as JSON in its first argument");
  }
  return JSON.parse(own[0]);
}

export async function loadPackage(args: ScenarioArgs): Promise<Package> {
  return await import(args.module);
}

interface NodeFs {
  readFile(path: URL): Promise<Uint8Array<ArrayBuffer>>;
}

// A variable specifier keeps deno check from asking for node:fs types.
export async function readBytes(url: string): Promise<Uint8Array<ArrayBuffer>> {
  const specifier = "node:fs/promises";
  const fs: NodeFs = await import(specifier);
  return await fs.readFile(new URL(url));
}

/**
 * Collects garbage and gives finalizers a turn until `done()` holds, at most
 * 50 times. Node needs --expose-gc for this, and Deno --v8-flags=--expose-gc;
 * run.mjs passes them.
 */
export async function collectUntil(done: () => boolean): Promise<boolean> {
  const bun: unknown = Reflect.get(globalThis, "Bun");
  const onBun = typeof bun === "object" && bun !== null;
  const gc: unknown = onBun ? Reflect.get(bun, "gc") : Reflect.get(globalThis, "gc");
  if (typeof gc !== "function") throw new Error("no gc(): run node with --expose-gc");
  // Bun.gc(true) is a synchronous full collection. V8's gc(true) is a minor
  // one, which never frees an object that has reached the old generation.
  const full = onBun ? [true] : [];
  for (let round = 0; round < 50 && !done(); round += 1) {
    Reflect.apply(gc, bun, full);
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  return done();
}

type ConsoleMethod = "error" | "warn" | "info" | "debug";

/** Runs `body` with the console's methods recording what they are given. */
export async function withConsole(
  body: (lines: { method: ConsoleMethod; text: string }[]) => void | Promise<void>,
): Promise<void> {
  const lines: { method: ConsoleMethod; text: string }[] = [];
  const methods: ConsoleMethod[] = ["error", "warn", "info", "debug"];
  const real = methods.map((method) => console[method]);
  for (const method of methods) {
    console[method] = (...data: unknown[]) => {
      lines.push({ method, text: data.map(String).join(" ") });
    };
  }
  try {
    await body(lines);
  } finally {
    methods.forEach((method, index) => {
      const original = real[index];
      if (original !== undefined) console[method] = original;
    });
  }
}

type Body = () => void | Promise<void>;

const tests: { name: string; body: Body }[] = [];

export function test(name: string, body: Body): void {
  tests.push({ name, body });
}

/** Runs the registered tests in order and throws if any failed. */
export async function runTests(name: string): Promise<void> {
  const label = `${runtime} ${name}`;
  let failed = 0;
  for (const { name, body } of tests) {
    try {
      await body();
      console.log(`ok - ${label}: ${name}`);
    } catch (error) {
      failed += 1;
      console.log(`not ok - ${label}: ${name}`);
      console.log(error instanceof Error ? (error.stack ?? error.message) : String(error));
    }
  }
  if (failed > 0) throw new Error(`${label}: ${failed} of ${tests.length} tests failed`);
}

export function assert(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

export function assertEqual<T>(actual: T, expected: T, what: string): void {
  if (!Object.is(actual, expected)) {
    throw new Error(`${what}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

/** The `Cg3Error` class under test: the package's export, or the module's own. */
export type ErrorClass = abstract new (...args: never[]) => Error & { code: string };

/** The `code` of the `Cg3Error` that `f` throws. Fails if it returns or throws anything else. */
export function thrownCode(errors: ErrorClass, f: () => unknown): string {
  try {
    f();
  } catch (error) {
    assert(error instanceof errors, `expected a Cg3Error, got ${String(error)}`);
    return error.code;
  }
  throw new Error("expected a Cg3Error, but nothing was thrown");
}

export async function rejectedCode(errors: ErrorClass, promise: Promise<unknown>): Promise<string> {
  try {
    await promise;
  } catch (error) {
    assert(error instanceof errors, `expected a Cg3Error, got ${String(error)}`);
    return error.code;
  }
  throw new Error("expected a Cg3Error rejection, but the promise resolved");
}
