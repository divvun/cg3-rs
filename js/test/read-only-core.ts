// The check that the module behind @divvun/cg3 links neither the textual
// parser nor the grammar writers, by the function names in its name section.
// js/build.mjs runs it on every build, on the modules wasm-bindgen wrote and
// before wasm-opt drops their names. It takes the modules' bytes, so it needs
// no node: types and deno check can check it.

/** Reads an unsigned LEB128 number from `bytes` at `at`, as `[value, next]`. */
function leb128(bytes: Uint8Array, at: number): [number, number] {
  let value = 0;
  let shift = 0;
  for (;;) {
    const byte = bytes[at];
    if (byte === undefined) throw new Error("a WebAssembly module ends inside a number");
    at += 1;
    value += (byte & 0x7f) * 2 ** shift;
    shift += 7;
    if ((byte & 0x80) === 0) return [value, at];
  }
}

/**
 * The names of the functions a WebAssembly module links, from its name
 * section. The linker drops what its exports do not reach, so these are the
 * functions reachable from them, but for what was inlined into another.
 */
export function functionNames(bytes: Uint8Array): string[] {
  const text = new TextDecoder();
  const names: string[] = [];
  let at = 8;
  while (at < bytes.length) {
    const id = bytes[at];
    const [size, start] = leb128(bytes, at + 1);
    at = start + size;
    if (id !== 0) continue;
    const [nameLength, nameStart] = leb128(bytes, start);
    if (text.decode(bytes.subarray(nameStart, nameStart + nameLength)) !== "name") continue;
    let sub = nameStart + nameLength;
    while (sub < at) {
      const subId = bytes[sub];
      const [subSize, subStart] = leb128(bytes, sub + 1);
      sub = subStart + subSize;
      if (subId !== 1) continue;
      let [count, entry] = leb128(bytes, subStart);
      for (; count > 0; count -= 1) {
        const [, afterIndex] = leb128(bytes, entry);
        const [nameSize, nameAt] = leb128(bytes, afterIndex);
        names.push(text.decode(bytes.subarray(nameAt, nameAt + nameSize)));
        entry = nameAt + nameSize;
      }
    }
  }
  return names;
}

/** A function defined under `path` in the cg3 library, as wasm-bindgen's demangled names give it. */
const definedIn = (path: string): RegExp => new RegExp(`^<?cg3(\\[[0-9a-f]+\\])?::(${path})`);

// [spec:cg3:req:wasm.read-only-core]
/**
 * The code that compiles and writes grammars, which the main module must not
 * link: each part as the functions defined in it. A function of another crate
 * made for one of its types, such as a `Vec` of the parser's, is not the
 * part's code. An optimised build merges functions that compile to the same
 * code under one of their names, so a container the main module uses can
 * carry the name of the parser's copy of it.
 */
export const COMPILE_CODE = [
  { part: "the textual grammar parser", pattern: definedIn("textual_parser::") },
  { part: "the textual grammar writer", pattern: definedIn("grammar_writer::") },
  {
    part: "the binary grammar writer",
    pattern: definedIn("binary_grammar::(write_binary_grammar|BinaryWriter)|grammar_sources::companion_bytes"),
  },
];

// [spec:cg3:req:wasm.read-only-core]
/**
 * Fails when the main module links code that compiles or writes grammars, and
 * when the compile module does not link every part of it: the second is what
 * shows the names can be read and the patterns still find that code.
 */
export function assertReadOnlyCore(main: Uint8Array, compile: Uint8Array): void {
  const [mainNames, compileNames] = [functionNames(main), functionNames(compile)];
  if (mainNames.length === 0 || compileNames.length === 0) {
    throw new Error("a module has no function names to check; build it with its symbols kept");
  }
  for (const { part, pattern } of COMPILE_CODE) {
    const linked = mainNames.filter((name) => pattern.test(name));
    if (linked.length > 0) {
      throw new Error(`the main module links ${part}: ${linked.slice(0, 5).join(", ")}`);
    }
    if (!compileNames.some((name) => pattern.test(name))) {
      throw new Error(`the compile module does not link ${part}, so the check cannot see it`);
    }
  }
}
