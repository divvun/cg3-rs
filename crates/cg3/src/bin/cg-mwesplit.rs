//! `cg-mwesplit` — split multi-word expressions (C++ `src/cg-mwesplit.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(cg3::tools::cg_mwesplit::main_mwesplit(&args));
}
