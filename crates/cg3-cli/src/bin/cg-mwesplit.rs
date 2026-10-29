//! `cg-mwesplit` — split multi-word expressions (C++ `src/cg-mwesplit.cpp`).
fn main() {
    let code = cg3_cli::run_tool(&cg3_cli::Tool {
        product: "MWE Splitter",
        version_aliases: &[],
        option_env: &[],
        main: cg3_cli::cg_mwesplit::main_mwesplit,
    });
    std::process::exit(code);
}
