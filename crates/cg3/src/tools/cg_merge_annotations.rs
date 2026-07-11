//! Port of `src/cg-merge-annotations.cpp` — merge several profiler databases.
//!
//! `argv`: `[prog, out_db, base_db, in_db...]`. Reads `base_db` into `out`, then
//! folds each `in_db` (argv[3..]) into it (summing match/fail/context counts and
//! filling in missing example windows), and writes the result to `out_db`.
//! LIVE flow (pure [`crate::profiler::Profiler`] I/O).

use crate::profiler::Profiler;

// [spec:cg3:def:cg-merge-annotations.main-fn]
// [spec:cg3:sem:cg-merge-annotations.main-fn]
/// C++ `int main(int argc, char* argv[])`.
pub fn main_merge_annotations(args: &[String]) -> i32 {
    // Profiler out; out.read(argv[2]);
    let mut out = Profiler::default();
    let _ = out.read(&args[2]);

    // std::map<size_t, std::string_view> out_strings; (id → string)
    let out_strings: std::collections::BTreeMap<usize, String> =
        out.strings.iter().map(|(k, &v)| (v, k.clone())).collect();

    // for (int i = 3; i < argc; ++i)
    for i in 3..args.len() {
        let mut in_ = Profiler::default();
        let _ = in_.read(&args[i]);

        let strings: std::collections::BTreeMap<usize, String> =
            in_.strings.iter().map(|(k, &v)| (v, k.clone())).collect();

        // if (out_strings[0] != strings[0]) throw ...;
        // `map::operator[]` default-inserts an empty value for a missing key; the
        // faithful analogue is "missing id 0 → empty string".
        let out0 = out_strings.get(&0).cloned().unwrap_or_default();
        let in0 = strings.get(&0).cloned().unwrap_or_default();
        if out0 != in0 {
            panic!("Cannot merge database from different grammars!");
        }

        // for (auto& it : in.rule_contexts) out.rule_contexts[it.first] += it.second;
        for (k, v) in &in_.rule_contexts {
            *out.rule_contexts.entry(*k).or_insert(0) += *v;
        }

        // for (auto& it : in.entries) { ... }
        //
        // The C++ body reads/writes `out.entries[it.first]` AND calls
        // `out.addString(...)` (which touches the SEPARATE `out.strings` map).
        // Rust's borrow checker cannot see the fields are disjoint through the
        // `entry(...)` handle, so the example-window interning is computed FIRST
        // (into `out.strings`) and the entry updated after — same observable
        // effect, same evaluation order (addString only runs when the guard held).
        let in_entries: Vec<_> = in_.entries.iter().map(|(k, e)| (*k, *e)).collect();
        for (k, ie) in in_entries {
            {
                let oe = out.entries.entry(k).or_default();
                oe.num_match += ie.num_match;
                oe.num_fail += ie.num_fail;
            }
            let need_window = out.entries[&k].example_window == 0 && ie.example_window != 0;
            if need_window {
                // auto id = out.addString(strings[ie.example_window]);
                let s = strings.get(&ie.example_window).cloned().unwrap_or_default();
                let id = out.add_string(&s);
                out.entries.get_mut(&k).unwrap().example_window = id;
            }
        }
    }

    // out.write(argv[1]);
    let _ = out.write(&args[1]);

    // C++ main falls off the end (implicit return 0).
    0
}
