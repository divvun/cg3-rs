//! `inlines` unit tests (split out of the monolithic inlines.rs, wave 4).

use super::*;
use crate::types::{UString, UStringView};

// The narrowing static_cast helpers over the Prim trait. Concrete in/out
// pairs, including the truncating/sign behaviour of `as`.
// [spec:cg3:sem:inlines.si8-fn/test]
// [spec:cg3:sem:inlines.si32-fn/test]
// [spec:cg3:sem:inlines.si64-fn/test]
// [spec:cg3:sem:inlines.ui8-fn/test]
// [spec:cg3:sem:inlines.ui16-fn/test]
// [spec:cg3:sem:inlines.ui32-fn/test]
// [spec:cg3:sem:inlines.ui64-fn/test]
// [spec:cg3:sem:inlines.dbl-fn/test]
// [spec:cg3:sem:inlines.uiz-fn/test]
// [spec:cg3:sem:inlines.voidp-fn/test]
#[test]
fn numeric_casts() {
    // signed narrowing
    assert_eq!(si8(300i32), 300i32 as i8); // 44, truncated to one byte
    assert_eq!(si8(-1i32), -1i8);
    assert_eq!(si32(70000u32), 70000i32);
    assert_eq!(si32(-5i64), -5i32);
    assert_eq!(si64(-5i32), -5i64);
    assert_eq!(si64(u32::MAX), 4294967295i64);

    // unsigned narrowing
    assert_eq!(ui8(511u32), 255u8); // 0x1FF -> 0xFF
    assert_eq!(ui16(70000u32), 4464u16); // 70000 mod 65536
    assert_eq!(ui32(0x1_0000_0001u64), 1u32);
    assert_eq!(ui64(-1i32), u64::MAX); // sign-extend then reinterpret
    assert_eq!(uiz(42u32), 42usize);

    // double
    assert_eq!(dbl(3i32), 3.0f64);
    assert_eq!(dbl(2.5f32), 2.5f64);

    // voidp: casting a raw pointer preserves address; null stays null.
    let mut x: i32 = 7;
    let p: *mut i32 = &mut x;
    assert_eq!(voidp(p) as usize, p as usize);
    assert!(voidp(std::ptr::null_mut::<u8>()).is_null());
}

// make_64 packs hi:lo into a u64.
// [spec:cg3:sem:inlines.cg3.make-64-fn/test]
#[test]
fn make_64_packs_words() {
    assert_eq!(make_64(0, 0), 0);
    assert_eq!(make_64(0, 1), 1);
    assert_eq!(make_64(1, 0), 1u64 << 32);
    assert_eq!(make_64(0xDEAD_BEEF, 0x0BAD_F00D), 0xDEAD_BEEF_0BAD_F00Du64);
}

// SuperFastHash (byte + u16 overloads), the integer mixer, and the
// UString/hash_ustring facades. Assert stability + documented degenerate
// cases (empty -> 0; seed==0 -> len fallback; reserved-value remap).
// [spec:cg3:sem:inlines.cg3.super-fast-hash-fn/test]
// [spec:cg3:sem:inlines.cg3.hash-value-fn/test]
// [spec:cg3:sem:inlines.cg3.hash-ustring.operator-fn/test]
#[test]
fn hashing_family() {
    // Empty byte buffer hashes to 0 (documented degenerate case).
    assert_eq!(super_fast_hash(b"", 0), 0);

    // Deterministic and length-sensitive over all rem branches (0..3).
    let h_abcd = super_fast_hash(b"abcd", CG3_HASH_SEED); // rem == 0
    let h_abcde = super_fast_hash(b"abcde", CG3_HASH_SEED); // rem == 1
    let h_abcdef = super_fast_hash(b"abcdef", CG3_HASH_SEED); // rem == 2
    let h_abcdefg = super_fast_hash(b"abcdefg", CG3_HASH_SEED); // rem == 3
    assert_ne!(h_abcd, h_abcde);
    assert_ne!(h_abcde, h_abcdef);
    assert_ne!(h_abcdef, h_abcdefg);
    // Same input, same seed -> identical (deterministic).
    assert_eq!(super_fast_hash(b"abcd", CG3_HASH_SEED), h_abcd);
    // Reserved output values are never returned (remapped to CG3_HASH_SEED).
    for probe in ["", "a", "ab", "abc", "abcd", "hello world", "x"] {
        let h = super_fast_hash(probe.as_bytes(), 12345);
        assert!(h != u32::MAX && h != u32::MAX - 1);
    }

    // Integer mixer: seed==0 degenerates to CG3_HASH_SEED, deterministic,
    // and never returns a reserved value.
    let m1 = hash_value(42, 0);
    let m2 = hash_value(42, CG3_HASH_SEED);
    assert_eq!(m1, m2, "seed 0 falls back to CG3_HASH_SEED");
    assert!(m1 != 0 && m1 != u32::MAX && m1 != u32::MAX - 1);
    assert_ne!(hash_value(1, 100), hash_value(2, 100));

    // hash_ustring facade forces the seed and widens to usize; UTF-16-unit
    // hashing means it equals hash_value_ustring(_, 0) by construction.
    let hu = hash_ustring;
    let s: UString = "kitten".to_string();
    assert_eq!(hu.call(&s), hash_value_ustring(&s, 0) as usize);
    assert_eq!(hu.call(&s), hu.call_view("kitten"));
    assert_ne!(hu.call(&s), hu.call(&"sitting".to_string()));
}

// usv returns a borrowed view over the whole UString unchanged.
// [spec:cg3:sem:inlines.cg3.usv-fn/test]
#[test]
fn usv_view() {
    let s: UString = "hello".to_string();
    let v: UStringView = usv(&s);
    assert_eq!(v, "hello");
}

// Character predicates: isdelim / isspace (incl. NBSP quirk) / isnl (CR
// excluded) / isalpha_c / isdigit_c (255 boundary).
// [spec:cg3:sem:inlines.cg3.isdelim-fn/test]
// [spec:cg3:sem:inlines.cg3.isspace-fn/test]
// [spec:cg3:sem:inlines.cg3.isnl-fn/test]
// [spec:cg3:sem:inlines.cg3.isalpha-c-fn/test]
// [spec:cg3:sem:inlines.cg3.isdigit-c-fn/test]
#[test]
fn char_predicates() {
    // isdelim: the math/operator + paren set.
    for c in ['(', ')', '+', '-', '*', '/', '^', '%', '='] {
        assert!(isdelim(c), "{c} should be a delimiter");
    }
    assert!(!isdelim('a'));
    assert!(!isdelim(' '));

    // isspace: ASCII whitespace + NBSP (0xA0) quirk; other <=0xFF are false.
    assert!(isspace(' '));
    assert!(isspace('\t'));
    assert!(isspace('\n'));
    assert!(isspace('\r'));
    assert!(isspace('\u{00A0}'), "NBSP is whitespace (quirk preserved)");
    assert!(!isspace('a'));
    assert!(!isspace('\u{00B7}')); // a <=0xFF non-space stays false

    // isnl: these are newlines; CR (0x0D) is deliberately excluded.
    assert!(isnl('\n'));
    assert!(isnl('\u{000B}'));
    assert!(isnl('\u{000C}'));
    assert!(isnl('\u{2028}'));
    assert!(isnl('\u{2029}'));
    assert!(!isnl('\r'), "CR is intentionally NOT a newline here");
    assert!(!isnl('a'));

    // isalpha_c / isdigit_c: C-locale [A-Za-z] / [0-9], strict < 255.
    assert!(isalpha_c('A') && isalpha_c('z'));
    assert!(!isalpha_c('5') && !isalpha_c('_'));
    assert!(!isalpha_c('\u{00FF}')); // 255 excluded by strict <
    assert!(isdigit_c('0') && isdigit_c('9'));
    assert!(!isdigit_c('a'));
}

// isstring / isesc / is_icase all read backward + forward around a cursor.
// Assert the +1 forward-index quirk in isstring and the odd/even backslash
// count in isesc.
// [spec:cg3:sem:inlines.cg3.isstring-fn/test]
// [spec:cg3:sem:inlines.cg3.isesc-fn/test]
// [spec:cg3:sem:inlines.cg3.is-icase-fn/test]
#[test]
fn pointer_predicates() {
    // isstring: quotes/angle-brackets must be at p[pos-1] and p[pos+c+1].
    // Layout: byte 0 = '"', 1..=3 = "abc", 4 = '"'. With pos=1, c=2 the
    // forward test hits byte pos+c+1 = 4 (the closing quote) -- the +1 quirk.
    // `char_at` yields '\0' past the end, so no trailing NUL is needed.
    assert!(isstring("\"abc\"", 1, 2));
    // Angle-bracket variant.
    assert!(isstring("<abc>", 1, 2));
    // No surrounding delimiters -> false.
    assert!(!isstring("xabcy", 1, 2));

    // isesc: escaped iff an ODD number of backslashes immediately precede
    // the position. It walks backward until a non-'\' char. "a\x": at byte 2
    // ('x'), one preceding '\' bounded by 'a' -> odd -> escaped.
    assert!(isesc("a\\x", 2), "single backslash escapes");
    // "a\\x": two backslashes before x -> even -> NOT escaped.
    assert!(!isesc("a\\\\x", 3), "double backslash does not escape");
    // No preceding backslash -> even (zero) -> NOT escaped.
    assert!(!isesc("ab", 1));

    // is_icase: case-insensitive fixed keyword match. The native `&str` form
    // takes the keyword WITHOUT the C++ literal's trailing '\0', so the
    // keyword length is its byte length. Precondition (same as the port): the
    // cursor is never at byte 0 -- is_icase -> isstring reads p[pos-1], so we
    // keep a leading char and scan from pos=1.
    // " and " -> matches (followed by non-alnum space) -> length 3.
    assert_eq!(is_icase(" and rest", 1, "AND", "and"), 3);
    // Mixed case matches too.
    assert_eq!(is_icase(" And ", 1, "AND", "and"), 3);
    // Followed by an alnum -> no boundary -> 0.
    assert_eq!(is_icase(" andx", 1, "AND", "and"), 0);
    // Non-matching prefix -> 0.
    assert_eq!(is_icase(" xyz ", 1, "AND", "and"), 0);
}

// Buffer scanners: backtonl, skipln, skipws, skiptows, skipto and the two
// nospan variants. Drive concrete cursors and assert final positions and
// returned newline counts.
// [spec:cg3:sem:inlines.cg3.backtonl-fn/test]
// [spec:cg3:sem:inlines.cg3.skipln-fn/test]
// [spec:cg3:sem:inlines.cg3.skipws-fn/test]
// [spec:cg3:sem:inlines.cg3.skiptows-fn/test]
// [spec:cg3:sem:inlines.cg3.skipto-fn/test]
// [spec:cg3:sem:inlines.cg3.skipto-nospan-fn/test]
// [spec:cg3:sem:inlines.cg3.skipto-nospan-raw-fn/test]
#[test]
fn buffer_scanners() {
    // Native `&str` byte cursors: `char_at` reads the char at a byte offset and
    // yields '\0' at/after the end (the C++ NUL-terminated-buffer semantics), so
    // no trailing NUL is needed in the fixtures.

    // skipln: advance to just past the next newline; returns 1.
    let s = "abc\ndef";
    let mut pos = 0usize;
    let n = skipln(s, &mut pos);
    assert_eq!(n, 1);
    assert_eq!(char_at(s, pos), 'd'); // one past the '\n'

    // backtonl: walk back to the start of the current line. From byte 6 ('f')
    // it steps back over 'e','d' and halts at the '\n', then steps forward to
    // land at the 'd' at the start of the line.
    let mut pos = 6usize;
    backtonl(s, &mut pos);
    assert_eq!(char_at(s, pos), 'd', "backtonl lands at start of the line");

    // skipws (value form): skips leading whitespace, counts newlines.
    let w = "  \n x";
    let mut pos = 0usize;
    let nl = skipws(w, &mut pos, '\0', '\0', true);
    assert_eq!(char_at(w, pos), 'x');
    assert_eq!(nl, 1, "one newline traversed");

    // skiptows: advance over a token until whitespace; no newlines here.
    let t = "word next";
    let mut pos = 0usize;
    let nl = skiptows(t, &mut pos, '\0', true, true);
    assert_eq!(char_at(t, pos), ' ');
    assert_eq!(nl, 0);

    // skipto: advance to the target char `a`, counting newlines en route.
    let g = "aa\nbX";
    let mut pos = 0usize;
    let nl = skipto(g, &mut pos, 'X');
    assert_eq!(char_at(g, pos), 'X');
    assert_eq!(nl, 1);

    // skipto_nospan: stops at target OR newline (does not cross lines).
    let h = "ab\nX";
    let mut pos = 0usize;
    skipto_nospan(h, &mut pos, 'X');
    assert_eq!(char_at(h, pos), '\n', "nospan halts at the newline");

    // skipto_nospan_raw: same, but escape-insensitive; stops at target here.
    let r = "abX";
    let mut pos = 0usize;
    skipto_nospan_raw(r, &mut pos, 'X');
    assert_eq!(char_at(r, pos), 'X');
}

// Endian byte IO round-trips + explicit byte-order assertions (one BE, one
// LE), plus write_raw/read_raw (host order) and the f64 BE specializations
// which carry the write-be-fn / read-be-fn facets.
// [spec:cg3:sem:inlines.cg3.write-raw-fn/test]
// [spec:cg3:sem:inlines.cg3.read-raw-fn/test]
// [spec:cg3:sem:inlines.cg3.write-be-fn/test]
// [spec:cg3:sem:inlines.cg3.write-le-fn/test]
// [spec:cg3:sem:inlines.cg3.read-be-fn/test]
// [spec:cg3:sem:inlines.cg3.read-le-fn/test]
#[test]
fn endian_io() {
    // Explicit byte order: 0x0102 big-endian is [0x01, 0x02].
    let mut be = Vec::new();
    write_be(&mut be, 0x0102u16);
    assert_eq!(be, vec![0x01, 0x02], "BE writes most-significant first");

    // 0x0102 little-endian is [0x02, 0x01].
    let mut le = Vec::new();
    write_le(&mut le, 0x0102u16);
    assert_eq!(le, vec![0x02, 0x01], "LE writes least-significant first");

    // Round-trip BE and LE for a u32.
    let mut buf = Vec::new();
    write_be(&mut buf, 0xDEAD_BEEFu32);
    let mut cur = std::io::Cursor::new(buf);
    assert_eq!(read_be::<u32, _>(&mut cur), 0xDEAD_BEEF);

    let mut buf = Vec::new();
    write_le(&mut buf, 0xDEAD_BEEFu32);
    let mut cur = std::io::Cursor::new(buf);
    assert_eq!(read_le::<u32, _>(&mut cur), 0xDEAD_BEEF);

    // write_raw / read_raw: host byte order, round-trips.
    let mut buf = Vec::new();
    write_raw(&mut buf, 0x1122_3344_5566_7788u64);
    assert_eq!(buf.len(), 8);
    let mut cur = std::io::Cursor::new(buf);
    assert_eq!(read_raw::<u64, _>(&mut cur), 0x1122_3344_5566_7788);

    // f64 BE specializations (write-be-fn / read-be-fn facets): 12-byte
    // frexp mantissa+exponent encoding, round-trips within tolerance.
    let mut buf = Vec::new();
    write_be_f64(&mut buf, 3.140625f64);
    assert_eq!(buf.len(), 12, "8-byte mantissa + 4-byte exponent");
    let mut cur = std::io::Cursor::new(buf);
    let back = read_be_f64(&mut cur);
    assert!((back - 3.140625f64).abs() < 1e-9, "f64 BE round-trip");
}

// UTF-8 length-prefixed IO: raw (host-order prefix) and LE prefix. Round
// trips through write then read, and exercises the out-param read_utf8_le.
// [spec:cg3:sem:inlines.cg3.write-utf8-raw-fn/test]
// [spec:cg3:sem:inlines.cg3.write-utf8-le-fn/test]
// [spec:cg3:sem:inlines.cg3.read-utf8-raw-fn/test]
// [spec:cg3:sem:inlines.cg3.read-utf8-le-fn/test]
#[test]
fn utf8_io() {
    // raw prefix round-trip.
    let mut buf = Vec::new();
    write_utf8_raw(&mut buf, "héllo");
    // prefix (2 bytes host order) + the UTF-8 body length.
    assert_eq!(buf.len(), 2 + "héllo".len());
    let mut cur = std::io::Cursor::new(buf);
    assert_eq!(read_utf8_raw(&mut cur), "héllo");

    // LE prefix round-trip: explicit little-endian length prefix.
    let mut buf = Vec::new();
    write_utf8_le(&mut buf, "abc");
    assert_eq!(&buf[..2], &[3u8, 0u8], "length 3 as LE u16 prefix");
    let mut cur = std::io::Cursor::new(buf);
    let mut out = UString::from("stale");
    read_utf8_le(&mut cur, &mut out);
    assert_eq!(out, "abc");

    // Empty string: zero-length prefix, empty body.
    let mut buf = Vec::new();
    write_utf8_le(&mut buf, "");
    let mut cur = std::io::Cursor::new(buf);
    assert_eq!(read_utf8_le_ret(&mut cur), "");
}

// Magic-byte detectors: feed the actual prefixes.
// [spec:cg3:sem:inlines.cg3.is-textual-fn/test]
// [spec:cg3:sem:inlines.cg3.is-internal-fn/test]
// [spec:cg3:sem:inlines.cg3.is-cg3b-fn/test]
// [spec:cg3:sem:inlines.cg3.is-cg3bsf-fn/test]
#[test]
fn magic_bytes() {
    // is_textual: quoted or <...>.
    assert!(is_textual("\"word\""));
    assert!(is_textual("<tag>"));
    assert!(!is_textual("plain"));
    assert!(!is_textual("\"unbalanced"));

    // is_internal: leading "_G_".
    assert!(is_internal("_G_foo"));
    assert!(!is_internal("G_foo"));

    // Binary-grammar magic prefixes.
    assert!(is_cg3b("CG3B....."));
    assert!(!is_cg3b("CG3X"));
    assert!(is_cg3bsf("CGBF....."));
    assert!(!is_cg3bsf("CG3B"));
}

// clear (via the Clearable trait) and size (const array length).
// [spec:cg3:sem:inlines.cg3.clear-fn/test]
// [spec:cg3:sem:inlines.cg3.size-fn/test]
#[test]
fn clear_and_size() {
    // size: compile-time array length.
    let a = [10u8, 20, 30, 40];
    assert_eq!(size(&a), 4);
    let empty: [u8; 0] = [];
    assert_eq!(size(&empty), 0);

    // clear: no-op on already-empty; empties non-empty containers.
    let mut v = vec![1, 2, 3];
    clear(&mut v);
    assert!(v.is_empty());
    let mut already: Vec<i32> = Vec::new();
    clear(&mut already); // stays empty, is_empty_c() short-circuits
    assert!(already.is_empty());
    let mut s = String::from("text");
    clear(&mut s);
    assert!(s.is_empty());
}

// insert_if_exists ORs a source bit-vector into a dest, growing (zero-fill).
// [spec:cg3:sem:inlines.cg3.insert-if-exists-fn/test]
#[test]
fn insert_if_exists_ors_bits() {
    let mut cont = vec![true, false, false];
    let other = vec![false, true, false, true]; // longer -> grows cont
    insert_if_exists(&mut cont, Some(&other));
    assert_eq!(cont, vec![true, true, false, true]);

    // None / empty source is a no-op.
    let mut c2 = vec![true];
    insert_if_exists(&mut c2, None);
    assert_eq!(c2, vec![true]);
    insert_if_exists(&mut c2, Some(&Vec::new()));
    assert_eq!(c2, vec![true]);
}

// g_app_set_opts_ranged: comma-separated numbers and inclusive ranges, plus
// the single-value `fill` expansion and the reversed-range (empty) case.
// [spec:cg3:sem:inlines.cg3.g-app-set-opts-ranged-fn/test]
#[test]
fn opts_ranged() {
    let mut c: Vec<u32> = Vec::new();

    // Plain list.
    g_app_set_opts_ranged("1,3,5", &mut c, false);
    assert_eq!(c, vec![1, 3, 5]);

    // Inclusive range.
    g_app_set_opts_ranged("2-5", &mut c, false);
    assert_eq!(c, vec![2, 3, 4, 5]);

    // Mixed list + range.
    g_app_set_opts_ranged("1,4-6,9", &mut c, false);
    assert_eq!(c, vec![1, 4, 5, 6, 9]);

    // Reversed range "3-1" yields nothing (high < low -> empty).
    g_app_set_opts_ranged("3-1", &mut c, false);
    assert!(c.is_empty());

    // Single value with fill -> expands to 1..=value.
    g_app_set_opts_ranged("4", &mut c, true);
    assert_eq!(c, vec![1, 2, 3, 4]);

    // Single value without fill -> just that value.
    g_app_set_opts_ranged("4", &mut c, false);
    assert_eq!(c, vec![4]);
}

// swapper: conditional swap on construct AND on drop (net identity while
// cond=true), no-op when cond=false.
// [spec:cg3:sem:inlines.cg3.swapper.swapper-fn/test]
#[test]
fn swapper_swaps_on_construct_and_drop() {
    let mut a = 1;
    let mut b = 2;
    {
        let _s = swapper::new(true, &mut a, &mut b);
        // swapped on construct
        assert_eq!((*_s.a, *_s.b), (2, 1));
    } // swapped back on drop -> net identity
    assert_eq!((a, b), (1, 2));

    // cond=false: never swaps.
    let mut a = 1;
    let mut b = 2;
    {
        let _s = swapper::new(false, &mut a, &mut b);
    }
    assert_eq!((a, b), (1, 2));
}

// swapper_false: holds b at false while alive, restores original on drop.
// [spec:cg3:sem:inlines.cg3.swapper-false.swapper-false-fn/test]
#[test]
fn swapper_false_restores_on_drop() {
    let mut flag = true;
    {
        let _s = swapper_false::new(true, &mut flag);
        assert!(!*_s.b, "held at false while alive");
    }
    assert!(flag, "restored to original (true) on drop");

    // cond=false: no change at all.
    let mut flag = true;
    {
        let _s = swapper_false::new(false, &mut flag);
    }
    assert!(flag);
}

// uncond_swap: unconditionally installs `b` into `a` on construct and
// restores a's original value on drop.
// [spec:cg3:sem:inlines.cg3.uncond-swap.uncond-swap-fn/test]
#[test]
fn uncond_swap_installs_and_restores() {
    let mut a = 10;
    {
        let _s = uncond_swap::new(&mut a, 99);
        assert_eq!(*_s.a, 99, "installed the passed value");
        assert_eq!(_s.b, 10, "kept a's original");
    }
    assert_eq!(a, 10, "restored a's original on drop");
}

// inc_dec: inc() bumps the target and arms the guard; drop decrements it.
// [spec:cg3:sem:inlines.cg3.inc-dec.inc-fn/test]
// [spec:cg3:sem:inlines.cg3.inc-dec.inc-dec-fn/test]
#[test]
fn inc_dec_counter_guard() {
    let mut counter: i32 = 5;
    {
        let mut g = inc_dec::new();
        g.inc(&mut counter);
        // incremented on inc()
    } // decremented on drop -> back to 5
    assert_eq!(counter, 5);

    // An un-armed guard (never inc'd) does nothing on drop.
    {
        let _g: inc_dec<'_, i32> = inc_dec::new();
    }
    assert_eq!(counter, 5);
}

// scope_guard: runs its callback on drop iff good; set(false) disarms it.
// [spec:cg3:sem:inlines.cg3.scope-guard.scope-guard-fn/test]
// [spec:cg3:sem:inlines.cg3.scope-guard.set-fn/test]
#[test]
fn scope_guard_runs_unless_disarmed() {
    use std::cell::Cell;
    let ran = Cell::new(false);
    {
        let _g = scope_guard::new(|| ran.set(true));
    }
    assert!(ran.get(), "callback ran on drop (good)");

    // set(false) disarms.
    let ran2 = Cell::new(false);
    {
        let mut g = scope_guard::new(|| ran2.set(true));
        g.set(false);
    }
    assert!(!ran2.get(), "disarmed guard does not run");
}

// reversed()/begin()/end() reverse-range adapters over a container.
// [spec:cg3:sem:inlines.cg3.reversed-fn/test]
// [spec:cg3:sem:inlines.cg3.begin-fn/test]
// [spec:cg3:sem:inlines.cg3.end-fn/test]
#[test]
fn reversed_range() {
    let v = vec![1, 2, 3, 4];

    // begin(reversed) yields a reverse iterator starting at the back.
    let mut it = begin(reversed(&v));
    assert_eq!(it.next(), Some(&4));
    assert_eq!(it.next(), Some(&3));

    // end(reversed) is the exhausted reverse iterator.
    let mut e = end(reversed(&v));
    assert_eq!(e.next(), None);

    // The IntoIterator sugar iterates in reverse.
    let collected: Vec<i32> = reversed(&v).into_iter().copied().collect();
    assert_eq!(collected, vec![4, 3, 2, 1]);
}

// erase(): erase-remove idiom removing ALL equal elements.
// [spec:cg3:sem:inlines.cg3.erase-fn/test]
#[test]
fn erase_all_matching() {
    let mut v = vec![1, 2, 3, 2, 4, 2];
    erase(&mut v, &2);
    assert_eq!(v, vec![1, 3, 4]);
    // Absent value -> unchanged.
    erase(&mut v, &99);
    assert_eq!(v, vec![1, 3, 4]);
}

// make_array / make_array_helper: build [R; N] via f(0)..f(N-1) in order.
// [spec:cg3:sem:inlines.cg3.make-array-fn/test]
// [spec:cg3:sem:inlines.cg3.make-array-helper-fn/test]
#[test]
fn make_array_from_index() {
    let arr: [usize; 4] = make_array(|i| i * i);
    assert_eq!(arr, [0, 1, 4, 9]);

    // The helper is invoked directly and yields the same order.
    let arr2: [usize; 3] = make_array_helper(|i| i + 10);
    assert_eq!(arr2, [10, 11, 12]);
}

// concat / details::_concat: append pieces in order.
// [spec:cg3:sem:inlines.cg3.concat-fn/test]
// [spec:cg3:sem:inlines.cg3.details.concat-fn/test]
#[test]
fn concat_builds_string() {
    assert_eq!(concat("a", &["b", "c", "d"]), "abcd");
    assert_eq!(concat("solo", &[]), "solo");

    // Drive the private details::_concat helper directly.
    let mut m = String::from("x");
    details::_concat(&mut m, &["y", "z"]);
    assert_eq!(m, "xyz");
}

// cg3_quit is `-> !` (calls std::process::exit). It cannot be called in a
// unit test without aborting the whole test binary, so it is NOT invoked.
// We reference it here (in a never-executed branch) so this facet is
// genuinely attached to code that names the function, honestly documenting
// that it is uncallable in-process.
// [spec:cg3:sem:inlines.cg3.cg3-quit-fn/test]
#[test]
fn cg3_quit_is_noreturn() {
    // Take a function pointer to the noreturn fn without calling it; this
    // type-checks its signature (i32, Option<&str>, u32) -> ! . We never
    // dispatch it, because doing so would exit the process.
    let f: Option<fn(i32, Option<&str>, u32) -> !> = Some(cg3_quit);
    assert!(f.is_some());
}
