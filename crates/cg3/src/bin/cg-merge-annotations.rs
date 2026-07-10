//! `cg-merge-annotations` — merge profiler outputs (C++ `src/cg-merge-annotations.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::tools::cg_merge_annotations::main_merge_annotations(&args));
}
