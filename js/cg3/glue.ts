// The generated wasm-bindgen glue, as the wrapper uses it. The generated
// module hands most results back untyped; these types give them the shapes
// the module builds, and tsc checks at build time that the generated module
// satisfies them when binding.ts hands it to Core. Declaring them here also
// lets the unit tests run the wrapper on a fake glue, and lets deno check the
// wrapper on a checkout that has no build output.

import type { CompileResult } from "./compiler.ts";
import type { ApplyResult, GrammarInfo } from "./grammar.ts";

/** What the generated init function is given. The wrapper resolves URLs itself. */
export type GlueInitInput = BufferSource | Response | WebAssembly.Module;

/**
 * What the module calls back into, from `crates/cg3-wasm-shared/src/sink.rs`.
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
  /**
   * Runs the grammar on `input` as cg-proc does. `options` is the JSON text of
   * a `ProcOptions` object; the module refuses a key it does not know.
   */
  proc(input: string, options: string): string;
}

/** What every module exports, through which `Core` starts it and sets its logging. */
export interface ModuleGlue {
  /** Instantiates the module. The generated glue keeps the instance it makes. */
  default(options: { module_or_path: GlueInitInput }): Promise<unknown>;
  /** Starts the instance with the wrapper's sink and a log level. Called once, by `init()`. */
  install(sink: GlueSink, level: number): void;
  /** Sends events at `level` and above from here on, and nothing below it. */
  setLogLevel(level: number): void;
  version(): string;
}

/** The exports of `crates/cg3-wasm`, the module behind `@divvun/cg3`. */
export interface Glue extends ModuleGlue {
  /**
   * Loads a `.cg3b`. `options` is the JSON text of a `LoadOptions` object;
   * the module refuses a key it does not know.
   */
  readGrammar(bytes: Uint8Array, options: string): GrammarGlue;
  /**
   * Converts a stream as cg-conv does. `options` is the JSON text of a
   * `ConvertOptions` object; the module refuses a key it does not know.
   */
  convert(input: string, options: string): string;
  /** Splits multi-word cohorts as cg-mwesplit does. */
  mwesplit(input: string): string;
}

/**
 * The files an `INCLUDE` may read, as the wrapper hands them to the module:
 * none, name and content pairs it made of a record or a `Map`, or the
 * caller's function.
 */
export type FilesGlue =
  | undefined
  | [string, string | Uint8Array][]
  | ((name: string) => string | Uint8Array | undefined);

/** The exports of `crates/cg3-wasm-compile`, the module behind `@divvun/cg3/compile`. */
export interface CompileGlue extends ModuleGlue {
  /**
   * Compiles `source`, called `name`, or `<utf8-memory>` when it is
   * undefined. `options` is the JSON text of a `LoadOptions` object; the
   * module refuses a key it does not know. The result is a plain object the
   * module builds. A function among the files is called while the parse runs,
   * and what it throws is rethrown as it was.
   */
  compileGrammar(source: string, name: string | undefined, files: FilesGlue, options: string): CompileResult;
  /** The text of the grammar in a `.cg3b`. */
  toText(bytes: Uint8Array): string;
  /** A `.cg3b` relabelled by relabel rules, as a `.cg3b` or as text in UTF-8. */
  relabel(bytes: Uint8Array, relabels: Uint8Array): Uint8Array;
}
