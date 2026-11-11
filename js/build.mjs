// Builds the npm package @divvun/cg3 from crates/cg3-wasm, behind the main
// entry point, and crates/cg3-wasm-compile, behind @divvun/cg3/compile.
//
//   node js/build.mjs          the package
//   node js/build.mjs --test   the package, then a test build of it
//
// This writes js/cg3/pkg and js/cg3/pkg-compile (the wasm-bindgen output of
// each module) and js/cg3/dist (the compiled wrapper, with a copy of both
// where the compiled binding modules import them from, and the licence). It
// needs cargo with the wasm32-unknown-unknown target, the wasm-bindgen CLI at
// the version the crates pin, wasm-opt (binaryen) and npx.
//
// Before it optimises them, it checks the modules' function names: the main
// module must not link the textual parser or the grammar writers, and the
// compile module must, which shows the check can see them.
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
import { assertReadOnlyCore } from "./test/read-only-core.ts";
import { buildFixtures, buildTools } from "./test/fixtures.mjs";

const js = dirname(fileURLToPath(import.meta.url));
const root = dirname(js);
const packageDir = join(js, "cg3");
const testBuild = join(root, "target", "js-test");

/** The package's two modules: the crate each is built from, and the directory its glue goes to. */
const MODULES = {
  main: { crate: "cg3-wasm", out: "pkg" },
  compile: { crate: "cg3-wasm-compile", out: "pkg-compile" },
};

function run(command, args) {
  console.log(`$ ${command} ${args.join(" ")}`);
  execFileSync(command, args, { cwd: root, stdio: "inherit" });
}

// The CLI writes glue for its own version of the wasm-bindgen ABI, so it must
// be the version every crate the modules are built from links.
function assertBindgenVersion() {
  const installed = execFileSync("wasm-bindgen", ["--version"], { encoding: "utf8" }).trim().split(" ")[1];
  for (const crate of ["cg3-wasm", "cg3-wasm-compile", "cg3-wasm-shared"]) {
    const manifest = readFileSync(join(root, "crates", crate, "Cargo.toml"), "utf8");
    const pinned = /^wasm-bindgen = "=([^"]+)"$/m.exec(manifest)?.[1];
    if (pinned === undefined) throw new Error(`crates/${crate}/Cargo.toml does not pin wasm-bindgen`);
    if (installed !== pinned) throw new Error(`the wasm-bindgen CLI is ${installed}; ${crate} pins ${pinned}`);
  }
}

// A test build uses the dev profile, so it never overwrites the release module.
function cargoBuild(crate, testHooks) {
  const profile = testHooks ? "dev" : "wasm-release";
  const features = testHooks ? ["--features", "test-hooks"] : [];
  run("cargo", ["build", "--target", "wasm32-unknown-unknown", "--profile", profile, "-p", crate, ...features]);
  const profileDir = testHooks ? "debug" : "wasm-release";
  return join(root, "target", "wasm32-unknown-unknown", profileDir, `${crate.replaceAll("-", "_")}.wasm`);
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
function assertNoTestHooks(pkgDir, crate) {
  const glue = readFileSync(join(pkgDir, `${crate.replaceAll("-", "_")}.d.ts`), "utf8");
  if (glue.includes("testPanic")) throw new Error(`the release glue of ${crate} has test hooks`);
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

/** Builds the package and returns the size of each module's .wasm file in bytes. */
function buildPackage() {
  assertBindgenVersion();
  const dist = join(packageDir, "dist");
  const modules = {};
  for (const [name, { crate, out }] of Object.entries(MODULES)) {
    const pkgDir = join(packageDir, out);
    modules[name] = { crate, pkgDir, module: bindgen(cargoBuild(crate, false), pkgDir) };
  }
  // The modules keep their function names until wasm-opt, which drops them.
  assertReadOnlyCore(readFileSync(modules.main.module), readFileSync(modules.compile.module));
  for (const { crate, pkgDir, module } of Object.values(modules)) {
    run("wasm-opt", ["-Oz", ...WASM_FEATURES, module, "-o", module]);
    assertNoTestHooks(pkgDir, crate);
  }
  tsc(join(packageDir, "tsconfig.json"), dist);
  for (const { out } of Object.values(MODULES)) {
    cpSync(join(packageDir, out), join(dist, out), { recursive: true });
  }
  cpSync(join(root, "COPYING"), join(dist, "COPYING"));
  assertNoAny(dist);
  return Object.fromEntries(Object.entries(modules).map(([name, { module }]) => [name, statSync(module).size]));
}

// tsc type-checks the testing entries against the release glue in js/cg3,
// so the release build must come first. The test hooks are looked up at run
// time, which the release glue's types do not need to know about.
function buildTestPackage() {
  const dist = join(testBuild, "dist");
  tsc(join(packageDir, "tsconfig.test.json"), dist);
  for (const { crate, out } of Object.values(MODULES)) {
    bindgen(cargoBuild(crate, true), join(dist, out));
  }
  // A browser runs no TypeScript, so the parity check it runs is compiled.
  tsc(join(js, "test", "tsconfig.browser.json"), join(testBuild, "browser"));
}

// The tests import this module for its checks, so it builds only when run.
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const sizes = buildPackage();
  if (process.argv.includes("--test")) {
    buildTestPackage();
    buildFixtures(buildTools(root, run), join(root, "test"), join(testBuild, "fixtures"));
  }
  for (const [name, bytes] of Object.entries(sizes)) {
    const entry = name === "main" ? "@divvun/cg3" : `@divvun/cg3/${name}`;
    console.log(`${entry}: ${bytes} bytes of WebAssembly (${(bytes / 1024).toFixed(1)} KiB)`);
  }
}
