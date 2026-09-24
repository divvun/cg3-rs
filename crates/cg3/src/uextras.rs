//! Port of `src/uextras.cpp` + `src/uextras.hpp`.
//!
//! Literal, bug-for-bug 1:1 translation of the CG-3 Unicode/stream helper
//! utilities (spec `docs/spec/port/src/uextras.md`): the flagged quirks are
//! reproduced rather than fixed.
//!
//! ## Naming
//! Control flow follows the original; the names do not. The C++ prefixes these
//! helpers and stream wrappers to mark "operates on Unicode text", a real
//! distinction in a codebase where the other half of the string functions take
//! `char*`. Every string here is UTF-8 `&str`, so the prefix marks nothing;
//! each function is named for what it does, with the C++ symbol kept in the
//! `[spec:...]` id above it. The FILE keeps the C++ name: module paths in this
//! crate map 1:1 onto the translation unit they port and key the spec ids
//! (`uextras.*`), so renaming it would cost that mapping to fix a prefix no
//! signature shows.
//!
//! ## Representation decisions (parity notes)
//!
//! * **UTF-8 / `char` model.** Text is `String` / `&str` (UTF-8) and a
//!   character is a `char` (a full Unicode scalar).
//!   The C++ code operates on UTF-16 code units. Where the algorithm
//!   scans a NUL-terminated UTF-16 buffer, the port uses `&[char]` / `&str`;
//!   the trailing NUL is represented by the slice/string length.
//!
//! * **Streams → `std::io`.** The C++ `std::istream&` / `std::ostream&`
//!   parameters become `&mut impl Read` / `&mut impl Write` generics (matching
//!   `crate::inlines`' binary-IO helpers). `strip_bom` additionally needs
//!   `Seek` because it "puts back" up to three bytes and `std::io::Read` has no
//!   `putback`; the C++ `istream::putback` calls map to `Seek::seek(Current(-n))`.
//!
//! * **No UTF-16 surrogates.** The C++ character reader caches a pending *low
//!   surrogate* per stream (`cps[4]`) so callers see non-BMP code points one
//!   UTF-16 unit at a time. A Rust `char` is a full scalar and cannot hold a
//!   lone surrogate, so this port decodes each UTF-8 sequence to a single
//!   `char` and the surrogate-cache machinery is elided. Observable divergence:
//!   a non-BMP code point occupies ONE `char` slot here vs TWO UTF-16 code
//!   units in C++. The 0xFFFF end-of-stream sentinel is preserved as
//!   `'\u{FFFF}'`.
//!
//! * **Formatted output.** Rust has no C `va_list`, and the C++ printf engine
//!   (plus its 500-unit / 1500-byte two-pass stack-buffer resize dance) has no
//!   std equivalent. The wrappers are dissolved into `write!` at each call
//!   site; the observable behavior — formatted UTF-8 written to the sink — is
//!   preserved. The narrow- vs UTF-16-format overloads collapse (all format
//!   strings are Rust/UTF-8).
//!
//! * **`throw` → `panic!`.** Every C++ `throw std::runtime_error(...)` becomes a
//!   `panic!` with the same message. The case-insensitive compare's error path
//!   (which in C++ `throw`s a *pointer*, uncatchable by
//!   `catch(const std::exception&)`) is unreachable in the std approximation
//!   and documented at the site.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::inlines::{isdelim, isnl, isspace};

// ---------------------------------------------------------------------------
// Set-operator codes.
//
// These are the `enum : uint32_t { S_IGNORE, S_OR = 3, ... }` constants from
// `Strings.hpp`. Their canonical home is `crate::strings`, but that module only
// ported the `KEYWORDS` enum so far; `crate::grammar` already carries a private
// `S_OR`/`S_MINUS` (as `u32`). They are (re)defined here as the `int` that
// `set_op_code` returns. NOTE for the lead: consolidate these into `strings.rs`
// and have `grammar.rs` + `uextras.rs` share one definition.
// ---------------------------------------------------------------------------
pub const S_IGNORE: i32 = 0;
pub const S_OR: i32 = 3;
pub const S_PLUS: i32 = 4;
pub const S_MINUS: i32 = 5;
pub const S_FAILFAST: i32 = 8;
pub const S_SET_DIFF: i32 = 9;
pub const S_SET_ISECT_U: i32 = 10;
pub const S_SET_SYMDIFF_U: i32 = 11;

/// End-of-stream sentinel (0xFFFF). U+FFFF is a noncharacter, so it never
/// appears in valid text — matching how C++ overloads it as the EOF marker.
pub const EOF_CHAR: char = '\u{FFFF}';

/// `Str::npos` (`SIZE_MAX`).
pub const NPOS: usize = usize::MAX;

// ===========================================================================
// Windows-only POSIX `basename` fallback (uextras.hpp, `#ifdef _WIN32`)
// ===========================================================================

// [spec:cg3:def:uextras.basename-fn]
// [spec:cg3:sem:uextras.basename-fn]
//
// Windows-only fallback in C++ (`#ifdef _WIN32`); ported unconditionally so it
// is type-checked on every platform. `const char* path` → `Option<&str>`
// (the C `nullptr` case is `None` → `"."`). The returned `&str` aliases into
// `path` exactly as the C++ pointer aliases into the caller's buffer.
pub fn basename(path: Option<&str>) -> &str {
    match path {
        None => ".",
        Some(path) => {
            // `std::max(strrchr(path, '\\'), strrchr(path, '/'))`: null (None)
            // is the smallest pointer, so `max` picks the found separator, or
            // the one nearer the end when both occur. `\` and `/` are single
            // ASCII bytes, so `rfind` byte offsets order the same as pointers.
            let pos = match (path.rfind('\\'), path.rfind('/')) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            match pos {
                Some(pos) => {
                    if pos + 1 < path.len() {
                        // `pos[1] != 0`: char after the separator is not the end
                        &path[pos + 1..]
                    } else {
                        // separator is the final character → point at it
                        &path[pos..]
                    }
                }
                // No separator found → return path unchanged ("probably
                // non-conformant" per the source comment).
                None => path,
            }
        }
    }
}

// ===========================================================================
// BOM stripping (uextras.hpp)
// ===========================================================================

/// Reads one byte, returning `None` on EOF (or IO error, which `istream`
/// likewise surfaces as EOF via `get()` returning `EOF`).
fn read_byte<R: Read>(stream: &mut R) -> Option<u8> {
    let mut b = [0u8; 1];
    match stream.read(&mut b) {
        Ok(0) => None,
        Ok(_) => Some(b[0]),
        Err(_) => None,
    }
}

// [spec:cg3:def:uextras.ux-strip-bom-fn]
// [spec:cg3:sem:uextras.ux-strip-bom-fn]
//
// `istream::putback` (up to 3 bytes) → `Seek::seek(SeekFrom::Current(-n))`,
// rewinding by the number of bytes consumed so the stream is left exactly as
// found on any non-BOM path. Byte comparisons are against the unsigned values
// 0xEF/0xBB/0xBF, as in the source.
pub fn strip_bom<S: Read + Seek>(stream: &mut S) -> bool {
    let a = match read_byte(stream) {
        Some(v) => v,
        None => return false, // EOF: nothing consumed
    };
    if a != 0xEF {
        let _ = stream.seek(SeekFrom::Current(-1)); // putback a
        return false;
    }

    let b = match read_byte(stream) {
        Some(v) => v,
        None => {
            let _ = stream.seek(SeekFrom::Current(-1)); // putback a
            return false;
        }
    };
    if b != 0xBB {
        let _ = stream.seek(SeekFrom::Current(-2)); // putback b, a
        return false;
    }

    let c = match read_byte(stream) {
        Some(v) => v,
        None => {
            let _ = stream.seek(SeekFrom::Current(-2)); // putback b, a
            return false;
        }
    };
    if c != 0xBF {
        let _ = stream.seek(SeekFrom::Current(-3)); // putback c, b, a
        return false;
    }

    true // all three matched: BOM consumed
}

// ===========================================================================
// std::istream input wrappers (uextras.cpp)
// ===========================================================================

// [spec:cg3:def:uextras.u-fgets-fn]
// [spec:cg3:sem:uextras.u-fgets-fn]
//
// Returns `bool` (`true` ≈ the C++'s non-null `s`, `false` ≈ `nullptr`).
// QUIRKS reproduced: (1) the terminator is written at `s[i+1]`, not `s[i]`;
// (2) a line that is just a newline stores `s[0]` then returns `false`
// (`i == 0`) — indistinguishable from EOF, so callers treat an empty line as
// "read nothing"; (3) an exactly-full buffer writes no terminator. The caller
// must provide `s.len() >= n + 1` (so the `s[i+1]` write stays in bounds), as
// `get_line_clean` does.
pub fn read_line_chars<R: Read>(s: &mut [char], n: i32, input: &mut R) -> bool {
    s[0] = '\0';
    let mut i: i32 = 0;
    while i < n {
        let c = read_char(input);
        if c == EOF_CHAR {
            break; // EOF: nothing stored at s[i]
        }
        s[i as usize] = c;
        if isnl(c) {
            break; // newline stored at s[i]
        }
        i += 1;
    }
    if i < n {
        s[(i + 1) as usize] = '\0';
    }

    if i == 0 {
        return false;
    }
    true
}

// [spec:cg3:def:uextras.u-fgetc-fn]
// [spec:cg3:sem:uextras.u-fgetc-fn]
//
// Reads one UTF-8 sequence and returns it as a single `char`. See the module
// note: the UTF-16 surrogate-pair cache (`cps[4]`) is elided because a `char`
// is a full scalar (no lone surrogates). Returns `EOF_CHAR` on end-of-stream and
// `'\0'` when the first byte read is a NUL. The lead-byte masks (0xF0/0xE0/0xC0,
// widest first) and the short-read `panic!`s mirror the source.
pub fn read_char<R: Read>(input: &mut R) -> char {
    let c = match read_byte(input) {
        Some(v) => v,
        None => return EOF_CHAR, // i == 0 && c == EOF
    };

    let mut buf = [0u8; 4];
    buf[0] = c;
    let mut i = 1usize;
    if (c & 0xF0) == 0xF0 {
        if input.read_exact(&mut buf[1..4]).is_err() {
            panic!("Could not read 3 expected bytes from stream");
        }
        i = 4;
    } else if (c & 0xE0) == 0xE0 {
        if input.read_exact(&mut buf[1..3]).is_err() {
            panic!("Could not read 2 expected bytes from stream");
        }
        i = 3;
    } else if (c & 0xC0) == 0xC0 {
        if input.read_exact(&mut buf[1..2]).is_err() {
            panic!("Could not read 1 expected byte from stream");
        }
        i = 2;
    }

    if c == 0 {
        return '\0';
    }

    match std::str::from_utf8(&buf[0..i]) {
        Ok(s) => s.chars().next().unwrap_or('\0'),
        Err(_) => panic!("Failed to convert from UTF-8 to UTF-16"),
    }
}

/// Reads up to `buf.len()` bytes, looping until the buffer is full or the stream
/// ends — matching `std::istream::read`'s "read N or until EOF" semantics (as
/// opposed to `Read::read`, which may return short). Returns the count read
/// (i.e. `input.gcount()`).
fn read_some<R: Read>(input: &mut R, buf: &mut [u8]) -> usize {
    let mut total = 0;
    while total < buf.len() {
        match input.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(k) => total += k,
            Err(_) => break,
        }
    }
    total
}

// [spec:cg3:def:uextras.read-utf8-fn]
// [spec:cg3:sem:uextras.read-utf8-fn]
//
// `std::string` (raw bytes) → `Vec<u8>`; the function is byte-oriented and does
// not validate, so `Vec<u8>` is the faithful return (NOTE for the lead: some
// callers may want `String`). `BUF_SIZE` has no default in Rust — pass `1000`
// to match the header default. The `sz == 0` and no-lower-bound backward-scan
// out-of-bounds reads are latent UB in C++; safe Rust guards them (`sz != 0`
// and an `i == 0` break) rather than reproducing the OOB.
pub fn read_utf8<R: Read>(input: &mut R, buf_size: usize) -> Vec<u8> {
    let mut buf8 = vec![0u8; buf_size];

    let mut sz = read_some(input, &mut buf8[0..buf_size - 4]);
    if sz != 0 && (buf8[sz - 1] & 0x80) != 0 {
        let mut i = sz - 1;
        loop {
            if (buf8[i] & 0xF0) == 0xF0 {
                let k = sz - 1 - i; // continuation bytes already present
                let need = 3 - k;
                if input.read_exact(&mut buf8[sz..sz + need]).is_err() {
                    panic!("Could not read expected bytes from stream");
                }
                sz += need;
                break;
            } else if (buf8[i] & 0xE0) == 0xE0 {
                let k = sz - 1 - i;
                let need = 2 - k;
                if input.read_exact(&mut buf8[sz..sz + need]).is_err() {
                    panic!("Could not read expected bytes from stream");
                }
                sz += need;
                break;
            } else if (buf8[i] & 0xC0) == 0xC0 {
                let k = sz - 1 - i;
                let need = 1 - k;
                if input.read_exact(&mut buf8[sz..sz + need]).is_err() {
                    panic!("Could not read expected bytes from stream");
                }
                sz += need;
                break;
            } else {
                // continuation byte (10xxxxxx): keep scanning backward.
                if i == 0 {
                    break; // safe lower-bound guard (C++ has none: latent UB)
                }
                i -= 1;
            }
        }
    }
    buf8.truncate(sz);

    buf8
}

// ===========================================================================
// std::ostream output wrappers (uextras.cpp)
// ===========================================================================

// [spec:cg3:def:uextras.u-fflush-fn]
// [spec:cg3:sem:uextras.u-fflush-fn]
//
// DISSOLVED: the C++ flush overload pair (`ostream&` / `ostream*`) exists so a
// `std::ostream` can be flushed through the same prefixed facade as the rest
// of the stdio wrapper family. Its whole body is `output.flush()` with the
// result discarded, which in Rust is `let _ = output.flush();` — a
// method call on the `Write` the caller already holds. Every former call site
// now writes that directly. `dissolved_printf_shims_are_plain_write` (tests
// below) pins the observable contract.

// [spec:cg3:def:uextras.u-vsnprintf-fn]
// [spec:cg3:sem:uextras.u-vsnprintf-fn]
// [spec:cg3:def:uextras.u-fprintf-fn]
// [spec:cg3:sem:uextras.u-fprintf-fn]
// [spec:cg3:def:uextras.u-fprintf-u-fn]
// [spec:cg3:sem:uextras.u-fprintf-u-fn]
//
// DISSOLVED: the C++ formatted-print overload family
// and its `vsnprintf`-style formatting core are printf-vararg C-isms with no
// Rust analog to preserve. Their observable contract — format
// the arguments and write the result to the output stream as UTF-8 bytes,
// ignoring I/O errors — is exactly `let _ = write!(out, ...)`, which is what
// every former call site now does directly. The C++-internal mechanics the sem
// rules describe (two-pass 500-unit/1500-byte stack buffers, the UTF-16
// code-unit return count) had no observable effect in the port: the buffers
// were a resize strategy and NO caller in the entire tree consumed the return
// value. `dissolved_printf_shims_are_plain_write` (tests below) pins the
// observable contract.

// [spec:cg3:def:uextras.u-fputc-fn]
// [spec:cg3:sem:uextras.u-fputc-fn]
//
// BUG/LIMITATION reproduced faithfully: the second branch cuts off at 0x7FFF,
// so every code point at or above 0x8000 `panic!`s ("can't handle >= 0x7FFF"),
// even though 0x7FFF itself is handled.
pub fn write_char<W: Write>(c32: char, output: &mut W) -> char {
    let v = c32 as u32;
    if v <= 0x7F {
        let _ = output.write_all(&[c32 as u8]);
    } else if v <= 0x7FFF {
        let mut buf = [0u8; 4];
        let s = c32.encode_utf8(&mut buf);
        let _ = output.write_all(s.as_bytes());
    } else {
        panic!("u_fputc() wrapper can't handle >= 0x7FFF");
    }

    c32
}

// ===========================================================================
// CG3 namespace utilities (uextras.cpp / uextras.hpp)
// ===========================================================================

// [spec:cg3:def:uextras.cg3.ux-dirname-fn]
// [spec:cg3:sem:uextras.cg3.ux-dirname-fn]
//
// POSIX branch only (the Windows `GetFullPathNameA` path is platform-specific
// and omitted). Returns the directory portion, guaranteed to end in a
// separator. `dirname(3)` is unavailable in std, so a POSIX-`dirname`
// reimplementation (`dirname_posix`) is used. The empty-`tmp` `tmp[tlen-1]`
// out-of-bounds read is latent UB in C++ (POSIX `dirname` never returns "");
// safe Rust simply never hits it (`dirname_posix` returns "." at minimum).
pub fn dir_prefix(input: &str) -> String {
    let mut tmp = dirname_posix(input);
    if !(tmp.ends_with('/') || tmp.ends_with('\\')) {
        tmp.push('/');
    }
    tmp
}

/// POSIX `dirname(3)` reimplementation (not in std). Mirrors
/// glibc/musl behavior: strips trailing slashes, drops the last component, and
/// returns "." when there is no directory part and "/" for the root. NOTE:
/// parity with the platform `dirname(3)` on unusual inputs is a known risk.
fn dirname_posix(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let bytes = path.as_bytes(); // '/' is ASCII, so byte scanning is char-safe
    let mut i = bytes.len() - 1;
    // strip trailing slashes
    while i > 0 && bytes[i] == b'/' {
        i -= 1;
    }
    // find the last '/' in bytes[..=i]
    let mut has = false;
    let mut j = i;
    loop {
        if bytes[j] == b'/' {
            has = true;
            break;
        }
        if j == 0 {
            break;
        }
        j -= 1;
    }
    if !has {
        return ".".to_string();
    }
    // strip trailing slashes before the component
    while j > 0 && bytes[j - 1] == b'/' {
        j -= 1;
    }
    if j == 0 {
        return "/".to_string();
    }
    path[..j].to_string()
}

// [spec:cg3:def:uextras.cg3.find-and-replace-fn]
// [spec:cg3:sem:uextras.cg3.find-and-replace-fn]
//
// The C++ mutates a UTF-16 string in place; here it is a `&mut String`.
// `offset` and the `from`/`to` sizes are byte offsets into the UTF-8 buffer
// (the direct analog of C++'s code-unit offsets). Advancing `offset` past the
// inserted `to` prevents re-scanning replacements, so a `to` containing `from`
// cannot loop.
pub fn find_and_replace(str: &mut String, from: &str, to: &str) -> usize {
    let mut rv = 0usize;
    let mut offset = 0usize;
    while let Some(idx) = str[offset..].find(from) {
        let pos = offset + idx;
        str.replace_range(pos..pos + from.len(), to);
        offset = pos + to.len();
        rv += 1;
    }
    rv
}

// [spec:cg3:def:uextras.cg3.get-line-clean-fn]
// [spec:cg3:sem:uextras.cg3.get-line-clean-fn]
//
// Wave 4 (w4-utf8-native-strings): native-`String` form. `line` receives the
// raw line exactly as read (newline INCLUDED, original spacing kept); `cleaned`
// receives the whitespace-collapsed copy (each run of non-newline whitespace
// becomes one `' '`, or the run's last `'\t'` when `keep_tabs`), stopping at
// the newline (which is NOT copied) or an embedded NUL. Returns `cleaned`'s
// byte length. A BLANK line yields `line == "\n"` with an empty `cleaned`
// (the C++ line reader's nullptr-on-lone-newline quirk); true EOF yields an empty
// `line` — callers distinguish the two exactly as the C++ did via `line[0]`.
// The C++ fixed-buffer doubling and NUL terminators are buffer management with
// no observable effect and are not reproduced.
pub fn get_line_clean<R: Read>(
    line: &mut String,
    cleaned: &mut String,
    input: &mut R,
    keep_tabs: bool,
) -> usize {
    line.clear();
    cleaned.clear();

    // As the C++ line reader: read chars (UTF-8-decoded) until a stored newline or EOF.
    loop {
        let c = read_char(input);
        if c == EOF_CHAR {
            break;
        }
        line.push(c);
        if isnl(c) {
            break;
        }
    }
    // The C++ line reader's lone-newline quirk: a blank line reports "read nothing", so
    // nothing is copied to `cleaned` (the C++ broke before the copy loop).
    if line == "\n" || (line.chars().count() == 1 && line.chars().next().map(isnl).unwrap_or(false))
    {
        return 0;
    }

    // Copy to `cleaned`, collapsing whitespace runs; stop at newline/NUL.
    let mut it = line.chars().peekable();
    while let Some(&c) = it.peek() {
        if isspace(c) && !isnl(c) {
            let mut space = if c == '\t' { '\t' } else { ' ' };
            while let Some(&d) = it.peek() {
                if !isspace(d) || isnl(d) {
                    break;
                }
                if d == '\t' {
                    space = d;
                }
                it.next();
            }
            if !keep_tabs {
                space = ' ';
            }
            cleaned.push(space);
            continue;
        }
        if isnl(c) || c == '\0' {
            break;
        }
        cleaned.push(c);
        it.next();
    }
    cleaned.len()
}

// Scalar-buffer (`Vec<char>`) form of [`get_line_clean`], retained for the
// reading/grammar LEXERS (`run_grammar_on_text`, the FST applicator, and the
// `TextualParser`) that scan their working buffer with `Char*`-style cursors and
// NUL-cut it in place, then rebuild the slices into `String` tags — the wave-4
// "symbols later rebuilt into a string" carve-out. Line-oriented stream readers
// (Niceline/Plaintext) use the native-`String` [`get_line_clean`]. Byte-for-byte
// identical to the wave-2/3 buffer semantics (fixed buffer, doubling, trailing
// NUL padding that the cursor loops rely on for out-of-range reads).
pub fn get_line_clean_chars<R: Read>(
    line: &mut Vec<char>,
    cleaned: &mut Vec<char>,
    input: &mut R,
    keep_tabs: bool,
) -> usize {
    let mut offset = 0usize;
    let mut packoff = 0usize;

    // Read as much of the next line as will fit in the current buffer
    loop {
        if offset >= line.len() {
            break;
        }
        let n = line.len() as i32 - offset as i32 - 1;
        if !read_line_chars(&mut line[offset..], n, input) {
            break;
        }

        // Copy the segment just read to cleaned
        while offset < line.len() {
            // Only copy one space character, regardless of how many are in input
            if isspace(line[offset]) && !isnl(line[offset]) {
                let mut space = if line[offset] == '\t' { '\t' } else { ' ' };
                while offset < line.len() && isspace(line[offset]) && !isnl(line[offset]) {
                    if line[offset] == '\t' {
                        space = line[offset];
                    }
                    offset += 1;
                }
                if !keep_tabs {
                    space = ' ';
                }
                cleaned[packoff] = space;
                packoff += 1;
            }
            // (safety) a run may have consumed to the buffer end; re-check
            if offset >= line.len() {
                break;
            }
            // Break if there is a newline
            if isnl(line[offset]) {
                cleaned[packoff + 1] = '\0';
                cleaned[packoff] = '\0';
                return packoff;
            }
            if line[offset] == '\0' {
                cleaned[packoff + 1] = '\0';
                cleaned[packoff] = '\0';
                break;
            }
            cleaned[packoff] = line[offset];
            packoff += 1;
            offset += 1;
        }

        // Either buffer wasn't big enough, or someone fed us malformed data
        // thinking U+0085 is ellipsis when it in fact is Next Line (NEL)
        if packoff > line.len() / 2 {
            // Buffer wasn't big enough. Double it and try again.
            let newlen = line.len() * 2;
            line.resize(newlen, '\0');
            cleaned.resize(line.len() + 1, '\0');
        }
    }

    packoff
}

// [spec:cg3:def:uextras.cg3.ux-is-set-op-fn]
// [spec:cg3:sem:uextras.cg3.ux-is-set-op-fn]
//
// The C++ `it[1] == 0` (a one-code-unit token) is "the string has exactly one
// char" (`c1 == None`). Returns the `S_*`
// code, or `S_IGNORE`.
pub fn set_op_code(it: &str) -> i32 {
    let mut chars = it.chars();
    let c0 = chars.next();
    let c1 = chars.next();
    let c2 = chars.next();

    match c1 {
        // it[1] == 0
        None => match c0 {
            Some('|') => S_OR,
            Some('+') => S_PLUS,
            Some('-') => S_MINUS,
            Some('^') => S_FAILFAST,
            Some('\\') => S_SET_DIFF,
            Some('\u{2229}') => S_SET_ISECT_U,
            Some('\u{2206}') => S_SET_SYMDIFF_U,
            _ => S_IGNORE,
        },
        // it[1] == 'R' or 'r'
        Some('R') | Some('r') => match c0 {
            Some('O') | Some('o') => match c2 {
                // it[2] == 0  → exactly "OR" (any case of O and R)
                None => S_OR,
                _ => S_IGNORE,
            },
            _ => S_IGNORE,
        },
        _ => S_IGNORE,
    }
}

// [spec:cg3:def:uextras.cg3.ux-is-empty-fn]
// [spec:cg3:sem:uextras.cg3.ux-is-empty-fn]
//
// Returns true when empty or all-whitespace per `ISSPACE`.
pub fn is_blank(text: &str) -> bool {
    for c in text.chars() {
        if !isspace(c) {
            return false;
        }
    }
    true
}

// [spec:cg3:def:uextras.cg3.ux-simplecasecmp-fn]
// [spec:cg3:sem:uextras.cg3.ux-simplecasecmp-fn]
//
// Crude ASCII-only, one-directional case-insensitive prefix compare of the
// first `n` code units of `a` against `b`, with a trailing word-boundary check.
// ASYMMETRY reproduced: `a[i]` matches `b[i]` iff equal OR `a[i] == b[i] + 32`
// (only when `a` is the lowercase form), and `+ 32` is applied blindly (false
// "case" matches outside A-Z). Reading past `a` is UB in C++; safe Rust treats
// a missing `a[i]` as a mismatch and a missing `a[n]` as end-of-string
// (`a[n] == 0`). The combining-class lookup is approximated as 0.
//
// The walk is driven by `a` up to the caller-supplied count `n` (the C++
// `for (i=0; i<n; ++i)` pointer walk): `a` running out inside the prefix is a
// mismatch, and `b[i]` is indexed with `n` — panicking where the C++ read past
// `b` (UB) if a caller ever passes `n > b.len()`.
pub fn matches_keyword_chars(a: &[char], b: &[char], n: usize) -> bool {
    for (i, &ai) in a.iter().enumerate().take(n) {
        if ai != b[i] && (ai as u32) != (b[i] as u32) + 32 {
            return false;
        }
    }
    if a.len() < n {
        return false;
    }

    // If there is a combining character after the last plain letter, it's not a
    // match. Short-circuit for the most likely suffixes (NUL/space/delim).
    match a.get(n) {
        None => true, // a[n] == 0
        Some(&an) => an == '\0' || isspace(an) || isdelim(an) || combining_class(an) == 0,
    }
}

/// `&str` form collapsing the C++ overloads that pass `b`'s data and size —
/// for `b` being an owned string, a string view, and a pair of views. `n` is
/// `b`'s char count.
/// `a` is the text being scanned and `b` the keyword it must start with; the
/// comparison is not symmetric (see [`matches_keyword_chars`]).
pub fn matches_keyword(a: &str, b: &str) -> bool {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let n = bc.len();
    matches_keyword_chars(&ac, &bc, n)
}

/// Canonical combining class lookup, not in std; combining class is 0 for
/// every ASCII char, which is all that reaches this branch in practice. NOTE:
/// parity risk for real combining marks (Wave 4 may wire a Unicode-data crate).
fn combining_class(_c: char) -> u8 {
    0
}

// [spec:cg3:def:uextras.cg3.ux-str-case-compare-fn]
// [spec:cg3:sem:uextras.cg3.ux-str-case-compare-fn]
//
// Proper full-Unicode case-insensitive equality. The C++ compares with full
// Unicode default case folding; this approximates it with Rust's
// Unicode-aware lowercase folding (parity risk: full case folding and Rust
// `to_lowercase` tables differ for some scripts). BUG note: the C++ error path
// `throw new std::runtime_error(...)` (a raw POINTER, uncatchable by
// `catch(const std::exception&)`) has no analog — the std folding path cannot
// fail, so it is simply unreachable here.
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

// [spec:cg3:def:uextras.cg3.substr-t.value-type]
// value_type = char — the element type of the underlying UTF-8 string.

// [spec:cg3:def:uextras.cg3.substr-t]
/// C++ `struct substr_t` — in-place substring proxy. In C++ this temporarily NUL-terminates the backing
/// string via `const_cast` (mutating through a shared ref) to hand a C API a
/// `count`-length string, restoring the overwritten unit on destruction. That
/// mutate-through-`&` trick is neither possible nor necessary in the `&str`
/// slice model, so `data()` returns a plain sub-slice and no restore is needed
/// (`old_value` is retained for shape fidelity but unused; there is no `Drop`).
/// `offset`/`count` are char indices, matching C++'s code-unit indices.
pub struct Substr<'a> {
    pub str: &'a str,
    pub offset: usize,
    pub count: usize,
    pub old_value: char,
}

impl<'a> Substr<'a> {
    // [spec:cg3:def:uextras.cg3.substr-t.substr-t-fn]
    // [spec:cg3:sem:uextras.cg3.substr-t.substr-t-fn]
    //
    // Stores `str`/`offset`/`count`; `old_value` starts at `'\0'`. When
    // `count != NPOS`, saves the char that `data()` would overwrite
    // (`str[offset + count]`) — cosmetic here, since nothing is restored.
    pub fn new(str: &'a str, offset: usize, count: usize) -> Substr<'a> {
        let old_value = if count != NPOS {
            str.chars().nth(offset + count).unwrap_or('\0')
        } else {
            '\0'
        };
        Substr {
            str,
            offset,
            count,
            old_value,
        }
    }

    // [spec:cg3:def:uextras.cg3.substr-t.data-fn]
    // [spec:cg3:sem:uextras.cg3.substr-t.data-fn]
    //
    // Returns the substring `[offset, offset + count)` as a `&str`. In C++ this
    // NUL-terminates in place and returns a C pointer; here it is a borrow of
    // the char range (byte offsets derived from the char indices). `count` must
    // not be `NPOS` (as in C++, that would index out of bounds).
    pub fn data(&self) -> &'a str {
        let start = char_byte(self.str, self.offset);
        let end = char_byte(self.str, self.offset + self.count);
        &self.str[start..end]
    }
}

/// Byte offset of the `n`-th char (or `str.len()` at/after the end).
fn char_byte(str: &str, n: usize) -> usize {
    str.char_indices()
        .nth(n)
        .map(|(b, _)| b)
        .unwrap_or(str.len())
}

// [spec:cg3:def:uextras.cg3.substr-fn]
// [spec:cg3:sem:uextras.cg3.substr-fn]
//
// Convenience factory. NOTE the C++ default `count` here is 0 (a zero-length
// view), NOT `substr_t`'s own `NPOS` default — Rust has no default args, so all
// three are passed explicitly; callers normally give an explicit `count`.
pub fn substr(str: &str, offset: usize, count: usize) -> Substr<'_> {
    Substr::new(str, offset, count)
}

// [spec:cg3:def:uextras.cg3.ux-bufcpy-fn]
// [spec:cg3:sem:uextras.cg3.ux-bufcpy-fn]
//
// Copies up to `n` chars from `src` to `dst`, mapping raw newline code units to
// their Unicode "Control Pictures" (LF 0x0A → 0x240A, CR 0x0D → 0x240D), and
// NUL-terminates `dst`. Stops early at the first NUL in `src` (represented by
// the slice end) or immediately if `src` is `None` (the C++ null check). The
// caller must ensure `dst` has room for at least `i + 1` chars.
pub fn copy_with_visible_newlines(dst: &mut [char], src: Option<&[char]>, n: usize) {
    let mut i = 0usize;
    while i < n {
        match src.and_then(|s| s.get(i)).copied() {
            Some(ch) if ch != '\0' => {
                dst[i] = ch;
                if dst[i] == '\u{0A}' || dst[i] == '\u{0D}' {
                    dst[i] = char::from_u32(dst[i] as u32 + 0x2400).unwrap();
                }
                i += 1;
            }
            _ => break,
        }
    }
    dst[i] = '\0';
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // Pure path helpers: dir_prefix reimplements POSIX dirname(3) and guarantees
    // a trailing separator; basename splits on the last '/' or '\\'.
    // [spec:cg3:sem:uextras.cg3.ux-dirname-fn/test]
    // [spec:cg3:sem:uextras.basename-fn/test]
    #[test]
    fn dirname_and_basename() {
        // dir_prefix: directory portion, always ending in a separator.
        assert_eq!(dir_prefix("/usr/lib/foo.txt"), "/usr/lib/");
        assert_eq!(dir_prefix("foo.txt"), "./"); // no dir part -> "." + '/'
        assert_eq!(dir_prefix("/foo"), "/"); // root already ends in sep
        assert_eq!(dir_prefix(""), "./");
        // trailing slashes are stripped before dropping the last component
        assert_eq!(dir_prefix("/a/b/"), "/a/");

        // basename: piece after the final separator.
        assert_eq!(basename(Some("/usr/lib/foo.txt")), "foo.txt");
        assert_eq!(basename(Some("bar")), "bar"); // no separator -> unchanged
        assert_eq!(basename(Some("a\\b\\c")), "c"); // backslash separator
        assert_eq!(basename(Some("/end/")), "/"); // trailing sep -> point at it
        assert_eq!(basename(None), "."); // null path
    }

    // find_and_replace mutates the string in place and returns the count; a `to`
    // containing `from` must not loop forever (offset advances past the insert).
    // [spec:cg3:sem:uextras.cg3.find-and-replace-fn/test]
    #[test]
    fn find_and_replace_counts_and_no_loop() {
        let mut s: String = "a.b.c".to_string();
        assert_eq!(find_and_replace(&mut s, ".", "-"), 2);
        assert_eq!(s, "a-b-c");

        // `to` contains `from`: must terminate, one replacement.
        let mut s2: String = "x".to_string();
        assert_eq!(find_and_replace(&mut s2, "x", "xx"), 1);
        assert_eq!(s2, "xx");

        // No occurrence -> 0 replacements, string unchanged.
        let mut s3: String = "abc".to_string();
        assert_eq!(find_and_replace(&mut s3, "z", "!"), 0);
        assert_eq!(s3, "abc");
    }

    // get_line_clean reads a line via read_line_chars/read_char, collapsing runs of spaces
    // to a single space and stopping at a newline; it returns the cleaned length.
    // Drives get_line_clean -> read_line_chars -> read_char together.
    // [spec:cg3:sem:uextras.cg3.get-line-clean-fn/test]
    // [spec:cg3:sem:uextras.u-fgets-fn/test]
    // [spec:cg3:sem:uextras.u-fgetc-fn/test]
    #[test]
    fn get_line_clean_collapses_spaces() {
        let mut input = Cursor::new("a  \t b\tc\nnext".as_bytes().to_vec());
        let mut line = String::new();
        let mut cleaned = String::new();
        let n = get_line_clean(&mut line, &mut cleaned, &mut input, false);
        assert_eq!(cleaned, "a b c");
        assert_eq!(n, cleaned.len());
        assert!(
            line.starts_with("a  \t b\tc"),
            "raw line keeps original spacing"
        );

        // keep_tabs: a run containing a tab collapses to the tab.
        let mut input = Cursor::new("x \t y\n".as_bytes().to_vec());
        let n = get_line_clean(&mut line, &mut cleaned, &mut input, true);
        assert_eq!(cleaned, "x\ty");
        assert_eq!(n, cleaned.len());

        // Blank line: raw newline in `line`, empty cleaned, 0 (NOT EOF).
        let mut input = Cursor::new("\nz\n".as_bytes().to_vec());
        let n = get_line_clean(&mut line, &mut cleaned, &mut input, false);
        assert_eq!(n, 0);
        assert_eq!(line, "\n");

        // EOF: empty line distinguishes it from a blank line.
        let mut input = Cursor::new(Vec::<u8>::new());
        let n = get_line_clean(&mut line, &mut cleaned, &mut input, false);
        assert_eq!(n, 0);
        assert!(line.is_empty());
    }

    // read_utf8 reads a byte block but never splits a multi-byte UTF-8 sequence:
    // it completes the trailing sequence, so the returned bytes are valid UTF-8.
    // [spec:cg3:sem:uextras.read-utf8-fn/test]
    #[test]
    fn read_utf8_completes_trailing_sequence() {
        // Small text that fits entirely in the buffer.
        let text = "abcé";
        let mut input = Cursor::new(text.as_bytes().to_vec());
        let out = read_utf8(&mut input, 1000);
        assert_eq!(out, text.as_bytes());
        // The result is valid UTF-8 (no split sequence).
        assert_eq!(std::str::from_utf8(&out).unwrap(), text);
    }

    // strip_bom consumes a leading UTF-8 BOM (EF BB BF) and returns true; on
    // any non-BOM prefix it rewinds (Seek) so the stream is left untouched.
    // [spec:cg3:sem:uextras.ux-strip-bom-fn/test]
    #[test]
    fn strip_bom_consumes_or_rewinds() {
        // With a BOM: consumed, true, cursor now at the real content.
        let mut with_bom = Cursor::new(vec![0xEF, 0xBB, 0xBF, b'h', b'i']);
        assert!(strip_bom(&mut with_bom));
        let rest = read_utf8(&mut with_bom, 1000);
        assert_eq!(rest, b"hi");

        // No BOM: false, and the stream is rewound to the start (nothing eaten).
        let mut no_bom = Cursor::new(vec![b'h', b'i']);
        assert!(!strip_bom(&mut no_bom));
        assert_eq!(no_bom.position(), 0);
        let rest = read_utf8(&mut no_bom, 1000);
        assert_eq!(rest, b"hi");

        // Partial BOM (EF BB then a non-BF byte): false, all three bytes put back.
        let mut partial = Cursor::new(vec![0xEF, 0xBB, b'x']);
        assert!(!strip_bom(&mut partial));
        assert_eq!(partial.position(), 0);
    }

    // Output helpers. The formatted-print and flush shims are
    // DISSOLVED: formatted stream output is plain `write!` and flushing is plain
    // `Write::flush` at every former call site. This pins their observable
    // contract — the formatted arguments land on the stream as UTF-8 bytes, I/O
    // errors ignored. write_char writes a single char.
    // [spec:cg3:sem:uextras.u-fprintf-fn/test]
    // [spec:cg3:sem:uextras.u-fprintf-u-fn/test]
    // [spec:cg3:sem:uextras.u-vsnprintf-fn/test]
    // [spec:cg3:sem:uextras.u-fputc-fn/test]
    // [spec:cg3:sem:uextras.u-fflush-fn/test]
    #[test]
    fn dissolved_printf_shims_are_plain_write() {
        let mut out: Vec<u8> = Vec::new();
        let _ = write!(out, "hi {}", 42);
        assert_eq!(String::from_utf8(out).unwrap(), "hi 42");

        // Non-BMP output is written as its UTF-8 encoding (no UTF-16 leg).
        let mut out2: Vec<u8> = Vec::new();
        let _ = write!(out2, "\u{1F600}");
        assert_eq!(String::from_utf8(out2).unwrap(), "\u{1F600}");

        // write_char: writes one char and echoes it back; 0x7FFF is the last handled.
        let mut out3: Vec<u8> = Vec::new();
        assert_eq!(write_char('A', &mut out3), 'A');
        assert_eq!(out3, b"A");
        let mut out4: Vec<u8> = Vec::new();
        assert_eq!(write_char('\u{7FFF}', &mut out4), '\u{7FFF}');
        assert_eq!(out4, "\u{7FFF}".as_bytes());

        // Flushing is just `Write::flush` (a Vec flush is infallible); no panic.
        let mut sink: Vec<u8> = Vec::new();
        let _ = sink.flush();
    }

    // write_char reproduces the >= 0x8000 panic bug (second branch cuts off at
    // 0x7FFF). The u-fputc-fn/test facet lives on output_helpers_write_and_count.
    #[test]
    #[should_panic(expected = "can't handle >= 0x7FFF")]
    fn write_char_panics_above_limit() {
        let mut out: Vec<u8> = Vec::new();
        write_char('\u{8000}', &mut out);
    }

    // Set-op detection: single tokens (|,+,-,^,\,U+2229,U+2206) and "OR"/case.
    // is_blank is true for empty / all-whitespace strings.
    // [spec:cg3:sem:uextras.cg3.ux-is-set-op-fn/test]
    // [spec:cg3:sem:uextras.cg3.ux-is-empty-fn/test]
    #[test]
    fn set_op_and_empty() {
        assert_eq!(set_op_code("|"), S_OR);
        assert_eq!(set_op_code("+"), S_PLUS);
        assert_eq!(set_op_code("-"), S_MINUS);
        assert_eq!(set_op_code("^"), S_FAILFAST);
        assert_eq!(set_op_code("\\"), S_SET_DIFF);
        assert_eq!(set_op_code("\u{2229}"), S_SET_ISECT_U);
        assert_eq!(set_op_code("\u{2206}"), S_SET_SYMDIFF_U);
        assert_eq!(set_op_code("OR"), S_OR); // two-char OR (any case)
        assert_eq!(set_op_code("or"), S_OR);
        assert_eq!(set_op_code("foo"), S_IGNORE);
        assert_eq!(set_op_code(""), S_IGNORE);

        assert!(is_blank(""));
        assert!(is_blank("   \t "));
        assert!(!is_blank("  x "));
    }

    // matches_keyword_chars: crude ASCII case-insensitive prefix compare with the
    // documented lowercase-of-`a` asymmetry; eq_ignore_case is full-Unicode.
    // [spec:cg3:sem:uextras.cg3.ux-simplecasecmp-fn/test]
    // [spec:cg3:sem:uextras.cg3.ux-str-case-compare-fn/test]
    #[test]
    fn case_compares() {
        // matches_keyword: prefix "abc" of `b`, matched case-insensitively.
        assert!(matches_keyword("abc", "abc"));
        // ASYMMETRY: a is the lowercase form (a[i] == b[i] + 32), so "abc" matches
        // the uppercase "ABC" prefix.
        assert!(matches_keyword("abc", "ABC"));
        // ...but the reverse direction does NOT (b[i] + 32 != a[i]).
        assert!(!matches_keyword("ABC", "abc"));
        // Different letters do not match.
        assert!(!matches_keyword("abc", "xyz"));

        // eq_ignore_case: proper Unicode case-insensitive equality.
        assert!(eq_ignore_case("Hello", "hello"));
        assert!(eq_ignore_case("GRüßE", "grüße"));
        assert!(!eq_ignore_case("abc", "abd"));
    }

    // substr / substr_t::new build a proxy; data() returns the [offset, offset+
    // count) char slice. old_value records the char that would be overwritten.
    // [spec:cg3:sem:uextras.cg3.substr-fn/test]
    // [spec:cg3:sem:uextras.cg3.substr-t.substr-t-fn/test]
    // [spec:cg3:sem:uextras.cg3.substr-t.data-fn/test]
    #[test]
    fn substring_proxy() {
        let s = "hello world";
        // Factory + data(): "world".
        let sub = substr(s, 6, 5);
        assert_eq!(sub.data(), "world");

        // Direct ctor: char-indexed offset/count over multibyte text.
        let t = "héllo";
        let sub2 = Substr::new(t, 1, 3);
        assert_eq!(sub2.data(), "éll");
        assert_eq!(sub2.offset, 1);
        assert_eq!(sub2.count, 3);
        // old_value = the char at offset+count (the 'o' at char index 4).
        assert_eq!(sub2.old_value, 'o');

        // count == NPOS => old_value stays '\0'.
        let sub3 = Substr::new(s, 0, NPOS);
        assert_eq!(sub3.old_value, '\0');
    }

    // copy_with_visible_newlines copies up to n chars, mapping LF/CR to Control
    // Pictures and NUL-terminating; a None src copies nothing.
    // [spec:cg3:sem:uextras.cg3.ux-bufcpy-fn/test]
    #[test]
    fn bufcpy_maps_newlines() {
        let src: Vec<char> = "a\nb".chars().collect();
        let mut dst = vec!['X'; 8];
        copy_with_visible_newlines(&mut dst, Some(&src), 8);
        assert_eq!(dst[0], 'a');
        assert_eq!(dst[1], '\u{240A}'); // LF -> Control Picture LF
        assert_eq!(dst[2], 'b');
        assert_eq!(dst[3], '\0'); // NUL-terminated

        // CR maps to its Control Picture too.
        let src_cr: Vec<char> = "\r".chars().collect();
        let mut dst_cr = vec!['X'; 4];
        copy_with_visible_newlines(&mut dst_cr, Some(&src_cr), 4);
        assert_eq!(dst_cr[0], '\u{240D}');
        assert_eq!(dst_cr[1], '\0');

        // None src copies nothing but still NUL-terminates at index 0.
        let mut dst_none = vec!['X'; 4];
        copy_with_visible_newlines(&mut dst_none, None, 4);
        assert_eq!(dst_none[0], '\0');
    }
}
