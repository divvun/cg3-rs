import { isPublicDeclaration, usesAny } from "./declarations.ts";
import { assert, runTests, test } from "./harness.ts";

// [spec:cg3:req:wasm.typed-surface/test]
test("declarations that mention any only in comments pass", () => {
  const text = "/** Takes any string. */\nexport declare function f(x: string): number; // any\n";
  assert(!usesAny(text), "comments are not code");
});

// [spec:cg3:req:wasm.typed-surface/test]
test("a declaration that uses any fails, but a word that merely contains it does not", () => {
  assert(usesAny("export declare const f: (x: any) => void;\n"), "a parameter");
  assert(usesAny("export type T = Record<string, any>;\n"), "a type argument");
  assert(!usesAny("export declare const company: string;\n"), "company");
});

test("the generated glue under pkg is not one of the wrapper's declarations", () => {
  assert(isPublicDeclaration("index.d.ts"), "index.d.ts");
  assert(isPublicDeclaration("nested/api.d.ts"), "nested/api.d.ts");
  assert(!isPublicDeclaration("pkg/cg3_wasm.d.ts"), "pkg/cg3_wasm.d.ts");
  assert(!isPublicDeclaration("index.js"), "index.js");
});

await runTests("declarations");
