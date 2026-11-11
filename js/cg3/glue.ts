// The generated wasm-bindgen glue, as the wrapper uses it. The generated
// module hands most results back untyped; these types give them the shapes
// the module builds, and tsc checks at build time that the generated module
// satisfies them when binding.ts hands it to Core. Declaring them here also
// lets the unit tests run the wrapper on a fake glue, and lets deno check the
// wrapper on a checkout that has no build output.

import type { ApplyResult, GrammarInfo } from "./grammar.ts";

/** What the generated init function is given. The wrapper resolves URLs itself. */
export type GlueInitInput = BufferSource | Response | WebAssembly.Module;

/**
 * What the module calls back into, from `crates/cg3-wasm/src/sink.rs`.
 * Levels are numbers: 0 is off, then 1 error, 2 warn, 3 info, 4 debug, 5 trace.
 * These calls arrive while the module is running, so none of them may throw:
 * an exception would unwind through the module and leave it half run.
 */
export interface GlueSink {
  /** One tracing event, which the module has already checked against the level. */
  log(level: number, target: string, message: string): void;
  /** A panic's message. The trap that ends the call follows at once. */
  panic(message: string): void;
}

/**
 * An object the glue hands out, which owns module memory until `free()`.
 * wasm-bindgen also frees one from its own `FinalizationRegistry` when the
 * object becomes unreachable, which the wrapper must never let happen after a
 * trap; see `Core`.
 */
export interface GlueObject {
  free(): void;
}

/** A grammar the glue holds, from `crates/cg3-wasm/src/grammar.rs`. */
export interface GrammarGlue extends GlueObject {
  /** A plain object the module builds. */
  info(): GrammarInfo;
  /**
   * Runs the grammar on `input`. `options` is the JSON text of an
   * `ApplyOptions` object; the module refuses a key it does not know. The
   * result is a plain object the module builds.
   */
  apply(input: string, options: string): ApplyResult;
}

/** The exports of `crates/cg3-wasm`. */
export interface Glue {
  /** Instantiates the module. The generated glue keeps the instance it makes. */
  default(options: { module_or_path: GlueInitInput }): Promise<unknown>;
  /** Starts the instance with the wrapper's sink and a log level. Called once, by `init()`. */
  install(sink: GlueSink, level: number): void;
  /** Sends events at `level` and above from here on, and nothing below it. */
  setLogLevel(level: number): void;
  version(): string;
  /**
   * Loads a `.cg3b`. `options` is the JSON text of a `LoadOptions` object;
   * the module refuses a key it does not know.
   */
  readGrammar(bytes: Uint8Array, options: string): GrammarGlue;
}
