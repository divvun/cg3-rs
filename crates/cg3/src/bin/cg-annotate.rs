//! `cg-annotate` — generate HTML/XML profiling reports (C++ `src/cg-annotate.cpp`).
fn main() {
    cg3::tools::init_diagnostics();
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::error::run_cli(|| {
        cg3::tools::cg_annotate::main_annotate(&args)
    }));
}
