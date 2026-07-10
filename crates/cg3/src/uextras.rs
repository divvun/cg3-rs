//! Port of `src/uextras.cpp` + `src/uextras.hpp`.
//!
//! Literal, bug-for-bug 1:1 translation of the CG-3 Unicode/stream helper
//! utilities (spec `docs/spec/port/src/uextras.md`). Same control flow and
//! names (snake_cased where the task requests) as the original; the flagged
//! quirks are reproduced rather than fixed (Wave 4 does the idiomatic cleanup).
//!
//! ## Representation decisions (parity notes)
//!
//! * **UTF-8 / `char` model.** Per `crate::types`, `UChar = char` (a full
//!   Unicode scalar) and `UString = String` / `UStringView = &str` (UTF-8).
//!   The C++ code operates on UTF-16 `UChar` code units. Where the algorithm
//!   scans a NUL-terminated `UChar*` buffer, the port uses `&[char]` / `&str`;
//!   the trailing NUL is represented by the slice/string length.
//!
//! * **Streams → `std::io`.** The C++ `std::istream&` / `std::ostream&`
//!   parameters become `&mut impl Read` / `&mut impl Write` generics (matching
//!   `crate::inlines`' binary-IO helpers). `ux_strip_bom` additionally needs
//!   `Seek` because it "puts back" up to three bytes and `std::io::Read` has no
//!   `putback`; the C++ `istream::putback` calls map to `Seek::seek(Current(-n))`.
//!
//! * **No UTF-16 surrogates.** `u_fgetc`'s C++ body caches a pending *low
//!   surrogate* per stream (`cps[4]`) so callers see non-BMP code points one
//!   UTF-16 unit at a time. A Rust `char` is a full scalar and cannot hold a
//!   lone surrogate, so this port decodes each UTF-8 sequence to a single
//!   `char` and the surrogate-cache machinery is elided. Observable divergence:
//!   a non-BMP code point occupies ONE `char` slot here vs TWO `UChar` units in
//!   C++. `U_EOF` (0xFFFF) is preserved as the sentinel `'\u{FFFF}'`.
//!
//! * **`u_fprintf` family.** Rust has no C `va_list`, and ICU's
//!   `u_vsnprintf`/`u_vsnprintf_u` printf engine (plus the 500-UChar / 1500-byte
//!   two-pass stack-buffer resize dance) has no std equivalent. The wrappers
//!   instead take `std::fmt::Arguments` (produced by `format_args!` at the call
//!   site); observable behavior — formatted UTF-8 written to the sink, and the
//!   UTF-16 code-unit count returned — is preserved. The `char*`- vs
//!   `UChar*`-format overloads collapse (all format strings are Rust/UTF-8).
//!
//! * **`throw` → `panic!`.** Every C++ `throw std::runtime_error(...)` becomes a
//!   `panic!` with the same message. `ux_strCaseCompare`'s error path (which in
//!   C++ `throw`s a *pointer*, uncatchable by `catch(const std::exception&)`) is
//!   unreachable in the std approximation and documented at the site.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::inlines::{isdelim, isnl, isspace};
use crate::types::{UChar, UString, UStringView};

// ---------------------------------------------------------------------------
// Set-operator codes.
//
// These are the `enum : uint32_t { S_IGNORE, S_OR = 3, ... }` constants from
// `Strings.hpp`. Their canonical home is `crate::strings`, but that module only
// ported the `KEYWORDS` enum so far; `crate::grammar` already carries a private
// `S_OR`/`S_MINUS` (as `u32`). They are (re)defined here as the `int` that
// `ux_isSetOp` returns. NOTE for the lead: consolidate these into `strings.rs`
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

/// ICU `U_EOF` end sentinel (0xFFFF). U+FFFF is a noncharacter, so it never
/// appears in valid text — matching how C++ overloads it as the EOF marker.
pub const U_EOF: char = '\u{FFFF}';

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
pub fn ux_strip_bom<S: Read + Seek>(stream: &mut S) -> bool {
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
// ICU std::istream input wrappers (uextras.cpp)
// ===========================================================================

// [spec:cg3:def:uextras.u-fgets-fn]
// [spec:cg3:sem:uextras.u-fgets-fn]
//
// `UChar* s` → `&mut [char]`; returns `bool` (`true` ≈ the non-null `s`,
// `false` ≈ `nullptr`). QUIRKS reproduced: (1) the terminator is written at
// `s[i+1]`, not `s[i]`; (2) a line that is just a newline stores `s[0]` then
// returns `false` (`i == 0`) — indistinguishable from EOF, so callers treat an
// empty line as "read nothing"; (3) an exactly-full buffer writes no
// terminator. The caller must provide `s.len() >= n + 1` (so the `s[i+1]`
// write stays in bounds), as `get_line_clean` does.
pub fn u_fgets<R: Read>(s: &mut [char], n: i32, input: &mut R) -> bool {
    s[0] = '\0';
    let mut i: i32 = 0;
    while i < n {
        let c = u_fgetc(input);
        if c == U_EOF {
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
// is a full scalar (no lone surrogates). Returns `U_EOF` on end-of-stream and
// `'\0'` when the first byte read is a NUL. The lead-byte masks (0xF0/0xE0/0xC0,
// widest first) and the short-read `panic!`s mirror the source.
pub fn u_fgetc<R: Read>(input: &mut R) -> char {
    let c = match read_byte(input) {
        Some(v) => v,
        None => return U_EOF, // i == 0 && c == EOF
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
// ICU std::ostream output wrappers (uextras.cpp)
// ===========================================================================

// [spec:cg3:def:uextras.u-fflush-fn]
// [spec:cg3:sem:uextras.u-fflush-fn]
//
// `output.flush()`. The C++ `ostream&` and `ostream*` overloads collapse into
// this one; IO errors are ignored, as in the source.
pub fn u_fflush<W: Write>(output: &mut W) {
    let _ = output.flush();
}

// [spec:cg3:def:uextras.u-vsnprintf-fn]
// [spec:cg3:sem:uextras.u-vsnprintf-fn]
//
// The ICU `u_vsnprintf`/`u_vsnprintf_u` dispatcher (selected by format-char
// type) has no std equivalent; both overloads collapse to formatting
// `std::fmt::Arguments` into a `String`. The C++ return (the number of UChars
// that WOULD be written, used by `_u_fprintf` to detect truncation) is
// superseded by returning the fully formatted string directly.
fn _u_vsnprintf(args: std::fmt::Arguments) -> String {
    args.to_string()
}

// [spec:cg3:def:uextras.u-fprintf-fn]
// [spec:cg3:sem:uextras.u-fprintf-fn]
//
// Shared core. Formats `args`, writes the UTF-8 result to `output`, and returns
// the UTF-16 code-unit length of the output (`n16` = Σ `c.len_utf16()`). The
// two-pass 500-UChar / 1500-byte stack-buffer resize logic is an internal
// detail with no observable effect and is not reproduced.
fn _u_fprintf<W: Write>(output: &mut W, args: std::fmt::Arguments) -> i32 {
    let s = _u_vsnprintf(args);
    let n16: i32 = s.chars().map(|c| c.len_utf16() as i32).sum();
    let _ = output.write_all(s.as_bytes());
    n16
}

// [spec:cg3:def:uextras.u-fprintf-fn]
// [spec:cg3:sem:uextras.u-fprintf-fn]
//
// Public wrapper over a `char*`-style format. The three C++ overloads
// (`ostream&`, `unique_ptr<ostream>&`, `ostream*`) collapse into `&mut impl
// Write`. Call as `u_fprintf(out, format_args!("..."))`.
pub fn u_fprintf<W: Write>(output: &mut W, args: std::fmt::Arguments) -> i32 {
    _u_fprintf(output, args)
}

// [spec:cg3:def:uextras.u-fprintf-u-fn]
// [spec:cg3:sem:uextras.u-fprintf-u-fn]
//
// The `UChar*`-format variant. Behaviorally identical to `u_fprintf` in this
// port (all format strings are Rust/UTF-8), so it shares the same core.
pub fn u_fprintf_u<W: Write>(output: &mut W, args: std::fmt::Arguments) -> i32 {
    _u_fprintf(output, args)
}

// [spec:cg3:def:uextras.u-fputc-fn]
// [spec:cg3:sem:uextras.u-fputc-fn]
//
// `UChar32 c` → `char`. BUG/LIMITATION reproduced faithfully: the second branch
// cuts off at 0x7FFF, so every code point at or above 0x8000 `panic!`s ("can't
// handle >= 0x7FFF"), even though 0x7FFF itself is handled.
pub fn u_fputc<W: Write>(c32: char, output: &mut W) -> char {
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
pub fn ux_dirname(input: &str) -> String {
    let mut tmp = dirname_posix(input);
    if !(tmp.ends_with('/') || tmp.ends_with('\\')) {
        tmp.push('/');
    }
    tmp
}

/// POSIX `dirname(3)` reimplementation (ICU/libc unavailable). Mirrors
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
// The C++ `UnicodeString&` (ICU UTF-16, mutable) → `&mut UString` (owned UTF-8
// `String`); the port has no separate ICU `UnicodeString` type. `offset` and
// the `from`/`to` sizes are byte offsets into the UTF-8 buffer (the direct
// analog of C++'s code-unit offsets). Advancing `offset` past the inserted `to`
// prevents re-scanning replacements, so a `to` containing `from` cannot loop.
pub fn find_and_replace(str: &mut UString, from: UStringView, to: UStringView) -> usize {
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
// `line`/`cleaned` are pre-sized `UChar` scratch buffers the algorithm indexes
// and grows; represented here as `&mut Vec<char>` (NOT `&mut UString`) to
// preserve the O(1) code-unit indexing and in-place mutation the C++ relies on.
// NOTE for the lead: callers must supply `Vec<char>` buffers (or wrappers), with
// `cleaned.len() >= line.len() + 1`. The `line.size()-offset-1` size is computed
// as `i32` so an over-run of `offset` yields `n <= 0` (→ `u_fgets` reads
// nothing) instead of a `usize` underflow, matching the C++ termination. The
// added `offset < line.len()` bounds (C++ relies solely on the NUL terminator)
// keep an all-whitespace exactly-full buffer from panicking where C++ would UB.
pub fn get_line_clean<R: Read>(
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
        if !u_fgets(&mut line[offset..], n, input) {
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
// `const UChar* it` (NUL-terminated) → `&str`. `it[1] == 0` (a one-code-unit
// token) is "the string has exactly one char" (`c1 == None`). Returns the `S_*`
// code, or `S_IGNORE`.
pub fn ux_is_set_op(it: &str) -> i32 {
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
// `const UChar* text` (NUL-terminated) → `&str`; `u_strlen` (length to NUL) is
// the string's char count. Returns true when empty or all-whitespace per
// `ISSPACE`.
pub fn ux_is_empty(text: &str) -> bool {
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
// (`a[n] == 0`). `u_getCombiningClass` is unavailable and approximated as 0.
pub fn ux_simplecasecmp(a: &[UChar], b: &[UChar], n: usize) -> bool {
    for i in 0..n {
        match a.get(i) {
            Some(&ai) => {
                if ai != b[i] && (ai as u32) != (b[i] as u32) + 32 {
                    return false;
                }
            }
            None => return false,
        }
    }

    // If there is a combining character after the last plain letter, it's not a
    // match. Short-circuit for the most likely suffixes (NUL/space/delim).
    match a.get(n) {
        None => true, // a[n] == 0
        Some(&an) => {
            an == '\0' || isspace(an) || isdelim(an) || u_get_combining_class(an) == 0
        }
    }
}

/// `&str` convenience form collapsing the C++ overloads
/// `ux_simplecasecmp(a, b.data(), b.size())` — for `b` being `UString`,
/// `UStringView`, and `(UStringView, UStringView)`. `n` is `b`'s char count.
pub fn ux_simplecasecmp_sv(a: &str, b: &str) -> bool {
    let ac: Vec<UChar> = a.chars().collect();
    let bc: Vec<UChar> = b.chars().collect();
    let n = bc.len();
    ux_simplecasecmp(&ac, &bc, n)
}

/// ICU `u_getCombiningClass` is unavailable in std; combining class is 0 for
/// every ASCII char, which is all that reaches this branch in practice. NOTE:
/// parity risk for real combining marks (Wave 4 may wire a Unicode-data crate).
fn u_get_combining_class(_c: UChar) -> u8 {
    0
}

// [spec:cg3:def:uextras.cg3.ux-str-case-compare-fn]
// [spec:cg3:sem:uextras.cg3.ux-str-case-compare-fn]
//
// Proper full-Unicode case-insensitive equality. ICU
// `u_strCaseCompare(U_FOLD_CASE_DEFAULT)` is approximated with Rust's
// Unicode-aware lowercase folding (parity risk: ICU `foldCase` and Rust
// `to_lowercase` tables differ for some scripts). BUG note: the C++ error path
// `throw new std::runtime_error(...)` (a raw POINTER, uncatchable by
// `catch(const std::exception&)`) has no analog — the std folding path has no
// `UErrorCode`, so it is simply unreachable here.
pub fn ux_strCaseCompare(a: &UString, b: &UString) -> bool {
    let fold = |s: &str| -> String { s.chars().flat_map(|c| c.to_lowercase()).collect() };
    fold(a) == fold(b)
}

// [spec:cg3:def:uextras.cg3.substr-t.value-type]
// value_type = char (UChar) — the element type of the underlying UTF-8 string.

// [spec:cg3:def:uextras.cg3.substr-t]
/// In-place substring proxy. In C++ this temporarily NUL-terminates the backing
/// string via `const_cast` (mutating through a shared ref) to hand a C API a
/// `count`-length string, restoring the overwritten unit on destruction. That
/// mutate-through-`&` trick is neither possible nor necessary in the `&str`
/// slice model, so `data()` returns a plain sub-slice and no restore is needed
/// (`old_value` is retained for shape fidelity but unused; there is no `Drop`).
/// `offset`/`count` are char indices, matching C++'s code-unit indices.
pub struct substr_t<'a> {
    pub str: &'a str,
    pub offset: usize,
    pub count: usize,
    pub old_value: UChar,
}

impl<'a> substr_t<'a> {
    // [spec:cg3:def:uextras.cg3.substr-t.substr-t-fn]
    // [spec:cg3:sem:uextras.cg3.substr-t.substr-t-fn]
    //
    // Stores `str`/`offset`/`count`; `old_value` starts at `'\0'`. When
    // `count != NPOS`, saves the char that `data()` would overwrite
    // (`str[offset + count]`) — cosmetic here, since nothing is restored.
    pub fn new(str: &'a str, offset: usize, count: usize) -> substr_t<'a> {
        let old_value = if count != NPOS {
            str.chars().nth(offset + count).unwrap_or('\0')
        } else {
            '\0'
        };
        substr_t { str, offset, count, old_value }
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
    str.char_indices().nth(n).map(|(b, _)| b).unwrap_or(str.len())
}

// [spec:cg3:def:uextras.cg3.substr-fn]
// [spec:cg3:sem:uextras.cg3.substr-fn]
//
// Convenience factory. NOTE the C++ default `count` here is 0 (a zero-length
// view), NOT `substr_t`'s own `NPOS` default — Rust has no default args, so all
// three are passed explicitly; callers normally give an explicit `count`.
pub fn substr(str: &str, offset: usize, count: usize) -> substr_t<'_> {
    substr_t::new(str, offset, count)
}

// [spec:cg3:def:uextras.cg3.ux-bufcpy-fn]
// [spec:cg3:sem:uextras.cg3.ux-bufcpy-fn]
//
// Copies up to `n` chars from `src` to `dst`, mapping raw newline code units to
// their Unicode "Control Pictures" (LF 0x0A → 0x240A, CR 0x0D → 0x240D), and
// NUL-terminates `dst`. Stops early at the first NUL in `src` (represented by
// the slice end) or immediately if `src` is `None` (the C++ null check). The
// caller must ensure `dst` has room for at least `i + 1` chars.
pub fn ux_bufcpy(dst: &mut [UChar], src: Option<&[UChar]>, n: usize) {
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

    // Pure path helpers: ux_dirname reimplements POSIX dirname(3) and guarantees
    // a trailing separator; basename splits on the last '/' or '\\'.
    // [spec:cg3:sem:uextras.cg3.ux-dirname-fn/test]
    // [spec:cg3:sem:uextras.basename-fn/test]
    #[test]
    fn dirname_and_basename() {
        // ux_dirname: directory portion, always ending in a separator.
        assert_eq!(ux_dirname("/usr/lib/foo.txt"), "/usr/lib/");
        assert_eq!(ux_dirname("foo.txt"), "./"); // no dir part -> "." + '/'
        assert_eq!(ux_dirname("/foo"), "/"); // root already ends in sep
        assert_eq!(ux_dirname(""), "./");
        // trailing slashes are stripped before dropping the last component
        assert_eq!(ux_dirname("/a/b/"), "/a/");

        // basename: piece after the final separator.
        assert_eq!(basename(Some("/usr/lib/foo.txt")), "foo.txt");
        assert_eq!(basename(Some("bar")), "bar"); // no separator -> unchanged
        assert_eq!(basename(Some("a\\b\\c")), "c"); // backslash separator
        assert_eq!(basename(Some("/end/")), "/"); // trailing sep -> point at it
        assert_eq!(basename(None), "."); // null path
    }

    // find_and_replace mutates the UString in place and returns the count; a `to`
    // containing `from` must not loop forever (offset advances past the insert).
    // [spec:cg3:sem:uextras.cg3.find-and-replace-fn/test]
    #[test]
    fn find_and_replace_counts_and_no_loop() {
        let mut s: UString = "a.b.c".to_string();
        assert_eq!(find_and_replace(&mut s, ".", "-"), 2);
        assert_eq!(s, "a-b-c");

        // `to` contains `from`: must terminate, one replacement.
        let mut s2: UString = "x".to_string();
        assert_eq!(find_and_replace(&mut s2, "x", "xx"), 1);
        assert_eq!(s2, "xx");

        // No occurrence -> 0 replacements, string unchanged.
        let mut s3: UString = "abc".to_string();
        assert_eq!(find_and_replace(&mut s3, "z", "!"), 0);
        assert_eq!(s3, "abc");
    }

    // get_line_clean reads a line via u_fgets/u_fgetc, collapsing runs of spaces
    // to a single space and stopping at a newline; it returns the cleaned length.
    // Drives get_line_clean -> u_fgets -> u_fgetc together.
    // [spec:cg3:sem:uextras.cg3.get-line-clean-fn/test]
    // [spec:cg3:sem:uextras.u-fgets-fn/test]
    // [spec:cg3:sem:uextras.u-fgetc-fn/test]
    #[test]
    fn get_line_clean_collapses_spaces() {
        let mut input = Cursor::new(b"foo    bar\nnext".to_vec());
        let mut line = vec!['\0'; 64];
        let mut cleaned = vec!['\0'; 128];
        let n = get_line_clean(&mut line, &mut cleaned, &mut input, false);
        let got: String = cleaned[..n].iter().collect();
        // The run of spaces between "foo" and "bar" collapses to one space.
        assert_eq!(got, "foo bar");

        // u_fgets directly: reads up to a newline, stores it, reports success.
        let mut in2 = Cursor::new(b"hi\nrest".to_vec());
        let mut buf = vec!['\0'; 16];
        assert!(u_fgets(&mut buf, 8, &mut in2));
        assert_eq!(buf[0], 'h');
        assert_eq!(buf[1], 'i');
        assert!(isnl(buf[2])); // the '\n' is stored at s[i]

        // u_fgetc directly: one code point at a time; U_EOF at end.
        let mut in3 = Cursor::new("é!".as_bytes().to_vec());
        assert_eq!(u_fgetc(&mut in3), 'é'); // 2-byte UTF-8 decoded to one char
        assert_eq!(u_fgetc(&mut in3), '!');
        assert_eq!(u_fgetc(&mut in3), U_EOF);
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

    // ux_strip_bom consumes a leading UTF-8 BOM (EF BB BF) and returns true; on
    // any non-BOM prefix it rewinds (Seek) so the stream is left untouched.
    // [spec:cg3:sem:uextras.ux-strip-bom-fn/test]
    #[test]
    fn strip_bom_consumes_or_rewinds() {
        // With a BOM: consumed, true, cursor now at the real content.
        let mut with_bom = Cursor::new(vec![0xEF, 0xBB, 0xBF, b'h', b'i']);
        assert!(ux_strip_bom(&mut with_bom));
        let rest = read_utf8(&mut with_bom, 1000);
        assert_eq!(rest, b"hi");

        // No BOM: false, and the stream is rewound to the start (nothing eaten).
        let mut no_bom = Cursor::new(vec![b'h', b'i']);
        assert!(!ux_strip_bom(&mut no_bom));
        assert_eq!(no_bom.position(), 0);
        let rest = read_utf8(&mut no_bom, 1000);
        assert_eq!(rest, b"hi");

        // Partial BOM (EF BB then a non-BF byte): false, all three bytes put back.
        let mut partial = Cursor::new(vec![0xEF, 0xBB, b'x']);
        assert!(!ux_strip_bom(&mut partial));
        assert_eq!(partial.position(), 0);
    }

    // Output helpers: u_fprintf / u_fprintf_u format Arguments to UTF-8, returning
    // the UTF-16 code-unit count; _u_vsnprintf (private) is the shared formatter;
    // u_fputc writes a single char; u_fflush flushes the sink.
    // [spec:cg3:sem:uextras.u-fprintf-fn/test]
    // [spec:cg3:sem:uextras.u-fprintf-u-fn/test]
    // [spec:cg3:sem:uextras.u-vsnprintf-fn/test]
    // [spec:cg3:sem:uextras.u-fputc-fn/test]
    // [spec:cg3:sem:uextras.u-fflush-fn/test]
    #[test]
    fn output_helpers_write_and_count() {
        // u_fprintf: writes the formatted UTF-8 and returns the UTF-16 unit count.
        let mut out: Vec<u8> = Vec::new();
        let n = u_fprintf(&mut out, format_args!("hi {}", 42));
        assert_eq!(String::from_utf8(out.clone()).unwrap(), "hi 42");
        assert_eq!(n, 5); // "hi 42" is 5 UTF-16 code units

        // Non-BMP char counts as 2 UTF-16 units (surrogate pair).
        let mut out2: Vec<u8> = Vec::new();
        let n2 = u_fprintf_u(&mut out2, format_args!("{}", '\u{1F600}'));
        assert_eq!(n2, 2);
        assert_eq!(String::from_utf8(out2).unwrap(), "\u{1F600}");

        // Private shared formatter.
        assert_eq!(_u_vsnprintf(format_args!("{}-{}", 1, 2)), "1-2");

        // u_fputc: writes one char and echoes it back; 0x7FFF is the last handled.
        let mut out3: Vec<u8> = Vec::new();
        assert_eq!(u_fputc('A', &mut out3), 'A');
        assert_eq!(out3, b"A");
        let mut out4: Vec<u8> = Vec::new();
        assert_eq!(u_fputc('\u{7FFF}', &mut out4), '\u{7FFF}');
        assert_eq!(out4, "\u{7FFF}".as_bytes());

        // u_fflush just flushes (a Vec flush is infallible); no panic.
        let mut sink: Vec<u8> = Vec::new();
        u_fflush(&mut sink);
    }

    // u_fputc reproduces the >= 0x8000 panic bug (second branch cuts off at
    // 0x7FFF). The u-fputc-fn/test facet lives on output_helpers_write_and_count.
    #[test]
    #[should_panic(expected = "can't handle >= 0x7FFF")]
    fn u_fputc_panics_above_limit() {
        let mut out: Vec<u8> = Vec::new();
        u_fputc('\u{8000}', &mut out);
    }

    // Set-op detection: single tokens (|,+,-,^,\,U+2229,U+2206) and "OR"/case.
    // ux_is_empty is true for empty / all-whitespace strings.
    // [spec:cg3:sem:uextras.cg3.ux-is-set-op-fn/test]
    // [spec:cg3:sem:uextras.cg3.ux-is-empty-fn/test]
    #[test]
    fn set_op_and_empty() {
        assert_eq!(ux_is_set_op("|"), S_OR);
        assert_eq!(ux_is_set_op("+"), S_PLUS);
        assert_eq!(ux_is_set_op("-"), S_MINUS);
        assert_eq!(ux_is_set_op("^"), S_FAILFAST);
        assert_eq!(ux_is_set_op("\\"), S_SET_DIFF);
        assert_eq!(ux_is_set_op("\u{2229}"), S_SET_ISECT_U);
        assert_eq!(ux_is_set_op("\u{2206}"), S_SET_SYMDIFF_U);
        assert_eq!(ux_is_set_op("OR"), S_OR); // two-char OR (any case)
        assert_eq!(ux_is_set_op("or"), S_OR);
        assert_eq!(ux_is_set_op("foo"), S_IGNORE);
        assert_eq!(ux_is_set_op(""), S_IGNORE);

        assert!(ux_is_empty(""));
        assert!(ux_is_empty("   \t "));
        assert!(!ux_is_empty("  x "));
    }

    // ux_simplecasecmp: crude ASCII case-insensitive prefix compare with the
    // documented lowercase-of-`a` asymmetry; ux_strCaseCompare is full-Unicode.
    // [spec:cg3:sem:uextras.cg3.ux-simplecasecmp-fn/test]
    // [spec:cg3:sem:uextras.cg3.ux-str-case-compare-fn/test]
    #[test]
    fn case_compares() {
        // ux_simplecasecmp_sv: prefix "abc" of `b`, matched case-insensitively.
        assert!(ux_simplecasecmp_sv("abc", "abc"));
        // ASYMMETRY: a is the lowercase form (a[i] == b[i] + 32), so "abc" matches
        // the uppercase "ABC" prefix.
        assert!(ux_simplecasecmp_sv("abc", "ABC"));
        // ...but the reverse direction does NOT (b[i] + 32 != a[i]).
        assert!(!ux_simplecasecmp_sv("ABC", "abc"));
        // Different letters do not match.
        assert!(!ux_simplecasecmp_sv("abc", "xyz"));

        // ux_strCaseCompare: proper Unicode case-insensitive equality.
        assert!(ux_strCaseCompare(&"Hello".to_string(), &"hello".to_string()));
        assert!(ux_strCaseCompare(&"GRüßE".to_string(), &"grüße".to_string()));
        assert!(!ux_strCaseCompare(&"abc".to_string(), &"abd".to_string()));
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
        let sub2 = substr_t::new(t, 1, 3);
        assert_eq!(sub2.data(), "éll");
        assert_eq!(sub2.offset, 1);
        assert_eq!(sub2.count, 3);
        // old_value = the char at offset+count (the 'o' at char index 4).
        assert_eq!(sub2.old_value, 'o');

        // count == NPOS => old_value stays '\0'.
        let sub3 = substr_t::new(s, 0, NPOS);
        assert_eq!(sub3.old_value, '\0');
    }

    // ux_bufcpy copies up to n chars, mapping LF/CR to Control Pictures and
    // NUL-terminating; a None src copies nothing.
    // [spec:cg3:sem:uextras.cg3.ux-bufcpy-fn/test]
    #[test]
    fn bufcpy_maps_newlines() {
        let src: Vec<UChar> = "a\nb".chars().collect();
        let mut dst = vec!['X'; 8];
        ux_bufcpy(&mut dst, Some(&src), 8);
        assert_eq!(dst[0], 'a');
        assert_eq!(dst[1], '\u{240A}'); // LF -> Control Picture LF
        assert_eq!(dst[2], 'b');
        assert_eq!(dst[3], '\0'); // NUL-terminated

        // CR maps to its Control Picture too.
        let src_cr: Vec<UChar> = "\r".chars().collect();
        let mut dst_cr = vec!['X'; 4];
        ux_bufcpy(&mut dst_cr, Some(&src_cr), 4);
        assert_eq!(dst_cr[0], '\u{240D}');
        assert_eq!(dst_cr[1], '\0');

        // None src copies nothing but still NUL-terminates at index 0.
        let mut dst_none = vec!['X'; 4];
        ux_bufcpy(&mut dst_none, None, 4);
        assert_eq!(dst_none[0], '\0');
    }
}
