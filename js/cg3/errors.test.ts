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
test("the codes are exactly the seven the package promises", () => {
  assertEqual(
    CG3_ERROR_CODES.join(" "),
    "not-initialised init-failed poisoned freed invalid-argument invalid-grammar run-failed",
    "codes",
  );
});

// [spec:cg3:req:wasm.errors+1/test]
test("a glue error object becomes a Cg3Error with its code and the library's message", () => {
  for (const code of ["invalid-argument", "invalid-grammar", "run-failed"] as const) {
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
