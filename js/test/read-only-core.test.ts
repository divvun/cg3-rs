import { assert, assertEqual, runTests, test } from "./harness.ts";
import { assertReadOnlyCore, functionNames } from "./read-only-core.ts";

/** An unsigned LEB128 number. */
function leb128(value: number): number[] {
  const bytes: number[] = [];
  do {
    let byte = value & 0x7f;
    value >>>= 7;
    if (value !== 0) byte |= 0x80;
    bytes.push(byte);
  } while (value !== 0);
  return bytes;
}

const sized = (bytes: number[]): number[] => [...leb128(bytes.length), ...bytes];
const utf8 = (text: string): number[] => [...new TextEncoder().encode(text)];

/**
 * A WebAssembly module whose name section names its functions `names`, after
 * a type section and before the section's local names, which the check must
 * step over.
 */
function moduleNamed(names: string[]): Uint8Array {
  const functions = names.flatMap((name, index) => [...leb128(index), ...sized(utf8(name))]);
  const name = [
    ...sized(utf8("name")),
    ...[0, ...sized(sized(utf8("module")))],
    ...[1, ...sized([...leb128(names.length), ...functions])],
    ...[2, ...sized([0])],
  ];
  return new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, ...[1, ...sized([0])], ...[0, ...sized(name)]]);
}

const COMPILES = [
  "<cg3[3cd0ff8ab8373db2]::textual_parser::TextualParser>::parse_grammar_named",
  "<cg3[3cd0ff8ab8373db2]::grammar_writer::GrammarWriter>::write_grammar::<alloc::vec::Vec<u8>>",
  "<cg3[3cd0ff8ab8373db2]::binary_grammar::BinaryWriter>::write_rule::<alloc::vec::Vec<u8>>",
];
const RUNS = ["cg3_wasm::grammar::apply", "<cg3[3cd0ff8ab8373db2]::binary_grammar::BinaryGrammar>::parse_cg3b"];

function failure(f: () => void): string | null {
  try {
    f();
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
  return null;
}

test("the function names are read from the name section, past the other sections", () => {
  assertEqual(functionNames(moduleNamed(["a", "b::c", "\u00e5"])).join(" "), "a b::c \u00e5", "names");
  assertEqual(functionNames(new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0])).length, 0, "a module without names");
});

// [spec:cg3:req:wasm.read-only-core/test]
test("a main module that runs and a compile module that compiles pass", () => {
  assertReadOnlyCore(moduleNamed(RUNS), moduleNamed([...RUNS, ...COMPILES]));
});

// [spec:cg3:req:wasm.read-only-core/test]
test("a main module that links the parser or a writer fails the build, naming it", () => {
  const parts = ["the textual grammar parser", "the textual grammar writer", "the binary grammar writer"];
  COMPILES.forEach((name, i) => {
    const message = failure(() => assertReadOnlyCore(moduleNamed([...RUNS, name]), moduleNamed(COMPILES)));
    assert(message?.includes(`the main module links ${parts[i]}`) === true, `${parts[i]}: ${message}`);
    assert(message?.includes(name) === true, "the message names the function");
  });
  const companion = "cg3[3cd0ff8ab8373db2]::grammar_sources::companion_bytes";
  assert(failure(() => assertReadOnlyCore(moduleNamed([companion]), moduleNamed(COMPILES))) !== null, "the companion writer");
});

// [spec:cg3:req:wasm.read-only-core/test]
test("another crate's code made for a parser type is not the parser", () => {
  const merged = [
    "<alloc[cb2b8d99256f33df]::vec::Vec<cg3[3cd0ff8ab8373db2]::textual_parser::SourceBuf>>::push",
    "core::ptr::drop_in_place::<cg3[3cd0ff8ab8373db2]::grammar_writer::GrammarWriter>",
  ];
  assertReadOnlyCore(moduleNamed([...RUNS, ...merged]), moduleNamed(COMPILES));
});

// [spec:cg3:req:wasm.read-only-core/test]
test("a compile module the check cannot see the parser and writers in fails the build", () => {
  for (const missing of COMPILES) {
    const message = failure(() => assertReadOnlyCore(moduleNamed(RUNS), moduleNamed(COMPILES.filter((n) => n !== missing))));
    assert(message?.includes("the compile module does not link") === true, `without ${missing}: ${message}`);
  }
  const stripped = failure(() => assertReadOnlyCore(new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]), moduleNamed(COMPILES)));
  assert(stripped?.includes("no function names") === true, `a stripped module: ${stripped}`);
});

await runTests("read-only-core");
