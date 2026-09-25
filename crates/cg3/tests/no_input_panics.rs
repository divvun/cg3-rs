//! Hostile variants of every fixture's grammar, compiled grammar and input
//! stream, run in-process through `vislcg3`: each must be processed or refused
//! with an error, never panic. The variants are cut, spliced and salted with
//! the text the readers and the parser treat specially, from a fixed seed, so
//! a failure names a variant that reproduces.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

/// Variants per fixture and input kind.
const VARIANTS: u64 = 16;

/// Text the parser, the stream readers or the engine give meaning to.
const SALT: &[&[u8]] = &[
    b"\"<",
    b">\"",
    b"\t",
    b"\n",
    b"\n\n",
    b";",
    b"(",
    b")",
    b"[",
    b"]",
    b"{",
    b"}",
    b"\"",
    b"<",
    b">",
    b"*",
    b"-1",
    b"0**",
    b"#1->0",
    b"#4294967295->1",
    b"ID:4294967294",
    b"R:x:1",
    b"$1",
    b"$$X",
    b"VAR:",
    b"SET:",
    b"/r",
    b"/i",
    b"/v",
    b" LINK ",
    b" OR ",
    b" NOT ",
    b" BARRIER ",
    b"TEMPLATE ",
    b"\xff",
    b"\xef\xbf\xbf",
    b"^",
    b"$",
    b"/",
    b"+",
    b"<STREAMCMD:FLUSH>",
    b"<STREAMCMD:SETVAR:x=1>",
    b"<STREAMCMD:REMVAR:x>",
    b"\x00",
];

/// xorshift64*, enough to pick positions and salt reproducibly.
struct Rng(u64);

impl Rng {
    fn new(seed: &str, variant: u64) -> Self {
        let mut h = 0xcbf2_9ce4_8422_2325_u64 ^ variant.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for b in seed.bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        Rng(h | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// One to four edits to `bytes`: a cut, a salted insert, a deleted or
/// repeated span, or a changed byte.
fn mutate(bytes: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for _ in 0..=rng.below(4) {
        let at = rng.below(out.len() + 1);
        match rng.below(5) {
            0 => out.truncate(at),
            1 => {
                let salt = SALT[rng.below(SALT.len())];
                out.splice(at..at, salt.iter().copied());
            }
            2 => {
                let end = (at + rng.below(64)).min(out.len());
                out.drain(at..end);
            }
            3 => {
                let end = (at + rng.below(64)).min(out.len());
                let span = out[at..end].repeat(1 + rng.below(3));
                out.splice(at..at, span);
            }
            _ => {
                if let Some(b) = out.get_mut(at) {
                    *b = rng.next() as u8;
                }
            }
        }
    }
    out
}

/// Stream formats a variant is read and written in, so every reader and
/// printer sees hostile text.
const FORMATS: &[(&str, &str)] = &[
    ("--in-cg", "--out-cg"),
    ("--in-cg", "--out-apertium"),
    ("--in-niceline", "--out-niceline"),
    ("--in-apertium", "--out-fst"),
    ("--in-fst", "--out-jsonl"),
    ("--in-plain", "--out-plain"),
    ("--in-jsonl", "--out-binary"),
    ("--in-cg", "--out-jsonl"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn fixtures() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(repo_root().join("test"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            name.starts_with("T_")
                && name != "T_External"
                && p.join("input.txt").exists()
                && std::fs::read_to_string(p.join("grammar.cg3")).is_ok_and(|g| !asks_for_loops(&g))
        })
        .collect();
    dirs.sort();
    dirs
}

/// Whether a grammar uses `JUMP` or `REPEAT`. A variant of one can loop as
/// the grammar asks, which `robustness.terminates` leaves to the grammar, so
/// these fixtures are left out. Other rules can be varied into such a loop
/// too (an `ADDCOHORT` whose target matches the cohort it adds); the fixed
/// seeds give none here.
fn asks_for_loops(grammar: &str) -> bool {
    grammar
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word == "JUMP" || word == "REPEAT")
}

fn scratch(fixture: &str, what: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cg3-no-panic-{}-{fixture}-{what}",
        std::process::id()
    ))
}

/// Runs `vislcg3` in-process from `dir`, returning the panic message if it
/// panicked.
fn vislcg3(dir: &Path, args: &[String]) -> Option<String> {
    let mut argv = vec!["vislcg3".to_string()];
    argv.extend_from_slice(args);
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let outcome = catch_unwind(AssertUnwindSafe(|| cg3::tools::vislcg3::main_run(&argv)));
    std::env::set_current_dir(cwd).unwrap();
    outcome.err().map(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

/// Mutates one input of every fixture `VARIANTS` times and runs each variant;
/// `run` writes the variant and returns the arguments that read it.
fn sweep(
    kind: &str,
    source: impl Fn(&Path) -> Vec<u8>,
    run: impl Fn(&Path, &str, u64, &[u8]) -> Vec<String>,
) {
    let mut panics = Vec::new();
    for dir in fixtures() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let original = source(&dir);
        for variant in 0..VARIANTS {
            let bytes = mutate(&original, &mut Rng::new(&name, variant));
            let args = run(&dir, &name, variant, &bytes);
            if let Some(message) = vislcg3(&dir, &args) {
                let kept = scratch(&name, &format!("{kind}-{variant}.panic"));
                std::fs::write(&kept, &bytes).unwrap();
                panics.push(format!(
                    "{name} {kind} variant {variant} ({}): {message}",
                    kept.display()
                ));
            }
        }
    }
    assert!(
        panics.is_empty(),
        "{} panics:\n{}",
        panics.len(),
        panics.join("\n")
    );
}

fn out_null() -> Vec<String> {
    strings(&["-O", "/dev/null"])
}

// [spec:cg3:req:robustness.no-input-panics+1/test]
#[test]
fn hostile_grammar_text_never_panics() {
    sweep(
        "grammar",
        |dir| std::fs::read(dir.join("grammar.cg3")).unwrap(),
        |_, name, variant, bytes| {
            let path = scratch(name, &format!("grammar-{variant}.cg3"));
            std::fs::write(&path, bytes).unwrap();
            let mut args = strings(&["-g", path.to_str().unwrap(), "-I", "input.txt"]);
            if variant % 2 == 1 {
                args.push("--trace".into());
            }
            args.extend(out_null());
            args
        },
    );
}

// [spec:cg3:req:robustness.no-input-panics+1/test]
#[test]
fn hostile_compiled_grammar_never_panics() {
    sweep(
        "cg3b",
        |dir| {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            let bin = scratch(&name, "compiled.cg3b");
            let args = strings(&[
                "-g",
                "grammar.cg3",
                "--grammar-only",
                "--grammar-bin",
                bin.to_str().unwrap(),
            ]);
            assert_eq!(vislcg3(dir, &args), None, "{name} compiles");
            std::fs::read(&bin).unwrap_or_default()
        },
        |_, name, variant, bytes| {
            let path = scratch(name, &format!("cg3b-{variant}.cg3b"));
            std::fs::write(&path, bytes).unwrap();
            let mut args = strings(&["-g", path.to_str().unwrap(), "-I", "input.txt"]);
            args.extend(out_null());
            args
        },
    );
}

// [spec:cg3:req:robustness.no-input-panics+1/test]
#[test]
fn hostile_input_streams_never_panic() {
    sweep(
        "input",
        |dir| std::fs::read(dir.join("input.txt")).unwrap(),
        |_, name, variant, bytes| {
            let path = scratch(name, &format!("input-{variant}.txt"));
            std::fs::write(&path, bytes).unwrap();
            let (fmt_in, fmt_out) = FORMATS[variant as usize % FORMATS.len()];
            let mut args = strings(&[
                "-g",
                "grammar.cg3",
                "-I",
                path.to_str().unwrap(),
                fmt_in,
                fmt_out,
            ]);
            if variant % 3 == 2 {
                args.push("--dep-delimit".into());
            }
            args.extend(out_null());
            args
        },
    );
}
