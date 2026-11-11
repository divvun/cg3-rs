// The fixtures of the tool parity check (js/test/tool-parity.ts): what this
// checkout's cg-comp, vislcg3 --grammar-out, cg-conv, cg-mwesplit, cg-proc and
// cg-relabel write for the grammars and inputs the repository's tests give
// them. A browser cannot start the tools, so they run here, at test-build
// time, and tools.json carries what they wrote to every runtime.
//
// The inputs are those of the tests that drive the tools:
// crates/cg3-cli/tests/embed_parity.rs (each tool against the library, over
// its option matrix and the corpus), apertium.rs (cg-proc over every Apertium
// test), jsonl_corpus.rs (cg-conv over T_Jsonl), and the cg-conv and cg-proc
// cases of applicators.rs and tools_cli.rs. A tool reads each on its standard
// input, as those tests feed it.

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, normalize, relative } from "node:path";

const camel = (long) => long.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase());

/**
 * A command line read as `cg3::arg_parser::parse_args` reads it, against
 * `table`: `long` to `[short, takes]`, where `takes` is "no", "required" or
 * "optional". Returns each option given, by its long name, with its value or
 * true, or null when the parser refuses the line.
 */
function parseArgs(args, table) {
  const byShort = new Map(Object.entries(table).map(([long, [short]]) => [short, long]));
  const given = new Map();
  const isOption = (arg) => arg !== undefined && arg.startsWith("-") && arg.length > 1;
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (!isOption(arg)) continue;
    if (arg.startsWith("--")) {
      const long = arg.slice(2);
      const known = table[long];
      if (known === undefined) return null;
      let value = true;
      if (known[1] !== "no" && i + 1 < args.length && !isOption(args[i + 1])) value = args[(i += 1)];
      else if (known[1] === "required") return null;
      given.set(long, value);
      continue;
    }
    for (let at = 1; at < arg.length; at += 1) {
      const long = byShort.get(arg[at]);
      if (long === undefined) return null;
      const takes = table[long][1];
      let value = true;
      if (takes !== "no") {
        if (at + 1 < arg.length) value = arg.slice(at + 1);
        else if (i + 1 < args.length && !isOption(args[i + 1])) value = args[(i += 1)];
        else if (takes === "required") return null;
        given.set(long, value);
        break;
      }
      given.set(long, value);
    }
  }
  return given;
}

// cg-conv's option table, from crates/cg3-cli/src/options_conv.rs.
const CONV = {
  prefix: ["p", "required"],
  "in-auto": ["u", "no"],
  "in-cg": ["c", "no"],
  v: ["v", "no"],
  "in-niceline": ["n", "no"],
  "in-apertium": ["a", "no"],
  "in-fst": ["f", "no"],
  "in-plain": ["x", "no"],
  "in-jsonl": ["j", "no"],
  "in-binary": ["z", "no"],
  "add-tags": ["", "no"],
  "out-cg": ["C", "no"],
  V: ["V", "no"],
  "out-apertium": ["A", "no"],
  "out-fst": ["F", "no"],
  "out-niceline": ["N", "no"],
  "out-plain": ["X", "no"],
  "out-jsonl": ["J", "no"],
  "out-binary": ["Z", "no"],
  wfactor: ["W", "required"],
  wtag: ["", "required"],
  "sub-delim": ["S", "required"],
  rtl: ["r", "no"],
  ltr: ["l", "no"],
  ordered: ["o", "no"],
  "parse-dep": ["D", "no"],
  "dep-delimit": ["", "optional"],
  "unicode-tags": ["", "no"],
  deleted: ["", "no"],
  "no-break": ["B", "no"],
};

/**
 * The ConvertOptions a cg-conv command line names, or why it names none. Of
 * several `--in-*` or `--out-*` options the first cg-conv checks wins, as in
 * crates/cg3-cli/src/cg_conv.rs; `-v` is `--in-cg`, and `-V` (`--out-cg`) is
 * the default.
 */
export function convertOptions(args) {
  const given = parseArgs(args, CONV);
  if (given === null) return { exempt: `a command line cg-conv refuses (${args.join(" ")})` };
  const first = (formats) => formats.find((format) => given.has(format));
  const input = first(["v", "in-cg", "in-niceline", "in-apertium", "in-fst", "in-plain", "in-jsonl", "in-binary"]);
  const output = first(["out-apertium", "out-fst", "out-niceline", "out-plain", "out-jsonl", "out-binary"]);
  if ([input, output].some((format) => format?.endsWith("binary"))) {
    return { exempt: "a binary stream, which a string cannot carry" };
  }
  const options = {};
  if (input !== undefined) options.input = input === "v" ? "cg" : input.slice("in-".length);
  if (output !== undefined) options.output = output.slice("out-".length);
  for (const [long, value] of given) {
    if (long === "in-auto") options.inAuto = true;
    else if (long === "v" || long === "V" || long.startsWith("in-") || long.startsWith("out-")) continue;
    else if (long === "wfactor") options.wfactor = Number(value);
    else if (long === "dep-delimit") options.depDelimit = value === true ? true : Number(value);
    else options[camel(long)] = value;
  }
  return { options };
}

// cg-proc's getopt_long table, from crates/cg3-cli/src/cg_proc.rs: optstring
// "ds:f:tr:n1wvhz", whose long --sections and --rule take no argument.
const PROC_SHORT = { d: false, s: true, f: true, t: false, r: true, n: false, 1: false, w: false, v: false, h: false, z: false };
const PROC_LONG = {
  disambiguation: ["d", false],
  sections: ["s", false],
  "stream-format": ["f", true],
  rule: ["r", false],
  trace: ["t", false],
  "wordform-case": ["w", false],
  "no-word-forms": ["n", false],
  generation: ["g", false],
  version: ["v", false],
  first: ["1", false],
  help: ["h", false],
  "null-flush": ["z", false],
};

/** cg-proc reads -f and -s with C's atoi: the whole number a value starts with, and 0 for any other. */
const atoi = (value) => Number.parseInt(String(value ?? "").trim(), 10) || 0;

/** The ProcOptions a cg-proc command line names, or why it names none. */
export function procOptions(args) {
  const events = [];
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg.startsWith("--")) {
      const [name, inline] = arg.slice(2).split(/=(.*)/s, 2);
      const known = PROC_LONG[name];
      if (known === undefined) return { exempt: `a command line cg-proc refuses (${args.join(" ")})` };
      const [letter, takes] = known;
      events.push([letter, takes ? (inline ?? args[(i += 1)]) : undefined]);
      continue;
    }
    for (let at = 1; at < arg.length; at += 1) {
      const takes = PROC_SHORT[arg[at]];
      if (takes === undefined) return { exempt: `a command line cg-proc refuses (${args.join(" ")})` };
      if (!takes) {
        events.push([arg[at], undefined]);
        continue;
      }
      events.push([arg[at], at + 1 < arg.length ? arg.slice(at + 1) : args[(i += 1)]]);
      break;
    }
  }
  const options = {};
  for (const [letter, value] of events) {
    switch (letter) {
      case "d":
        options.disambiguation = true;
        break;
      case "f": {
        const format = atoi(value);
        if (format === 3) return { exempt: "a binary stream, which a string cannot carry" };
        options.streamFormat = format === 0 ? "cg" : format === 2 ? "matxin" : "apertium";
        break;
      }
      case "s":
        options.sections = atoi(value);
        break;
      case "r":
        if (value !== undefined) options.rule = value;
        break;
      case "t":
        options.trace = true;
        break;
      case "n":
        options.noWordForms = true;
        break;
      case "g":
        options.generation = true;
        break;
      case "1":
        options.first = true;
        break;
      case "w":
        options.wordformCase = true;
        break;
      case "z":
        options.nullFlush = true;
        break;
      default:
        return { exempt: `an option that does not run a grammar (-${letter})` };
    }
  }
  return { options };
}

// The cases of crates/cg3-cli/tests/embed_parity.rs, which hold each tool to
// the library function the package binds.
const CONV_MATRIX = [
  [],
  ["--in-cg", "--out-apertium"],
  ["--out-fst", "-W", "2", "--wtag", "X", "-S", "#"],
  ["--out-niceline", "--deleted"],
  ["--out-plain", "--no-break"],
  ["--out-jsonl", "--unicode-tags"],
  ["--out-binary"],
  ["--add-tags"],
  ["--add-tags", "--in-auto"],
  ["--in-plain", "--in-jsonl"],
  ["--out-apertium", "--ordered", "--ltr", "--prefix", "%"],
  ["--parse-dep"],
  ["--dep-delimit", "3"],
  ["--in-apertium"],
];
const CONV_INPUTS = [
  "T_Jsonl/input.txt",
  "T_Jsonl/input-rules.jsonl",
  "T_Dependency/input.txt",
  "Apertium/T_MultiWords/input.txt",
];
const PROC_APERTIUM = [[], ["-t", "-w"], ["-n", "-1"], ["--generation"], ["-s", "1", "-r", "r1"], ["-f", "2"]];
const PROC_CG = [
  ["-f", "0"],
  ["-f", "0", "-t", "-s", "2"],
];
const RELABELS = ["T_RelabelList", "T_RelabelSet", "T_RelabelList_Apertium"];

/** Every cg-conv case: the command line and the input, a corpus path or inline text. */
function convCases() {
  const cases = CONV_INPUTS.flatMap((input) => CONV_MATRIX.map((args) => ({ args, input })));
  // jsonl_corpus.rs
  const cgToJsonl = "T_Jsonl/expected.cg-to-jsonl.txt";
  cases.push(
    { args: ["-c", "-D", "--deleted", "-J"], input: "T_Jsonl/input.txt" },
    { args: ["-j", "-D", "--deleted", "-C"], input: cgToJsonl },
    { args: ["-j", "-D", "--deleted", "-J"], input: cgToJsonl },
    { args: ["-j", "-D", "-C"], input: cgToJsonl },
    { args: ["-j", "-J"], input: "T_Jsonl/input-errors.jsonl" },
  );
  // applicators.rs: the format sniff and its explicit formats.
  for (const args of [[], ["--in-cg"], ["--in-cg", "--out-cg"]]) cases.push({ args, input: "T_InputCommands/input.txt" });
  const niceline = { text: 'word\t"word" notwanted @1\n' };
  cases.push({ args: [], input: niceline }, { args: ["--in-niceline"], input: niceline });
  // tools_cli.rs: the grammar flags cg-conv sets.
  const subreadings = { text: "^w/a<n>+b<v>$\n" };
  cases.push(
    { args: ["-o"], input: { text: '"<a>"\n\t"a" N\n' } },
    { args: ["-a", "-p", "%"], input: { text: "^w/a<n><%X>$\n" } },
    { args: ["-a"], input: subreadings },
    { args: ["-a", "-l"], input: subreadings },
  );
  return cases;
}

/** Every cg-proc case: the grammar's directory, the command line and the input. */
function procCases(corpus) {
  const cases = [];
  for (const dir of ["Apertium/T_Select", "Apertium/T_MultiWords", "Apertium/T_Sections"]) {
    for (const args of PROC_APERTIUM) cases.push({ dir, args, input: `${dir}/input.txt` });
  }
  for (const dir of ["T_Sections", "T_Dependency"]) {
    for (const args of PROC_CG) cases.push({ dir, args, input: `${dir}/input.txt` });
  }
  // apertium.rs, which skips T_Generate: cg-proc refuses its run.pl's -g.
  for (const name of readdirSync(join(corpus, "Apertium")).filter((n) => n.startsWith("T_")).sort()) {
    const dir = `Apertium/${name}`;
    if (name === "T_Generate" || !existsSync(join(corpus, dir, "input.txt"))) continue;
    cases.push({ dir, args: name === "T_Flush" ? ["-z", "-d"] : ["-d"], input: `${dir}/input.txt` });
  }
  // applicators.rs
  const subreadings = "T_SubReadings_Apertium";
  cases.push({ dir: subreadings, args: [], input: `${subreadings}/input.txt` });
  cases.push({
    dir: subreadings,
    args: ["-f", "2"],
    input: { text: "^word/word<n><sg>$ ^dog/dog<n><sg>/dog<vblex><pres>$.\n" },
  });
  return cases;
}

/** The corpus grammars, by directory name: every test/T_* with a grammar.cg3. */
function corpusGrammars(corpus) {
  return readdirSync(corpus)
    .filter((name) => name.startsWith("T_") && existsSync(join(corpus, name, "grammar.cg3")))
    .sort();
}

/** The files a grammar's INCLUDEs name, transitively, relative to its directory. */
function includedFiles(dir, file = "grammar.cg3", found = new Set()) {
  const text = readFileSync(join(dir, file), "utf8");
  for (const [, name] of text.matchAll(/^\s*INCLUDE\s+(?:STATIC\s+)?([^\s;]+)\s*;/gim)) {
    const path = normalize(join(dirname(file), name));
    if (found.has(path) || !existsSync(join(dir, path))) continue;
    found.add(path);
    includedFiles(dir, path, found);
  }
  return found;
}

/** Whether `bytes` are UTF-8, which a JavaScript string carries unchanged. */
function isUtf8(bytes) {
  try {
    new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return true;
  } catch {
    return false;
  }
}

/**
 * Writes the tool parity fixtures for the corpus in `corpus` to `out`, with
 * the tools in `bin`, and tools.json listing them. Returns how many cases it
 * wrote.
 */
export function buildToolFixtures(bin, corpus, out) {
  const tool = (name) => join(bin, name);
  for (const sub of ["compile", "inputs", "expected", "grammars"]) mkdirSync(join(out, sub), { recursive: true });
  const run = (name, args, { cwd = corpus, input } = {}) => {
    const ran = spawnSync(tool(name), args, { cwd, input: input ?? "", maxBuffer: 1 << 28 });
    if (ran.error !== undefined) throw ran.error;
    return { stdout: ran.stdout, failed: ran.status !== 0 };
  };

  // [spec:cg3:req:wasm.compile-parity/test]
  const compile = [];
  for (const name of corpusGrammars(corpus)) {
    const dir = join(corpus, name);
    const cg3b = join(out, "compile", `${name}.cg3b`);
    const compiled = run("cg-comp", ["grammar.cg3", cg3b], { cwd: dir });
    const source = `compile/${name}.cg3`;
    copyFileSync(join(dir, "grammar.cg3"), join(out, source));
    const files = {};
    for (const file of includedFiles(dir)) {
      const copy = `compile/${name}/${file}`;
      mkdirSync(dirname(join(out, copy)), { recursive: true });
      copyFileSync(join(dir, file), join(out, copy));
      files[file] = copy;
    }
    const entry = { name, source, files, failed: compiled.failed };
    if (!isUtf8(readFileSync(join(dir, "grammar.cg3")))) entry.exempt = "a source that is not UTF-8, which a string cannot carry";
    if (!compiled.failed) {
      const text = join(out, "compile", `${name}.txt`);
      run("vislcg3", ["--grammar-only", "-g", cg3b, "--grammar-out", text]);
      entry.cg3b = `compile/${name}.cg3b`;
      entry.companion = existsSync(`${cg3b}.cg3src`) ? `compile/${name}.cg3b.cg3src` : null;
      entry.text = `compile/${name}.txt`;
    }
    compile.push(entry);
  }

  const inputs = new Map();
  /** The fixture path of an input, a corpus path or `{ text }`, written once. */
  const inputFile = (input) => {
    const key = typeof input === "string" ? input : `inline:${input.text}`;
    if (!inputs.has(key)) {
      const path = `inputs/${inputs.size}`;
      if (typeof input === "string") copyFileSync(join(corpus, input), join(out, path));
      else writeFileSync(join(out, path), input.text);
      inputs.set(key, path);
    }
    return inputs.get(key);
  };
  const inputBytes = (input) => readFileSync(join(out, inputFile(input)));
  const describe = (input) => (typeof input === "string" ? input : JSON.stringify(input.text));
  const tools = [];
  const expected = (stdout) => {
    const path = `expected/${tools.length}`;
    writeFileSync(join(out, path), stdout);
    return path;
  };
  const textCase = (entry, input, ran) => {
    if (!isUtf8(inputBytes(input))) return { ...entry, exempt: "an input that is not UTF-8, which a string cannot carry" };
    return { ...entry, input: inputFile(input), expected: expected(ran().stdout), failed: ran().failed };
  };

  // [spec:cg3:req:wasm.compile-parity/test]
  for (const { args, input } of convCases()) {
    const label = `cg-conv ${args.join(" ")} < ${describe(input)}`;
    const { options, exempt } = convertOptions(args);
    if (exempt !== undefined) {
      tools.push({ tool: "cg-conv", label, exempt });
      continue;
    }
    let ran;
    const once = () => (ran ??= run("cg-conv", args, { input: inputBytes(input) }));
    tools.push(textCase({ tool: "cg-conv", label, options }, input, once));
  }

  for (const name of readdirSync(corpus).sort()) {
    if (!existsSync(join(corpus, name, "input.txt"))) continue;
    const input = `${name}/input.txt`;
    let ran;
    const once = () => (ran ??= run("cg-mwesplit", [], { input: inputBytes(input) }));
    tools.push(textCase({ tool: "cg-mwesplit", label: `cg-mwesplit < ${input}` }, input, once));
  }

  const grammars = new Map();
  /** The .cg3b cg-comp writes for the grammar in the corpus directory `dir`. */
  const grammarFile = (dir) => {
    if (!grammars.has(dir)) {
      const path = `grammars/${grammars.size}.cg3b`;
      const compiled = run("cg-comp", ["grammar.cg3", join(out, path)], { cwd: join(corpus, dir) });
      if (compiled.failed) throw new Error(`cg-comp cannot compile ${dir}`);
      grammars.set(dir, path);
    }
    return grammars.get(dir);
  };

  for (const { dir, args, input } of procCases(corpus)) {
    const label = `cg-proc ${[...args, `${dir}/grammar.cg3b`].join(" ")} < ${describe(input)}`;
    const { options, exempt } = procOptions(args);
    if (exempt !== undefined) {
      tools.push({ tool: "cg-proc", label, exempt });
      continue;
    }
    const grammar = grammarFile(dir);
    let ran;
    const once = () => (ran ??= run("cg-proc", [...args, join(out, grammar)], { input: inputBytes(input) }));
    tools.push(textCase({ tool: "cg-proc", label, options, grammar }, input, once));
  }

  const everyFourth = corpusGrammars(corpus).filter((_, i) => i % 4 === 0);
  for (const relabel of RELABELS) {
    const rules = `inputs/${relabel}.cg3r`;
    copyFileSync(join(corpus, relabel, "relabel.cg3r"), join(out, rules));
    for (const dir of [...everyFourth, relabel]) {
      const grammar = grammarFile(dir);
      const written = join(out, "expected", `${tools.length}.cg3b`);
      const ran = run("cg-relabel", [join(out, grammar), join(out, rules), written]);
      const label = `cg-relabel ${dir}/grammar.cg3b ${relabel}/relabel.cg3r`;
      const entry = { tool: "cg-relabel", label, grammar, rules, failed: ran.failed };
      if (!ran.failed) entry.expected = relative(out, written);
      tools.push(entry);
    }
  }

  writeFileSync(join(out, "tools.json"), `${JSON.stringify({ compile, tools }, null, 1)}\n`);
  return compile.length + tools.length;
}
