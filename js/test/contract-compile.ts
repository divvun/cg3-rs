// The compile entry point's contract against the real modules of a test
// build: compiles from text and from files given every way, compile errors
// and their diagnostics, the writers, logging, and grammars crossing to the
// main entry point as .cg3b bytes.

import type * as CompileTesting from "../cg3/compile-testing.ts";
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
} from "./harness.ts";

const args = scenarioArgs();
const compile: typeof CompileTesting = await import(args.entries.compile.module);
const main: typeof Testing = await import(args.entries.main.module);
await compile.init(await readBytes(args.entries.compile.wasm));
await main.init(await readBytes(args.entries.main.wasm));

const fixture = async (name: string) => await readBytes(new URL(`contract/${name}`, args.fixtures).href);
const text = async (name: string) => new TextDecoder().decode(await fixture(name));
const select = { source: await text("select.cg3"), cg3b: await fixture("select.cg3b") };
const selectCompanion = await fixture("select.cg3b.cg3src");
const include = {
  source: await text("include-grammar.cg3"),
  sets: await fixture("include-sets.cg3"),
  rules: await text("include-rules.cg3"),
  cg3b: await fixture("include.cg3b"),
  companion: await fixture("include.cg3b.cg3src"),
};
const input = await text("select.input");

function same(actual: Uint8Array | null | undefined, expected: Uint8Array, what: string): void {
  assert(actual instanceof Uint8Array, `${what} is a Uint8Array`);
  assertEqual(actual.length, expected.length, `${what}'s length`);
  assert(
    actual.every((byte, i) => byte === expected[i]),
    `${what} is cg-comp's, byte for byte`,
  );
}

function thrown(f: () => unknown): unknown {
  try {
    f();
  } catch (error) {
    return error;
  }
  throw new Error("nothing was thrown");
}

// [spec:cg3:req:wasm.compile/test]
test("compileGrammar writes the .cg3b and companion cg-comp writes, which readGrammar loads", () => {
  const result = compile.compileGrammar(select.source, { name: "grammar.cg3" });
  same(result.cg3b, select.cg3b, "the .cg3b");
  same(result.companion, selectCompanion, "the companion");
  assertEqual(result.warnings.length, 0, "warnings");
  using grammar = main.readGrammar(result.cg3b);
  using compiled = main.readGrammar(select.cg3b);
  assertEqual(grammar.apply(input).output, compiled.apply(input).output, "a run of it");
});

// [spec:cg3:req:wasm.compile-includes/test]
test("includes read from a record, a Map or a function compile as cg-comp compiles them from disk", () => {
  const asked: string[] = [];
  const given: [string, CompileTesting.IncludeFiles][] = [
    ["a record", { "sets.cg3": include.sets, "rules.cg3": include.rules }],
    ["a Map", new Map<string, string | Uint8Array>([["./sets.cg3", include.sets], ["rules.cg3", include.rules]])],
    [
      "a function",
      (name: string) => {
        asked.push(name);
        return name === "sets.cg3" ? include.sets : name === "rules.cg3" ? include.rules : undefined;
      },
    ],
  ];
  for (const [what, files] of given) {
    const result = compile.compileGrammar(include.source, { name: "grammar.cg3", files });
    same(result.cg3b, include.cg3b, `the .cg3b from ${what}`);
    same(result.companion, include.companion, `the companion from ${what}`);
  }
  assertEqual(asked.join(" "), "sets.cg3 rules.cg3", "the names the function was asked for");
});

// [spec:cg3:req:wasm.compile-includes/test]
// [spec:cg3:req:wasm.compile-errors/test]
test("an include the files do not give fails the compile naming the file", () => {
  for (const files of [undefined, { "sets.cg3": include.sets }, () => undefined]) {
    const error = thrown(() => compile.compileGrammar(include.source, { name: "grammar.cg3", files }));
    assert(error instanceof compile.Cg3Error, `a Cg3Error, not ${String(error)}`);
    assertEqual(error.code, "compile-failed", "code");
    const missing = files === undefined || typeof files === "function" ? "sets.cg3" : "rules.cg3";
    assert(error.message.includes(missing), `the message names ${missing}: ${error.message}`);
    assert(
      error.diagnostics.some((d) => d.text.includes(missing)),
      `a diagnostic names ${missing}: ${JSON.stringify(error.diagnostics)}`,
    );
  }
});

// [spec:cg3:req:wasm.compile-includes/test]
test("what the files function throws is thrown as it was, and a value that is no file is invalid-argument", () => {
  const mine = new RangeError("no such file here");
  const error = thrown(() =>
    compile.compileGrammar(include.source, {
      files: () => {
        throw mine;
      },
    }),
  );
  assert(error === mine, `the function's own error, not ${String(error)}`);
  const odd = () => 42 as unknown as string;
  assertEqual(thrownCode(compile.Cg3Error, () => compile.compileGrammar(include.source, { files: odd })), "invalid-argument", "a number");
  assert(compile.compileGrammar(select.source).cg3b.length > 0, "the module still compiles");
});

// [spec:cg3:req:wasm.compile-errors/test]
test("a failed compile throws compile-failed with every error, its source, line, text and UTF-16 span", () => {
  const source = 'DELIMITERS = "<.>" ;\n# \u{1F600}\nSELECT (a ;\nLIST X = ;\n';
  const error = thrown(() => compile.compileGrammar(source, { name: "broken.cg3" }));
  assert(error instanceof main.Cg3Error, "the Cg3Error both entry points export");
  assertEqual(error.code, "compile-failed", "code");
  assert(error.diagnostics.length >= 2, `every error: ${JSON.stringify(error.diagnostics)}`);
  for (const diagnostic of error.diagnostics) {
    assertEqual(diagnostic.source, "broken.cg3", "source");
    assert(!diagnostic.text.includes("\u001b"), "no colour");
    assert(diagnostic.text.includes(diagnostic.message), "the text says what went wrong");
    assert(error.message.includes(diagnostic.message), "so does the error's message");
  }
  const [first] = error.diagnostics;
  assertEqual(first?.line, 3, "line");
  const span = first?.span;
  assert(span !== null && span !== undefined, "placed");
  const marked = source.slice(span.start, span.end).trim();
  assert(marked.length > 0 && (source.split("\n")[2] ?? "").includes(marked), `the span marks line 3: ${marked}`);
});

// [spec:cg3:req:wasm.compile/test]
// [spec:cg3:req:wasm.load-options/test]
test("compileGrammar returns the parse's warnings and takes the load options", () => {
  const { warnings } = compile.compileGrammar('DELIMITERS = "<.>" ;\nSELECT: (a) ;\nREMOVE: (b) ;\n');
  assertEqual(warnings.length, 2, "warnings");
  assert(warnings.every((w) => w.includes("Rule had : but no name")), `the parse's: ${warnings.join("; ")}`);
  const mapped = 'DELIMITERS = "<.>" ;\nMAP (%x) (a) ;\n';
  const { cg3b } = compile.compileGrammar(mapped, { prefix: "%" });
  using grammar = main.readGrammar(cg3b, { prefix: "%" });
  assertEqual(grammar.info().rules, 1, "the grammar compiled under the prefix");
  const other = () => main.readGrammar(cg3b, { prefix: "@" });
  assertEqual(thrownCode(main.Cg3Error, other), "invalid-grammar", "it loads under no other prefix");
  const misspelt = { nRules: "x" } as unknown as CompileTesting.CompileOptions;
  assertEqual(thrownCode(compile.Cg3Error, () => compile.compileGrammar(mapped, misspelt)), "invalid-argument", "nRules");
});

// [spec:cg3:req:wasm.compile/test]
test("toText writes a .cg3b as text, which compiles to a grammar that runs as it does", () => {
  const written = compile.toText(select.cg3b);
  assert(written.includes("SELECT"), `the rules: ${written}`);
  using again = main.readGrammar(compile.compileGrammar(written).cg3b);
  using original = main.readGrammar(select.cg3b);
  assertEqual(again.apply(input).output, original.apply(input).output, "a run of the grammar compiled from it");
  assertEqual(thrownCode(compile.Cg3Error, () => compile.toText(new Uint8Array([1, 2, 3]))), "invalid-grammar", "no .cg3b");
});

// [spec:cg3:req:wasm.compile/test]
test("relabel takes its rules as text or as a .cg3b, and returns a .cg3b readGrammar loads", () => {
  const rules = "LIST notwanted = notwanted ;\nMAP (wanted) (notwanted) ;\n";
  const fromText = compile.relabel(select.cg3b, rules);
  const fromCg3b = compile.relabel(select.cg3b, compile.compileGrammar(rules).cg3b);
  same(fromCg3b, fromText, "rules as a .cg3b");
  using grammar = main.readGrammar(fromText);
  assert(grammar.info().rules > 0, "the relabelled grammar loads");
  const broken = thrown(() => compile.relabel(select.cg3b, "MAP (a ;"));
  assert(broken instanceof compile.Cg3Error && broken.code === "compile-failed", "rules that do not compile");
  assert(broken.diagnostics.length > 0, "with their diagnostics");
  assertEqual(thrownCode(compile.Cg3Error, () => compile.relabel(new Uint8Array([1]), rules)), "invalid-grammar", "no .cg3b");
});

// [spec:cg3:req:wasm.logging/test]
test("the compile entry point logs to its own logger, at its own level", () => {
  const compiling: string[] = [];
  const running: string[] = [];
  compile.setLogger((event) => compiling.push(`${event.level} ${event.message}`));
  main.setLogger((event) => running.push(event.message));
  compile.compileGrammar('DELIMITERS = "<.>" ;\nSELECT: (a) ;\n');
  assertEqual(compiling.length, 1, "events from the compile");
  assert(compiling[0]?.startsWith("warn ") === true, `at warn: ${String(compiling[0])}`);
  assertEqual(running.length, 0, "events at the main entry point's logger");
  compile.setLogLevel("off");
  compile.compileGrammar('DELIMITERS = "<.>" ;\nSELECT: (a) ;\n');
  assertEqual(compiling.length, 1, "events with logging off");
  compile.setLogLevel("warn");
  compile.setLogger(null);
  main.setLogger(null);
});

// [spec:cg3:req:wasm.compile-errors/test]
// [spec:cg3:req:wasm.handle-lifetime/test]
test("the compile entry point's objects are freed once, by free, using or the garbage collector", async () => {
  const dropped: string[] = [];
  compile.setLogger((event) => dropped.push(event.message));
  compile.setLogLevel("debug");
  const object = compile.testing.createObject("a");
  object.free();
  object.free();
  assertEqual(thrownCode(compile.Cg3Error, () => object.label()), "freed", "a call after free");
  {
    using scoped = compile.testing.createObject("scoped");
    assertEqual(scoped.label(), "scoped", "label");
  }
  (() => {
    compile.testing.createObject("collected");
  })();
  assert(await collectUntil(() => dropped.includes("dropped collected")), "the collected object was dropped");
  assertEqual(dropped.filter((m) => m === "dropped a" || m === "dropped scoped").join(), "dropped a,dropped scoped", "drops");
  compile.setLogLevel("warn");
  compile.setLogger(null);
});

// [spec:cg3:req:wasm.module+3/test]
test("both entry points report the package's version", () => {
  assertEqual(compile.version(), args.version, "the compile entry point");
  assertEqual(main.version(), args.version, "the main entry point");
});

await runTests(args.label);
