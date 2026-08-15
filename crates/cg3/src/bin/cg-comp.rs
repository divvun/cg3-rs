//! `cg-comp` — compile a text grammar to binary `.cg3b` (C++ `src/cg-comp.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cg3::tools::handle_divvun_version(&args, "Compiler", &[]) {
        return;
    }
    cg3::tools::init_diagnostics();
    std::process::exit(cg3::tools::cg_comp::main_comp(&args));
}
