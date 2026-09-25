//! `vislcg3` — the main CG-3 disambiguator binary (C++ `src/main.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "Disambiguator",
        version_aliases: &["-V"],
        option_env: &["CG3_DEFAULT", "CG3_OVERRIDE"],
        main: cg3::tools::vislcg3::main_run,
    });
    std::process::exit(code);
}
