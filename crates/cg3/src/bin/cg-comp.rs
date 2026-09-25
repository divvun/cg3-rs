//! `cg-comp` — compile a text grammar to binary `.cg3b` (C++ `src/cg-comp.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "Compiler",
        version_aliases: &[],
        option_env: &[],
        main: cg3::tools::cg_comp::main_comp,
    });
    std::process::exit(code);
}
