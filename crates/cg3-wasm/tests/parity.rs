//! The binding against vislcg3 on the host, over the golden corpus: each
//! grammar compiled to `.cg3b` by cg-comp, run by vislcg3 with its args.txt
//! and by `apply` with the same options read as JavaScript would pass them,
//! must give the same bytes or fail in both. Both tools run in this process.
//! js/test/parity.ts makes the same comparison through the built package in
//! every runtime.
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use cg3::arg_parser::parse_args;
use cg3::options::{HasArg, Opt, OptionsTable, options};
use cg3_cli::{cg_comp::main_comp, vislcg3::main_run};
use cg3_wasm::{ErrorCode, apply, parse_options, read_grammar};
use serde_json::{Map, Value};

/// The run options vislcg3 reads as numbers, which `ApplyOptions` takes as numbers too.
const NUMERIC: [&str; 4] = ["max-runs", "num-windows", "soft-limit", "hard-limit"];

/// The tests the binding cannot run, as the JavaScript check names them.
const EXEMPT: [&str; 3] = ["T_External", "T_NRules", "T_CG2Compat"];

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test")
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

fn vislcg3_options(args: &[String]) -> OptionsTable {
    let mut table = options();
    let mut argv: Vec<Vec<char>> = std::iter::once("vislcg3")
        .chain(args.iter().map(String::as_str))
        .map(|arg| arg.chars().collect())
        .collect();
    let remaining = parse_args(
        argv.len() as i32,
        &mut argv,
        Opt::NumOptions as i32,
        &mut table,
    );
    assert!(remaining >= 0, "vislcg3 refuses {args:?}");
    table
}

/// The `ApplyOptions` JSON for `args`: each long option in camelCase, the
/// `--in-*` and `--out-*` options as `input` and `output`. None for a binary
/// stream, which a string cannot carry.
fn apply_options(args: &[String]) -> Option<String> {
    let mut json = Map::new();
    for option in vislcg3_options(args).iter().filter(|o| o.does_occur) {
        let long = option.long_name.expect("every option has a long name");
        if long.ends_with("-binary") {
            return None;
        }
        if let Some(format) = long.strip_prefix("in-") {
            json.insert("input".into(), format.into());
            continue;
        }
        if let Some(format) = long.strip_prefix("out-") {
            json.insert("output".into(), format.into());
            continue;
        }
        let value = match (option.has_arg, option.value.as_str()) {
            (HasArg::No, _) | (HasArg::Optional, "") => Value::Bool(true),
            (_, number) if NUMERIC.contains(&long) => Value::from(
                number
                    .parse::<u64>()
                    .expect("a numeric option has a number"),
            ),
            (_, text) => Value::from(text),
        };
        json.insert(camel(long), value);
    }
    Some(Value::Object(json).to_string())
}

fn camel(long: &str) -> String {
    let mut parts = long.split('-');
    let first = parts.next().unwrap_or_default().to_owned();
    parts.fold(first, |mut name, part| {
        let mut chars = part.chars();
        name.extend(chars.next().map(|c| c.to_ascii_uppercase()));
        name.push_str(chars.as_str());
        name
    })
}

enum Outcome {
    Same,
    Exempt(String),
    Differs(String),
}

fn compare(dir: &Path, work: &Path) -> Outcome {
    let name = dir.file_name().expect("a test dir").to_string_lossy();
    let source = std::fs::read_to_string(dir.join("grammar.cg3")).expect("grammar.cg3");
    if source
        .lines()
        .any(|l| l.trim_start().starts_with("EXTERNAL"))
    {
        return Outcome::Exempt("an EXTERNAL rule".into());
    }
    let args: Vec<String> = std::fs::read_to_string(dir.join("args.txt"))
        .map(|text| text.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default();
    let Some(json) = apply_options(&args) else {
        return Outcome::Exempt("a binary stream".into());
    };
    let options = match parse_options(&json) {
        Ok(options) => options,
        Err(e) if e.message().contains("unknown field") => {
            return Outcome::Exempt(format!("an option the package does not offer ({args:?})"));
        }
        Err(e) => return Outcome::Differs(format!("{json} is refused: {}", e.message())),
    };

    let path = |p: &Path| p.to_string_lossy().into_owned();
    let cg3b = work.join(format!("{name}.cg3b"));
    let comp = main_comp(&strings(&[
        "cg-comp",
        &path(&dir.join("grammar.cg3")),
        &path(&cg3b),
    ]));
    assert_eq!(comp, 0, "{name}: cg-comp failed");
    let out = work.join(format!("{name}.out"));
    let mut argv = strings(&["vislcg3"]);
    argv.extend(args);
    argv.extend(strings(&[
        "-g",
        &path(&cg3b),
        "-I",
        &path(&dir.join("input.txt")),
    ]));
    argv.extend(strings(&["-O", &path(&out)]));
    let native_ok = main_run(&argv) == 0;
    let native = std::fs::read(&out).unwrap_or_default();

    let grammar = read_grammar(&std::fs::read(&cg3b).expect("the .cg3b")).expect("loads");
    let input = String::from_utf8_lossy(&std::fs::read(dir.join("input.txt")).expect("input.txt"))
        .into_owned();
    match apply(grammar.compiled(), &input, &options) {
        Err(e) if e.code() == ErrorCode::RunFailed && !native_ok => Outcome::Same,
        Err(e) => Outcome::Differs(format!("vislcg3 succeeded, apply failed: {}", e.message())),
        Ok(_) if !native_ok => Outcome::Differs("vislcg3 failed, apply succeeded".into()),
        Ok(applied) if applied.output.as_bytes() == native => Outcome::Same,
        Ok(_) => Outcome::Differs("the output differs".into()),
    }
}

// [spec:cg3:req:wasm.parity+2/test]
#[test]
fn every_corpus_grammar_gives_vislcg3s_bytes() {
    let work = std::env::temp_dir().join(format!("cg3-wasm-parity-{}", std::process::id()));
    std::fs::create_dir_all(&work).expect("a scratch dir");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(corpus())
        .expect("the golden corpus")
        .map(|entry| entry.expect("a dir entry").path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("T_"))
        })
        .filter(|p| p.join("grammar.cg3").is_file() && p.join("input.txt").is_file())
        .collect();
    dirs.sort();
    let (mut same, mut exempt, mut differs) = (0, Vec::new(), Vec::new());
    for dir in &dirs {
        let name = dir
            .file_name()
            .expect("a test dir")
            .to_string_lossy()
            .into_owned();
        match compare(dir, &work) {
            Outcome::Same => same += 1,
            Outcome::Exempt(why) => {
                println!("exempt {name}: {why}");
                exempt.push(name);
            }
            Outcome::Differs(why) => differs.push(format!("{name}: {why}")),
        }
    }
    std::fs::remove_dir_all(&work).expect("the scratch dir is removed");
    assert!(
        differs.is_empty(),
        "differ from vislcg3:\n{}",
        differs.join("\n")
    );
    exempt.sort();
    let mut expected = EXEMPT.map(str::to_owned).to_vec();
    expected.sort();
    assert_eq!(exempt, expected, "the exempt tests");
    assert!(same > 50, "only {same} tests compared");
}
