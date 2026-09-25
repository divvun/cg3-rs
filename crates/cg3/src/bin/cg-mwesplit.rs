//! `cg-mwesplit` — split multi-word expressions (C++ `src/cg-mwesplit.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "MWE Splitter",
        version_aliases: &[],
        option_env: &[],
        main: cg3::tools::cg_mwesplit::main_mwesplit,
    });
    std::process::exit(code);
}
