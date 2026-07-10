//! `vislcg3` — the main CG-3 disambiguator binary (C++ `src/main.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::tools::vislcg3::main_run(&args));
}
