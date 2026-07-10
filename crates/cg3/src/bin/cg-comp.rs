//! `cg-comp` — compile a text grammar to binary `.cg3b` (C++ `src/cg-comp.cpp`).
fn main() {
    cg3::tools::init_diagnostics();
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::tools::cg_comp::main_comp(&args));
}
