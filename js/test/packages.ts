// Checks the npm package must pass, run against its built package directory.

import { isPublicDeclaration, usesAny } from "./declarations.ts";
import { assert, assertEqual, readBytes, test } from "./harness.ts";

/** The fields of package.json these checks read. */
interface Manifest {
  name: string;
  version: string;
  description: string;
  license: string;
  type: string;
  files: string[];
  exports: { ".": { types: string; default: string } };
}

interface NodeFs {
  readdir(path: URL, options: { recursive: true }): Promise<string[]>;
}

async function readManifest(dir: URL): Promise<Manifest> {
  return JSON.parse(await readText(new URL("package.json", dir)));
}

async function readText(url: URL): Promise<string> {
  return new TextDecoder().decode(await readBytes(url.href));
}

async function listFiles(dir: URL): Promise<string[]> {
  const specifier = "node:fs/promises";
  const fs: NodeFs = await import(specifier);
  return await fs.readdir(dir, { recursive: true });
}

/** The entry module of the built package in `dir`. */
export async function entryOf(dir: URL): Promise<Record<string, unknown>> {
  const manifest = await readManifest(dir);
  return await import(new URL(manifest.exports["."].default, dir).href);
}

/** Registers the checks for the package in `dir`, which must be called `name`. */
export function packageTests(dir: URL, name: string): void {
  // [spec:cg3:req:wasm.module+1/test]
  test("package.json names the package, VISL CG-3 and the licence", async () => {
    const manifest = await readManifest(dir);
    assertEqual(manifest.name, name, "name");
    assert(/^\d+\.\d+\.\d+$/.test(manifest.version), `a release version: ${manifest.version}`);
    assertEqual(manifest.license, "GPL-3.0-or-later", "license");
    assertEqual(manifest.type, "module", "type");
    assert(manifest.description.includes("VISL CG-3"), "the description names VISL CG-3");
    assertEqual(manifest.files.join(" "), "dist", "files");
  });

  // [spec:cg3:req:wasm.module+1/test]
  test("the package carries the text of the GPL", async () => {
    const copying = await readText(new URL("dist/COPYING", dir));
    assert(copying.includes("GNU GENERAL PUBLIC LICENSE"), "dist/COPYING is the GPL");
    assert(copying.includes("Version 3, 29 June 2007"), "version 3 of it");
  });

  test("the package ships a README naming it", async () => {
    const readme = await readText(new URL("README.md", dir));
    assert(readme.startsWith(`# ${name}`), "README.md is the package's");
  });

  // [spec:cg3:req:wasm.module+1/test]
  // [spec:cg3:req:wasm.typed-surface/test]
  test("the entry module and its declarations are where the exports map says", async () => {
    const manifest = await readManifest(dir);
    const files = await listFiles(dir);
    const entry = manifest.exports["."];
    for (const path of [entry.default, entry.types]) {
      assert(files.includes(path.replace(/^\.\//, "")), `${path} exists`);
    }
  });

  // [spec:cg3:req:wasm.module+1/test]
  test("one .wasm file ships, beside the entry module", async () => {
    const manifest = await readManifest(dir);
    const wasm = (await listFiles(new URL("dist/", dir))).filter((file) => file.endsWith(".wasm"));
    assertEqual(wasm.length, 1, ".wasm files");
    const entryDir = manifest.exports["."].default.replace(/^\.\/dist\//, "").replace(/[^/]*$/, "");
    assert(wasm[0]?.startsWith(entryDir) === true, `${String(wasm[0])} sits under ${entryDir}`);
  });

  // [spec:cg3:req:wasm.typed-surface/test]
  test("no declaration of the public API uses any", async () => {
    const dist = new URL("dist/", dir);
    const declarations = (await listFiles(dist)).filter(isPublicDeclaration);
    assert(declarations.length > 0, "the package has declarations");
    for (const path of declarations) {
      assert(!usesAny(await readText(new URL(path, dist))), `${path} uses any`);
    }
  });
}
