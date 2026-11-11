// The tool parity check reads the tools' command lines into the package's
// options. These are the pairs crates/cg3-cli/tests/embed_parity.rs holds the
// tools to, with the library's field names in the package's camelCase.

import { assertEqual, runTests, test } from "./harness.ts";
import { convertOptions, procOptions } from "./tool-fixtures.mjs";

const read = (f, args) => JSON.stringify(f(args));

test("cg-conv's command lines read as embed_parity.rs reads them", () => {
  const pairs = [
    [[], {}],
    [["--in-cg", "--out-apertium"], { input: "cg", output: "apertium" }],
    [["--out-fst", "-W", "2", "--wtag", "X", "-S", "#"], { output: "fst", wfactor: 2, wtag: "X", subDelim: "#" }],
    [["--out-niceline", "--deleted"], { output: "niceline", deleted: true }],
    [["--out-plain", "--no-break"], { output: "plain", noBreak: true }],
    [["--out-jsonl", "--unicode-tags"], { output: "jsonl", unicodeTags: true }],
    [["--add-tags"], { addTags: true }],
    [["--add-tags", "--in-auto"], { addTags: true, inAuto: true }],
    [["--in-plain", "--in-jsonl"], { input: "plain" }],
    [["--out-apertium", "--ordered", "--ltr", "--prefix", "%"], { output: "apertium", ordered: true, ltr: true, prefix: "%" }],
    [["--parse-dep"], { parseDep: true }],
    [["--dep-delimit", "3"], { depDelimit: 3 }],
    [["--dep-delimit"], { depDelimit: true }],
    [["--in-apertium"], { input: "apertium" }],
    [["-c", "-D", "--deleted", "-J"], { input: "cg", output: "jsonl", parseDep: true, deleted: true }],
    [["-a", "-p", "%"], { input: "apertium", prefix: "%" }],
  ];
  for (const [args, options] of pairs) {
    assertEqual(read(convertOptions, args), JSON.stringify({ options }), args.join(" "));
  }
  for (const args of [["--out-binary"], ["-z"], ["-Z"]]) {
    assertEqual(convertOptions(args).exempt, "a binary stream, which a string cannot carry", args.join(" "));
  }
  assertEqual(typeof convertOptions(["--in-everything"]).exempt, "string", "an option cg-conv refuses");
});

test("cg-proc's command lines read as embed_parity.rs and apertium.rs read them", () => {
  const pairs = [
    [[], {}],
    [["-t", "-w"], { trace: true, wordformCase: true }],
    [["-n", "-1"], { noWordForms: true, first: true }],
    [["--generation"], { generation: true }],
    [["-s", "1", "-r", "r1"], { sections: 1, rule: "r1" }],
    [["-f", "2"], { streamFormat: "matxin" }],
    [["-f", "0"], { streamFormat: "cg" }],
    [["-f", "0", "-t", "-s", "2"], { streamFormat: "cg", trace: true, sections: 2 }],
    [["-f", "7"], { streamFormat: "apertium" }],
    [["-d"], { disambiguation: true }],
    [["-z", "-d"], { nullFlush: true, disambiguation: true }],
    [["-s2"], { sections: 2 }],
  ];
  for (const [args, options] of pairs) {
    assertEqual(read(procOptions, args), JSON.stringify({ options }), args.join(" "));
  }
  assertEqual(procOptions(["-f", "3"]).exempt, "a binary stream, which a string cannot carry", "-f 3");
  assertEqual(typeof procOptions(["-g"]).exempt, "string", "-g, which the getopt_long build refuses");
});

await runTests("tool-fixtures");
