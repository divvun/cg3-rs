//! Port of `src/cg-merge-annotations.cpp` — merge several profiler databases.
//!
//! `argv`: `[prog, out_db, base_db, in_db...]`. Reads `base_db` into `out`, then
//! folds each `in_db` (argv[3..]) into it (summing match/fail/context counts and
//! filling in missing example windows), and writes the result to `out_db`.
//! LIVE flow (pure [`crate::profiler::Profiler`] I/O).

use std::collections::BTreeMap;

use crate::profiler::Profiler;

use super::{EXIT_FAILURE, EXIT_SUCCESS, ProfileReadError, basename, read_profile};

/// Why the databases could not be merged.
#[derive(Debug, thiserror::Error)]
enum MergeError {
    #[error(transparent)]
    Read(#[from] ProfileReadError),
    #[error(
        "Error: Cannot merge database from different grammars! {input} does not profile the grammar {base} does"
    )]
    DifferentGrammars { input: String, base: String },
    #[error("Error: the counts in {input} overflow when merged into {base}")]
    CountOverflow { input: String, base: String },
}

// [spec:cg3:def:cg-merge-annotations.main-fn+1]
// [spec:cg3:sem:cg-merge-annotations.main-fn+1]
// [spec:cg3:req:robustness.cli-arguments]
/// C++ `int main(int argc, char* argv[])`.
///
/// DIVERGENCE: the C++ ignores `argc`, ends by an uncaught exception on a
/// database it cannot read or one from another grammar, and sums counts in a
/// `size_t` that wraps. Here too few arguments, an unreadable database and one
/// from another grammar are reported, and so is a sum that overflows: no
/// profiling run records a count that large, so the database is malformed.
pub fn main_merge_annotations(args: &[String]) -> i32 {
    let [_, out_path, base_path, inputs @ ..] = args else {
        let program = args.first().map_or("cg-merge-annotations", String::as_str);
        tracing::error!(
            "USAGE: {} output_db base_db [input_db ...]",
            basename(program)
        );
        return EXIT_FAILURE;
    };
    match merge(base_path, inputs) {
        Ok(out) => {
            // out.write(argv[1]);
            let _ = out.write(out_path);
            // C++ main falls off the end (implicit return 0).
            EXIT_SUCCESS
        }
        Err(e) => {
            tracing::error!("{e}");
            EXIT_FAILURE
        }
    }
}

/// Read the database at `base` and fold every one of `inputs` into it.
fn merge(base: &str, inputs: &[String]) -> Result<Profiler, MergeError> {
    // Profiler out; out.read(argv[2]);
    let mut out = read_profile(base)?;

    // std::map<size_t, std::string_view> out_strings; (id → string)
    let out_strings: BTreeMap<usize, String> =
        out.strings.iter().map(|(k, &v)| (v, k.clone())).collect();

    // for (int i = 3; i < argc; ++i) — every input database after out/base.
    for in_path in inputs {
        let in_ = read_profile(in_path)?;

        let strings: BTreeMap<usize, String> =
            in_.strings.iter().map(|(k, &v)| (v, k.clone())).collect();

        // if (out_strings[0] != strings[0]) throw ...;
        // `map::operator[]` default-inserts an empty value for a missing key; the
        // faithful analogue is "missing id 0 → empty string".
        let out0 = out_strings.get(&0).cloned().unwrap_or_default();
        let in0 = strings.get(&0).cloned().unwrap_or_default();
        if out0 != in0 {
            return Err(MergeError::DifferentGrammars {
                input: in_path.clone(),
                base: base.to_string(),
            });
        }

        fold_profile(&mut out, &in_, &strings).ok_or_else(|| MergeError::CountOverflow {
            input: in_path.clone(),
            base: base.to_string(),
        })?;
    }
    Ok(out)
}

/// Fold `in_`'s counts and example windows into `out`, `strings` being `in_`'s
/// string table by id. `None` when a sum overflows.
fn fold_profile(
    out: &mut Profiler,
    in_: &Profiler,
    strings: &BTreeMap<usize, String>,
) -> Option<()> {
    // for (auto& it : in.rule_contexts) out.rule_contexts[it.first] += it.second;
    for (k, v) in &in_.rule_contexts {
        let sum = out.rule_contexts.entry(*k).or_insert(0);
        *sum = sum.checked_add(*v)?;
    }

    // for (auto& it : in.entries) { ... }
    //
    // The C++ body reads/writes `out.entries[it.first]` AND calls
    // `out.addString(...)` (which touches the SEPARATE `out.strings` map).
    // Rust's borrow checker cannot see the fields are disjoint through the
    // `entry(...)` handle, so the example-window interning is computed FIRST
    // (into `out.strings`) and the entry updated after — same observable
    // effect, same evaluation order (addString only runs when the guard held).
    for (k, ie) in &in_.entries {
        let need_window = {
            let oe = out.entries.entry(*k).or_default();
            oe.num_match = oe.num_match.checked_add(ie.num_match)?;
            oe.num_fail = oe.num_fail.checked_add(ie.num_fail)?;
            oe.example_window == 0 && ie.example_window != 0
        };
        if need_window {
            // auto id = out.addString(strings[ie.example_window]);
            let s = strings.get(&ie.example_window).cloned().unwrap_or_default();
            let id = out.add_string(&s);
            out.entries.entry(*k).or_default().example_window = id;
        }
    }
    Some(())
}
