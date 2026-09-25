//! `cg-merge-annotations` — merge profiler outputs (C++ `src/cg-merge-annotations.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "Annotation Merger",
        version_aliases: &[],
        option_env: &[],
        main: cg3::tools::cg_merge_annotations::main_merge_annotations,
    });
    std::process::exit(code);
}
