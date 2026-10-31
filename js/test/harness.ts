// A small test harness for Node and Bun. Nothing here has node: types, so
// deno check can check the suite without @types/node.

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
 * 50 times. Node needs --expose-gc for this.
 */
export async function collectUntil(done: () => boolean): Promise<boolean> {
  const bun: unknown = Reflect.get(globalThis, "Bun");
  const gc: unknown =
    typeof bun === "object" && bun !== null ? Reflect.get(bun, "gc") : Reflect.get(globalThis, "gc");
  if (typeof gc !== "function") throw new Error("no gc(): run node with --expose-gc");
  for (let round = 0; round < 50 && !done(); round += 1) {
    Reflect.apply(gc, bun, [true]);
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

const runtime = Reflect.has(globalThis, "Bun") ? "bun" : "node";

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
