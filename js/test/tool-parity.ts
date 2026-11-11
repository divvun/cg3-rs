// The tool parity check: the package against cg-comp, vislcg3 --grammar-out,
// cg-conv, cg-mwesplit, cg-proc and cg-relabel. js/test/tool-fixtures.mjs
// writes what the tools wrote for every case at test-build time; this module
// starts both entry points as they ship, gives each case to the package and
// compares the bytes. It is written once for every environment:
// tool-parity-runtime.ts runs it under Node, Bun and Deno, and
// parity-browser.ts in a browser, on a page and in a Web Worker.

import type * as Compile from "../cg3/compile.ts";
import type { ConvertOptions, ProcOptions } from "../cg3/index.ts";
import type * as Main from "../cg3/index.ts";
import { difference, type ParityResult, type ReadBytes } from "./parity.ts";

/** Where the check finds both entry points and the fixtures, as URLs the environment can read. */
export interface ToolParityArgs {
  module: string;
  compile: string;
  /** URL of the directory of built fixtures, ending in a slash. */
  fixtures: string;
}

/** A corpus grammar as tools.json describes it: its source, included files, and what the tools wrote. */
interface CompileFixture {
  name: string;
  source: string;
  /** Each included file, by the name the grammar's INCLUDE gives it, at its fixture path. */
  files: Record<string, string>;
  failed: boolean;
  exempt?: string;
  cg3b?: string;
  companion?: string | null;
  text?: string;
}

type ToolFixture = { label: string; exempt: string } | (Ran & ToolCase);

/** What the tool did with a case. */
interface Ran {
  label: string;
  failed: boolean;
  /** The fixture path of what it wrote, when it did not fail. */
  expected?: string;
}

type ToolCase =
  | { tool: "cg-conv"; input: string; options: ConvertOptions }
  | { tool: "cg-mwesplit"; input: string }
  | { tool: "cg-proc"; input: string; grammar: string; options: ProcOptions }
  | { tool: "cg-relabel"; grammar: string; rules: string };

/** What a call returned, or the error it threw. */
function attempt<T>(f: () => T): { value: T } | { error: unknown } {
  try {
    return { value: f() };
  } catch (error) {
    return { error };
  }
}

const utf8 = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

// [spec:cg3:req:wasm.compile-parity/test]
/**
 * Starts both entry points from the `.wasm` files beside them, as a caller who
 * passes init nothing does, and gives every case in tools.json to the
 * package. Throws only when the package or tools.json cannot be loaded; a
 * difference, or a failure where the tool did not fail, is a difference.
 */
export async function runToolParity(args: ToolParityArgs, read: ReadBytes): Promise<ParityResult> {
  const main: typeof Main = await import(args.module);
  const compile: typeof Compile = await import(args.compile);
  await Promise.all([main.init(), compile.init()]);
  main.setLogLevel("off");
  compile.setLogLevel("off");
  const fixture = (path: string) => read(new URL(`tools/${path}`, args.fixtures).href);
  const manifest: { compile: CompileFixture[]; tools: ToolFixture[] } = JSON.parse(
    new TextDecoder().decode(await fixture("tools.json")),
  );
  const result: ParityResult = { same: [], exempt: [], differ: [] };

  /** Compares one outcome with the tool's: both failed, or the same bytes. */
  const compare = (label: string, outcome: { value: Uint8Array } | { error: unknown }, failed: boolean, expected?: Uint8Array) => {
    if ("error" in outcome) {
      if (!(outcome.error instanceof main.Cg3Error)) result.differ.push(`${label}: the package threw ${String(outcome.error)}`);
      else if (failed) result.same.push(label);
      else result.differ.push(`${label}: the tool succeeded, and the package threw ${outcome.error.code}: ${outcome.error.message}`);
      return;
    }
    if (failed || expected === undefined) {
      result.differ.push(`${label}: the tool failed, and the package succeeded`);
      return;
    }
    const found = difference(outcome.value, expected, "the tool");
    if (found === null) result.same.push(label);
    else result.differ.push(`${label}: ${found}`);
  };
  const encode = (text: string) => new TextEncoder().encode(text);
  const expectedOf = async (path: string | undefined) => (path === undefined ? undefined : await fixture(path));

  for (const grammar of manifest.compile) {
    const label = `cg-comp ${grammar.name}/grammar.cg3`;
    if (grammar.exempt !== undefined) {
      result.exempt.push(`${label}: ${grammar.exempt}`);
      continue;
    }
    const source = utf8.decode(await fixture(grammar.source));
    const files: Record<string, Uint8Array> = {};
    for (const [name, path] of Object.entries(grammar.files)) files[name] = await fixture(path);
    const compiled = attempt(() => compile.compileGrammar(source, { name: "grammar.cg3", files }));
    const parts = "value" in compiled ? compiled.value : null;
    compare(label, "value" in compiled ? { value: compiled.value.cg3b } : compiled, grammar.failed, await expectedOf(grammar.cg3b));
    if (parts === null || grammar.failed) continue;
    const companion = grammar.companion === null || grammar.companion === undefined ? null : await fixture(grammar.companion);
    if (companion === null || parts.companion === null) {
      if (companion === parts.companion) result.same.push(`${label}: no companion`);
      else result.differ.push(`${label}: the companion is ${parts.companion === null ? "missing" : "extra"}`);
    } else {
      compare(`${label}: its companion`, { value: parts.companion }, false, companion);
    }
    const text = attempt(() => encode(compile.toText(parts.cg3b)));
    compare(`vislcg3 --grammar-only --grammar-out of ${grammar.name}`, text, false, await expectedOf(grammar.text));
  }

  const grammars = new Map<string, Main.Grammar>();
  for (const test of manifest.tools) {
    if ("exempt" in test) {
      result.exempt.push(`${test.label}: ${test.exempt}`);
      continue;
    }
    const expected = await expectedOf(test.expected);
    let outcome: { value: Uint8Array } | { error: unknown };
    if (test.tool === "cg-relabel") {
      const [bytes, rules] = await Promise.all([fixture(test.grammar), fixture(test.rules)]);
      outcome = attempt(() => compile.relabel(bytes, rules));
    } else {
      const input = utf8.decode(await fixture(test.input));
      if (test.tool === "cg-conv") {
        outcome = attempt(() => encode(main.convert(input, test.options)));
      } else if (test.tool === "cg-mwesplit") {
        outcome = attempt(() => encode(main.mwesplit(input)));
      } else {
        let grammar = grammars.get(test.grammar);
        if (grammar === undefined) {
          grammar = main.readGrammar(await fixture(test.grammar));
          grammars.set(test.grammar, grammar);
        }
        const loaded = grammar;
        outcome = attempt(() => encode(loaded.proc(input, test.options)));
      }
    }
    compare(test.label, outcome, test.failed, expected);
  }
  for (const grammar of grammars.values()) grammar.free();
  return result;
}
