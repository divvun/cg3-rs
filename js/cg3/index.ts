/**
 * `@divvun/cg3`: VISL CG-3 for JavaScript. It loads compiled Constraint
 * Grammars (`.cg3b`) and runs them, in browsers, Web Workers, Deno, Node and
 * Bun, giving the output vislcg3 gives for the same grammar, input and
 * options. It does not read textual grammars: compile them with `cg-comp`.
 *
 * @module
 */
import { core } from "./binding.ts";
import type { InitSource } from "./core.ts";
import { grammarReader } from "./grammar.ts";
import type { Logger, LogLevel } from "./logging.ts";

// [spec:cg3:req:wasm.typed-surface]
// This module declares the whole public API. Results are plain values, and
// js/build.mjs fails a build whose declarations use any.

// [spec:cg3:req:wasm.init]
/**
 * Loads and starts the package's WebAssembly module. Call it once and wait
 * for it before anything else. With no argument it loads the `.wasm` file
 * that ships beside the package. Calling it again returns the same promise.
 */
export function init(source?: InitSource): Promise<void> {
  return core.init(source);
}

/** The package's version. The package is versioned on its own, apart from the cg3 library it carries. */
export function version(): string {
  return core.call((glue) => glue.version());
}

// [spec:cg3:req:wasm.logging]
/**
 * Sends the library's log events to `logger`. `null` puts back the default,
 * which writes each event to the console method for its level. Whatever the
 * logger, `apply` still returns its run's warnings.
 */
export function setLogger(logger: Logger | null): void {
  core.setLogger(logger);
}

/**
 * Logs events at `level` and the more severe levels; `off` logs nothing.
 * The default is `warn`. The module never sends the levels below it.
 */
export function setLogLevel(level: LogLevel): void {
  core.setLogLevel(level);
}

export const { readGrammar } = grammarReader(core);
export { Grammar } from "./grammar.ts";
export type { ApplyOptions, ApplyResult, Format, GrammarInfo } from "./grammar.ts";
export { Cg3Error } from "./errors.ts";
export type { Cg3ErrorCode } from "./errors.ts";
export type { InitSource } from "./core.ts";
export type { LogEvent, Logger, LogLevel } from "./logging.ts";
