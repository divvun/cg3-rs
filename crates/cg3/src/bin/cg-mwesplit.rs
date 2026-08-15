//! `cg-mwesplit` — split multi-word expressions (C++ `src/cg-mwesplit.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cg3::tools::handle_divvun_version(&args, "MWE Splitter", &[]) {
        return;
    }
    cg3::tools::init_diagnostics();
    std::process::exit(cg3::tools::cg_mwesplit::main_mwesplit(&args));
}
