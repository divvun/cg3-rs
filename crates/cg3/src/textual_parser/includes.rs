//! Where an `INCLUDE` finds the files it names.
//!
//! ADDED — no C++ analog. The C++ parser opens an included file itself; here
//! it asks an [`IncludeResolver`], so a grammar can be compiled where there are
//! no files: from named files a caller holds in memory, as a browser does.
//! [`FileResolver`] is the C++ behaviour, and the parser's default.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::uextras::dir_prefix;

// [spec:cg3:req:embed.include-resolver]
/// What reads the files a grammar's `INCLUDE`s name.
pub trait IncludeResolver {
    /// The file an `INCLUDE` in the source called `includer` names as `name`,
    /// as the grammar writes it.
    fn read(&self, name: &str, includer: &str) -> Result<Included, UnreadableInclude>;

    /// What makes the source called `name` the file it is, so that a file
    /// that includes itself is caught however the grammar spells its name.
    /// `None` when `name` is no file this resolver knows.
    fn identity(&self, name: &str) -> Option<PathBuf>;
}

/// A file an `INCLUDE` read.
#[derive(Clone, Debug)]
pub struct Included {
    /// What the parse calls the file: in its diagnostics, and in the companion
    /// file of the `.cg3b` compiled from it.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// An `INCLUDE` whose file could not be read: where it was looked for, and why
/// it is not there. The parse fails with it as vislcg3 does.
#[derive(Debug)]
pub struct UnreadableInclude {
    pub path: String,
    pub source: std::io::Error,
}

// [spec:cg3:req:embed.include-resolver]
/// Reads an `INCLUDE`d file from the filesystem as vislcg3 does: a leading `~`
/// is the home directory, and a relative name is found beside the source that
/// includes it, else in the working directory.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileResolver;

impl IncludeResolver for FileResolver {
    fn read(&self, name: &str, includer: &str) -> Result<Included, UnreadableInclude> {
        let mut expanded = name.to_string();
        if expanded.contains('~') || expanded.contains('$') || expanded.contains('*') {
            expanded = shell_expand(&expanded);
        }
        let dir = dir_prefix(includer);
        let path = if expanded.starts_with('/') {
            expanded.clone()
        } else {
            format!("{dir}{expanded}")
        };

        // PORT WIDENING, not C++ parity: the C++ stats the including file's
        // directory once and bails. The second chance against the process CWD is
        // kept because it is the only thing that reaches a relative include
        // nested under an absolute- or `~`-rooted parent. It retries the
        // SHELL-EXPANDED name, never the raw token — retrying the token would
        // re-try a literal `~/…` that cannot exist. `dir == "./"` means the two
        // spellings name one file, so there is nothing to retry.
        let cwd_retry = (dir != "./" && !expanded.starts_with('/')).then_some(expanded);

        match std::fs::read(&path) {
            Ok(bytes) => Ok(Included { name: path, bytes }),
            Err(primary) => match cwd_retry.and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((name, bytes)) => Ok(Included { name, bytes }),
                None => Err(UnreadableInclude {
                    path,
                    source: primary,
                }),
            },
        }
    }

    fn identity(&self, name: &str) -> Option<PathBuf> {
        std::fs::canonicalize(name).ok()
    }
}

/// Best-effort `wordexp` stand-in for the INCLUDE path (`~`/`$`/`*`). Only `~`
/// (home) is expanded; env-var / glob expansion is a deliberate simplification
/// (documented). The C++ uses `wordexp(WRDE_NOCMD|WRDE_UNDEF)`.
fn shell_expand(s: &str) -> String {
    let mut out = s.to_string();
    if (out == "~" || out.starts_with("~/"))
        && let Ok(home) = std::env::var("HOME")
    {
        out = out.replacen('~', &home, 1);
    }
    out
}

// [spec:cg3:req:embed.include-resolver]
/// Reads an `INCLUDE`d file from named files held in memory, for a host with
/// no filesystem. A relative name is found as [`FileResolver`] finds it on
/// disk — beside the source that includes it, else at the top of the names,
/// where the working directory would be — and names are compared as paths, so
/// `./a.cg3`, `a.cg3` and `b/../a.cg3` are one file. There is no home
/// directory, so a `~` is part of the name.
#[derive(Clone, Debug, Default)]
pub struct MemoryResolver {
    /// By name, as [`normalize`] spells it.
    files: HashMap<String, Vec<u8>>,
}

impl MemoryResolver {
    /// Adds the file called `name`, replacing one of the same name.
    pub fn insert(&mut self, name: &str, bytes: impl Into<Vec<u8>>) {
        self.files.insert(normalize(name), bytes.into());
    }
}

impl<N: AsRef<str>, B: Into<Vec<u8>>> FromIterator<(N, B)> for MemoryResolver {
    fn from_iter<I: IntoIterator<Item = (N, B)>>(files: I) -> MemoryResolver {
        let mut resolver = MemoryResolver::default();
        for (name, bytes) in files {
            resolver.insert(name.as_ref(), bytes);
        }
        resolver
    }
}

impl IncludeResolver for MemoryResolver {
    fn read(&self, name: &str, includer: &str) -> Result<Included, UnreadableInclude> {
        let path = if name.starts_with('/') {
            name.to_string()
        } else {
            format!("{}{name}", dir_prefix(includer))
        };
        let found = |path: &str| {
            self.files.get(&normalize(path)).map(|bytes| Included {
                name: path.to_string(),
                bytes: bytes.clone(),
            })
        };
        found(&path)
            .or_else(|| found(name).filter(|_| !name.starts_with('/')))
            .ok_or_else(|| UnreadableInclude {
                path,
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
    }

    fn identity(&self, name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(normalize(name)))
    }
}

/// `path` with its `.` and empty components dropped and each `..` taking the
/// component before it away, as the filesystem reads the path.
fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|&p| p != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative names are found beside the including source, then at the top
    /// of the names, and are compared as paths; what is missing fails naming
    /// where it was looked for.
    // [spec:cg3:req:embed.include-resolver/test]
    #[test]
    fn memory_resolver_finds_names_as_the_disk_does() {
        let files: MemoryResolver = [
            ("grammar.cg3", "top"),
            ("lists/sets.cg3", "sets"),
            ("rules.cg3", "rules"),
        ]
        .into_iter()
        .collect();
        let read = |name, includer| files.read(name, includer).map(|f| (f.name, f.bytes));

        assert_eq!(
            read("sets.cg3", "lists/main.cg3").unwrap(),
            ("lists/sets.cg3".to_string(), b"sets".to_vec())
        );
        assert_eq!(
            read("lists/../rules.cg3", "grammar.cg3").unwrap().1,
            b"rules"
        );
        assert_eq!(
            read("./rules.cg3", "grammar.cg3").unwrap().0,
            "././rules.cg3"
        );
        assert_eq!(
            read("rules.cg3", "lists/main.cg3").unwrap().0,
            "rules.cg3",
            "a name not beside the includer is found at the top"
        );
        let missing = read("nowhere.cg3", "lists/main.cg3").unwrap_err();
        assert_eq!(missing.path, "lists/nowhere.cg3");
        assert_eq!(missing.source.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            files.identity("./lists/../grammar.cg3"),
            files.identity("grammar.cg3")
        );
    }

    #[test]
    fn normalize_reads_paths_as_the_filesystem_does() {
        assert_eq!(normalize("./a//b/./c.cg3"), "a/b/c.cg3");
        assert_eq!(normalize("a/../../b.cg3"), "../b.cg3");
        assert_eq!(normalize("/../a/../b.cg3"), "/b.cg3");
        assert_eq!(normalize("~/x.cg3"), "~/x.cg3");
    }
}
