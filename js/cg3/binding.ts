import { Core } from "./core.ts";
import * as glue from "./pkg/cg3_wasm.js";

// [spec:cg3:req:wasm.module+1]
// The package's one .wasm file ships in pkg/, beside the compiled entry
// module, so a URL relative to this module finds it in every runtime.
/** The package's module instance. Making it loads nothing. */
export const core: Core = new Core(glue, new URL("./pkg/cg3_wasm_bg.wasm", import.meta.url));
