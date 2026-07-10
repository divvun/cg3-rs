# src/filesystem.hpp

> [spec:cg3:def:filesystem.path-fn]
> inline std::filesystem::path path(std::string_view sv)

> [spec:cg3:sem:filesystem.path-fn]
> Builds a `std::filesystem::path` from a `std::string_view`. Constructs
> the path from the iterator pair `[sv.begin(), sv.end())` (i.e. the
> view's bytes) and returns it by value. The iterator-pair constructor is
> used because `std::filesystem::path` has no direct `string_view`
> constructor in the targeted standards. The bytes are interpreted as the
> native narrow encoding for a path (no transcoding is performed here).
> Under `HAS_FS` this uses `<filesystem>`; otherwise it aliases
> `std::experimental::filesystem`. A Rust port simply builds a
> `PathBuf`/`Path` from the string bytes (e.g. via `OsStr`/`PathBuf::from`).

