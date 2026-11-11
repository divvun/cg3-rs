// Starting an entry point: what importing does, the init sources, and calls
// made too early. `variant` names the source this process starts the module
// from, and `entry` the entry point it starts. The other entry point is
// imported too, and must stay unstarted.

import type * as Compile from "../cg3/compile.ts";
import type * as Main from "../cg3/index.ts";
import {
  assert,
  assertEqual,
  readBytes,
  rejectedCode,
  runTests,
  scenarioArgs,
  test,
  thrownCode,
} from "./harness.ts";

const args = scenarioArgs();
const otherEntry = args.entry === "main" ? args.entries.compile : args.entries.main;

const fetched: string[] = [];
/** The byte length of every module given as bytes to be compiled or instantiated. */
const compiledBytes: number[] = [];
let compiles = 0;
const realFetch = globalThis.fetch;
globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
  fetched.push(input instanceof Request ? input.url : String(input));
  return realFetch(input, init);
}) as typeof fetch;
for (const name of ["compile", "compileStreaming", "instantiate", "instantiateStreaming"]) {
  const real: unknown = Reflect.get(WebAssembly, name);
  assert(typeof real === "function", `WebAssembly.${name} is a function`);
  Reflect.set(WebAssembly, name, (...rest: unknown[]) => {
    compiles += 1;
    const [source] = rest;
    if (ArrayBuffer.isView(source) || source instanceof ArrayBuffer) compiledBytes.push(source.byteLength);
    return Reflect.apply(real, WebAssembly, rest);
  });
}

/** What both entry points export that this scenario uses. */
type Entry = Pick<typeof Main & typeof Compile, "init" | "version" | "setLogLevel" | "Cg3Error">;

const pkg: Entry = await import(args.module);
const other: Entry = await import(otherEntry.module);
let started: Promise<void> | undefined;

/** A call into the entry point that needs its module, other than version and setLogLevel. */
function entryCall(): void {
  if (args.entry === "main") (pkg as typeof Main).readGrammar(new Uint8Array([1, 2, 3]));
  else (pkg as typeof Compile).compileGrammar("SELECT (a) ;");
}

async function source(variant: string): Promise<Parameters<typeof pkg.init>[0]> {
  switch (variant) {
    case "bytes":
    case "retry":
      return await readBytes(args.wasm);
    case "file-url":
      return new URL(args.wasm);
    case "http-url":
      return args.http;
    case "response":
      return await realFetch(args.http);
    case "module":
      return await WebAssembly.compile(await readBytes(args.wasm));
    case "default":
      return undefined;
    default:
      throw new Error(`no init source called ${variant}`);
  }
}

// [spec:cg3:req:wasm.init/test]
// [spec:cg3:req:wasm.module+3/test]
test("importing both entry points fetches and compiles nothing", () => {
  assertEqual(fetched.length, 0, "fetches during import");
  assertEqual(compiles, 0, "compiles during import");
});

// [spec:cg3:req:wasm.init/test]
test("a call before init throws not-initialised", () => {
  assertEqual(thrownCode(pkg.Cg3Error, () => pkg.version()), "not-initialised", "version() before init");
  assertEqual(thrownCode(pkg.Cg3Error, entryCall), "not-initialised", "a call into the module before init");
  assertEqual(thrownCode(pkg.Cg3Error, () => pkg.setLogLevel("info")), "not-initialised", "setLogLevel before init");
});

if (args.variant === "retry") {
  // [spec:cg3:req:wasm.init/test]
  // [spec:cg3:req:wasm.errors+1/test]
  test("a failed init rejects with init-failed, and the next init starts afresh", async () => {
    const failing = pkg.init(new Uint8Array([0, 0x61, 0x73, 0x6d, 9, 9, 9, 9]));
    assertEqual(await rejectedCode(pkg.Cg3Error, failing), "init-failed", "init with bad bytes");
    assertEqual(thrownCode(pkg.Cg3Error, () => pkg.version()), "not-initialised", "version() after it");
  });
}

// [spec:cg3:req:wasm.init/test]
// [spec:cg3:req:wasm.module+3/test]
test(`init from ${args.variant} starts the entry point at the version the package declares`, async () => {
  started = pkg.init(await source(args.variant));
  assertEqual(thrownCode(pkg.Cg3Error, () => pkg.version()), "not-initialised", "a call while init runs");
  await started;
  assertEqual(pkg.version(), args.version, "version()");
});

// [spec:cg3:req:wasm.init/test]
test("init again returns the first call's promise and instance", async () => {
  const again = pkg.init(new Uint8Array(0));
  assert(again === started, "a second init returns the same promise");
  await again;
  assertEqual(pkg.version(), args.version, "version() after the second init");
});

// [spec:cg3:req:wasm.module+3/test]
// [spec:cg3:req:wasm.compile/test]
test("starting one entry point fetches and compiles its own .wasm alone, and leaves the other unstarted", async () => {
  const own = (await readBytes(args.wasm)).byteLength;
  const others = (await readBytes(otherEntry.wasm)).byteLength;
  assert(own !== others, "the two .wasm files differ in length, so a length tells them apart");
  assertEqual(compiledBytes.filter((length) => length === others).length, 0, "the other's .wasm compiled from its bytes");
  const otherFile = otherEntry.http.slice(otherEntry.http.lastIndexOf("/") + 1);
  assertEqual(fetched.filter((url) => url.endsWith(otherFile)).join(), "", "fetches of the other entry point's .wasm");
  assertEqual(thrownCode(other.Cg3Error, () => other.version()), "not-initialised", "the other entry point");
});

await runTests(args.label);
