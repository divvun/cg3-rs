//! `cg-conv` — stream format converter (C++ `src/cg-conv.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cg3::tools::handle_divvun_version(&args, "Format Converter", &[]) {
        return;
    }
    cg3::tools::init_diagnostics();
    std::process::exit(cg3::error::run_cli(|| {
        cg3::tools::cg_conv::main_conv(&args)
    }));
}
