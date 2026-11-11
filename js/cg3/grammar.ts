import type { Core } from "./core.ts";
import { Cg3Error, messageOf } from "./errors.ts";
import type { GrammarGlue } from "./glue.ts";
import { Handle } from "./handle.ts";

/**
 * A stream format, as vislcg3's `--in-*` and `--out-*` options name it. The
 * binary format is left out: a string cannot carry it.
 */
export type Format = "cg" | "niceline" | "apertium" | "fst" | "plain" | "jsonl";

/**
 * The options a run reads, each named after its vislcg3 long option in
 * camelCase: `--num-windows 4` is `{ numWindows: 4 }`. An option whose value
 * vislcg3 makes optional is `true` without one, as `--trace` is
 * `{ trace: true }`. `input` and `output` stand for the `--in-*` and
 * `--out-*` options. Numbers are whole numbers from 0 to 4294967295. The
 * options a grammar sets with `CMDARGS` apply as they do in vislcg3.
 */
export interface ApplyOptions {
  /** The input stream's format; `cg` when left out. */
  input?: Format;
  /** The output stream's format; `cg` when left out. */
  output?: Format;
  ordered?: boolean;
  unsafe?: boolean;
  sections?: string;
  rules?: string;
  rule?: string;
  debugRules?: string;
  verbose?: boolean | string;
  noMappings?: boolean;
  noCorrections?: boolean;
  noBeforeSections?: boolean;
  noSections?: boolean;
  noAfterSections?: boolean;
  trace?: boolean | string;
  traceNameOnly?: boolean;
  traceNoRemoved?: boolean;
  traceEncl?: boolean;
  deleted?: boolean;
  singleRun?: boolean;
  maxRuns?: number;
  unicodeTags?: boolean;
  uniqueTags?: boolean;
  printIds?: boolean;
  printDep?: boolean;
  numWindows?: number;
  alwaysSpan?: boolean;
  softLimit?: number;
  hardLimit?: number;
  textDelimit?: boolean | string;
  depDelimit?: boolean | string;
  depAbsolute?: boolean;
  depOriginal?: boolean;
  depAllowLoops?: boolean;
  depNoCrossing?: boolean;
  noMagicReadings?: boolean;
  noPassOrigin?: boolean;
  splitMappings?: boolean;
  showEndTags?: boolean;
  noBreak?: boolean;
}

// [spec:cg3:req:wasm.load-options]
/**
 * The options vislcg3 applies to a grammar while it loads it, each named after
 * its vislcg3 long option in camelCase: `--nrules-v X` is `{ nrulesV: "X" }`.
 * A `--prefix` in the grammar's `CMDARGS` applies as it does in vislcg3.
 */
export interface LoadOptions {
  /** Keep only the rules whose names this pattern matches. */
  nrules?: string;
  /** Leave out the rules whose names this pattern matches. */
  nrulesV?: string;
  /** Read the grammar as VISLCG2 did; a `.cg3b` reads the same either way. */
  vislcgCompat?: boolean;
  /** The mapping prefix, one character, which the `.cg3b` must have been compiled with. */
  prefix?: string;
}

/** A run's output, and every warning and non-fatal error it logged, in order. */
export interface ApplyResult {
  output: string;
  warnings: string[];
}

// [spec:cg3:req:wasm.grammar-load+1]
/** What vislcg3's verbose summary reports about a loaded grammar. */
export interface GrammarInfo {
  sections: number;
  templates: number;
  rules: number;
  sets: number;
  tags: number;
  /** Rules no tag index can skip. */
  rulesAny: number;
  hasDependencyRules: boolean;
  hasRelationRules: boolean;
}

let wrap: (core: Core, glue: GrammarGlue) => Grammar;

/**
 * A compiled grammar, loaded once and applied any number of times. It owns
 * module memory: call `free()` when done with it, or declare it with `using`.
 */
export class Grammar extends Handle<GrammarGlue> {
  private constructor(core: Core, glue: GrammarGlue) {
    super(core, glue);
  }

  static {
    wrap = (core, glue) => new Grammar(core, glue);
  }

  // [spec:cg3:req:wasm.grammar-load+1]
  /** What vislcg3's verbose summary reports about the grammar, as a plain object. */
  info(): GrammarInfo {
    return this.use((glue) => glue.info());
  }

  // [spec:cg3:req:wasm.parity+2]
  // The input and options reach the module unchanged and the output comes
  // back as the module wrote it, so a run gives what vislcg3 gives.
  /**
   * Runs the grammar on `input`, as vislcg3 runs it on one standard input,
   * and returns the output with every warning and non-fatal error the run
   * logged. Each call is a run of its own. A run that fails throws
   * `run-failed` with the library's message, and the grammar stays usable.
   * An option it does not know throws `invalid-argument`.
   */
  apply(input: string, options?: ApplyOptions): ApplyResult {
    return this.use((glue) => glue.apply(checkedInput(input), optionsJson("apply", options)));
  }
}

function checkedInput(input: unknown): string {
  if (typeof input !== "string") {
    throw new Cg3Error("invalid-argument", `apply takes the input as a string, not ${typeof input}`);
  }
  return input;
}

/** The JSON text of the options `call` takes, which the module reads and checks key by key. */
function optionsJson(call: string, options: unknown): string {
  if (options === undefined) return "{}";
  if (typeof options !== "object" || options === null || Array.isArray(options)) {
    throw new Cg3Error("invalid-argument", `${call} takes its options as an object`);
  }
  try {
    return JSON.stringify(options);
  } catch (error) {
    throw new Cg3Error("invalid-argument", `the options cannot be read: ${messageOf(error)}`, {
      cause: error,
    });
  }
}

export interface GrammarReader {
  /**
   * Loads a compiled grammar from the bytes of a `.cg3b`, as `cg-comp`
   * writes it, under the load options `options` gives. The bytes are copied
   * into the module and not kept. Bytes that are not a `.cg3b` the library
   * reads throw `invalid-grammar`, with the library's message. An option it
   * does not know, a `prefix` that is not one character or a pattern that
   * does not compile throws `invalid-argument`.
   */
  readGrammar(bytes: Uint8Array, options?: LoadOptions): Grammar;
}

// [spec:cg3:req:wasm.grammar-load+1]
// [spec:cg3:req:wasm.load-options]
/** A {@link GrammarReader} that loads into the module instance `core`. */
export function grammarReader(core: Core): GrammarReader {
  return {
    readGrammar: (bytes, options) =>
      core.call((glue) => {
        if (!(bytes instanceof Uint8Array)) {
          throw new Cg3Error("invalid-argument", "a grammar loads from the bytes of a .cg3b, as a Uint8Array");
        }
        return wrap(core, glue.readGrammar(bytes, optionsJson("readGrammar", options)));
      }),
  };
}
