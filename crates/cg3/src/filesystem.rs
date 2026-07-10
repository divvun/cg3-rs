//! Port of `src/filesystem.hpp`.
//!
//! The C++ header is a `std::filesystem` vs `std::experimental::filesystem`
//! shim plus a single helper `path()` that builds a `std::filesystem::path`
//! from a `std::string_view` (needed because `path` had no direct
//! `string_view` constructor in the targeted standards). Rust's `PathBuf`
//! subsumes the shim, so only the helper is ported.

use std::path::PathBuf;

// [spec:cg3:def:filesystem.path-fn]
// [spec:cg3:sem:filesystem.path-fn]
/// Builds a `PathBuf` from the view's bytes, no transcoding performed.
///
/// C++: `std::filesystem::path rv(sv.begin(), sv.end()); return rv;` — the
/// iterator-pair constructor over the view's bytes, interpreted as the native
/// narrow path encoding. In this UTF-8 port `sv` is a `&str` and
/// `PathBuf::from` copies its bytes as-is (byte-exact non-UTF-8 paths would
/// need `OsStrExt`, which is platform-specific and out of scope here).
pub fn path(sv: &str) -> PathBuf {
    PathBuf::from(sv)
}

#[cfg(test)]
mod tests {
    use super::*;

    // `path` builds a `PathBuf` from the view's bytes verbatim (no transcoding).
    // Drives the helper directly and checks the round-trip plus that path
    // structure (components) is understood by the OS path type.
    // [spec:cg3:sem:filesystem.path-fn/test]
    #[test]
    fn path_builds_from_view() {
        // Round-trips the exact string bytes.
        let p = path("some/dir/grammar.cg3");
        assert_eq!(p.as_os_str(), "some/dir/grammar.cg3");
        assert_eq!(p.to_str(), Some("some/dir/grammar.cg3"));

        // The resulting PathBuf behaves like a real path (components split).
        let names: Vec<&str> = p.iter().filter_map(|c| c.to_str()).collect();
        assert_eq!(names, vec!["some", "dir", "grammar.cg3"]);
        assert_eq!(p.file_name().and_then(|n| n.to_str()), Some("grammar.cg3"));

        // Empty view -> empty path.
        assert_eq!(path("").as_os_str(), "");
    }
}
