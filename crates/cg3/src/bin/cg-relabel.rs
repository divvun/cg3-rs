//! `cg-relabel` — relabel tags/sets in a binary grammar (C++ `src/cg-relabel.cpp`).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if cg3::tools::handle_divvun_version(&args, "Relabeller", &[]) {
        return;
    }
    cg3::tools::init_diagnostics();
    std::process::exit(cg3::tools::cg_relabel::main_relabel(&args));
}
