// The tool parity check (tool-parity.ts) under Node, Bun and Deno, in a
// process of its own, against the package as it ships. Deno runs it with read
// access to the package and the fixtures alone.

import { readBytes, runTests, runtime, scenarioArgs, test } from "./harness.ts";
import { report } from "./parity.ts";
import { runToolParity } from "./tool-parity.ts";

const args = scenarioArgs();

// [spec:cg3:req:wasm.compile-parity/test]
test("the package compiles, writes, converts, splits, relabels and runs as the tools do", async () => {
  const checked = { module: args.entries.main.module, compile: args.entries.compile.module, fixtures: args.fixtures };
  const label = `${runtime} ${args.label}`;
  const { lines, passed } = report(label, await runToolParity(checked, readBytes), "cases the same as the tools");
  for (const line of lines) console.log(line);
  if (!passed) throw new Error("the package's output differs from the tools'");
});

await runTests(args.label);
