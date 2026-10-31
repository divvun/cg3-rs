// The parity check in a headless Chromium, on a page and in a Web Worker, for
// run.mjs. Node, Bun and Deno run parity-runtime.ts from run.mjs's scenario
// list; a browser needs the check compiled and served, which browser.mjs does.

import { join } from "node:path";
import { findBrowser, runInBrowser } from "./browser.mjs";

/**
 * Whether every runtime is required: --require-all, or
 * CG3_JS_TEST_REQUIRE_ALL=1. Then a runtime that is missing, or that
 * --runtime leaves out, fails the run; otherwise it is skipped.
 */
export function requireAll(argv, env = process.env) {
  return argv.includes("--require-all") || env.CG3_JS_TEST_REQUIRE_ALL === "1";
}

/**
 * Runs the parity check in Chromium against the package `pkg` (as run.mjs
 * locates it), with the fixtures and the compiled check under `testBuild`.
 * `unavailable(runtime, label, why)` reports a run that cannot happen.
 * Returns how many runs there were and how many failed.
 */
export async function runParityInChromium({ pkg, testBuild, unavailable }) {
  const variants = ["main-thread", "worker"];
  const label = (variant) => `${pkg.name} parity-browser.ts ${variant}`;
  const browser = findBrowser();
  if (browser === null) {
    for (const variant of variants) unavailable("chromium", label(variant), "no Chromium or Chrome is installed");
    return { runs: 0, failures: 0 };
  }
  console.log(`# chromium: ${browser}`);
  return await runInBrowser(browser, {
    packageDir: pkg.release.dir,
    compiled: join(testBuild, "browser", "test"),
    fixtures: join(testBuild, "fixtures"),
    label: (variant) => `chromium ${label(variant)}`,
    variants,
  });
}
