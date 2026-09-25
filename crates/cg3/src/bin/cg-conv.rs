//! `cg-conv` — stream format converter (C++ `src/cg-conv.cpp`).
fn main() {
    let code = cg3::tools::run_tool(&cg3::tools::Tool {
        product: "Format Converter",
        version_aliases: &[],
        option_env: &["CG3_CONV_DEFAULT", "CG3_CONV_OVERRIDE"],
        main: cg3::tools::cg_conv::main_conv,
    });
    std::process::exit(code);
}
