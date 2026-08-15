//! `vislcg3` — the main CG-3 disambiguator binary (C++ `src/main.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cg3::tools::handle_divvun_version(&args, "Disambiguator", &["-V"]) {
        return;
    }
    cg3::tools::init_diagnostics();
    std::process::exit(cg3::tools::vislcg3::main_run(&args));
}
