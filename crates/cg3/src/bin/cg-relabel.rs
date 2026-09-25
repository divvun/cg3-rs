//! `cg-relabel` — relabel tags/sets in a binary grammar (C++ `src/cg-relabel.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "Relabeller",
        version_aliases: &[],
        option_env: &[],
        main: cg3::tools::cg_relabel::main_relabel,
    });
    std::process::exit(code);
}
