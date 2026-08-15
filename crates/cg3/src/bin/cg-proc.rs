//! `cg-proc` — stream processor (C++ `src/cg-proc.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cg3::tools::handle_divvun_version(&args, "Disambiguator", &["-v"]) {
        return;
    }
    cg3::tools::init_diagnostics();
    std::process::exit(cg3::tools::cg_proc::main_proc(&args));
}
