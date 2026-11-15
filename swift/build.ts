// Builds the Swift package divvun.cg3: the static library crates/cg3-swift-ffi
// for each slice of CG3FFI.xcframework, the Swift bindings
// uniffi-bindgen-swift generates for it, the XCFramework itself, and, for a
// release, the source archive SwiftPM's registry serves.
//
//   node swift/build.ts              the macOS slice for this machine, for
//                                    `swift test` in swift/
//   node swift/build.ts --simulator  that, and the iOS simulator slice, for
//                                    `xcodebuild test` on a simulator
//   node swift/build.ts --release    every slice, and the source archive with
//                                    its package-metadata.json in
//                                    target/swift/release
//   --test                           also writes the fixtures the tests read
//
// Each slice's library is built with fat LTO and then prelinked into one
// object whose only global symbols are its C interface
// ([dec:cg3:swift-package]), so it links beside any other Rust library. The
// build checks that, and that a binary calling only CG3's glue carries no
// compiler.
//
// It writes swift/CG3FFI.xcframework, swift/Sources/CG3Bindings and
// swift/Sources/CG3CompileBindings, and the linking test's
// swift/LinkProbeFFI.xcframework and swift/Tests/LinkProbe, which git
// ignores, and works in target/swift. The fixtures are those js/test/fixtures.mjs writes for
// @divvun/cg3's tests, with this checkout's tools, in target/swift/fixtures.
// It installs uniffi-bindgen-swift at the version of uniffi in Cargo.lock.

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { buildFixtures, buildTools } from "../js/test/fixtures.mjs";

const swift = dirname(fileURLToPath(import.meta.url));
const root = dirname(swift);
const work = join(root, "target", "swift");

/** The crate whose static library the XCFramework holds. */
const LIBRARY = { crate: "cg3-swift-ffi", file: "libcg3_swift_ffi.a" };

/**
 * Each UniFFI crate the library holds: the Swift module its bindings are
 * compiled into, and the prefix of its C functions.
 */
const MODULES = [
  { crate: "cg3-swift", module: "CG3Bindings", prefix: "cg3_swift_" },
  { crate: "cg3-swift-compile", module: "CG3CompileBindings", prefix: "cg3_swift_compile_" },
];

/** The XCFramework, which the manifests name as a binary target. */
const FRAMEWORK = "CG3FFI";

/** The cargo profile the library is built with: fat LTO and one codegen unit. */
const PROFILE = "swift-release";

/** The package's name in its identifier, divvun.cg3, and its archive's. */
const PACKAGE = "cg3";

/**
 * The linking test's second Rust library, swift/link-probe: its own cargo
 * workspace, built with LTO by its own profile and not prelinked, as another
 * package's library may be.
 */
const PROBE = { manifest: join(swift, "link-probe", "Cargo.toml"), file: "libcg3_link_probe.a", module: "LinkProbe" };

/** A slice of the XCFramework: the Rust targets whose libraries it joins, and the least OS version they are built for. */
type Slice = { name: string; targets: string[]; env: Record<string, string> };

// [spec:cg3:req:swift.platforms]
const MACOS: Slice = {
  name: "macos",
  targets: ["aarch64-apple-darwin", "x86_64-apple-darwin"],
  env: { MACOSX_DEPLOYMENT_TARGET: "11.0" },
};
const IOS: Slice = { name: "ios", targets: ["aarch64-apple-ios"], env: { IPHONEOS_DEPLOYMENT_TARGET: "13.0" } };
// Rust's aarch64-apple-ios-sim supports nothing older than iOS 14.
const SIMULATOR: Slice = {
  name: "ios-simulator",
  targets: ["aarch64-apple-ios-sim"],
  env: { IPHONEOS_DEPLOYMENT_TARGET: "14.0" },
};

/** The parser's and the writers' types, as their names appear in symbols. */
export const COMPILER = ["13TextualParser", "13GrammarWriter", "10Relabeller"];

function run(command: string, args: string[], options: { env?: Record<string, string>; cwd?: string } = {}): void {
  console.log(`$ ${command} ${args.join(" ")}`);
  execFileSync(command, args, { cwd: options.cwd ?? root, stdio: "inherit", env: { ...process.env, ...options.env } });
}

function output(command: string, args: string[], cwd = root): string {
  return execFileSync(command, args, { cwd, encoding: "utf8", maxBuffer: 1 << 28 });
}

// [spec:cg3:req:swift.package]
/** The package's version, which crates/cg3-swift carries. */
export function packageVersion(): string {
  const manifest = readFileSync(join(root, "crates", "cg3-swift", "Cargo.toml"), "utf8");
  const version = /^version = "([^"]+)"$/m.exec(manifest)?.[1];
  if (version === undefined) throw new Error("crates/cg3-swift/Cargo.toml has no version");
  return version;
}

/** Builds the library for `target` and returns where cargo wrote it. */
function cargoBuild(target: string, env: Record<string, string>): string {
  run("cargo", ["build", "--locked", "--profile", PROFILE, "--target", target, "-p", LIBRARY.crate], { env });
  return join(root, "target", target, PROFILE, LIBRARY.file);
}

/** The arch `ld` and `clang` name a Rust target's by. */
function arch(target: string): string {
  return target.startsWith("x86_64") ? "x86_64" : "arm64";
}

/**
 * One member of an archive as `nm -m` lists it: the symbols it defines, with
 * their visibility, and those it needs. `nm` runs with `--no-llvm-bc`: Rust's
 * prebuilt compiler_builtins objects embed bitcode of a newer LLVM than
 * Xcode's `nm` reads.
 */
type Member = { defines: Defined[]; needs: string[] };

/** A symbol a member defines: its name, whether it is visible beyond its image, its section and its address there. */
type Defined = { name: string; external: boolean; section: string; address: number };

/**
 * Each member of an archive, by name, from `nm -m` of the archive: a symbol
 * is external when it is visible beyond its image, as `external` or `weak
 * external` and not `private external`.
 */
export function members(nm: string): Map<string, Member> {
  const found = new Map<string, Member>();
  let current: Member | undefined;
  for (const line of nm.split("\n")) {
    // `member.o:`, or `archive.a(member.o):` from other versions of nm.
    const header = /(?:^|\()([^()\s/]+\.o)\)?:$/.exec(line);
    if (header?.[1] !== undefined) {
      current = { defines: [], needs: [] };
      found.set(header[1], current);
      continue;
    }
    const symbol = /^(?:([0-9a-f]+) )?\(([^)]*)\) (.*?)(\S+)(?: \(.*\))?$/.exec(line.trim());
    if (current === undefined || symbol === null) continue;
    const [, address = "", section = "", kind = "", name = ""] = symbol;
    if (section === "undefined") current.needs.push(name);
    else if (!kind.includes("non-external")) {
      current.defines.push({ name, external: !kind.includes("private"), section, address: Number.parseInt(address, 16) });
    }
  }
  return found;
}

// [spec:cg3:req:swift.linking]
/** Whether a symbol is the library's C interface, `uniffi_cg3_swift*` or `ffi_cg3_swift*`, with Mach-O's leading underscore. */
export function isCInterface(name: string): boolean {
  return /^_(uniffi|ffi)_cg3_swift/.test(name);
}

// [spec:cg3:req:swift.linking]
/**
 * Whether `member` defines one function, at the start of its text, and
 * nothing else, as each of compiler-rt's outline-atomics helpers does: split
 * at its symbols it is the same one piece, so it may carry
 * `MH_SUBSECTIONS_VIA_SYMBOLS` though its assembler did not set it.
 */
export function oneFunction(member: Member): boolean {
  const [only, ...rest] = member.defines;
  return only !== undefined && rest.length === 0 && only.section === "__TEXT,__text" && only.address === 0;
}

/** `MH_SUBSECTIONS_VIA_SYMBOLS` in a Mach-O header's flags. */
const SUBSECTIONS_VIA_SYMBOLS = 0x2000;

// [spec:cg3:req:swift.linking]
/** `object`, a 64-bit Mach-O object, with `MH_SUBSECTIONS_VIA_SYMBOLS` set in its header's flags. */
export function withSubsections(object: Uint8Array): Uint8Array {
  const header = new DataView(object.buffer, object.byteOffset, object.byteLength);
  if (object.byteLength < 28 || header.getUint32(0, true) !== 0xfeedfacf) throw new Error("not a 64-bit Mach-O object");
  // A copy of its own: a Buffer from readFileSync may be a view into a
  // shared pool, whose slice() would not copy.
  const flagged = new Uint8Array(object);
  const view = new DataView(flagged.buffer, flagged.byteOffset, flagged.byteLength);
  view.setUint32(24, view.getUint32(24, true) | SUBSECTIONS_VIA_SYMBOLS, true);
  return flagged;
}

// [spec:cg3:req:swift.linking]
/**
 * The members to leave out of the prelink: those without
 * `MH_SUBSECTIONS_VIA_SYMBOLS`, since `ld -r` drops the flag from an object
 * when any input lacks it, and those that define what they need, which the
 * prelink would make local. The members compiler-rt adds to each slice
 * hold one function each and get the flag instead, so none is left out; one
 * that held more would be, with its symbols private but global in the image.
 */
export function leftOut(all: Map<string, Member>, unflagged: Set<string>): Set<string> {
  const out = new Set(unflagged);
  for (let grew = true; grew; ) {
    grew = false;
    const needed = new Set([...out].flatMap((name) => all.get(name)?.needs ?? []));
    for (const [name, member] of all) {
      if (!out.has(name) && member.defines.some((symbol) => needed.has(symbol.name))) {
        out.add(name);
        grew = true;
      }
    }
  }
  return out;
}

// [spec:cg3:req:swift.linking]
/**
 * Prelinks the library cargo built for `target` into one object whose only
 * global symbols are its C interface, as Xcode's single-object prelink does
 * with an exported symbols file: every other global, std's
 * `rust_eh_personality` among them, becomes local, so no other Rust library
 * in the same binary can clash with it, even under `-all_load`. A member
 * that lacks `MH_SUBSECTIONS_VIA_SYMBOLS` and holds one function is given
 * the flag; the members `leftOut` names stay beside the object as they were. Fails when any symbol is visible beyond
 * the image that links the library, when the object holds a global beside
 * the C interface, when a member left out would hold the C interface, or
 * when the object loses `MH_SUBSECTIONS_VIA_SYMBOLS`, without which an app's
 * linker could not drop the functions it does not call.
 */
function prelink(target: string, library: string): string {
  const dir = join(work, "prelink", target);
  const objects = join(dir, "objects");
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(objects, { recursive: true });
  run("ar", ["x", library], { cwd: objects });
  const all = members(output("nm", ["-m", "--no-llvm-bc", library]));
  const unflagged = new Set<string>();
  for (const name of all.keys()) {
    const path = join(objects, name);
    if (output("otool", ["-hv", path]).includes("SUBSECTIONS_VIA_SYMBOLS")) continue;
    const member = all.get(name);
    if (member !== undefined && oneFunction(member)) writeFileSync(path, withSubsections(readFileSync(path)));
    else unflagged.add(name);
  }
  const out = leftOut(all, unflagged);
  const kept = [...out].filter((name) => all.get(name)?.defines.some((symbol) => isCInterface(symbol.name)));
  if (kept.length > 0) throw new Error(`the ${target} library's C interface is in members the prelink leaves out: ${kept.join(", ")}`);

  const exports = join(dir, "exports");
  writeFileSync(exports, "_uniffi_cg3_swift*\n_ffi_cg3_swift*\n");
  const inputs = [...all.keys()].filter((name) => !out.has(name)).map((name) => join(objects, name));
  const global = join(dir, "global.o");
  run("xcrun", ["ld", "-r", "-arch", arch(target), "-exported_symbols_list", exports, "-o", global, ...inputs]);
  // The C interface is then made private to the image that links it: a
  // dynamic framework exports every global it holds, and its exports are
  // roots the linker keeps, so CG3Compile's functions, and the compiler
  // behind them, would stay in a framework that uses only CG3.
  const object = join(dir, "prelinked.o");
  const hide = ["ld", "-r", "-arch", arch(target), "-keep_private_externs", "-unexported_symbols_list", exports];
  run("xcrun", [...hide, "-o", object, global]);
  if (!output("otool", ["-hv", object]).includes("SUBSECTIONS_VIA_SYMBOLS")) {
    throw new Error(`the ${target} library's object lost MH_SUBSECTIONS_VIA_SYMBOLS, so nothing in it can be dead-stripped`);
  }
  const prelinked = join(dir, LIBRARY.file);
  run("xcrun", ["ar", "rcs", prelinked, object, ...[...out].map((name) => join(objects, name))]);

  const result = members(output("nm", ["-m", "--no-llvm-bc", prelinked]));
  const visible = [...result.values()].flatMap((member) => member.defines).filter((symbol) => symbol.external);
  const prelinkedSymbols = result.get("prelinked.o")?.defines ?? [];
  const stray = prelinkedSymbols.filter((symbol) => !isCInterface(symbol.name));
  if (visible.length > 0 || stray.length > 0 || prelinkedSymbols.length === 0) {
    const named = [...visible, ...stray].map((symbol) => symbol.name).slice(0, 10).join(", ");
    throw new Error(`the ${target} library's globals are not its C interface, private to the image: ${named}`);
  }
  return prelinked;
}

/** A slice's prelinked library, and the library cargo built for its first target, which the bindings are read from. */
function buildSlice(slice: Slice): { library: string; built: string } {
  const built: string[] = [];
  const prelinked: string[] = [];
  for (const target of slice.targets) {
    const library = cargoBuild(target, slice.env);
    built.push(library);
    prelinked.push(prelink(target, library));
  }
  const [first] = built;
  const [only] = prelinked;
  if (first === undefined || only === undefined) throw new Error(`the ${slice.name} slice has no target`);
  if (prelinked.length === 1) return { library: only, built: first };
  const fat = join(work, "slices", slice.name, LIBRARY.file);
  mkdirSync(dirname(fat), { recursive: true });
  run("xcrun", ["lipo", "-create", ...prelinked, "-output", fat]);
  return { library: fat, built: first };
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
 * Generates the bindings of `library`, as cargo built it, before the
 * prelink makes the metadata uniffi-bindgen-swift reads local: each of
 * `modules`' Swift into `sources(module)`, and its C header with a module
 * map into a directory of the Clang module's name, under the directory it
 * returns, `headers`. `workspace` is the cargo workspace the library's
 * crates are in.
 */
function bindgen(
  library: string,
  modules: string[],
  sources: (module: string) => string,
  headers: string,
  workspace = root,
): string {
  const generated = join(work, "generated");
  rmSync(generated, { recursive: true, force: true });
  const tool = bindgenTool();
  // It finds each crate's uniffi.toml through `cargo metadata` of `workspace`.
  for (const kind of ["--swift-sources", "--headers"]) run(tool, [kind, library, generated], { cwd: workspace });

  rmSync(headers, { recursive: true, force: true });
  for (const module of modules) {
    const dir = sources(module);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    copyFileSync(join(generated, `${module}.swift`), join(dir, `${module}.swift`));
    // Each Clang module in a directory of its name, where Clang looks for
    // `import <name>`, so no other package's module map shares its path.
    const clang = `${module}FFI`;
    const at = join(headers, clang);
    mkdirSync(at, { recursive: true });
    copyFileSync(join(generated, `${clang}.h`), join(at, `${clang}.h`));
    writeFileSync(join(at, "module.modulemap"), `module ${clang} {\n    header "${clang}.h"\n    export *\n}\n`);
  }
  return headers;
}

// [spec:cg3:req:swift.linking]
/**
 * Builds the linking test's second library for each of `slices` and makes
 * swift/LinkProbeFFI.xcframework of it, with its bindings in
 * swift/Tests/LinkProbe. It is built as its own Cargo.toml says, with LTO,
 * and is not prelinked: it stands for a library that keeps std's symbols
 * global.
 */
function buildProbe(slices: Slice[]): void {
  const targetDir = join(work, "link-probe");
  const libraries = slices.map((slice) => {
    const built = slice.targets.map((target) => {
      const cargo = ["build", "--locked", "--release", "--manifest-path", PROBE.manifest, "--target", target];
      run("cargo", [...cargo, "--target-dir", targetDir], { env: slice.env });
      return join(targetDir, target, "release", PROBE.file);
    });
    if (built.length === 1) return built[0] ?? "";
    const fat = join(work, "slices", `probe-${slice.name}`, PROBE.file);
    mkdirSync(dirname(fat), { recursive: true });
    run("xcrun", ["lipo", "-create", ...built, "-output", fat]);
    return fat;
  });
  const [first] = libraries;
  if (first === undefined) throw new Error("no slice to build the probe for");
  const sources = () => join(swift, "Tests", PROBE.module);
  const headers = bindgen(first, [PROBE.module], sources, join(work, "probe-headers"), dirname(PROBE.manifest));
  xcframework(libraries, headers, join(swift, `${PROBE.module}FFI.xcframework`));
}

// [spec:cg3:req:swift.platforms]
/** Makes `output` an XCFramework of `libraries`, one per slice, each with `headers`. */
function xcframework(libraries: string[], headers: string, output: string): void {
  rmSync(output, { recursive: true, force: true });
  const args = libraries.flatMap((library) => ["-library", library, "-headers", headers]);
  run("xcodebuild", ["-create-xcframework", ...args, "-output", output]);
}

// [spec:cg3:req:swift.read-only-core]
/** The C functions a module's generated header declares, whose names are `uniffi_<prefix>…` and `ffi_<prefix>…`. */
export function declaredFunctions(header: string, prefix: string): string[] {
  const names = header.matchAll(new RegExp(`\\b((?:uniffi|ffi)_${prefix}\\w+)\\s*\\(`, "g"));
  return [...new Set([...names].map((match) => match[1] ?? ""))].sort();
}

// [spec:cg3:req:swift.read-only-core]
/** The parser's and the writers' functions among the symbols `nm` lists: those in a text section whose names hold one of `COMPILER`. */
export function compilerFunctions(nm: string): string[] {
  return nm
    .split("\n")
    .map((line) => line.trim().split(/\s+/))
    .filter((fields) => fields.length >= 3 && /^[tT]$/.test(fields[1] ?? ""))
    .map((fields) => fields[2] ?? "")
    .filter((name) => COMPILER.some((part) => name.includes(part)));
}

/**
 * Links a macOS binary for `target` that takes the address of every
 * function in `functions`, so the linker keeps them and all they reach and
 * drops the rest, and returns its path.
 */
function linkKeeping(library: string, target: string, functions: string[], name: string): string {
  const dir = join(work, "core");
  mkdirSync(dir, { recursive: true });
  const source = join(dir, `${name}.c`);
  const declarations = functions.map((f) => `extern void ${f}(void);`).join("\n");
  const kept = functions.map((f) => `    (void *)${f},`).join("\n");
  writeFileSync(
    source,
    `${declarations}\nvoid *const kept[] = {\n${kept}\n};\nint main(void) {\n    volatile int n = 0;\n    for (unsigned long i = 0; i < sizeof kept / sizeof *kept; i++) n += kept[i] != 0;\n    return n == 0;\n}\n`,
  );
  const binary = join(dir, name);
  run("xcrun", ["clang", "-arch", arch(target), "-mmacosx-version-min=11.0", "-w", source, library, "-Wl,-dead_strip", "-o", binary]);
  return binary;
}

// [spec:cg3:req:swift.read-only-core]
/**
 * Links one binary that calls every function of CG3's glue and one that also
 * calls every function of CG3Compile's, against the prelinked macOS library,
 * and fails when the first holds a function of the parser or the writers or
 * the second holds none, which would show the check sees nothing. Returns
 * each binary's size in bytes.
 */
function checkReadOnlyCore(library: string, target: string, headers: string): { runtime: number; compiler: number } {
  const [runtime, compiler] = MODULES.map(({ module, prefix }) =>
    declaredFunctions(readFileSync(join(headers, `${module}FFI`, `${module}FFI.h`), "utf8"), prefix)
  );
  if (runtime === undefined || compiler === undefined) throw new Error("the bindings declare no functions");
  const only = linkKeeping(library, target, runtime, "cg3-only");
  const both = linkKeeping(library, target, [...runtime, ...compiler], "cg3-and-compile");
  const leaked = compilerFunctions(output("nm", ["--no-llvm-bc", only]));
  if (leaked.length > 0) {
    throw new Error(`a binary that calls only CG3's glue carries the compiler: ${leaked.slice(0, 5).join(", ")}`);
  }
  if (compilerFunctions(output("nm", ["--no-llvm-bc", both])).length === 0) {
    throw new Error("a binary that calls CG3Compile's glue carries no compiler functions the check can see");
  }
  return { runtime: statSync(only).size, compiler: statSync(both).size };
}

// [spec:cg3:req:swift.package]
/** The `package-metadata.json` of `version`, as SwiftPM's publishing reads it. */
export function packageMetadata(version: string): Record<string, unknown> {
  const repository = "https://github.com/divvun/cg3-rs";
  const tree = `${repository}/blob/swift/v${version}`;
  return {
    author: { name: "UiT The Arctic University of Norway", url: repository },
    description:
      "VISL CG-3 (Constraint Grammar) for Swift on iOS and macOS: runs compiled .cg3b grammars and gives the output vislcg3 gives, and compiles and writes them as cg-comp does.",
    licenseURL: `${tree}/COPYING`,
    readmeURL: `${tree}/swift/README.md`,
    repositoryURLs: [repository, `${repository}.git`, "git@github.com:divvun/cg3-rs.git"],
  };
}

// [spec:cg3:req:swift.package]
/**
 * Assembles the package as its source archive holds it, with the release
 * manifest, the sources, the XCFramework, the README and the licence, and
 * archives it with `swift package archive-source` as `cg3-<version>.zip`
 * beside its `package-metadata.json`, in target/swift/release. The staging
 * directory is made afresh each time: the archive takes everything in it.
 * Returns the archive's path.
 */
function archive(version: string, framework: string): string {
  const release = join(work, "release");
  const staging = join(release, PACKAGE);
  rmSync(release, { recursive: true, force: true });
  mkdirSync(join(staging, "Sources"), { recursive: true });
  copyFileSync(join(swift, "Package.release.swift"), join(staging, "Package.swift"));
  for (const target of ["CG3", "CG3Compile", ...MODULES.map((m) => m.module)]) {
    cpSync(join(swift, "Sources", target), join(staging, "Sources", target), { recursive: true });
  }
  cpSync(framework, join(staging, `${FRAMEWORK}.xcframework`), { recursive: true });
  copyFileSync(join(swift, "README.md"), join(staging, "README.md"));
  copyFileSync(join(root, "COPYING"), join(staging, "COPYING"));

  const zip = join(release, `${PACKAGE}-${version}.zip`);
  run("swift", ["package", "archive-source", "--output", zip], { cwd: staging });
  const entries = output("unzip", ["-Z1", zip]).split("\n");
  if (!entries.includes(`${PACKAGE}/Package.swift`) || entries.some((entry) => entry.includes("/.build/"))) {
    throw new Error(`${zip} does not hold the package as SwiftPM's registry takes it`);
  }
  writeFileSync(join(release, "package-metadata.json"), `${JSON.stringify(packageMetadata(version), null, 2)}\n`);
  return zip;
}

/** The stripped size in bytes of what uses only CG3, and of what uses CG3Compile too. */
type Sizes = { runtime: number; compiler: number };

// [spec:cg3:req:swift.read-only-core]
/**
 * Builds from the unpacked archive, as an app would depend on it: two
 * macOS programs, one using only CG3 and one using CG3Compile too, which it
 * runs; and two iOS frameworks alike, for devices in release and, the one
 * using both, for the simulator. Fails when a framework exports the C
 * interface, which would keep all of it, the compiler too. Returns the
 * stripped size of each program and each device framework.
 */
function checkArchive(zip: string): { macos: Sizes; ios: Sizes } {
  const dir = join(work, "consumer");
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  run("unzip", ["-q", zip, "-d", join(dir, "unpacked")]);
  const app = join(dir, "app");
  const files: Record<string, string> = {
    "Package.swift": `// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "Consumer",
    platforms: [.iOS(.v13), .macOS(.v11)],
    products: [
        .library(name: "Reads", type: .dynamic, targets: ["Reads"]),
        .library(name: "Uses", type: .dynamic, targets: ["Uses"]),
    ],
    dependencies: [.package(path: "../unpacked/${PACKAGE}")],
    targets: [
        .executableTarget(name: "Runs", dependencies: [.product(name: "CG3", package: "${PACKAGE}")]),
        .executableTarget(name: "Compiles", dependencies: [.product(name: "CG3Compile", package: "${PACKAGE}")]),
        .target(name: "Reads", dependencies: [.product(name: "CG3", package: "${PACKAGE}")]),
        .target(name: "Uses", dependencies: [
            .product(name: "CG3", package: "${PACKAGE}"),
            .product(name: "CG3Compile", package: "${PACKAGE}"),
        ]),
    ]
)
`,
    "Sources/Compiles/main.swift": `import CG3Compile
import Foundation

let compiled = try compileGrammar("DELIMITERS = \\"<.>\\" ;\\nREMOVE (v) ;\\n")
try compiled.cg3b.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
print(try Grammar(cg3b: compiled.cg3b).apply(to: "\\"<w>\\"\\n\\t\\"w\\" n\\n\\t\\"w\\" v\\n").output, terminator: "")
`,
    "Sources/Runs/main.swift": `import CG3
import Foundation

let grammar = try Grammar(cg3b: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
print(try grammar.apply(to: "\\"<w>\\"\\n\\t\\"w\\" n\\n\\t\\"w\\" v\\n").output, terminator: "")
`,
    "Sources/Reads/Reads.swift": `import CG3
import Foundation

public func run(_ cg3b: Data, on input: String) throws -> String {
    try Grammar(cg3b: cg3b).apply(to: input).output
}
`,
    "Sources/Uses/Uses.swift": `import CG3
import CG3Compile
import Foundation

public func run(_ source: String, on input: String) throws -> String {
    try Grammar(cg3b: compileGrammar(source).cg3b).apply(to: input).output
}
`,
  };
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(app, path)), { recursive: true });
    writeFileSync(join(app, path), text);
  }
  run("swift", ["build", "-c", "release", "--product", "Runs"], { cwd: app });
  run("swift", ["build", "-c", "release", "--product", "Compiles"], { cwd: app });
  const bin = output("swift", ["build", "-c", "release", "--show-bin-path"], app).trim();
  const cg3b = join(dir, "remove.cg3b");
  const expected = '"<w>"\n\t"w" n\n';
  for (const program of ["Compiles", "Runs"]) {
    const ran = output(join(bin, program), [cg3b]);
    if (ran !== expected) throw new Error(`${program} from the archive wrote ${JSON.stringify(ran)}`);
  }
  const stripped = (path: string, name: string) => {
    const copy = join(dir, `${name}.stripped`);
    run("strip", ["-x", "-o", copy, path]);
    return statSync(copy).size;
  };
  const macos = { runtime: stripped(join(bin, "Runs"), "Runs"), compiler: stripped(join(bin, "Compiles"), "Compiles") };

  // The simulator slice is arm64 alone, and a build for every simulator
  // would also build x86_64, so the builds name arm64.
  const derived = join(dir, "derived");
  const xcode = (scheme: string, destination: string, configuration: string) =>
    run("xcodebuild", [
      "build",
      "-quiet",
      ...["-scheme", scheme, "-destination", destination, "-configuration", configuration],
      ...["-derivedDataPath", derived, "ARCHS=arm64"],
    ], { cwd: app });
  xcode("Uses", "generic/platform=iOS Simulator", "Debug");
  const framework = (name: string) => {
    xcode(name, "generic/platform=iOS", "Release");
    const path = join(derived, "Build", "Products", "Release-iphoneos", "PackageFrameworks", `${name}.framework`, name);
    const exported = output("nm", ["-gU", "--no-llvm-bc", path]).split("\n").filter((line) => /_cg3_swift/.test(line));
    if (exported.length > 0) throw new Error(`the ${name} framework exports the C interface: ${exported[0]}`);
    return stripped(path, `${name}.framework`);
  };
  return { macos, ios: { runtime: framework("Reads"), compiler: framework("Uses") } };
}

const megabytes = (bytes: number) => `${(bytes / 1_000_000).toFixed(2)} MB`;

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const release = process.argv.includes("--release");
  const host = process.arch === "x64" ? "x86_64-apple-darwin" : "aarch64-apple-darwin";
  const macos = release ? MACOS : { ...MACOS, targets: [host] };
  const simulator = release || process.argv.includes("--simulator");
  const slices = [macos, ...(release ? [IOS] : []), ...(simulator ? [SIMULATOR] : [])];
  const built = slices.map(buildSlice);
  const [first] = built;
  if (first === undefined) throw new Error("no slice to build");
  const headers = bindgen(
    first.built,
    MODULES.map((m) => m.module),
    (module) => join(swift, "Sources", module),
    join(work, "headers"),
  );
  const framework = join(swift, `${FRAMEWORK}.xcframework`);
  xcframework(
    built.map((slice) => slice.library),
    headers,
    framework,
  );
  buildProbe(slices);
  const core = checkReadOnlyCore(join(work, "prelink", host, LIBRARY.file), host, headers);
  console.log(
    `read-only core: a binary calling all of CG3's glue is ${megabytes(core.runtime)}, and one calling CG3Compile's too ${megabytes(core.compiler)}`,
  );
  if (release) {
    const zip = archive(packageVersion(), framework);
    const sizes = checkArchive(zip);
    console.log(`${zip}: ${megabytes(statSync(zip).size)}`);
    for (const [what, built] of [["a macOS program", sizes.macos], ["an iOS framework", sizes.ios]] as const) {
      console.log(
        `from the archive, stripped: ${what} using CG3 is ${megabytes(built.runtime)}, and using CG3Compile ${megabytes(built.compiler)}`,
      );
    }
  }
  // [spec:cg3:req:swift.parity]
  // [spec:cg3:req:swift.compile-parity]
  // The fixtures @divvun/cg3's tests read, which a simulator cannot write:
  // it cannot start vislcg3 or the other tools.
  if (process.argv.includes("--test")) {
    buildFixtures(buildTools(root, run), join(root, "test"), join(work, "fixtures"));
  }
}
