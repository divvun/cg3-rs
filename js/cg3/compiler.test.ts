// compileGrammar, toText and relabel, against a fake compile glue.

import { FakeCompileGlue } from "../test/fake-glue.ts";
import { assert, assertEqual, runTests, test, thrownCode } from "../test/harness.ts";
import { type CompileOptions, compiler, type IncludeFiles } from "./compiler.ts";
import { Core } from "./core.ts";
import { Cg3Error } from "./errors.ts";
import type { CompileGlue } from "./glue.ts";

async function started() {
  const glue = new FakeCompileGlue();
  const core = new Core<CompileGlue>(glue, new URL("http://127.0.0.1:9/never.wasm"), "@divvun/cg3/compile");
  await core.init(new Uint8Array(0));
  return { glue, ...compiler(core) };
}

const bytes = new TextEncoder().encode("LIST A = a ;");

// [spec:cg3:req:wasm.compile/test]
// [spec:cg3:req:wasm.load-options/test]
test("compileGrammar hands the source, its name and its load options' JSON to the module", async () => {
  const { glue, compileGrammar } = await started();
  const result = compileGrammar("SELECT (a) ;", { name: "g.cg3", prefix: "%", nrules: "x" });
  assertEqual(result.cg3b.length, 4, "the .cg3b the module returned");
  compileGrammar("SELECT (b) ;");
  const calls = glue.compiles.map(({ source, name, files, options }) => `${source} ${name} ${files} ${options}`);
  assertEqual(
    calls.join("; "),
    'SELECT (a) ; g.cg3 undefined {"prefix":"%","nrules":"x"}; SELECT (b) ; undefined undefined {}',
    "calls",
  );
});

// [spec:cg3:req:wasm.compile-includes/test]
test("files given as a record or a Map reach the module as name and content pairs", async () => {
  const { glue, compileGrammar } = await started();
  compileGrammar("x", { files: { "sets.cg3": "LIST A = a ;", "rules.cg3": bytes } });
  compileGrammar("x", { files: new Map<string, string | Uint8Array>([["a/b.cg3", bytes]]) });
  const [record, map] = glue.compiles.map(({ files }) => files);
  assert(Array.isArray(record) && Array.isArray(map), "pairs");
  assertEqual(JSON.stringify(record?.map(([name, content]) => [name, typeof content])), '[["sets.cg3","string"],["rules.cg3","object"]]', "the record");
  assert(map[0]?.[0] === "a/b.cg3" && map[0][1] === bytes, "the Map's bytes reach the module as they are");
});

// [spec:cg3:req:wasm.compile-includes/test]
test("a files function reaches the module as it is, and what it gives comes back to it", async () => {
  const { glue, compileGrammar } = await started();
  const asked: string[] = [];
  compileGrammar("x", {
    files: (name) => {
      asked.push(name);
      return name === "included.cg3" ? "LIST A = a ;" : undefined;
    },
  });
  assertEqual(asked.join(), "included.cg3", "the names asked");
  assertEqual(glue.asked.join(), "LIST A = a ;", "what the module was given");
});

// [spec:cg3:req:wasm.compile-includes/test]
// [spec:cg3:req:wasm.errors+1/test]
test("what compileGrammar cannot use throws invalid-argument without entering the module", async () => {
  const { glue, compileGrammar, toText, relabel } = await started();
  const before = glue.entered;
  const bad: [string, () => unknown][] = [
    ["a source of bytes", () => compileGrammar(bytes as unknown as string)],
    ["options that are an array", () => compileGrammar("x", [] as unknown as CompileOptions)],
    ["a name that is a number", () => compileGrammar("x", { name: 3 } as unknown as CompileOptions)],
    ["files that are a string", () => compileGrammar("x", { files: "a.cg3" } as unknown as CompileOptions)],
    ["files that are an array", () => compileGrammar("x", { files: [["a", "b"]] } as unknown as CompileOptions)],
    ["a file that is a number", () => compileGrammar("x", { files: { "a.cg3": 3 } as unknown as IncludeFiles })],
    ["a Map key that is a number", () => compileGrammar("x", { files: new Map([[1, "a"]]) as unknown as IncludeFiles })],
    ["a BigInt option", () => compileGrammar("x", { prefix: 1n } as unknown as CompileOptions)],
    ["toText of an array", () => toText([1, 2] as unknown as Uint8Array)],
    ["relabel of a string", () => relabel("x" as unknown as Uint8Array, bytes)],
    ["relabel rules that are a number", () => relabel(bytes, 3 as unknown as Uint8Array)],
  ];
  for (const [what, call] of bad) {
    assertEqual(thrownCode(Cg3Error, call), "invalid-argument", what);
  }
  assertEqual(glue.entered, before, "calls into the module");
});

// [spec:cg3:req:wasm.compile-errors/test]
test("a failed compile throws compile-failed carrying the module's diagnostics", async () => {
  const { compileGrammar } = await started();
  let error: unknown = null;
  try {
    compileGrammar("broken");
  } catch (thrown) {
    error = thrown;
  }
  assert(error instanceof Cg3Error, "a Cg3Error");
  assertEqual(error.code, "compile-failed", "code");
  assertEqual(error.message, "grammar could not be parsed: 1 error(s)", "message");
  assertEqual(
    JSON.stringify(error.diagnostics),
    '[{"message":"syntax error","source":"x.cg3","line":1,"text":"Error: syntax error","span":null}]',
    "diagnostics",
  );
});

// [spec:cg3:req:wasm.compile/test]
test("relabel takes its rules as bytes or as text, which reaches the module in UTF-8", async () => {
  const { glue, relabel, toText } = await started();
  relabel(bytes, bytes);
  relabel(bytes, "MAP (å) (a) ;");
  assert(glue.relabels[0]?.[1] === bytes, "bytes as they are");
  assertEqual(new TextDecoder().decode(glue.relabels[1]?.[1]), "MAP (å) (a) ;", "text in UTF-8");
  assertEqual(toText(bytes), 'DELIMITERS = "<.>" ;\n', "toText");
  assertEqual(thrownCode(Cg3Error, () => toText(new Uint8Array([0]))), "invalid-grammar", "no .cg3b");
});

// [spec:cg3:req:wasm.init/test]
test("the compile entry point throws not-initialised before its own init", () => {
  const glue = new FakeCompileGlue();
  const core = new Core<CompileGlue>(glue, new URL("http://127.0.0.1:9/never.wasm"), "@divvun/cg3/compile");
  const { compileGrammar } = compiler(core);
  let message = "";
  try {
    compileGrammar("x");
  } catch (error) {
    assert(error instanceof Cg3Error && error.code === "not-initialised", "not-initialised");
    message = error.message;
  }
  assert(message.startsWith("@divvun/cg3/compile is not initialised"), `the entry point is named: ${message}`);
  assertEqual(glue.entered, 0, "calls into the module");
});

await runTests("cg3/compiler");
