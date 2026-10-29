//! `cg-merge-annotations` — merge profiler outputs (C++ `src/cg-merge-annotations.cpp`).
fn main() {
    let code = cg3_cli::run_tool(&cg3_cli::Tool {
        product: "Annotation Merger",
        version_aliases: &[],
        option_env: &[],
        main: cg3_cli::cg_merge_annotations::main_merge_annotations,
    });
    std::process::exit(code);
}
