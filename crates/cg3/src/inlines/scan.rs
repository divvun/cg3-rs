//! `inlines.hpp` — character predicates and the Char*&-cursor buffer-scanning helpers.
//!
//! Split out of the wave-2 monolithic `inlines.rs` (wave 4, w4-file-split-fmt).

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::types::UChar;

// ---------------------------------------------------------------------------
// Character predicates
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.isdelim-fn]
// [spec:cg3:sem:inlines.cg3.isdelim-fn]
#[inline]
pub fn isdelim(c: UChar) -> bool {
    c == '('
        || c == ')'
        || c == '+'
        || c == '-'
        || c == '*'
        || c == '/'
        || c == '^'
        || c == '%'
        || c == '='
}

// [spec:cg3:def:inlines.cg3.isspace-fn]
// [spec:cg3:sem:inlines.cg3.isspace-fn]
// Value form. `u_isWhitespace(c)` -> `char::is_whitespace()` (parity risk: ICU
// and Rust's White_Space tables may differ for c > 0xFF). The NBSP (0xA0) quirk
// is preserved via the explicit test.
#[inline]
pub fn isspace(c: UChar) -> bool {
    let u = c as u32;
    if u <= 0xFF && u != 0x09 && u != 0x0A && u != 0x0D && u != 0x20 && u != 0xA0 {
        return false;
    }
    u == 0x20 || u == 0x09 || u == 0x0A || u == 0x0D || u == 0xA0 || c.is_whitespace()
}

// [spec:cg3:def:inlines.cg3.isstring-fn]
// [spec:cg3:sem:inlines.cg3.isstring-fn]
// Reads p[-1] and p[c+1] (NOT p[c] — reproduce the exact +1 index quirk).
#[inline]
pub fn isstring(p: &[char], pos: usize, c: u32) -> bool {
    if p[pos - 1] == '"' && p[pos + c as usize + 1] == '"' {
        return true;
    }
    if p[pos - 1] == '<' && p[pos + c as usize + 1] == '>' {
        return true;
    }
    false
}

// [spec:cg3:def:inlines.cg3.isnl-fn]
// [spec:cg3:sem:inlines.cg3.isnl-fn]
// U+000D (CR) is deliberately NOT included.
#[inline]
pub fn isnl(c: UChar) -> bool {
    let u = c as u32;
    u == 0x2028 || u == 0x2029 || u == 0x000C || u == 0x000B || u == 0x000A
}

// [spec:cg3:def:inlines.cg3.isesc-fn]
// [spec:cg3:sem:inlines.cg3.isesc-fn]
// Counts consecutive backslashes immediately before p; escaped iff odd count.
#[inline]
pub fn isesc(p: &[char], pos: usize) -> bool {
    let mut a: usize = 1;
    while p[pos - a] == '\\' {
        a += 1;
    }
    a % 2 == 0
}

// Unspecced pointer-overload of ISSPACE: unescaped whitespace only.
#[inline]
pub fn isspace_p(p: &[char], pos: usize) -> bool {
    isspace(p[pos]) && !isesc(p, pos)
}

// C "C"-locale isalpha: [A-Za-z] only (no libc/crate available).
#[inline]
fn c_isalpha(c: u32) -> bool {
    (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c)
}

// C "C"-locale isdigit: [0-9].
#[inline]
fn c_isdigit(c: u32) -> bool {
    (0x30..=0x39).contains(&c)
}

// [spec:cg3:def:inlines.cg3.isalpha-c-fn]
// [spec:cg3:sem:inlines.cg3.isalpha-c-fn]
// (p < 255) && isalpha(p). Strict `<` (255 excluded). The C++ signed-char UB
// caveat does not arise here since our Char (char) is an unsigned scalar.
#[inline]
pub fn isalpha_c(p: UChar) -> bool {
    (p as u32) < 255 && c_isalpha(p as u32)
}

// [spec:cg3:def:inlines.cg3.isdigit-c-fn]
// [spec:cg3:sem:inlines.cg3.isdigit-c-fn]
#[inline]
pub fn isdigit_c(p: UChar) -> bool {
    (p as u32) < 255 && c_isdigit(p as u32)
}

// u_isalnum -> char::is_alphanumeric (ICU-vs-Rust Unicode parity risk).
#[inline]
fn u_isalnum(c: char) -> bool {
    c.is_alphanumeric()
}

// [spec:cg3:def:inlines.cg3.is-icase-fn]
// [spec:cg3:sem:inlines.cg3.is-icase-fn]
// Case-insensitive fixed keyword matcher. `uc`/`lc` mirror the C++ string
// literals `const C (&)[N]` — they MUST include the trailing '\0', so
// `N = uc.len()` and the keyword length is `N - 1`. Returns the matched length
// (N-1) on success, 0 on failure.
pub fn is_icase(p: &[char], pos: usize, uc: &[char], lc: &[char]) -> usize {
    let n = uc.len(); // N (incl. NUL terminator, as for a string constant)
    if isstring(p, pos, (n - 1) as u32) {
        return 0;
    }
    let mut i = 0usize;
    while i < n - 1 {
        if p[pos + i] != uc[i] && p[pos + i] != lc[i] {
            return 0;
        }
        i += 1;
    }
    if !u_isalnum(p[pos + (n - 1)]) {
        return i;
    }
    0
}

// ---------------------------------------------------------------------------
// Buffer scanning (Char*& p -> &[char] + &mut usize cursor)
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.backtonl-fn]
// [spec:cg3:sem:inlines.cg3.backtonl-fn]
pub fn backtonl(p: &[char], pos: &mut usize) {
    while p[*pos] != '\0' && !isnl(p[*pos]) && (p[*pos] != ';' || isesc(p, *pos)) {
        *pos -= 1;
    }
    *pos += 1;
}

// [spec:cg3:def:inlines.cg3.skipln-fn]
// [spec:cg3:sem:inlines.cg3.skipln-fn]
pub fn skipln(p: &[char], pos: &mut usize) -> u32 {
    while p[*pos] != '\0' && !isnl(p[*pos]) {
        *pos += 1;
    }
    *pos += 1;
    1
}

// [spec:cg3:def:inlines.cg3.skipws-fn]
// [spec:cg3:sem:inlines.cg3.skipws-fn]
// Stop test uses the VALUE form `!isspace(*p)` (escape-INsensitive). `a`/`b`
// default to '\0' at call sites (no Rust default args).
pub fn skipws(p: &[char], pos: &mut usize, a: UChar, b: UChar, allowhash: bool) -> u32 {
    let mut s = 0u32;
    while p[*pos] != '\0' && p[*pos] != a && p[*pos] != b {
        if isnl(p[*pos]) {
            s += 1;
        }
        if !allowhash && p[*pos] == '#' && !isesc(p, *pos) {
            s += skipln(p, pos);
            *pos -= 1;
        }
        if !isspace(p[*pos]) {
            break;
        }
        *pos += 1;
    }
    s
}

// [spec:cg3:def:inlines.cg3.skiptows-fn]
// [spec:cg3:sem:inlines.cg3.skiptows-fn]
// Loop guard uses the escape-aware pointer form `!isspace_p(p)`. Statement order
// reproduced exactly (comment-line double-count and post-newline step-over
// quirks preserved).
pub fn skiptows(p: &[char], pos: &mut usize, a: UChar, allowhash: bool, allowscol: bool) -> u32 {
    let mut s = 0u32;
    while p[*pos] != '\0' && !isspace_p(p, *pos) {
        if !allowhash && p[*pos] == '#' && !isesc(p, *pos) {
            s += skipln(p, pos);
            *pos -= 1;
        }
        if isnl(p[*pos]) {
            s += 1;
            *pos += 1;
        }
        if !allowscol && p[*pos] == ';' && !isesc(p, *pos) {
            break;
        }
        if p[*pos] == a && !isesc(p, *pos) {
            break;
        }
        *pos += 1;
    }
    s
}

// [spec:cg3:def:inlines.cg3.skipto-fn]
// [spec:cg3:sem:inlines.cg3.skipto-fn]
pub fn skipto(p: &[char], pos: &mut usize, a: UChar) -> u32 {
    let mut s = 0u32;
    while p[*pos] != '\0' && (p[*pos] != a || isesc(p, *pos)) {
        if isnl(p[*pos]) {
            s += 1;
        }
        *pos += 1;
    }
    s
}

// [spec:cg3:def:inlines.cg3.skipto-nospan-fn]
// [spec:cg3:sem:inlines.cg3.skipto-nospan-fn]
pub fn skipto_nospan(p: &[char], pos: &mut usize, a: UChar) {
    while p[*pos] != '\0' && (p[*pos] != a || isesc(p, *pos)) {
        if isnl(p[*pos]) {
            break;
        }
        *pos += 1;
    }
}

// [spec:cg3:def:inlines.cg3.skipto-nospan-raw-fn]
// [spec:cg3:sem:inlines.cg3.skipto-nospan-raw-fn]
pub fn skipto_nospan_raw(p: &[char], pos: &mut usize, a: UChar) {
    while p[*pos] != '\0' && p[*pos] != a {
        if isnl(p[*pos]) {
            break;
        }
        *pos += 1;
    }
}
