import { assert, assertEqual, runTests, test } from "../test/harness.ts";
import { CG3_ERROR_CODES, Cg3Error, fromGlue, messageOf } from "./errors.ts";

// [spec:cg3:req:wasm.errors+1/test]
test("a Cg3Error is an Error with a name, a code, a message and a cause", () => {
  const cause = new Error("inner");
  const error = new Cg3Error("init-failed", "outer", { cause });
  assert(error instanceof Error, "an Error");
  assertEqual(error.name, "Cg3Error", "name");
  assertEqual(error.code, "init-failed", "code");
  assertEqual(error.message, "outer", "message");
  assert(error.cause === cause, "the cause is kept");
  assert(String(error.stack).includes("outer"), "the stack names the message");
});

// [spec:cg3:req:wasm.errors+1/test]
// [spec:cg3:req:wasm.compile-errors/test]
test("the codes are exactly the eight the package promises", () => {
  assertEqual(
    CG3_ERROR_CODES.join(" "),
    "not-initialised init-failed poisoned freed invalid-argument invalid-grammar run-failed compile-failed",
    "codes",
  );
});

// [spec:cg3:req:wasm.compile-errors/test]
test("a compile-failed glue error keeps its diagnostics, and any other code has none", () => {
  const diagnostic = { message: "syntax error", source: "g.cg3", line: 2, text: "Error: syntax error", span: { start: 3, end: 5 } };
  const failed = fromGlue({ code: "compile-failed", message: "it failed", diagnostics: [diagnostic] });
  assertEqual(failed?.code, "compile-failed", "code");
  assertEqual(JSON.stringify(failed?.diagnostics), JSON.stringify([diagnostic]), "diagnostics");
  assertEqual(fromGlue({ code: "run-failed", message: "no" })?.diagnostics.length, 0, "a run's failure");
  assertEqual(new Cg3Error("freed", "gone").diagnostics.length, 0, "the wrapper's own");
});

// [spec:cg3:req:wasm.errors+1/test]
test("a glue error object becomes a Cg3Error with its code and the library's message", () => {
  for (const code of ["invalid-argument", "invalid-grammar", "run-failed", "compile-failed"] as const) {
    const error = fromGlue({ code, message: "the library says no" });
    assert(error instanceof Cg3Error, "a Cg3Error");
    assertEqual(error.code, code, "code");
    assertEqual(error.message, "the library says no", "message");
  }
});

test("an unknown code becomes run-failed and keeps the code in the message", () => {
  const error = fromGlue({ code: "brand-new", message: "something" });
  assertEqual(error?.code, "run-failed", "code");
  assert(error?.message.includes("brand-new") === true, "the message names the code");
});

test("anything that is not a glue error object is left alone", () => {
  for (const thrown of [new TypeError("x"), "text", 3, null, { code: 1, message: "m" }, {}]) {
    assertEqual(fromGlue(thrown), null, String(thrown));
  }
});

test("messageOf reads an Error's message and stringifies anything else", () => {
  assertEqual(messageOf(new TypeError("bad")), "bad", "an Error");
  assertEqual(messageOf("plain"), "plain", "a string");
  assertEqual(messageOf(42), "42", "a number");
});

await runTests("cg3/errors");
