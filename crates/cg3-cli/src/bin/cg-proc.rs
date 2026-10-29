//! `cg-proc` — stream processor (C++ `src/cg-proc.cpp`).
fn main() {
    let code = cg3_cli::run_tool(&cg3_cli::Tool {
        product: "Disambiguator",
        version_aliases: &["-v"],
        option_env: &["CG3_DEFAULT", "CG3_OVERRIDE"],
        main: cg3_cli::cg_proc::main_proc,
    });
    std::process::exit(code);
}
