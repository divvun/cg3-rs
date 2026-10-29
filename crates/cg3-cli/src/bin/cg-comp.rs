//! `cg-comp` — compile a text grammar to binary `.cg3b` (C++ `src/cg-comp.cpp`).
fn main() {
    let code = cg3_cli::run_tool(&cg3_cli::Tool {
        product: "Compiler",
        version_aliases: &[],
        option_env: &[],
        main: cg3_cli::cg_comp::main_comp,
    });
    std::process::exit(code);
}
