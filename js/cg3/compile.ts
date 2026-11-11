/**
 * `@divvun/cg3/compile`: compiles textual Constraint Grammars to `.cg3b` and
 * writes them as cg-comp, vislcg3 `--grammar-out` and cg-relabel do, in
 * browsers, Web Workers, Deno, Node and Bun. It is a WebAssembly module of its
 * own, apart from `@divvun/cg3`'s, which only a caller who compiles loads:
 * start it with this entry point's `init`. A grammar compiled here crosses to
 * `@divvun/cg3` as its `.cg3b` bytes, which `readGrammar` loads.
 *
 * @module
 */
import { core } from "./compile-binding.ts";
import { compiler } from "./compiler.ts";
import type { InitSource } from "./core.ts";
import type { Logger, LogLevel } from "./logging.ts";

// [spec:cg3:req:wasm.typed-surface]
// This module declares the whole public API of the compile entry point.
// Results are plain values, and js/build.mjs fails a build whose declarations
// use any.

// [spec:cg3:req:wasm.compile]
/**
 * Loads and starts the compile entry point's WebAssembly module. Call it once
 * and wait for it before anything else here. With no argument it loads the
 * `.wasm` file that ships beside it, not `@divvun/cg3`'s, which has an `init`
 * of its own. Calling it again returns the same promise.
 */
export function init(source?: InitSource): Promise<void> {
  return core.init(source);
}

// [spec:cg3:req:wasm.module+3]
/** The package's version, which both entry points report. */
export function version(): string {
  return core.call((glue) => glue.version());
}

// [spec:cg3:req:wasm.logging]
/**
 * Sends the log events of compiling to `logger`. `null` puts back the default,
 * which writes each event to the console method for its level. This entry
 * point logs on its own: `@divvun/cg3`'s `setLogger` does not reach it.
 */
export function setLogger(logger: Logger | null): void {
  core.setLogger(logger);
}

/** Logs events at `level` and the more severe levels; `off` logs nothing. The default is `warn`. */
export function setLogLevel(level: LogLevel): void {
  core.setLogLevel(level);
}

export const { compileGrammar, toText, relabel } = compiler(core);
export type { CompileOptions, CompileResult, IncludeFiles } from "./compiler.ts";
export type { LoadOptions } from "./grammar.ts";
export { Cg3Error } from "./errors.ts";
export type { Cg3ErrorCode, Diagnostic } from "./errors.ts";
export type { InitSource } from "./core.ts";
export type { LogEvent, Logger, LogLevel } from "./logging.ts";
