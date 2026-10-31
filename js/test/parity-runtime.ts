// The parity check (parity.ts) under Node, Bun and Deno, in a process of its
// own. run.mjs passes the arguments as JSON: Node and Bun run this from its
// scenario list, and Deno with read access to the package and the fixtures
// alone, which is all the check needs.

import { readBytes, runtime, scenarioArgs } from "./harness.ts";
import { report, runParity } from "./parity.ts";

// [spec:cg3:req:wasm.parity+2/test]
const args = scenarioArgs();
const label = `${runtime} ${args.label}`;
const { lines, passed } = report(label, await runParity(args, readBytes));
for (const line of lines) console.log(line);
if (!passed) throw new Error(`${label}: the package's output differs from vislcg3's`);
