//! `inlines.hpp` — character predicates and the `Char*&`-cursor buffer-scanning
//! helpers.
//!
//! Split out of the wave-2 monolithic `inlines.rs` (wave 4, w4-file-split-fmt).
//!
//! ## Native-string cursors (wave 4, w4-utf8-native-strings)
//! The canonical scanners walk a `&str` with a BYTE-offset cursor (always on a
//! char boundary; advancement is by `len_utf8`). The C++ NUL terminator maps
//! to end-of-string: [`char_at`] yields `'\0'` at/after the end, so every
//! `p[*pos] != '\0'` loop guard translates 1:1. These are used by the
//! line-oriented STREAM readers (`get_line_clean`, Niceline/Plaintext) that
//! read one line into a `String` and scan it.
//!
//! ## Scalar-buffer cursors — the `*_chars` variants
//! The `*_chars` forms scan a `&[char]` scratch buffer with a `usize`-index
//! cursor and rely on a trailing `'\0'` sentinel. They are the PERMANENT form
//! for the reading/grammar LEXERS — `run_grammar_on_text`, the FST applicator,
//! and the `TextualParser` — which scan a whole working buffer with
//! `Char*`-style cursors, NUL-cut it in place, and rebuild the slices into
//! `String` tags. That is the wave-4 "symbols later rebuilt into a string"
//! carve-out: a random-access lexer over Unicode scalar values (NOT UTF-16,
//! NOT C pointers) is a faithful, idiomatic representation, and migrating its
//! ~300 in-place byte offsets carries poor risk/reward in a soft-gate wave.

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

// [spec:cg3:def:inlines.cg3.isnl-fn]
// [spec:cg3:sem:inlines.cg3.isnl-fn]
// U+000D (CR) is deliberately NOT included.
#[inline]
pub fn isnl(c: UChar) -> bool {
    let u = c as u32;
    u == 0x2028 || u == 0x2029 || u == 0x000C || u == 0x000B || u == 0x000A
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

// ---------------------------------------------------------------------------
// Native-string cursor primitives
// ---------------------------------------------------------------------------

/// The char at byte offset `pos`, or `'\0'` at/after the end — the exact
/// analog of dereferencing the C++ `Char*` cursor on a NUL-terminated buffer.
/// `pos` must be a char boundary.
#[inline]
pub fn char_at(s: &str, pos: usize) -> char {
    if pos >= s.len() {
        '\0'
    } else {
        s[pos..].chars().next().unwrap()
    }
}

/// The char ENDING at byte offset `pos` (`p[-1]`), or `'\0'` at the start.
#[inline]
pub fn prev_char(s: &str, pos: usize) -> char {
    s[..pos].chars().next_back().unwrap_or('\0')
}

/// Advance `pos` one char (`++p`). No-op at the end.
#[inline]
pub fn step(s: &str, pos: &mut usize) {
    if *pos < s.len() {
        *pos += char_at(s, *pos).len_utf8();
    } else {
        // Past-the-end advancement (the C++ cursor stepping past the NUL):
        // clamp — every caller loop is guarded by the '\0' sentinel anyway.
        *pos = s.len().max(*pos);
    }
}

/// Step `pos` back one char (`--p`). No-op at the start.
#[inline]
pub fn step_back(s: &str, pos: &mut usize) {
    let c = prev_char(s, *pos);
    if c != '\0' || *pos > 0 {
        *pos -= c.len_utf8().min(*pos);
    }
}

// [spec:cg3:def:inlines.cg3.isstring-fn]
// [spec:cg3:sem:inlines.cg3.isstring-fn]
// Reads `p[-1]` and `p[c+1]` (NOT `p[c]` — reproduce the exact +1 index
// quirk). `c` is the token's BYTE length here (the C++ counted code units);
// `p[c+1]` is "one char past the end of the token".
#[inline]
pub fn isstring(s: &str, pos: usize, c: usize) -> bool {
    let before = prev_char(s, pos);
    let after_tok = pos + c;
    let one_past = after_tok + char_at(s, after_tok).len_utf8();
    let after = char_at(s, one_past);
    if before == '"' && after == '"' {
        return true;
    }
    if before == '<' && after == '>' {
        return true;
    }
    false
}

// [spec:cg3:def:inlines.cg3.isesc-fn]
// [spec:cg3:sem:inlines.cg3.isesc-fn]
// Counts consecutive backslashes immediately before `pos`; escaped iff odd.
#[inline]
pub fn isesc(s: &str, pos: usize) -> bool {
    let mut n = 0usize;
    for c in s[..pos].chars().rev() {
        if c != '\\' {
            break;
        }
        n += 1;
    }
    n % 2 == 1
}

// Unspecced pointer-overload of ISSPACE: unescaped whitespace only.
#[inline]
pub fn isspace_p(s: &str, pos: usize) -> bool {
    isspace(char_at(s, pos)) && !isesc(s, pos)
}

// [spec:cg3:def:inlines.cg3.is-icase-fn]
// [spec:cg3:sem:inlines.cg3.is-icase-fn]
// Case-insensitive fixed keyword matcher. `uc`/`lc` are the ASCII keyword in
// upper/lower case (the C++ literals carried a trailing '\0'; the &str form
// drops it — the returned length and the `p[N-1]` "char after the keyword"
// alnum test are unchanged). Returns the matched length on success, 0 on
// failure.
pub fn is_icase(s: &str, pos: usize, uc: &str, lc: &str) -> usize {
    let n = uc.len(); // keyword byte length (ASCII)
    if isstring(s, pos, n) {
        return 0;
    }
    let b = s.as_bytes();
    if pos + n > b.len() {
        return 0;
    }
    let ub = uc.as_bytes();
    let lb = lc.as_bytes();
    for i in 0..n {
        if b[pos + i] != ub[i] && b[pos + i] != lb[i] {
            return 0;
        }
    }
    if !u_isalnum(char_at(s, pos + n)) {
        return n;
    }
    0
}

// ---------------------------------------------------------------------------
// Buffer scanning (Char*& p -> &str + &mut usize byte cursor)
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.backtonl-fn]
// [spec:cg3:sem:inlines.cg3.backtonl-fn]
pub fn backtonl(s: &str, pos: &mut usize) {
    while char_at(s, *pos) != '\0'
        && !isnl(char_at(s, *pos))
        && (char_at(s, *pos) != ';' || isesc(s, *pos))
    {
        step_back(s, pos);
    }
    step(s, pos);
}

// [spec:cg3:def:inlines.cg3.skipln-fn]
// [spec:cg3:sem:inlines.cg3.skipln-fn]
pub fn skipln(s: &str, pos: &mut usize) -> u32 {
    while char_at(s, *pos) != '\0' && !isnl(char_at(s, *pos)) {
        step(s, pos);
    }
    step(s, pos);
    1
}

// [spec:cg3:def:inlines.cg3.skipws-fn]
// [spec:cg3:sem:inlines.cg3.skipws-fn]
// Stop test uses the VALUE form `!isspace(*p)` (escape-INsensitive). `a`/`b`
// default to '\0' at call sites (no Rust default args).
pub fn skipws(s: &str, pos: &mut usize, a: UChar, b: UChar, allowhash: bool) -> u32 {
    let mut n = 0u32;
    loop {
        let c = char_at(s, *pos);
        if c == '\0' || c == a || c == b {
            break;
        }
        if isnl(c) {
            n += 1;
        }
        if !allowhash && c == '#' && !isesc(s, *pos) {
            n += skipln(s, pos);
            // C++ `--p` after the skipln (steps back onto the char before the
            // cursor so the loop's `++p` lands past it).
            step_back(s, pos);
        }
        if !isspace(char_at(s, *pos)) {
            break;
        }
        step(s, pos);
    }
    n
}

// [spec:cg3:def:inlines.cg3.skiptows-fn]
// [spec:cg3:sem:inlines.cg3.skiptows-fn]
// Loop guard uses the escape-aware pointer form `!isspace_p(p)`. Statement order
// reproduced exactly (comment-line double-count and post-newline step-over
// quirks preserved).
pub fn skiptows(s: &str, pos: &mut usize, a: UChar, allowhash: bool, allowscol: bool) -> u32 {
    let mut n = 0u32;
    while char_at(s, *pos) != '\0' && !isspace_p(s, *pos) {
        if !allowhash && char_at(s, *pos) == '#' && !isesc(s, *pos) {
            n += skipln(s, pos);
            step_back(s, pos);
        }
        if isnl(char_at(s, *pos)) {
            n += 1;
            step(s, pos);
        }
        if !allowscol && char_at(s, *pos) == ';' && !isesc(s, *pos) {
            break;
        }
        if char_at(s, *pos) == a && !isesc(s, *pos) {
            break;
        }
        step(s, pos);
    }
    n
}

// [spec:cg3:def:inlines.cg3.skipto-fn]
// [spec:cg3:sem:inlines.cg3.skipto-fn]
pub fn skipto(s: &str, pos: &mut usize, a: UChar) -> u32 {
    let mut n = 0u32;
    while char_at(s, *pos) != '\0' && (char_at(s, *pos) != a || isesc(s, *pos)) {
        if isnl(char_at(s, *pos)) {
            n += 1;
        }
        step(s, pos);
    }
    n
}

// [spec:cg3:def:inlines.cg3.skipto-nospan-fn]
// [spec:cg3:sem:inlines.cg3.skipto-nospan-fn]
pub fn skipto_nospan(s: &str, pos: &mut usize, a: UChar) {
    while char_at(s, *pos) != '\0' && (char_at(s, *pos) != a || isesc(s, *pos)) {
        if isnl(char_at(s, *pos)) {
            break;
        }
        step(s, pos);
    }
}

// [spec:cg3:def:inlines.cg3.skipto-nospan-raw-fn]
// [spec:cg3:sem:inlines.cg3.skipto-nospan-raw-fn]
pub fn skipto_nospan_raw(s: &str, pos: &mut usize, a: UChar) {
    while char_at(s, *pos) != '\0' && char_at(s, *pos) != a {
        if isnl(char_at(s, *pos)) {
            break;
        }
        step(s, pos);
    }
}

// ---------------------------------------------------------------------------
// Scalar-buffer `&[char]` variants — the PERMANENT form for the reading/grammar
// lexers (`run_grammar_on_text`, the FST applicator, the `TextualParser`), which
// scan a `Char*`-style working buffer and NUL-cut it in place before rebuilding
// the slices into `String` tags (the wave-4 "symbols later rebuilt into a
// string" carve-out). Semantics identical to the wave-2 originals.
// ---------------------------------------------------------------------------

#[inline]
pub fn isstring_chars(p: &[char], pos: usize, c: u32) -> bool {
    if p[pos - 1] == '"' && p[pos + c as usize + 1] == '"' {
        return true;
    }
    if p[pos - 1] == '<' && p[pos + c as usize + 1] == '>' {
        return true;
    }
    false
}

#[inline]
pub fn isesc_chars(p: &[char], pos: usize) -> bool {
    let mut a: usize = 1;
    while p[pos - a] == '\\' {
        a += 1;
    }
    a % 2 == 0
}

#[inline]
pub fn isspace_p_chars(p: &[char], pos: usize) -> bool {
    isspace(p[pos]) && !isesc_chars(p, pos)
}

pub fn is_icase_chars(p: &[char], pos: usize, uc: &[char], lc: &[char]) -> usize {
    let n = uc.len();
    if isstring_chars(p, pos, (n - 1) as u32) {
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

pub fn backtonl_chars(p: &[char], pos: &mut usize) {
    while p[*pos] != '\0' && !isnl(p[*pos]) && (p[*pos] != ';' || isesc_chars(p, *pos)) {
        *pos -= 1;
    }
    *pos += 1;
}

pub fn skipln_chars(p: &[char], pos: &mut usize) -> u32 {
    while p[*pos] != '\0' && !isnl(p[*pos]) {
        *pos += 1;
    }
    *pos += 1;
    1
}

pub fn skipws_chars(p: &[char], pos: &mut usize, a: UChar, b: UChar, allowhash: bool) -> u32 {
    let mut s = 0u32;
    while p[*pos] != '\0' && p[*pos] != a && p[*pos] != b {
        if isnl(p[*pos]) {
            s += 1;
        }
        if !allowhash && p[*pos] == '#' && !isesc_chars(p, *pos) {
            s += skipln_chars(p, pos);
            *pos -= 1;
        }
        if !isspace(p[*pos]) {
            break;
        }
        *pos += 1;
    }
    s
}

pub fn skiptows_chars(
    p: &[char],
    pos: &mut usize,
    a: UChar,
    allowhash: bool,
    allowscol: bool,
) -> u32 {
    let mut s = 0u32;
    while p[*pos] != '\0' && !isspace_p_chars(p, *pos) {
        if !allowhash && p[*pos] == '#' && !isesc_chars(p, *pos) {
            s += skipln_chars(p, pos);
            *pos -= 1;
        }
        if isnl(p[*pos]) {
            s += 1;
            *pos += 1;
        }
        if !allowscol && p[*pos] == ';' && !isesc_chars(p, *pos) {
            break;
        }
        if p[*pos] == a && !isesc_chars(p, *pos) {
            break;
        }
        *pos += 1;
    }
    s
}

pub fn skipto_chars(p: &[char], pos: &mut usize, a: UChar) -> u32 {
    let mut s = 0u32;
    while p[*pos] != '\0' && (p[*pos] != a || isesc_chars(p, *pos)) {
        if isnl(p[*pos]) {
            s += 1;
        }
        *pos += 1;
    }
    s
}

pub fn skipto_nospan_chars(p: &[char], pos: &mut usize, a: UChar) {
    while p[*pos] != '\0' && (p[*pos] != a || isesc_chars(p, *pos)) {
        if isnl(p[*pos]) {
            break;
        }
        *pos += 1;
    }
}

pub fn skipto_nospan_raw_chars(p: &[char], pos: &mut usize, a: UChar) {
    while p[*pos] != '\0' && p[*pos] != a {
        if isnl(p[*pos]) {
            break;
        }
        *pos += 1;
    }
}
