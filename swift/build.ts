// Builds what the Swift package divvun.cg3 compiles against: the static
// library crates/cg3-swift-ffi, the Swift bindings uniffi-bindgen-swift
// generates for it, and CG3FFI.xcframework, which holds the library and the
// C headers of the bindings. It installs uniffi-bindgen-swift at the version
// of uniffi in Cargo.lock.
//
//   node swift/build.ts          the macOS slice, for `swift test` in swift/
//   node swift/build.ts --test   that, and the fixtures the tests read
//
// It writes swift/CG3FFI.xcframework and swift/Sources/CG3Bindings, which
// git ignores, and works in target/swift. The fixtures are those
// js/test/fixtures.mjs writes for @divvun/cg3's tests, with this checkout's
// tools, in target/swift/fixtures.

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildFixtures, buildTools } from "../js/test/fixtures.mjs";

const swift = dirname(fileURLToPath(import.meta.url));
const root = dirname(swift);
const work = join(root, "target", "swift");

/** The crate whose static library the XCFramework holds. */
const LIBRARY = { crate: "cg3-swift-ffi", file: "libcg3_swift_ffi.a" };

/** Each UniFFI crate the library holds: the Swift module its bindings are compiled into. */
const MODULES = [{ crate: "cg3-swift", module: "CG3Bindings" }];

/** The XCFramework. */
const FRAMEWORK = "CG3FFI";

/** The cargo profile the library is built with: fat LTO and one codegen unit. */
const PROFILE = "swift-release";

function run(command: string, args: string[], env: Record<string, string> = {}): void {
  console.log(`$ ${command} ${args.join(" ")}`);
  execFileSync(command, args, { cwd: root, stdio: "inherit", env: { ...process.env, ...env } });
}

/** Builds the library for `target` and returns where cargo wrote it. */
function cargoBuild(target: string): string {
  run("cargo", ["build", "--profile", PROFILE, "--target", target, "-p", LIBRARY.crate], {
    MACOSX_DEPLOYMENT_TARGET: "11.0",
  });
  return join(root, "target", target, PROFILE, LIBRARY.file);
}

/** The version of uniffi in Cargo.lock, which every glue crate links. */
function uniffiVersion(): string {
  const lock = readFileSync(join(root, "Cargo.lock"), "utf8");
  const version = /\[\[package\]\]\nname = "uniffi"\nversion = "([^"]+)"/.exec(lock)?.[1];
  if (version === undefined) throw new Error("Cargo.lock has no uniffi");
  return version;
}

/**
 * uniffi-bindgen-swift, the binary of uniffi's `cli` feature, at the version
 * the glue links: the code it generates must match the scaffolding the
 * crates were built with. It is installed in target/swift/bindgen once per
 * version.
 */
function bindgenTool(): string {
  const version = uniffiVersion();
  const prefix = join(work, "bindgen", version);
  const tool = join(prefix, "bin", "uniffi-bindgen-swift");
  if (!existsSync(tool)) {
    const install = ["install", "--locked", "--quiet", "uniffi", "--version", version, "--features", "cli"];
    run("cargo", [...install, "--bin", "uniffi-bindgen-swift", "--root", prefix]);
  }
  return tool;
}

/**
 * Generates the bindings of `library` with uniffi-bindgen-swift: each
 * module's Swift into swift/Sources/<module>, and the C headers with a module
 * map declaring each module's Clang module into the directory it returns.
 */
function bindgen(library: string): string {
  const generated = join(work, "generated");
  rmSync(generated, { recursive: true, force: true });
  const tool = bindgenTool();
  for (const kind of ["--swift-sources", "--headers"]) run(tool, [kind, library, generated]);

  const headers = join(work, "headers");
  rmSync(headers, { recursive: true, force: true });
  for (const { module } of MODULES) {
    const sources = join(swift, "Sources", module);
    rmSync(sources, { recursive: true, force: true });
    mkdirSync(sources, { recursive: true });
    copyFileSync(join(generated, `${module}.swift`), join(sources, `${module}.swift`));
    // Each Clang module in a directory of its name, where Clang looks for
    // `import <name>`, so no other package's module map shares its path.
    const clang = `${module}FFI`;
    const dir = join(headers, clang);
    mkdirSync(dir, { recursive: true });
    copyFileSync(join(generated, `${clang}.h`), join(dir, `${clang}.h`));
    writeFileSync(join(dir, "module.modulemap"), `module ${clang} {\n    header "${clang}.h"\n    export *\n}\n`);
  }
  return headers;
}

/** Makes swift/CG3FFI.xcframework of `libraries`, each with `headers`. */
function xcframework(libraries: string[], headers: string): void {
  const output = join(swift, `${FRAMEWORK}.xcframework`);
  rmSync(output, { recursive: true, force: true });
  const args = libraries.flatMap((library) => ["-library", library, "-headers", headers]);
  run("xcodebuild", ["-create-xcframework", ...args, "-output", output]);
}

const library = cargoBuild("aarch64-apple-darwin");
xcframework([library], bindgen(library));
if (process.argv.includes("--test")) {
  buildFixtures(buildTools(root, run), join(root, "test"), join(work, "fixtures"));
}
