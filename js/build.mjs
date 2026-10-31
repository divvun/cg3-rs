// Builds the npm package @divvun/cg3 from crates/cg3-wasm.
//
//   node js/build.mjs          the package
//   node js/build.mjs --test   the package, then a test build of it
//
// This writes js/cg3/pkg (the wasm-bindgen output) and js/cg3/dist (the
// compiled wrapper, with a copy of pkg where the compiled binding module
// imports it from, and the licence). It needs cargo with the
// wasm32-unknown-unknown target, the wasm-bindgen CLI at the version
// crates/cg3-wasm pins, wasm-opt (binaryen) and npx.
//
// A test build turns on the crate's test-hooks feature and adds the testing
// entry module. It is written to target/js-test, never to js/, so the hooks
// cannot reach the published package. It also builds the cg3 tools, writes
// the fixtures to target/js-test/fixtures, and compiles the parity check for
// browsers to target/js-test/browser.

import { execFileSync } from "node:child_process";
import { cpSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { basename, dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isPublicDeclaration, usesAny } from "./test/declarations.ts";
import { buildFixtures, buildTools } from "./test/fixtures.mjs";

const js = dirname(fileURLToPath(import.meta.url));
const root = dirname(js);
const packageDir = join(js, "cg3");
const crateDir = join(root, "crates", "cg3-wasm");
const testBuild = join(root, "target", "js-test");

function run(command, args) {
  console.log(`$ ${command} ${args.join(" ")}`);
  execFileSync(command, args, { cwd: root, stdio: "inherit" });
}

// The CLI writes glue for its own version of the wasm-bindgen ABI, so it must
// be the version the crate links.
function assertBindgenVersion() {
  const pinned = /^wasm-bindgen = "=([^"]+)"$/m.exec(readFileSync(join(crateDir, "Cargo.toml"), "utf8"))?.[1];
  if (pinned === undefined) throw new Error("crates/cg3-wasm/Cargo.toml does not pin wasm-bindgen");
  const installed = execFileSync("wasm-bindgen", ["--version"], { encoding: "utf8" }).trim().split(" ")[1];
  if (installed !== pinned) throw new Error(`the wasm-bindgen CLI is ${installed}; cg3-wasm pins ${pinned}`);
}

// A test build uses the dev profile, so it never overwrites the release module.
function cargoBuild(testHooks) {
  const profile = testHooks ? "dev" : "wasm-release";
  const features = testHooks ? ["--features", "test-hooks"] : [];
  run("cargo", ["build", "--target", "wasm32-unknown-unknown", "--profile", profile, "-p", "cg3-wasm", ...features]);
  const profileDir = testHooks ? "debug" : "wasm-release";
  return join(root, "target", "wasm32-unknown-unknown", profileDir, "cg3_wasm.wasm");
}

function bindgen(wasm, outDir) {
  rmSync(outDir, { recursive: true, force: true });
  run("wasm-bindgen", ["--target", "web", "--out-dir", outDir, wasm]);
  return join(outDir, `${basename(wasm, ".wasm")}_bg.wasm`);
}

// The features rustc enables by default for wasm32-unknown-unknown; wasm-opt
// refuses a module that uses one it was not told about.
const WASM_FEATURES = [
  "--enable-bulk-memory",
  "--enable-nontrapping-float-to-int",
  "--enable-sign-ext",
  "--enable-reference-types",
  "--enable-multivalue",
  "--enable-mutable-globals",
];

// The release build never turns test-hooks on; this makes that a checked fact.
function assertNoTestHooks(pkgDir) {
  const glue = readFileSync(join(pkgDir, "cg3_wasm.d.ts"), "utf8");
  if (glue.includes("testPanic")) throw new Error("the release glue has test hooks");
}

function tsc(project, outDir) {
  rmSync(outDir, { recursive: true, force: true });
  run("npx", ["-y", "-p", "typescript@5", "tsc", "-p", project, "--outDir", outDir]);
}

function* files(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) yield* files(path);
    else yield path;
  }
}

// The wrapper's declarations are the public API, so none of them may use any.
export function assertNoAny(dist) {
  const offenders = [...files(dist)].filter(
    (file) => isPublicDeclaration(relative(dist, file)) && usesAny(readFileSync(file, "utf8")),
  );
  if (offenders.length > 0) {
    throw new Error(`declarations use any: ${offenders.join(", ")}`);
  }
}

/** Builds the package and returns the size of its .wasm file in bytes. */
function buildPackage() {
  assertBindgenVersion();
  const pkgDir = join(packageDir, "pkg");
  const dist = join(packageDir, "dist");
  const module = bindgen(cargoBuild(false), pkgDir);
  run("wasm-opt", ["-Oz", ...WASM_FEATURES, module, "-o", module]);
  assertNoTestHooks(pkgDir);
  tsc(join(packageDir, "tsconfig.json"), dist);
  cpSync(pkgDir, join(dist, "pkg"), { recursive: true });
  cpSync(join(root, "COPYING"), join(dist, "COPYING"));
  assertNoAny(dist);
  return statSync(module).size;
}

// tsc type-checks the testing entry against the release glue in js/cg3/pkg,
// so the release build must come first. The test hooks are looked up at run
// time, which the release glue's types do not need to know about.
function buildTestPackage() {
  const dist = join(testBuild, "dist");
  tsc(join(packageDir, "tsconfig.test.json"), dist);
  bindgen(cargoBuild(true), join(dist, "pkg"));
  // A browser runs no TypeScript, so the parity check it runs is compiled.
  tsc(join(js, "test", "tsconfig.browser.json"), join(testBuild, "browser"));
}

// The tests import this module for its checks, so it builds only when run.
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const bytes = buildPackage();
  if (process.argv.includes("--test")) {
    buildTestPackage();
    buildFixtures(buildTools(root, run), join(root, "test"), join(testBuild, "fixtures"));
  }
  console.log(`@divvun/cg3: ${bytes} bytes of WebAssembly (${(bytes / 1024).toFixed(1)} KiB)`);
}
