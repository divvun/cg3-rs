// The API contract against the real module of a test build: errors from the
// library, grammar and object lifetime, and logging fed by the library's
// tracing.

import type * as Testing from "../cg3/testing.ts";
import {
  assert,
  assertEqual,
  collectUntil,
  readBytes,
  runTests,
  scenarioArgs,
  test,
  thrownCode,
  withConsole,
} from "./harness.ts";

const args = scenarioArgs();
const pkg: typeof Testing = await import(args.module);
const { testing } = pkg;
await pkg.init(await readBytes(args.wasm));

const fixture = async (name: string) => await readBytes(new URL(`contract/${name}`, args.fixtures).href);
const select = await fixture("select.cg3b");
const external = await fixture("external.cg3b");
const input = new TextDecoder().decode(await fixture("select.input"));
/** Apertium input whose stray `$` the library reports, and carries on past. */
const strayDollar = "word$ ^word/word<n>$\n";
const apertium = { input: "apertium", output: "apertium" } as const;

type LogEvent = Parameters<Parameters<typeof pkg.setLogger>[0] & {}>[0];

/** Sends events at `level` and above to the returned array. */
function capture(level: Parameters<typeof pkg.setLogLevel>[0]): LogEvent[] {
  const events: LogEvent[] = [];
  pkg.setLogger((event) => events.push(event));
  pkg.setLogLevel(level);
  return events;
}

function reset(): void {
  pkg.setLogger(null);
  pkg.setLogLevel("warn");
}

function dropped(events: LogEvent[], label: string): number {
  return events.filter((event) => event.message === `dropped ${label}`).length;
}

function thrown(f: () => unknown): unknown {
  try {
    f();
  } catch (error) {
    return error;
  }
  throw new Error("nothing was thrown");
}

// [spec:cg3:req:wasm.grammar-load+1/test]
test("a grammar loads from a .cg3b, and info is the library's summary as a plain object", () => {
  using grammar = pkg.readGrammar(select);
  const info = grammar.info();
  assertEqual(Object.getPrototypeOf(info), Object.prototype, "a plain object");
  assertEqual(
    JSON.stringify(info),
    '{"sections":1,"templates":0,"rules":4,"sets":7,"tags":30,"rulesAny":0,"hasDependencyRules":false,"hasRelationRules":false}',
    "info",
  );
  const { output, warnings } = grammar.apply(input);
  assert(output.length > 0 && warnings.length === 0, "a run gives output and no warnings");
});

// [spec:cg3:req:wasm.errors+1/test]
test("bytes that are no .cg3b throw invalid-grammar with the library's message", () => {
  for (const bytes of [new Uint8Array([1, 2, 3]), select.subarray(0, select.length / 2)]) {
    const error = thrown(() => pkg.readGrammar(bytes));
    assert(error instanceof pkg.Cg3Error, "a Cg3Error");
    assertEqual(error.name, "Cg3Error", "name");
    assertEqual(error.code, "invalid-grammar", "code");
    assert(error.message.length > 0, "the library's message");
  }
});

// [spec:cg3:req:wasm.errors+1/test]
test("a run the library fails throws run-failed with its message, and the grammar stays usable", () => {
  using grammar = pkg.readGrammar(external);
  const error = thrown(() => grammar.apply(input));
  assert(error instanceof pkg.Cg3Error, "a Cg3Error");
  assertEqual(error.code, "run-failed", "code");
  assert(/EXTERNAL on line \d+ could not be started/.test(error.message), `the library's message: ${error.message}`);
  assertEqual(thrownCode(pkg.Cg3Error, () => grammar.apply(input)), "run-failed", "the same grammar fails again");
  using other = pkg.readGrammar(select);
  assert(other.apply(input).output.length > 0, "the module still runs");
});

// [spec:cg3:req:wasm.errors+1/test]
test("an option the package does not know throws invalid-argument, and the grammar stays usable", () => {
  using grammar = pkg.readGrammar(select);
  const misspelt = { noMapings: true } as unknown as Parameters<typeof grammar.apply>[1];
  const error = thrown(() => grammar.apply(input, misspelt));
  assert(error instanceof pkg.Cg3Error && error.code === "invalid-argument", "invalid-argument");
  assert(error.message.includes("noMapings"), `the message names the option: ${error.message}`);
  assertEqual(grammar.apply(input).warnings.length, 0, "the next run");
});

// [spec:cg3:req:wasm.tools/test]
test("convert, mwesplit and grammar.proc write what cg-conv, cg-mwesplit and cg-proc write", () => {
  const apertium = "^w/a<n>+b<v>$\n";
  assertEqual(pkg.convert(apertium, { input: "apertium" }), '"<w>"\n\t"b" v\n\t\t"a" n\n\n', "convert");
  assertEqual(
    pkg.convert(apertium, { input: "apertium", ltr: true }),
    '"<w>"\n\t"a" n\n\t\t"b" v\n\n',
    "convert with ltr",
  );
  const multiword = '"<dalle go>"\n\t"go" CS <W:0> "<go>"\n\t\t"dalle" Adv <W:0> "<dalle >"\n';
  assertEqual(pkg.mwesplit(multiword), '"<dalle>"\n\t"dalle" Adv <W:0>\n: \n"<go>"\n\t"go" CS <W:0>\n\n', "mwesplit");
  using grammar = pkg.readGrammar(select);
  const word = '"<word>"\n\t"word" notwanted\n\t"word" wanted\n';
  assertEqual(grammar.proc(word, { streamFormat: "cg" }), '"<word>"\n\t"word" wanted\n', "proc in CG");
  const lexical = "^word/word<notwanted>/word<wanted>$\n";
  assertEqual(grammar.proc(lexical), "^word/word<wanted>$\n", "proc in Apertium");
  assertEqual(
    grammar.proc(lexical, { trace: true }),
    "^word/word<wanted><SELECT:11>/\u00acword<notwanted><SELECT:11>$\n",
    "proc with trace",
  );
});

// [spec:cg3:req:wasm.tools/test]
// [spec:cg3:req:wasm.errors+1/test]
test("a tool the library fails throws run-failed with its message, and an unknown option invalid-argument", () => {
  const reserved = '{"w": "a", "rs": [{"l": "a", "ts": ["n"]}], "ds": 4294967295}\n';
  const failed = thrown(() => pkg.convert(reserved, { input: "jsonl" }));
  assert(failed instanceof pkg.Cg3Error && failed.code === "run-failed", `run-failed: ${String(failed)}`);
  assert(failed.message.length > 0, "the library's message");
  using grammar = pkg.readGrammar(external);
  const error = thrown(() => grammar.proc(input, { streamFormat: "cg" }));
  assert(error instanceof pkg.Cg3Error && error.code === "run-failed", "run-failed from proc");
  assert(/EXTERNAL on line \d+ could not be started/.test(error.message), `the library's message: ${error.message}`);
  const misspelt = { wordFormCase: true } as unknown as Parameters<typeof grammar.proc>[1];
  assertEqual(thrownCode(pkg.Cg3Error, () => grammar.proc(input, misspelt)), "invalid-argument", "proc");
  const binary = { output: "binary" } as unknown as Parameters<typeof pkg.convert>[1];
  assertEqual(thrownCode(pkg.Cg3Error, () => pkg.convert(input, binary)), "invalid-argument", "a binary stream");
  using other = pkg.readGrammar(select);
  assert(other.proc(input, { streamFormat: "cg" }).length > 0, "the module still runs");
});

// [spec:cg3:req:wasm.handle-lifetime/test]
test("a freed grammar throws freed, and freeing it again does nothing", () => {
  const grammar = pkg.readGrammar(select);
  grammar.free();
  grammar.free();
  assertEqual(thrownCode(pkg.Cg3Error, () => grammar.info()), "freed", "info");
  assertEqual(thrownCode(pkg.Cg3Error, () => grammar.apply(input)), "freed", "apply");
});

// [spec:cg3:req:wasm.handle-lifetime/test]
test("free releases an object's memory once, and a call after it throws freed", () => {
  const events = capture("debug");
  const object = testing.createObject("a");
  assertEqual(object.label(), "a", "label");
  object.free();
  object.free();
  assertEqual(dropped(events, "a"), 1, "times the module dropped it");
  assertEqual(thrownCode(pkg.Cg3Error, () => object.label()), "freed", "a call after free");
  reset();
});

// [spec:cg3:req:wasm.handle-lifetime/test]
test("a using declaration and Symbol.dispose free the object", () => {
  const events = capture("debug");
  {
    using scoped = testing.createObject("scoped");
    assertEqual(scoped.label(), "scoped", "label inside the block");
  }
  assertEqual(dropped(events, "scoped"), 1, "dropped at the end of the block");
  const disposed = testing.createObject("disposed");
  disposed[Symbol.dispose]();
  assertEqual(dropped(events, "disposed"), 1, "dropped by Symbol.dispose");
  reset();
});

// [spec:cg3:req:wasm.handle-lifetime/test]
test("an object dropped without free is released after garbage collection", async () => {
  const events = capture("debug");
  (() => {
    testing.createObject("collected");
  })();
  assert(await collectUntil(() => dropped(events, "collected") === 1), "the module dropped it");
  reset();
});

// [spec:cg3:req:wasm.init/test]
test("init again keeps the instance and the objects already made in it", async () => {
  const object = testing.createObject("before");
  await pkg.init();
  assertEqual(object.label(), "before", "the object made before the second init");
  object.free();
});

// [spec:cg3:req:wasm.logging/test]
test("by default warnings and errors from the library reach the console, and lower levels do not", async () => {
  reset();
  await withConsole((lines) => {
    for (const level of ["error", "warn", "info", "debug", "trace"] as const) {
      testing.log(level, `at ${level}`);
    }
    const seen = lines.map(({ method, text }) => `${method} ${text}`);
    assertEqual(
      seen.join("; "),
      "error cg3_wasm::test_hooks: at error; warn cg3_wasm::test_hooks: at warn",
      "console lines",
    );
  });
});

// [spec:cg3:req:wasm.logging/test]
test("a replaced logger gets each event's level, target and message", () => {
  const events = capture("info");
  testing.log("info", "hello");
  assertEqual(
    JSON.stringify(events),
    '[{"level":"info","target":"cg3_wasm::test_hooks","message":"hello"}]',
    "events",
  );
  reset();
});

// [spec:cg3:req:wasm.logging/test]
test("the level set is the level the module sends, and off sends nothing", () => {
  const events = capture("info");
  for (const level of ["error", "warn", "info", "debug", "trace"] as const) testing.log(level, "x");
  pkg.setLogLevel("trace");
  testing.log("trace", "y");
  pkg.setLogLevel("off");
  testing.log("error", "z");
  assertEqual(events.map((event) => event.level).join(" "), "error warn info trace", "levels");
  reset();
});

// [spec:cg3:req:wasm.logging/test]
test("what a run logs reaches the logger, and apply still returns it", () => {
  using grammar = pkg.readGrammar(select);
  const events = capture("warn");
  const { warnings } = grammar.apply(strayDollar, apertium);
  assertEqual(warnings.length, 1, "warnings returned");
  assert(warnings[0]?.includes("$ found without prior ^") === true, `the warning: ${String(warnings[0])}`);
  assertEqual(events.length, 1, "events logged");
  assertEqual(events[0]?.level, "error", "level");
  assertEqual(events[0]?.target, "cg3::apertium_applicator", "target");
  assertEqual(events[0]?.message, warnings[0], "message");
  pkg.setLogLevel("off");
  assertEqual(grammar.apply(strayDollar, apertium).warnings.join(), warnings.join(), "warnings with logging off");
  assertEqual(events.length, 1, "events logged with logging off");
  reset();
});

await runTests(args.label);
