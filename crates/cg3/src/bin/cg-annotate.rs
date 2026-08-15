//! `cg-annotate` — generate HTML/XML profiling reports (C++ `src/cg-annotate.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cg3::tools::handle_divvun_version(&args, "Profiler Annotator", &[]) {
        return;
    }
    cg3::tools::init_diagnostics();
    std::process::exit(cg3::tools::cg_annotate::main_annotate(&args));
}
