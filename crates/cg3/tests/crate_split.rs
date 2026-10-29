//! The library builds without anything only a process needs.

/// The crates a `[dependencies]` table of the library's manifest names,
/// including target-specific and optional ones; dev-dependencies are the
/// tests', not the library's.
fn library_dependencies() -> Vec<String> {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read the library's manifest");
    let mut in_deps = false;
    let mut names = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            let table = line.trim_matches(|c| c == '[' || c == ']');
            in_deps = table.ends_with("dependencies") && !table.ends_with("dev-dependencies");
            continue;
        }
        if in_deps && let Some((name, _)) = line.split_once('=') {
            let name = name.trim();
            if !name.is_empty() && !name.starts_with('#') {
                names.push(name.to_string());
            }
        }
    }
    names
}

// [spec:cg3:req:crate-split.no-cli-deps/test]
#[test]
fn the_library_depends_on_no_cli_crate() {
    let deps = library_dependencies();
    assert!(
        deps.iter().any(|d| d == "tracing"),
        "the scan finds the library's dependencies: {deps:?}"
    );
    for cli_only in ["cg3-cli", "sigpipe", "tracing-subscriber"] {
        assert!(
            !deps.iter().any(|d| d == cli_only),
            "the library depends on {cli_only}: {deps:?}"
        );
    }
}
