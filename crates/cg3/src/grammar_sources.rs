//! The grammar source companion file, and the one place a runtime failure asks
//! for the grammar text it needs to quote.
//!
//! ADDED — no C++ analog. The C++ prints at the raise site with the parse
//! buffers still open, so it never has to get a grammar's source past the
//! compile step. This port raises a runtime failure long after the parse is
//! gone, and the artefact in between is a `.cg3b` that cannot grow: the format
//! is byte-compatible with the C++ at revision 13898 and nothing in it is
//! length-prefixed, so an unknown field does not degrade for a C++ reader — it
//! desynchronises every record after it (`[spec:cg3:req:diagnostics.wire-frozen]`).
//!
//! So the source travels beside the binary instead. `cg-comp` writes
//! `<output>.cg3src` next to the `.cg3b` it just wrote; a runtime failure reads
//! it back, and only then. See `[dec:cg3:grammar-source-sidecar]`.
//!
//! ## Wire layout
//! Big-endian ints via [`crate::inlines::write_be`] / [`crate::inlines::read_be`],
//! the same idiom as `binary_grammar.rs`; strings are a `u32` byte length + UTF-8
//! bytes. This file is ours alone — no other tool reads it — so it is a plain
//! sequence with a version on the front rather than anything extensible:
//!
//!   1. 4 raw magic bytes `"CG3S"`.
//!   2. `u32` format version ([`FORMAT_VERSION`]).
//!   3. `u64` length of the `.cg3b` this describes, then `u64` digest of it.
//!   4. `u32` source count, then per source: name, then text.
//!   5. `u32` rule count, then per rule: `u32` number, source index, begin, end.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{ParseSource, ParseSpan};
use crate::grammar::Grammar;
use crate::inlines::{read_be, write_be};
use crate::rule::RuleProvenance;

/// Appended to the `.cg3b`'s own path rather than replacing its extension, so
/// `nb.cg3b` and `nb.cg3b.bak` cannot land on the same companion file.
pub const SIDECAR_SUFFIX: &str = ".cg3src";

/// Magic bytes on the front of a companion file.
pub const SIDECAR_MAGIC: [u8; 4] = *b"CG3S";

/// Bumped whenever the layout above changes. A reader that does not recognise a
/// version treats the file as absent, which is the same degradation
/// `[spec:cg3:req:diagnostics.sidecar]` requires for no file at all.
pub const FORMAT_VERSION: u32 = 1;

/// Sanity ceiling on the counts read out of a companion file. `read_be`
/// swallows a short read, so a truncated file yields a plausible-looking count
/// and would otherwise have this allocate against it.
const MAX_ENTRIES: u32 = 1 << 24;

// [spec:cg3:req:diagnostics.sidecar]
/// The companion file for a `.cg3b` at `binary`.
pub fn sidecar_path(binary: &Path) -> PathBuf {
    let mut name = binary.as_os_str().to_os_string();
    name.push(SIDECAR_SUFFIX);
    PathBuf::from(name)
}

// [spec:cg3:req:diagnostics.sidecar-identity]
/// The stamp that ties a companion file to one exact `.cg3b`.
///
/// FNV-1a over the whole binary, alongside its length. Not a cryptographic
/// digest: nothing here defends against a forged companion file, only against
/// the one that actually happens — a stale file left by an earlier compile of
/// the same grammar, whose length may well match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinaryStamp {
    pub len: u64,
    pub digest: u64,
}

impl BinaryStamp {
    /// Stamp the bytes of a `.cg3b`.
    pub fn of(bytes: &[u8]) -> BinaryStamp {
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            digest ^= b as u64;
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
        BinaryStamp {
            len: bytes.len() as u64,
            digest,
        }
    }
}

// [spec:cg3:req:diagnostics.sidecar]
/// A grammar's sources and the rules written in them — what a companion file
/// holds, and what a textual load reconstructs from disk.
#[derive(Debug, Default)]
pub struct GrammarSources {
    /// Indexed by [`RuleProvenance::source`].
    pub sources: Vec<ParseSource>,
    /// `(rule number, where it was written)`, ascending by rule number.
    pub rules: Vec<(u32, RuleProvenance)>,
}

impl GrammarSources {
    /// Where rule `number` was written, as a span into [`sources`](Self::sources).
    ///
    /// `None` when the rule is not described here — a rule the companion file
    /// predates, or one whose span points outside the source it names, which a
    /// mismatched pairing would produce.
    pub fn locate(&self, number: u32) -> Option<ParseSpan> {
        let i = self.rules.binary_search_by_key(&number, |&(n, _)| n).ok()?;
        let p = self.rules[i].1;
        let source = self.sources.get(p.source as usize)?;
        let len = source.text.chars().count();
        if p.begin as usize > len || p.end as usize > len || p.begin > p.end {
            return None;
        }
        Some(ParseSpan {
            source: p.source as usize,
            range: p.begin as usize..p.end as usize,
        })
    }
}

/// Collect the provenance a textual parse stamped onto a grammar's rules.
pub fn provenance_of(grammar: &Grammar) -> Vec<(u32, RuleProvenance)> {
    let mut rules: Vec<(u32, RuleProvenance)> = (0..grammar.rule_by_number.capacity())
        .filter_map(|i| grammar.rule_by_number.try_get(i))
        .filter_map(|r| r.provenance.map(|p| (r.number, p)))
        .collect();
    rules.sort_unstable_by_key(|&(n, _)| n);
    rules
}

// [spec:cg3:req:diagnostics.sidecar]
/// Write the companion file for the `.cg3b` at `binary`, whose bytes are
/// `binary_bytes`.
///
/// Unconditional at every call site: there is no option gating it, because the
/// port tracks the C++ `Options` table and a companion file is invisible to the
/// C++ tools while a new flag would not be.
pub fn write_sidecar(
    binary: &Path,
    binary_bytes: &[u8],
    sources: &[ParseSource],
    rules: &[(u32, RuleProvenance)],
) -> std::io::Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(sidecar_path(binary))?);
    out.write_all(&SIDECAR_MAGIC)?;
    write_be(&mut out, FORMAT_VERSION);
    let stamp = BinaryStamp::of(binary_bytes);
    write_be(&mut out, stamp.len);
    write_be(&mut out, stamp.digest);

    write_be(&mut out, sources.len() as u32);
    for source in sources {
        write_str(&mut out, &source.name);
        write_str(&mut out, &source.text);
    }

    write_be(&mut out, rules.len() as u32);
    for &(number, p) in rules {
        write_be(&mut out, number);
        write_be(&mut out, p.source);
        write_be(&mut out, p.begin);
        write_be(&mut out, p.end);
    }
    out.flush()
}

// [spec:cg3:req:diagnostics.sidecar]
/// Write the companion file for a `.cg3b` a tool has just written, taking the
/// per-rule provenance off the grammar itself.
///
/// A failure here is a `warn!`, not an exit code: the grammar compiled, and the
/// companion file only decides whether a later runtime failure can quote it.
/// Refusing to produce a working `.cg3b` because a diagnostic aid could not be
/// written would be the wrong trade.
pub fn write_beside(
    binary: &Path,
    binary_bytes: &[u8],
    grammar: &Grammar,
    sources: &[ParseSource],
) {
    let rules = provenance_of(grammar);
    if let Err(e) = write_sidecar(binary, binary_bytes, sources, &rules) {
        tracing::warn!(
            "Warning: could not write grammar sources to {} ({e}); runtime errors in this grammar will not be quoted",
            sidecar_path(binary).display()
        );
    }
}

// [spec:cg3:req:diagnostics.sidecar-identity]
/// Read the companion file for the `.cg3b` at `binary`, or `None`.
///
/// `None` covers every way this can come up empty — no companion file, a
/// version this build does not read, a truncated one, and above all one whose
/// stamp does not match the binary in hand. A stale companion file would quote
/// confidently from the wrong lines of the wrong grammar and nothing downstream
/// could tell, so it is refused rather than trusted; the `debug!` is there for
/// whoever wonders why their grammar stopped being quoted.
///
/// Reads the whole `.cg3b` to check the stamp. That is the price of the check,
/// and it is charged on the failure path only —
/// `[spec:cg3:req:diagnostics.source-lazy]`.
pub fn read_sidecar(binary: &Path) -> Option<GrammarSources> {
    let path = sidecar_path(binary);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("no grammar sources at {}: {e}", path.display());
            return None;
        }
    };
    let mut input = std::io::Cursor::new(bytes);

    let mut magic = [0u8; 4];
    if input.read_exact(&mut magic).is_err() || magic != SIDECAR_MAGIC {
        tracing::debug!("{} does not begin with the CG3S magic", path.display());
        return None;
    }
    let version = read_be::<u32, _>(&mut input);
    if version != FORMAT_VERSION {
        tracing::debug!(
            "{} is format version {version}; this build reads {FORMAT_VERSION}",
            path.display()
        );
        return None;
    }

    let stamped = BinaryStamp {
        len: read_be(&mut input),
        digest: read_be(&mut input),
    };
    let actual = BinaryStamp::of(&std::fs::read(binary).ok()?);
    if stamped != actual {
        tracing::debug!(
            "{} describes a different build of {} - ignoring it",
            path.display(),
            binary.display()
        );
        return None;
    }

    let mut out = GrammarSources::default();
    let count = read_be::<u32, _>(&mut input).min(MAX_ENTRIES);
    for _ in 0..count {
        out.sources.push(ParseSource {
            name: read_str(&mut input)?,
            text: read_str(&mut input)?,
        });
    }
    let count = read_be::<u32, _>(&mut input).min(MAX_ENTRIES);
    for _ in 0..count {
        out.rules.push((
            read_be(&mut input),
            RuleProvenance {
                source: read_be(&mut input),
                begin: read_be(&mut input),
                end: read_be(&mut input),
            },
        ));
    }
    out.rules.sort_unstable_by_key(|&(n, _)| n);
    Some(out)
}

// [spec:cg3:req:diagnostics.source-lazy]
/// Materialise the sources of a loaded grammar — the ONE path a runtime failure
/// takes to get at the text it wants to quote, whichever way the grammar was
/// loaded.
///
/// Call this on the failure path and nowhere else. Both branches do real I/O:
/// a textual load re-reads the files it was parsed from, and a binary load reads
/// the companion file plus the `.cg3b` its stamp covers. That cost is the reason
/// nothing retains the text — see `[dec:cg3:grammar-source-sidecar]`.
///
/// `None` whenever the sources cannot be had: a grammar parsed from bytes with
/// no file behind them, a `.cg3b` with no companion file or a stale one, a
/// source since deleted. The caller then reports what it always reported.
///
/// A textual load's re-read is checked the only way it can be — every span is
/// bounds-tested against the text that comes back
/// ([`GrammarSources::locate`]), so a file truncated since the parse yields no
/// place rather than the wrong one.
pub fn resolve(grammar: &Grammar) -> Option<GrammarSources> {
    if let Some(binary) = grammar.binary_path.as_deref() {
        return read_sidecar(Path::new(binary));
    }
    if grammar.source_names.is_empty() {
        return None;
    }
    let mut sources = Vec::with_capacity(grammar.source_names.len());
    for name in &grammar.source_names {
        match std::fs::read_to_string(name) {
            Ok(text) => sources.push(ParseSource {
                name: name.clone(),
                text,
            }),
            Err(e) => {
                tracing::debug!("cannot re-read grammar source {name}: {e}");
                return None;
            }
        }
    }
    Some(GrammarSources {
        sources,
        rules: provenance_of(grammar),
    })
}

/// How much of the offending rule the one-line runtime summary quotes. Matches
/// the parser's own near-context width.
const RT_NEAR_CHARS: usize = 20;

// [spec:cg3:req:diagnostics.runtime-placed]
/// Upgrade a runtime failure from the label and line the applicator's
/// `error_at` could give it to the rule it happened in, quoted.
///
/// `RT RULE 3467` is the whole of what the C++ could say, because by the time a
/// stream runs the grammar text is gone — and for a `.cg3b` it may never have
/// been on this machine. It is not a location: it names no file, and the line
/// may belong to any of a dozen `INCLUDE`d ones. `rule`'s NUMBER is the stable
/// key that resolves all of that, being both the plan the grammar was built
/// against and the one thing that survives into the binary.
///
/// Returns the error untouched, with no sources, whenever it cannot be placed —
/// no rule in flight, no companion source file or a stale one, a grammar source
/// since deleted. The caller then reports exactly what it reported before,
/// which is the fallback the spec asks for rather than a guess at where the
/// failure was.
pub fn place_in_grammar(
    grammar: &Grammar,
    rule: Option<crate::arena::RuleId>,
    mut error: crate::error::ParseError,
) -> (crate::error::ParseError, Vec<ParseSource>) {
    let number = match rule {
        Some(rid) if grammar.rule_by_number[rid.0].line != 0 => {
            grammar.rule_by_number[rid.0].number
        }
        // No rule in flight: this failure belongs to the input stream, which is
        // not in the grammar and has nothing here to quote.
        _ => return (error, Vec::new()),
    };

    let Some(resolved) = resolve(grammar) else {
        return (error, Vec::new());
    };
    let Some(span) = resolved.locate(number) else {
        return (error, Vec::new());
    };

    let source = &resolved.sources[span.source];
    error.file = crate::uextras::basename(Some(&source.name)).to_string();
    // The head of the rule, so the one-line summary says something even where
    // the quoted report cannot be shown.
    error.near = source
        .text
        .chars()
        .skip(span.range.start)
        .take(span.range.len().min(RT_NEAR_CHARS))
        .take_while(|&c| c != '\n' && c != '\r')
        .collect();
    error.span = Some(span);
    (error, resolved.sources)
}

/// `u32` byte length + UTF-8 bytes. Not `write_utf8`: that carries the C++
/// format's 16-bit host-order prefix, which wraps past 65535 bytes — and a
/// grammar source is routinely larger than that.
fn write_str<W: Write>(out: &mut W, s: &str) {
    write_be(out, s.len() as u32);
    let _ = out.write_all(s.as_bytes());
}

/// Inverse of [`write_str`]. `None` on a length that runs off the end, which is
/// what a truncated file looks like from here.
fn read_str(input: &mut std::io::Cursor<Vec<u8>>) -> Option<String> {
    let len = read_be::<u32, _>(input) as usize;
    let start = input.position() as usize;
    let end = start.checked_add(len)?;
    let bytes = input.get_ref().get(start..end)?;
    let s = String::from_utf8(bytes.to_vec()).ok()?;
    input.set_position(end as u64);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> Vec<ParseSource> {
        vec![
            ParseSource {
                name: "nb.cg3".to_string(),
                text: "SECTION\nSELECT a ;\n".to_string(),
            },
            ParseSource {
                name: "lists.cg3".to_string(),
                text: "LIST a = x ;\n".to_string(),
            },
        ]
    }

    fn rules() -> Vec<(u32, RuleProvenance)> {
        vec![(
            7,
            RuleProvenance {
                source: 0,
                begin: 8,
                end: 17,
            },
        )]
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cg3-sidecar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(format!("{tag}.cg3b"))
    }

    /// What went in comes back out: the sources under their own names, and each
    /// rule's span still selecting its own text.
    // [spec:cg3:req:diagnostics.sidecar/test]
    #[test]
    fn a_sidecar_round_trips() {
        let binary = scratch("roundtrip");
        std::fs::write(&binary, b"CG3B not really, but stamped").expect("write binary");
        let blob = std::fs::read(&binary).expect("read binary");
        write_sidecar(&binary, &blob, &sources(), &rules()).expect("write sidecar");

        let got = read_sidecar(&binary).expect("the sidecar must be accepted");
        assert_eq!(got.sources.len(), 2);
        assert_eq!(got.sources[1].name, "lists.cg3");
        assert_eq!(got.sources[1].text, "LIST a = x ;\n");

        let span = got.locate(7).expect("rule 7 is described");
        let text: Vec<char> = got.sources[span.source].text.chars().collect();
        let quoted: String = text[span.range].iter().collect();
        assert_eq!(quoted, "SELECT a ");
        assert!(got.locate(8).is_none(), "an undescribed rule has no place");

        let _ = std::fs::remove_file(sidecar_path(&binary));
        let _ = std::fs::remove_file(&binary);
    }

    /// A sidecar left behind by an earlier compile describes lines that are no
    /// longer there. It must be refused, not read: quoting the wrong grammar
    /// with full confidence is worse than quoting nothing.
    // [spec:cg3:req:diagnostics.sidecar-identity/test]
    #[test]
    fn a_stale_sidecar_is_refused() {
        let binary = scratch("stale");
        std::fs::write(&binary, b"the FIRST compile.").expect("write binary");
        let blob = std::fs::read(&binary).expect("read binary");
        write_sidecar(&binary, &blob, &sources(), &rules()).expect("write sidecar");
        assert!(read_sidecar(&binary).is_some(), "fresh, so accepted");

        // Recompiled to the same 18 bytes, sidecar untouched: the length alone
        // would not notice, which is why the stamp carries a digest as well.
        std::fs::write(&binary, b"the SECOND compile").expect("recompile");
        assert!(
            read_sidecar(&binary).is_none(),
            "a sidecar that does not describe this binary must be ignored"
        );

        let _ = std::fs::remove_file(sidecar_path(&binary));
        let _ = std::fs::remove_file(&binary);
    }

    /// No sidecar is the ordinary case for a grammar the C++ compiled, and it
    /// degrades to nothing rather than to an error.
    // [spec:cg3:req:diagnostics.sidecar/test]
    #[test]
    fn a_missing_sidecar_is_not_an_error() {
        let binary = scratch("absent");
        std::fs::write(&binary, b"compiled elsewhere").expect("write binary");
        assert!(read_sidecar(&binary).is_none());
        let _ = std::fs::remove_file(&binary);
    }
}
