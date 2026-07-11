//! `cg-relabel` — relabel tags/sets in a binary grammar (C++ `src/cg-relabel.cpp`).
fn main() {
    cg3::tools::init_diagnostics();
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::error::run_cli(|| {
        cg3::tools::cg_relabel::main_relabel(&args)
    }));
}
