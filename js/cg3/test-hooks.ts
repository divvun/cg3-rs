// The test hooks a module of a test build exports, from
// `crates/cg3-wasm-shared/src/test_hooks.rs`, as the test entry modules
// testing.ts and compile-testing.ts hand them out. Nothing here loads a module.

import type { Core } from "./core.ts";
import type { GlueObject, ModuleGlue } from "./glue.ts";
import { Handle } from "./handle.ts";
import { levelNumber, type LogEvent } from "./logging.ts";

interface TestObjectGlue extends GlueObject {
  label(): string;
  panicWhileBorrowed(message: string): void;
}

/** The exports of `crates/cg3-wasm-shared/src/test_hooks.rs`. */
interface TestHooksGlue {
  TestObject: new (label: string) => TestObjectGlue;
  testPanic(message: string): void;
  testLog(level: number, message: string): void;
}

/** A small object that owns module memory. Freeing it logs `dropped <label>` at debug level. */
export class TestObject extends Handle<TestObjectGlue> {
  constructor(core: Core<ModuleGlue>, glue: TestObjectGlue) {
    super(core, glue);
  }

  label(): string {
    return this.use((glue) => glue.label());
  }

  /** Panics inside a method, while the module holds a borrow of the object. */
  panicWhileBorrowed(message: string): void {
    this.use((glue) => glue.panicWhileBorrowed(message));
  }
}

export interface TestHooks {
  createObject(label: string): TestObject;
  /** Panics in the module, which traps it. */
  panic(message: string): void;
  /** Logs `message` from the module at `level`, through the library's tracing. */
  log(level: LogEvent["level"], message: string): void;
}

function hasTestHooks(glue: object): glue is TestHooksGlue {
  return ["TestObject", "testPanic", "testLog"].every((name) => name in glue);
}

/** The test hooks of the module behind `core`, whose generated glue is `glue`. */
export function testHooks(core: Core<ModuleGlue>, glue: object): TestHooks {
  if (!hasTestHooks(glue)) {
    throw new Error("this build has no test hooks: build it with node js/build.mjs --test");
  }
  return {
    createObject: (label) => new TestObject(core, core.call(() => new glue.TestObject(label))),
    panic: (message) => core.call(() => glue.testPanic(message)),
    log: (level, message) => core.call(() => glue.testLog(levelNumber(level), message)),
  };
}
