//! `vislcg3` — the main CG-3 disambiguator binary (C++ `src/main.cpp`).
fn main() {
    let code = cg3_cli::run_tool(&cg3_cli::Tool {
        product: "Disambiguator",
        version_aliases: &["-V"],
        option_env: &["CG3_DEFAULT", "CG3_OVERRIDE"],
        main: cg3_cli::vislcg3::main_run,
    });
    std::process::exit(code);
}
