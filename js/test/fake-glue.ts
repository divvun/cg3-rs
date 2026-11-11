// A stand-in for the generated glue, so the wrapper can be tested without a
// build. It behaves as the real module does at the boundary: it filters log
// events by the level set, reports a panic to the sink before it traps, and
// throws plain error objects.

import type { Glue, GlueInitInput, GlueObject, GlueSink, GrammarGlue } from "../cg3/glue.ts";
import type { ApplyResult, GrammarInfo } from "../cg3/grammar.ts";

export class FakeGlue implements Glue {
  readonly inputs: GlueInitInput[] = [];
  /** How many init calls fail before one succeeds. */
  failures = 0;
  sink: GlueSink | null = null;
  level = 0;
  installs = 0;
  levelChanges = 0;
  /** Calls into the module after init: every method below counts. */
  entered = 0;
  freed: string[] = [];
  /** Every grammar handed out, in order. */
  readonly made: FakeGrammar[] = [];
  /** The load options of every readGrammar call, in order. */
  readonly loads: string[] = [];

  default(options: { module_or_path: GlueInitInput }): Promise<unknown> {
    this.inputs.push(options.module_or_path);
    if (this.failures > 0) {
      this.failures -= 1;
      return Promise.reject(new Error("not a module"));
    }
    return Promise.resolve({});
  }

  install(sink: GlueSink, level: number): void {
    this.installs += 1;
    this.sink = sink;
    this.level = level;
  }

  setLogLevel(level: number): void {
    this.entered += 1;
    this.levelChanges += 1;
    this.level = level;
  }

  version(): string {
    this.entered += 1;
    return "0.0.0-fake";
  }

  /** Bytes that start with 0 are no grammar, as the module would say. */
  readGrammar(bytes: Uint8Array, options: string): GrammarGlue {
    this.entered += 1;
    this.loads.push(options);
    if (bytes[0] === 0) this.fail("invalid-grammar", "not a binary grammar");
    const grammar = new FakeGrammar(this, `grammar ${this.made.length}`);
    this.made.push(grammar);
    return grammar;
  }

  /** The arguments of every convert and mwesplit call, in order. */
  readonly conversions: string[] = [];

  /** Answers every input with the input itself; an input of "fail" fails as the library does. */
  convert(input: string, options: string): string {
    this.entered += 1;
    this.conversions.push(`convert ${input} ${options}`);
    if (input === "fail") this.fail("run-failed", "the conversion failed");
    return input;
  }

  mwesplit(input: string): string {
    this.entered += 1;
    this.conversions.push(`mwesplit ${input}`);
    if (input === "fail") this.fail("run-failed", "the split failed");
    return input;
  }

  /** Logs as the module does: only at or above the level set. */
  log(level: number, message: string): void {
    this.entered += 1;
    if (level <= this.level) this.sink?.log(level, "fake", message);
  }

  /** Panics with `message`, or traps without one when it is null. */
  trap(message: string | null): never {
    this.entered += 1;
    if (message !== null) this.sink?.panic(message);
    throw new WebAssembly.RuntimeError("unreachable");
  }

  /** Throws the way a failing export does. */
  fail(code: string, message: string): never {
    this.entered += 1;
    throw { code, message };
  }

  object(label: string): FakeObject {
    this.entered += 1;
    return new FakeObject(this, label);
  }
}

export class FakeObject implements GlueObject {
  readonly #glue: FakeGlue;
  readonly label: string;
  /** When set, free() traps and frees nothing. */
  trapOnFree = false;

  constructor(glue: FakeGlue, label: string) {
    this.#glue = glue;
    this.label = label;
  }

  protected get glue(): FakeGlue {
    return this.#glue;
  }

  free(): void {
    if (this.trapOnFree) this.#glue.trap("free failed");
    this.#glue.entered += 1;
    this.#glue.freed.push(this.label);
  }
}

const FAKE_INFO: GrammarInfo = {
  sections: 2,
  templates: 0,
  rules: 3,
  sets: 4,
  tags: 5,
  rulesAny: 1,
  hasDependencyRules: false,
  hasRelationRules: true,
};

export class FakeGrammar extends FakeObject implements GrammarGlue {
  /** The arguments of every run, in order. */
  readonly runs: [string, string][] = [];
  /** The warnings every run reports. */
  warnings: string[] = [];
  /** When set, a run fails with this message, as the library reports a failed run. */
  failure: string | null = null;

  info(): GrammarInfo {
    this.glue.entered += 1;
    return { ...FAKE_INFO };
  }

  /** Answers every input with the input itself. */
  apply(input: string, options: string): ApplyResult {
    this.glue.entered += 1;
    this.runs.push([input, options]);
    if (this.failure !== null) this.glue.fail("run-failed", this.failure);
    return { output: input, warnings: [...this.warnings] };
  }

  /** Answers every input with the input itself. */
  proc(input: string, options: string): string {
    this.glue.entered += 1;
    this.runs.push([input, options]);
    if (this.failure !== null) this.glue.fail("run-failed", this.failure);
    return input;
  }
}
