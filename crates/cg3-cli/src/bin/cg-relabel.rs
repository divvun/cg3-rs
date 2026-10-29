//! `cg-relabel` — relabel tags/sets in a binary grammar (C++ `src/cg-relabel.cpp`).
fn main() {
    let code = cg3_cli::run_tool(&cg3_cli::Tool {
        product: "Relabeller",
        version_aliases: &[],
        option_env: &[],
        main: cg3_cli::cg_relabel::main_relabel,
    });
    std::process::exit(code);
}
