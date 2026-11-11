// What deno check sees in place of pkg-compile/cg3_wasm_compile.js, which only
// exists after a build, as glue-stub.d.ts stands in for the main module's.
import type { CompileGlue } from "./glue.ts";

declare const init: CompileGlue["default"];
export default init;
export declare const install: CompileGlue["install"];
export declare const setLogLevel: CompileGlue["setLogLevel"];
export declare const version: CompileGlue["version"];
export declare const compileGrammar: CompileGlue["compileGrammar"];
export declare const toText: CompileGlue["toText"];
export declare const relabel: CompileGlue["relabel"];
