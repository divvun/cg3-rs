// Tests for the checks swift/build.ts runs on what it builds:
//
//   node --test swift/build.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  compilerFunctions,
  declaredFunctions,
  isCInterface,
  leftOut,
  members,
  packageMetadata,
} from "./build.ts";

const NM = `
cg3_swift_ffi.cgu.0.rcgu.o:
00000000002c348c (__TEXT,__gcc_except_tab) non-external GCC_except_table1003
000000000019b994 (__TEXT,__text) external _uniffi_cg3_swift_fn_func_version
000000000028c0d0 (__TEXT,__text) external _rust_eh_personality
                 (undefined) private external __aarch64_cas16_acq
                 (undefined) external _malloc (from libSystem)

aarch64.o:
0000000000001c28 (__DATA,__common) private external ___aarch64_have_lse_atomics

lse_cas16_acq.o:
                 (undefined) private external ___aarch64_have_lse_atomics
0000000000000000 (__TEXT,__text) private external __aarch64_cas16_acq

builtins.o:
0000000000000000 (__TEXT,__text) weak private external ___udivti3
`;

// [spec:cg3:req:swift.linking/test]
test("nm -m reads as each member's symbols, with their visibility", () => {
  const read = members(NM);
  assert.deepEqual([...read.keys()], ["cg3_swift_ffi.cgu.0.rcgu.o", "aarch64.o", "lse_cas16_acq.o", "builtins.o"]);
  const rust = read.get("cg3_swift_ffi.cgu.0.rcgu.o");
  assert.deepEqual(rust?.defines, [
    { name: "_uniffi_cg3_swift_fn_func_version", external: true },
    { name: "_rust_eh_personality", external: true },
  ]);
  assert.deepEqual(rust?.needs, ["__aarch64_cas16_acq", "_malloc"]);
  assert.deepEqual(read.get("builtins.o")?.defines, [{ name: "___udivti3", external: false }]);
  assert.equal(members("libcg3_swift_ffi.a(prelinked.o):\n").has("prelinked.o"), true, "the older header form");
});

// [spec:cg3:req:swift.linking/test]
test("the members left out of the prelink are those without the flag and what they need", () => {
  const out = leftOut(members(NM), new Set(["lse_cas16_acq.o"]));
  assert.deepEqual([...out].sort(), ["aarch64.o", "lse_cas16_acq.o"]);
  assert.deepEqual([...leftOut(members(NM), new Set())], []);
});

// [spec:cg3:req:swift.linking/test]
test("the C interface is uniffi_cg3_swift and ffi_cg3_swift, and nothing else", () => {
  assert.equal(isCInterface("_uniffi_cg3_swift_fn_func_version"), true);
  assert.equal(isCInterface("_uniffi_cg3_swift_compile_fn_func_relabel"), true);
  assert.equal(isCInterface("_ffi_cg3_swift_rustbuffer_alloc"), true);
  assert.equal(isCInterface("_rust_eh_personality"), false);
  assert.equal(isCInterface("_uniffi_hfst_swift_fn_func_version"), false);
});

// [spec:cg3:req:swift.read-only-core/test]
test("a header's functions are those of its own prefix", () => {
  const header = `
RustBuffer uniffi_cg3_swift_fn_func_version(RustCallStatus *_Nonnull out_status
);
#ifndef UNIFFI_FFIDEF_UNIFFI_CG3_SWIFT_FN_FUNC_MWESPLIT
uint16_t uniffi_cg3_swift_checksum_func_mwesplit(void
RustBuffer ffi_cg3_swift_rustbuffer_alloc(uint64_t size, RustCallStatus *_Nonnull out_status
`;
  assert.deepEqual(declaredFunctions(header, "cg3_swift_"), [
    "ffi_cg3_swift_rustbuffer_alloc",
    "uniffi_cg3_swift_checksum_func_mwesplit",
    "uniffi_cg3_swift_fn_func_version",
  ]);
});

// [spec:cg3:req:swift.read-only-core/test]
test("the compiler's functions are text symbols naming the parser, the writer or the relabeller", () => {
  const nm = `
0000000100001000 t __RNvMs_NtCs1_3cg314textual_parserNtB2_13TextualParser10parse_rule
0000000100002000 T __RNvMs_NtCs1_3cg314grammar_writerNtB2_13GrammarWriter13write_grammar
0000000100003000 s __RNvNvNvMNtCs1_3cg314textual_parser13TextualParser10parse_rule10___CALLSITE4META
0000000100004000 t __RINvMs4_NtCs7_5alloc7raw_vecINtB5_6RawVecNtNtCs1_3cg314grammar_writer8RulePartE8grow_oneBQ_
0000000100005000 t __RNvMNtCs1_3cg310relabellerNtB2_10Relabeller7relabel
`;
  assert.equal(compilerFunctions(nm).length, 3);
  assert.equal(compilerFunctions("0000000100004000 t __RINvMs4_NtCs7_5alloc7raw_vec").length, 0);
});

// [spec:cg3:req:swift.package/test]
test("package-metadata.json has the fields SwiftPM's publishing reads, at the version's tag", () => {
  const metadata = packageMetadata("1.2.3");
  assert.deepEqual(Object.keys(metadata).sort(), ["author", "description", "licenseURL", "readmeURL", "repositoryURLs"]);
  assert.equal(metadata.licenseURL, "https://github.com/divvun/cg3-rs/blob/swift/v1.2.3/COPYING");
  assert.equal(metadata.readmeURL, "https://github.com/divvun/cg3-rs/blob/swift/v1.2.3/swift/README.md");
});
