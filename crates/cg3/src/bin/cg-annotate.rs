//! `cg-annotate` — generate HTML/XML profiling reports (C++ `src/cg-annotate.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "Profiler Annotator",
        version_aliases: &[],
        option_env: &[],
        main: cg3::tools::cg_annotate::main_annotate,
    });
    std::process::exit(code);
}
