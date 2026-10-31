import { FakeGlue } from "../test/fake-glue.ts";
import { assert, assertEqual, runTests, test, thrownCode, withConsole } from "../test/harness.ts";
import { Core } from "./core.ts";
import { Cg3Error } from "./errors.ts";
import { grammarReader } from "./grammar.ts";
import { consoleLogger, eventLevel, isLogLevel, type LogEvent, levelNumber } from "./logging.ts";

async function started(): Promise<{ core: Core; glue: FakeGlue }> {
  const glue = new FakeGlue();
  const core = new Core(glue, new URL("http://127.0.0.1:9/never.wasm"));
  await core.init(new Uint8Array(0));
  return { core, glue };
}

test("levels number from off to trace, and unknown numbers read as trace", () => {
  assertEqual(levelNumber("off"), 0, "off");
  assertEqual(levelNumber("warn"), 2, "warn");
  assertEqual(eventLevel(5), "trace", "5");
  assertEqual(eventLevel(1), "error", "1");
  assertEqual(eventLevel(42), "trace", "42");
  assert(isLogLevel("debug") && !isLogLevel("verbose") && !isLogLevel(3), "isLogLevel");
});

// [spec:cg3:req:wasm.logging/test]
test("the default logger writes each event to the console method for its level", async () => {
  await withConsole((lines) => {
    consoleLogger({ level: "error", target: "cg3::a", message: "e" });
    consoleLogger({ level: "warn", target: "cg3::b", message: "w" });
    consoleLogger({ level: "info", target: "cg3::c", message: "i" });
    consoleLogger({ level: "trace", target: "cg3::d", message: "t" });
    const seen = lines.map(({ method, text }) => `${method} ${text}`).join("; ");
    assertEqual(seen, "error cg3::a: e; warn cg3::b: w; info cg3::c: i; debug cg3::d: t", "lines");
  });
});

// [spec:cg3:req:wasm.logging/test]
test("by default warnings and errors reach the console and lower levels are dropped", async () => {
  const { glue } = await started();
  await withConsole((lines) => {
    for (const level of [1, 2, 3, 4, 5]) glue.log(level, `at ${level}`);
    assertEqual(lines.map((line) => line.method).join(" "), "error warn", "console methods used");
  });
});

// [spec:cg3:req:wasm.logging/test]
test("a replaced logger gets each event's level, target and message", async () => {
  const { core, glue } = await started();
  const events: LogEvent[] = [];
  core.setLogger((event) => events.push(event));
  glue.log(2, "careful");
  assertEqual(JSON.stringify(events), '[{"level":"warn","target":"fake","message":"careful"}]', "events");
  await withConsole((lines) => {
    core.setLogger(null);
    glue.log(2, "back on the console");
    assertEqual(lines.length, 1, "console lines after setLogger(null)");
  });
});

// [spec:cg3:req:wasm.logging/test]
test("setLogLevel tells the module, which filters what it sends", async () => {
  const { core, glue } = await started();
  assertEqual(glue.level, 2, "the level init starts the module with");
  const levels: string[] = [];
  core.setLogger((event) => levels.push(event.level));
  core.setLogLevel("debug");
  assertEqual(glue.levelChanges, 1, "level changes sent to the module");
  for (const level of [1, 2, 3, 4, 5]) glue.log(level, "x");
  core.setLogLevel("error");
  for (const level of [1, 2, 3]) glue.log(level, "y");
  core.setLogLevel("off");
  glue.log(1, "z");
  assertEqual(levels.join(" "), "error warn info debug error", "levels delivered");
});

// [spec:cg3:req:wasm.logging/test]
test("whatever the logger and level, apply returns its run's warnings", async () => {
  const { core, glue } = await started();
  const grammar = grammarReader(core).readGrammar(new Uint8Array([1]));
  const made = glue.made[0];
  assert(made !== undefined, "the glue made a grammar");
  made.warnings = ["a warning"];
  core.setLogger(() => {});
  core.setLogLevel("off");
  assertEqual(grammar.apply("x").warnings.join(), "a warning", "warnings");
});

test("setLogger and setLogLevel refuse what is not a logger or a level", async () => {
  const { core } = await started();
  const notALogger = "console" as unknown as null;
  const notALevel = "verbose" as unknown as "warn";
  assertEqual(thrownCode(Cg3Error, () => core.setLogger(notALogger)), "invalid-argument", "logger");
  assertEqual(thrownCode(Cg3Error, () => core.setLogLevel(notALevel)), "invalid-argument", "level");
});

// [spec:cg3:req:wasm.logging/test]
test("a logger that throws is reported on the console and does not reach the module", async () => {
  const { core, glue } = await started();
  core.setLogger(() => {
    throw new Error("logger bug");
  });
  await withConsole((lines) => {
    glue.log(1, "anything");
    assertEqual(lines[0]?.method, "error", "reported with console.error");
    assert(lines[0]?.text.includes("logger bug") === true, "the report names the error");
  });
  assertEqual(core.call(() => glue.version()), "0.0.0-fake", "the module carries on");
});

test("setLogger and setLogLevel before init throw not-initialised", () => {
  const core = new Core(new FakeGlue(), new URL("http://127.0.0.1:9/never.wasm"));
  assertEqual(thrownCode(Cg3Error, () => core.setLogger(null)), "not-initialised", "setLogger");
  assertEqual(thrownCode(Cg3Error, () => core.setLogLevel("info")), "not-initialised", "setLogLevel");
});

await runTests("cg3/logging");
