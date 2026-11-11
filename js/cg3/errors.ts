// [spec:cg3:req:wasm.errors+1]
/**
 * Every code a {@link Cg3Error} can carry. Code may test `error.code` against
 * these; they do not change between releases.
 *
 * From the wrapper:
 * - `not-initialised`: a call came before `init()` had resolved.
 * - `init-failed`: `init()` could not load, compile or start the module.
 * - `poisoned`: the module trapped, for instance on a panic. Its memory cannot
 *   be trusted after that, so every later call throws this without entering it.
 * - `freed`: a call on a grammar after its `free()`.
 *
 * From the wrapper or the library, which `crates/cg3-wasm-shared/src/error.rs` maps
 * here:
 * - `invalid-argument`: an argument or run option the package cannot use,
 *   such as bytes that are not a `Uint8Array`, or an option it does not know.
 * - `invalid-grammar`: bytes that are not a `.cg3b` the library reads. The
 *   message is the library's.
 * - `run-failed`: a run the library reports as failed, such as one that
 *   reaches an `EXTERNAL` rule. The message is the library's. The grammar
 *   stays usable.
 * - `compile-failed`: a textual grammar `@divvun/cg3/compile` could not
 *   compile. The message is the library's, and `diagnostics` holds every
 *   error the parse found.
 *
 * Each entry point starts, and is poisoned, on its own: `not-initialised` and
 * `poisoned` speak of the module of the entry point that threw them.
 */
export const CG3_ERROR_CODES = [
  "not-initialised",
  "init-failed",
  "poisoned",
  "freed",
  "invalid-argument",
  "invalid-grammar",
  "run-failed",
  "compile-failed",
] as const;

export type Cg3ErrorCode = (typeof CG3_ERROR_CODES)[number];

// [spec:cg3:req:wasm.compile-errors]
/** One error a failed parse found. */
export interface Diagnostic {
  /** What went wrong, in the library's words. */
  message: string;
  /** The name of the source it is in: the compiled source's, or an included file's as the `INCLUDE` resolved it. */
  source: string;
  /** The line it is on, from 1. */
  line: number;
  /** The error as vislcg3 prints it, quoting the grammar, without colour. */
  text: string;
  /**
   * Where it sits in its source's text, as offsets in UTF-16 code units, which
   * is how a JavaScript string is indexed; `null` for an error with no place.
   */
  span: { start: number; end: number } | null;
}

/** An error from `@divvun/cg3`. `code` says what went wrong, in a form code can test. */
export class Cg3Error extends Error {
  readonly code: Cg3ErrorCode;
  /** Every error a failed compile's parse found, for `compile-failed`; empty for any other code. */
  readonly diagnostics: readonly Diagnostic[];

  constructor(code: Cg3ErrorCode, message: string, options?: ErrorOptions & { diagnostics?: readonly Diagnostic[] }) {
    super(message, options);
    this.name = "Cg3Error";
    this.code = code;
    this.diagnostics = options?.diagnostics ?? [];
  }
}

function isCg3ErrorCode(code: string): code is Cg3ErrorCode {
  return (CG3_ERROR_CODES as readonly string[]).includes(code);
}

/**
 * The error for an object a glue call threw, or null when it is not one. The
 * glue throws plain objects with `code` and `message`, and `diagnostics` for
 * `compile-failed`, which the module builds as `Diagnostic`s. It only sends the codes
 * `crates/cg3-wasm-shared/src/error.rs` lists, which a test there checks are listed
 * here; a code from a mismatched build reads as `run-failed` and is named in
 * the message.
 */
export function fromGlue(thrown: unknown): Cg3Error | null {
  if (typeof thrown !== "object" || thrown === null || thrown instanceof Error) return null;
  const code: unknown = Reflect.get(thrown, "code");
  const message: unknown = Reflect.get(thrown, "message");
  if (typeof code !== "string" || typeof message !== "string") return null;
  if (!isCg3ErrorCode(code)) {
    return new Cg3Error("run-failed", `${message} (the module sent the unknown code ${code})`);
  }
  const diagnostics: unknown = Reflect.get(thrown, "diagnostics");
  return new Cg3Error(code, message, Array.isArray(diagnostics) ? { diagnostics } : {});
}

/** The message of anything a caller might throw. */
export function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
