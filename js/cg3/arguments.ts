// The checks every entry point makes of its arguments before the module sees
// them.

import { Cg3Error, messageOf } from "./errors.ts";

/** `input`, or `invalid-argument` naming `call` when it is not a string. */
export function checkedInput(call: string, input: unknown): string {
  if (typeof input !== "string") {
    throw new Cg3Error("invalid-argument", `${call} takes the input as a string, not ${typeof input}`);
  }
  return input;
}

/** The JSON text of the options `call` takes, which the module reads and checks key by key. */
export function optionsJson(call: string, options: unknown): string {
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
