// Runs the JavaScript test suite against the built package: the unit and
// package tests under Node and Bun, the scenarios (starting the package, the
// API contract, poisoning, and the parity check) under Node, Bun and Deno,
// and the parity check in a headless Chromium on a page and in a Web Worker.
//
//   node js/build.mjs --test && node js/test/run.mjs [--runtime node|bun|deno|chromium]... [--require-all]
//
// A runtime that is not installed is skipped with a message. --require-all,
// or CG3_JS_TEST_REQUIRE_ALL=1, fails the run instead, and fails it when
// --runtime leaves any of the four out. CG3_JS_TEST_BROWSER names the browser
// to run.
//
// Each scenario variant runs in a process of its own, once per runtime,
// because a started or poisoned module instance cannot be reset. Scenarios
// that need the test hooks run against the test build in target/js-test; the
// rest run against the package as it would ship. Deno runs each with read
// access to the package and the fixtures alone. The runner also serves the
// package's .wasm file over HTTP, so the suite can start the package from an
// http: URL and from a fetched Response.

import { spawn, spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { requireAll, runParityInChromium } from "./parity-runner.mjs";

const js = dirname(dirname(fileURLToPath(import.meta.url)));
const testBuild = join(dirname(js), "target", "js-test");
const fixtures = join(testBuild, "fixtures");

const wasmFile = "cg3_wasm_bg.wasm";

const RUNTIMES = ["node", "bun", "deno", "chromium"];

const scenarios = [
  {
    file: "init.ts",
    build: "release",
    variants: ["bytes", "file-url", "http-url", "response", "module", "default", "retry"],
    network: true,
  },
  { file: "contract.ts", build: "test", variants: ["main"] },
  { file: "poison.ts", build: "test", variants: ["function", "method"] },
  { file: "parity-runtime.ts", build: "release", variants: ["main"] },
];

const UNIT_RUNTIMES = ["node", "bun"];

// Unit and package tests: every *.test.ts and *.test.mjs outside build output.
function unitTests(dir = js) {
  const found = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!["dist", "pkg", "node_modules"].includes(entry.name)) found.push(...unitTests(path));
    } else if (/\.test\.m?[jt]s$/.test(entry.name)) {
      found.push(path);
    }
  }
  return found.sort();
}

function chosenRuntimes(argv) {
  const chosen = [];
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--runtime" && argv[i + 1] !== undefined) chosen.push(argv[(i += 1)]);
  }
  return chosen.length > 0 ? chosen : RUNTIMES;
}

function installed(runtime) {
  const probe = spawnSync(runtime, ["--version"], { stdio: "ignore" });
  return probe.error === undefined && probe.status === 0;
}

function locate() {
  const release = join(js, "cg3", "dist");
  const test = join(testBuild, "dist");
  const manifest = JSON.parse(readFileSync(join(js, "cg3", "package.json"), "utf8"));
  return {
    name: manifest.name,
    version: manifest.version,
    release: { dir: release, module: join(release, "index.js"), wasm: join(release, "pkg", wasmFile) },
    test: { dir: test, module: join(test, "testing.js"), wasm: join(test, "pkg", wasmFile) },
  };
}

function serve(pkg) {
  const server = createServer((request, response) => {
    if (request.url !== `/${wasmFile}`) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, { "content-type": "application/wasm" });
    response.end(readFileSync(pkg.release.wasm));
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

/**
 * The command that runs the script at `path` under `runtime`. Node and Bun
 * collect garbage on request only with a flag or through Bun.gc; Deno gets
 * read access to `readable` and nothing else but, when `network`, the
 * runner's own server, and reads no config file, import map or lock file.
 */
function command(runtime, path, args, { readable, network }) {
  switch (runtime) {
    case "node":
      return ["node", ["--expose-gc", path, ...args]];
    case "deno": {
      if (readable.some((dir) => dir.includes(","))) {
        throw new Error(`deno takes --allow-read as a comma-separated list, so it cannot name ${readable.join(" or ")}`);
      }
      const flags = ["run", "--no-config", "--no-prompt", "--v8-flags=--expose-gc", `--allow-read=${readable.join(",")}`];
      if (network) flags.push("--allow-net=127.0.0.1");
      return ["deno", [...flags, path, ...args]];
    }
    default:
      return [runtime, [path, ...args]];
  }
}

function runFile(runtime, path, args, access = { readable: [], network: false }) {
  return new Promise((resolve) => {
    const [program, argv] = command(runtime, path, args, access);
    const child = spawn(program, argv, { stdio: ["ignore", "inherit", "inherit"] });
    child.on("error", (error) => {
      console.log(`not ok - ${runtime} ${path}: cannot start ${runtime}: ${error.message}`);
      resolve(false);
    });
    child.on("close", (code) => resolve(code === 0));
  });
}

const argv = process.argv.slice(2);
const required = requireAll(argv);
const runtimes = chosenRuntimes(argv);
const pkg = locate();
const server = await serve(pkg);
const base = `http://127.0.0.1:${server.address().port}`;
let failures = 0;
let runs = 0;

function unavailable(runtime, label, why) {
  if (required) {
    console.log(`not ok - ${runtime} ${label}: ${why}, and every runtime is required`);
    runs += 1;
    failures += 1;
  } else {
    console.log(`ok - ${runtime} ${label} # SKIP ${why} (--require-all makes this a failure)`);
  }
}

for (const runtime of RUNTIMES.filter((name) => !runtimes.includes(name))) {
  unavailable(runtime, pkg.name, "--runtime leaves it out");
}

for (const runtime of runtimes.filter((name) => name !== "chromium")) {
  if (!installed(runtime)) {
    unavailable(runtime, pkg.name, `${runtime} is not on PATH`);
    continue;
  }
  if (UNIT_RUNTIMES.includes(runtime)) {
    for (const path of unitTests()) {
      runs += 1;
      if (!(await runFile(runtime, path, []))) failures += 1;
    }
  }
  for (const { file, build, variants, network = false } of scenarios) {
    for (const variant of variants) {
      const label = `${pkg.name} ${file} ${variant}`;
      const args = {
        module: pathToFileURL(pkg[build].module).href,
        wasm: pathToFileURL(pkg[build].wasm).href,
        http: `${base}/${wasmFile}`,
        package: pkg.name,
        version: pkg.version,
        fixtures: `${pathToFileURL(fixtures).href}/`,
        variant,
        label,
      };
      const access = { readable: [pkg[build].dir, fixtures], network };
      const passed = await runFile(runtime, join(js, "test", file), [JSON.stringify(args)], access);
      runs += 1;
      if (!passed) failures += 1;
    }
  }
}

if (runtimes.includes("chromium")) {
  const outcome = await runParityInChromium({ pkg, testBuild, unavailable });
  runs += outcome.runs;
  failures += outcome.failures;
}

server.close();
console.log(`${runs - failures} of ${runs} test files passed`);
if (failures > 0) process.exitCode = 1;
