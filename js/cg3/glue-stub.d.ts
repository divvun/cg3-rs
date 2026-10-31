// What deno check sees in place of pkg/cg3_wasm.js, which only exists after a
// build. The root deno.json maps that path here; a member's own import map
// would not do, because deno resolves a member's keys against the workspace
// root. tsc compiles against the real one.
import type { Glue } from "./glue.ts";

declare const init: Glue["default"];
export default init;
export declare const install: Glue["install"];
export declare const setLogLevel: Glue["setLogLevel"];
export declare const version: Glue["version"];
export declare const readGrammar: Glue["readGrammar"];
