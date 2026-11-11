import { optionsJson } from "./arguments.ts";
import type { Core } from "./core.ts";
import { Cg3Error } from "./errors.ts";
import type { CompileGlue, FilesGlue } from "./glue.ts";
import type { LoadOptions } from "./grammar.ts";

// [spec:cg3:req:wasm.compile-includes]
/**
 * The files a grammar's `INCLUDE`s may read, and no others: a record or a
 * `Map` from names to their text or bytes, or a function that takes a name
 * and returns the file's text or bytes, or `undefined` for a file it does not
 * have. A name is resolved as vislcg3 resolves it on disk: beside the source
 * that includes it, else at the top of the names, with `./`, `..` and
 * repeated slashes read as a path reads them, so `INCLUDE sets.cg3 ;` in
 * `grammar.cg3` reads `sets.cg3`. The function is called with each name
 * resolved so, while the compile runs, and a file it gives is kept, so it is
 * not asked for that file again.
 */
export type IncludeFiles =
  | Readonly<Record<string, string | Uint8Array>>
  | ReadonlyMap<string, string | Uint8Array>
  | ((name: string) => string | Uint8Array | undefined);

// [spec:cg3:req:wasm.compile]
/**
 * What a compile is given beside the source: the source's name, the files its
 * includes may read, and the load options vislcg3 applies while it loads a
 * grammar.
 */
export interface CompileOptions extends LoadOptions {
  /**
   * What diagnostics and the companion file call the source, and what its
   * relative `INCLUDE`s are resolved against; `<utf8-memory>` when left out.
   * Give the name cg-comp was given to get the companion file it writes.
   */
  name?: string;
  /** The files the source's `INCLUDE`s may read; none when left out. */
  files?: IncludeFiles;
}

/** A compiled grammar, as cg-comp writes it, and what its parse logged. */
export interface CompileResult {
  /** The `.cg3b`, which `readGrammar` from `@divvun/cg3` loads. */
  cg3b: Uint8Array;
  /** The companion file cg-comp writes beside the `.cg3b`, with the grammar's sources. */
  companion: Uint8Array | null;
  /** Every warning and error the parse logged, in order. */
  warnings: string[];
}

export interface Compiler {
  /**
   * Compiles the textual grammar `source` and writes it as cg-comp does. A
   * grammar the library will not compile throws `compile-failed`, with every
   * error the parse found in `diagnostics`; an `INCLUDE` of a file the options
   * do not give is one of them, naming the file. An option it does not know
   * throws `invalid-argument`, and what the files function throws is thrown
   * as it was.
   */
  compileGrammar(source: string, options?: CompileOptions): CompileResult;
  /**
   * The grammar in the `.cg3b` `bytes` as text, as vislcg3 `--grammar-out`
   * writes it. Bytes that are not a `.cg3b` throw `invalid-grammar`.
   */
  toText(bytes: Uint8Array): string;
  /**
   * The `.cg3b` of the grammar in `bytes` relabelled by `relabels`, as
   * cg-relabel writes it. `relabels` is the relabel rules, as text or as the
   * bytes cg-relabel reads: a `.cg3b`, or a textual grammar in UTF-8. Rules
   * that do not compile throw `compile-failed`, and bytes or rules that are no
   * `.cg3b` the library reads, or that it cannot relabel with, `invalid-grammar`.
   */
  relabel(bytes: Uint8Array, relabels: Uint8Array | string): Uint8Array;
}

function invalid(message: string): Cg3Error {
  return new Cg3Error("invalid-argument", message);
}

function checkedBytes(call: string, bytes: unknown): Uint8Array {
  if (!(bytes instanceof Uint8Array)) throw invalid(`${call} takes a grammar as the bytes of a .cg3b, as a Uint8Array`);
  return bytes;
}

function isFile(content: unknown): content is string | Uint8Array {
  return typeof content === "string" || content instanceof Uint8Array;
}

// [spec:cg3:req:wasm.compile-includes]
/** The files as the module takes them: a function as it is, a record or a Map as name and content pairs. */
function filesGlue(files: unknown): FilesGlue {
  if (files === undefined || typeof files === "function") return files as FilesGlue;
  if (typeof files !== "object" || files === null || Array.isArray(files)) {
    throw invalid("files is a record or a Map from names to text or bytes, or a function");
  }
  const entries: [unknown, unknown][] = files instanceof Map ? [...files.entries()] : Object.entries(files);
  return entries.map(([name, content]) => {
    if (typeof name !== "string") throw invalid(`a file's name is a string, not ${typeof name}`);
    if (!isFile(content)) throw invalid(`the file ${name} is a string or a Uint8Array, not ${typeof content}`);
    return [name, content];
  });
}

/**
 * Splits `options` into the name and files the module takes as arguments and
 * the load options it reads as JSON, refusing a name or files it cannot use.
 */
function compileArguments(options: unknown): { name: string | undefined; files: FilesGlue; load: string } {
  if (options === undefined) return { name: undefined, files: undefined, load: "{}" };
  if (typeof options !== "object" || options === null || Array.isArray(options)) {
    throw invalid("compileGrammar takes its options as an object");
  }
  const { name, files, ...load } = options as CompileOptions;
  if (name !== undefined && typeof name !== "string") throw invalid(`a source's name is a string, not ${typeof name}`);
  return { name, files: filesGlue(files), load: optionsJson("compileGrammar", load) };
}

// [spec:cg3:req:wasm.compile]
// [spec:cg3:req:wasm.compile-parity]
// The source, the files and the bytes reach the module unchanged and come back
// as the module wrote them, so a compile writes what cg-comp writes.
/** A {@link Compiler} over the compile module instance `core`. */
export function compiler(core: Core<CompileGlue>): Compiler {
  return {
    compileGrammar: (source, options) =>
      core.call((glue) => {
        if (typeof source !== "string") throw invalid(`compileGrammar takes the source as a string, not ${typeof source}`);
        const { name, files, load } = compileArguments(options);
        return glue.compileGrammar(source, name, files, load);
      }),
    toText: (bytes) => core.call((glue) => glue.toText(checkedBytes("toText", bytes))),
    relabel: (bytes, relabels) =>
      core.call((glue) => {
        const rules = typeof relabels === "string" ? new TextEncoder().encode(relabels) : relabels;
        return glue.relabel(checkedBytes("relabel", bytes), checkedBytes("relabel", rules));
      }),
  };
}
