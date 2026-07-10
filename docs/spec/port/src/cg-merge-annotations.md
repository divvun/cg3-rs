# src/cg-merge-annotations.cpp

> [spec:cg3:def:cg-merge-annotations.main-fn]
> int main(int argc, char* argv[])

> [spec:cg3:sem:cg-merge-annotations.main-fn]
> Merges several profiling SQLite databases (produced by `vislcg3 --profile`)
> into one. Positional-only, NO option parsing and NO ICU init. Argument layout:
> `argv[1]` = output DB path, `argv[2]` = the base/primary input DB, `argv[3..]`
> = additional input DBs to fold into the base.
> Steps:
> - `Profiler out; out.read(argv[2])` — load the base profile.
> - Build `out_strings`: a `map<size_t id, string_view>` inverting `out.strings`
>   (which maps string→id) so it can be indexed by id.
> - For `i` from 3 to `argc-1`:
>   - `Profiler in; in.read(argv[i])`. Build `strings` = inverted `in.strings`
>     (id→string_view).
>   - If `out_strings[0] != strings[0]` throw
>     `std::runtime_error("Cannot merge database from different grammars!")`.
>     (String id 0 is the full grammar AST XML — `Profiler::write` remaps the
>     `grammar_ast` string's id to key 0 — so this compares the two DBs' grammar
>     ASTs. NOTE: `map::operator[]` is used, so if id 0 were absent it would
>     insert an empty string_view rather than error.)
>   - For each `(key, count)` in `in.rule_contexts`, do
>     `out.rule_contexts[key] += count` (sum context hit counts;
>     `operator[]` default-inserts 0 for new keys).
>   - For each `(key, in-entry ie)` in `in.entries`: take `oe =
>     out.entries[key]` (`operator[]` default-inserts a fresh `Entry` — type
>     ET_RULE, grammar 0, b=0, e=0 — if the key is new to the base); add
>     `oe.num_match += ie.num_match` and `oe.num_fail += ie.num_fail`; and if
>     `oe.example_window` is 0 (unset) but `ie.example_window` is set, copy the
>     input's example text into the OUT string table via
>     `out.addString(strings[ie.example_window])` and set `oe.example_window` to
>     the returned id.
> - `out.write(argv[1])` — write the merged profile to the output path.
> - No explicit return (0). EDGE (faithfulness): no argc validation — it
>   unconditionally uses `argv[2]`, so fewer than 3 args reads a null/garbage
>   path (SQLite open error). With exactly `argc==3` the merge loop never runs
>   and it simply copies the base DB to the output path.

