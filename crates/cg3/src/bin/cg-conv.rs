//! `cg-conv` — stream format converter (C++ `src/cg-conv.cpp`).
fn main() {
    cg3::tools::init_diagnostics();
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::error::run_cli(|| {
        cg3::tools::cg_conv::main_conv(&args)
    }));
}
