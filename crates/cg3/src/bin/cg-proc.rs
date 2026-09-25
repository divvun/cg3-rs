//! `cg-proc` — stream processor (C++ `src/cg-proc.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "Disambiguator",
        version_aliases: &["-v"],
        option_env: &["CG3_DEFAULT", "CG3_OVERRIDE"],
        main: cg3::tools::cg_proc::main_proc,
    });
    std::process::exit(code);
}
