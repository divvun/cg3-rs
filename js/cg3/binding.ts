import { Core } from "./core.ts";
import * as glue from "./pkg/cg3_wasm.js";

// [spec:cg3:req:wasm.module+3]
// The main entry point's .wasm file ships in pkg/, beside the compiled entry
// module, so a URL relative to this module finds it in every runtime. Nothing
// here names the compile module's, which compile-binding.ts does.
/** The main entry point's module instance. Making it loads nothing. */
export const core: Core = new Core(glue, new URL("./pkg/cg3_wasm_bg.wasm", import.meta.url));
