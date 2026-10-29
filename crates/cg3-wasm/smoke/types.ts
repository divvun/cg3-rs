// What a Deno or TypeScript user writes against the package's declarations;
// `deno check` in this directory fails if they stop describing it.
import init, { type ApplyOptions, type ApplyResult, Grammar, type GrammarInfo } from "../pkg/cg3.js";

export async function run(cg3b: Uint8Array, input: string): Promise<[ApplyResult, GrammarInfo]> {
  await init();
  const grammar = Grammar.fromBytes(cg3b);
  const options: ApplyOptions = { trace: true, numWindows: 4, input: "cg", output: "apertium" };
  const result: ApplyResult = grammar.apply(input, options);
  const info: GrammarInfo = grammar.info();
  grammar.apply(input);
  grammar.free();
  return [result, info];
}
