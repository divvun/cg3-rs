#!/bin/sh
# Builds the @divvun/cg3 npm package into crates/cg3-wasm/pkg: the library
# compiled to wasm32, wasm-bindgen's ES module and TypeScript declarations for
# it, the module shrunk by wasm-opt, and a package.json at the library's
# version. Needs the wasm32-unknown-unknown target, the wasm-bindgen CLI at the
# version cg3-wasm pins, and wasm-opt (binaryen).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out="$here/pkg"

pinned=$(sed -n 's/^wasm-bindgen = "=\(.*\)"$/\1/p' "$here/Cargo.toml")
installed=$(wasm-bindgen --version | cut -d' ' -f2)
if [ "$pinned" != "$installed" ]; then
    echo "wasm-bindgen CLI is $installed; cg3-wasm pins $pinned" >&2
    exit 1
fi
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml")

cargo build --manifest-path "$root/Cargo.toml" -p cg3-wasm \
    --target wasm32-unknown-unknown --profile wasm-release
rm -rf "$out"
wasm-bindgen --target web --out-dir "$out" --out-name cg3 \
    "$root/target/wasm32-unknown-unknown/wasm-release/cg3_wasm.wasm"
# The features rustc enables by default for wasm32-unknown-unknown; wasm-opt
# refuses a module that uses one it was not told about.
wasm-opt -Oz \
    --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-reference-types --enable-multivalue --enable-mutable-globals \
    "$out/cg3_bg.wasm" -o "$out/cg3_bg.wasm"

cp "$here/npm/README.md" "$out/README.md"
cp "$root/COPYING" "$out/COPYING"
cat > "$out/package.json" <<JSON
{
  "name": "@divvun/cg3",
  "version": "$version",
  "description": "VISL CG-3 (Constraint Grammar): run compiled .cg3b grammars in browsers, Deno and Node",
  "license": "GPL-3.0-or-later",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/divvun/cg3-rs.git",
    "directory": "crates/cg3-wasm"
  },
  "homepage": "https://github.com/divvun/cg3-rs",
  "keywords": ["constraint-grammar", "cg3", "vislcg3", "nlp", "linguistics", "wasm"],
  "type": "module",
  "main": "./cg3.js",
  "types": "./cg3.d.ts",
  "exports": {
    ".": {
      "types": "./cg3.d.ts",
      "default": "./cg3.js"
    },
    "./cg3_bg.wasm": "./cg3_bg.wasm"
  },
  "files": ["cg3.js", "cg3.d.ts", "cg3_bg.wasm", "cg3_bg.wasm.d.ts", "README.md", "COPYING"],
  "sideEffects": false
}
JSON
echo "built @divvun/cg3 $version in $out"
