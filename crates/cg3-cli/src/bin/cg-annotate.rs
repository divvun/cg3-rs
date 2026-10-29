//! `cg-annotate` — generate HTML/XML profiling reports (C++ `src/cg-annotate.cpp`).
fn main() {
    let code = cg3_cli::run_tool(&cg3_cli::Tool {
        product: "Profiler Annotator",
        version_aliases: &[],
        option_env: &[],
        main: cg3_cli::cg_annotate::main_annotate,
    });
    std::process::exit(code);
}
