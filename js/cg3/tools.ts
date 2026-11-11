import { checkedInput, optionsJson } from "./arguments.ts";
import type { Core } from "./core.ts";
import type { Glue } from "./glue.ts";
import type { Format } from "./grammar.ts";

// [spec:cg3:req:wasm.tools]
/**
 * cg-conv's options, each named after its long option in camelCase:
 * `--sub-delim "#"` is `{ subDelim: "#" }`. `input` and `output` stand for the
 * `--in-*` and `--out-*` options. Numbers are whole numbers from 0 to
 * 4294967295, but for `wfactor`.
 */
export interface ConvertOptions {
  /** The input stream's format; detected from the input when left out, as cg-conv does. */
  input?: Format;
  /** Detect the input's format even where `input` or `addTags` names one. */
  inAuto?: boolean;
  /** Give each word of plain text a minimal reading. It reads plain text unless `input` names CG, Niceline, Apertium or FST. */
  addTags?: boolean;
  /** The output stream's format; `cg` when left out. */
  output?: Format;
  /** The mapping prefix, one character; `@` when left out. */
  prefix?: string;
  /** What FST weights are multiplied by; 1 when left out. */
  wfactor?: number;
  /** The tag FST weights are written as; `W` when left out. */
  wtag?: string;
  /** The characters that separate FST sub-readings; `#` when left out. */
  subDelim?: string;
  /** Sub-readings run right to left: cg-conv's default, so it has no effect. */
  rtl?: boolean;
  /** Sub-readings run left to right. */
  ltr?: boolean;
  ordered?: boolean;
  parseDep?: boolean;
  /** Delimit windows by dependency; `true` is cg-conv's default of 10. */
  depDelimit?: boolean | number;
  unicodeTags?: boolean;
  deleted?: boolean;
  noBreak?: boolean;
}

/**
 * A stream format cg-proc reads and writes, which `--stream-format` numbers
 * 0, 1 and 2. The binary format, 3, is left out: a string cannot carry it.
 */
export type ProcFormat = "cg" | "apertium" | "matxin";

// [spec:cg3:req:wasm.tools]
/**
 * cg-proc's options, each named after its long option in camelCase:
 * `--wordform-case` is `{ wordformCase: true }`. The options a grammar sets
 * with `CMDARGS` apply as they do in cg-proc.
 */
export interface ProcOptions {
  /** The stream format; `apertium` when left out. */
  streamFormat?: ProcFormat;
  /** Run only the first this many sections. */
  sections?: number;
  /** Run only the rules of this name. */
  rule?: string;
  trace?: boolean;
  /** Give each baseform the case of its wordform. */
  wordformCase?: boolean;
  /** Print no cohort's wordform. */
  noWordForms?: boolean;
  /** No `^`/`$` around lexical units, and readings as surface forms. */
  generation?: boolean;
  /** Print only the first of the readings left. */
  first?: boolean;
  /** Disambiguate: the only thing cg-proc does, so it has no effect. */
  disambiguation?: boolean;
  /** Flush on a null byte: cg-proc always does, so it has no effect. */
  nullFlush?: boolean;
}

export interface Tools {
  /**
   * Converts `input` from one stream format to another, as cg-conv does, and
   * returns what cg-conv writes. A conversion the library reports as failed
   * throws `run-failed` with its message. An option it does not know throws
   * `invalid-argument`.
   */
  convert(input: string, options?: ConvertOptions): string;
  /**
   * Splits each cohort whose wordform tags give its words into one cohort per
   * word, as cg-mwesplit does, and returns what cg-mwesplit writes. A split
   * the library reports as failed throws `run-failed` with its message.
   */
  mwesplit(input: string): string;
}

// [spec:cg3:req:wasm.tools]
// [spec:cg3:req:wasm.compile-parity]
// The input and options reach the module unchanged and the output comes back
// as the module wrote it, so a conversion writes what cg-conv writes.
/** The {@link Tools} of the module instance `core`. */
export function tools(core: Core): Tools {
  return {
    convert: (input, options) =>
      core.call((glue) => glue.convert(checkedInput("convert", input), optionsJson("convert", options))),
    mwesplit: (input) => core.call((glue) => glue.mwesplit(checkedInput("mwesplit", input))),
  };
}
