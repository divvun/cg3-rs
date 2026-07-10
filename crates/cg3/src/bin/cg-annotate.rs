//! `cg-annotate` — generate HTML/XML profiling reports (C++ `src/cg-annotate.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::tools::cg_annotate::main_annotate(&args));
}
