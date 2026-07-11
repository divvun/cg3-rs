//! `inlines.hpp` — the SuperFastHash family and hash_value overloads.
//!
//! Split out of the wave-2 monolithic `inlines.rs` (wave 4, w4-file-split-fmt).

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::types::{UString, UStringView};

use super::*;

// ---------------------------------------------------------------------------
// Hashing
// ---------------------------------------------------------------------------

// get16bits: the portable strictly-little-endian form. On the "known LE"
// compilers the C++ used a raw `*(const uint16_t*)d` load; both agree on LE
// hosts and the portable form defines the byte-order contract as little-endian.
#[inline]
fn get16bits(data: &[u8], i: usize) -> u32 {
    ((data[i + 1] as u32) << 8) + (data[i] as u32)
}

// [spec:cg3:def:inlines.cg3.super-fast-hash-fn]
// [spec:cg3:sem:inlines.cg3.super-fast-hash-fn]
// Paul Hsieh's SuperFastHash over a BYTE buffer. Ported from
// `SuperFastHash(const char* data, size_t len, uint32_t hash)`. The `data`
// pointer + `len` become a `&[u8]` slice (empty slice == null/len 0). All
// arithmetic is 32-bit wraparound. BYTE-PARITY: because the C++ `data` is
// `const char*` (signed on most targets), the single-byte reads at rem==1
// (`*data`) and rem==3 (`data[2]`) sign-extend bytes >= 0x80 to negative ints
// before shift/add — reproduced here via `as i8 as i32`. get16bits stays
// strictly little-endian.
pub fn super_fast_hash(bytes: &[u8], seed: u32) -> u32 {
    let mut hash = seed;
    let mut len = bytes.len();

    if hash == 0 {
        hash = len as u32; // UI32(len)
    }

    if len == 0 {
        return 0;
    }

    let rem = len & 3;
    len >>= 2;

    // Main loop
    let mut i = 0usize;
    for _ in 0..len {
        hash = hash.wrapping_add(get16bits(bytes, i));
        let tmp = (get16bits(bytes, i + 2) << 11) ^ hash;
        hash = (hash << 16) ^ tmp;
        i += 2 * 2; // 2 * sizeof(uint16_t)
        hash = hash.wrapping_add(hash >> 11);
    }

    // Handle end cases
    match rem {
        3 => {
            hash = hash.wrapping_add(get16bits(bytes, i));
            hash ^= hash << 16;
            hash ^= ((bytes[i + 2] as i8 as i32) << 18) as u32;
            hash = hash.wrapping_add(hash >> 11);
        }
        2 => {
            hash = hash.wrapping_add(get16bits(bytes, i));
            hash ^= hash << 11;
            hash = hash.wrapping_add(hash >> 17);
        }
        1 => {
            hash = hash.wrapping_add((bytes[i] as i8 as i32) as u32);
            hash ^= hash << 10;
            hash = hash.wrapping_add(hash >> 1);
        }
        _ => {}
    }

    // Force "avalanching" of final 127 bits
    hash ^= hash << 3;
    hash = hash.wrapping_add(hash >> 5);
    hash ^= hash << 4;
    hash = hash.wrapping_add(hash >> 17);
    hash ^= hash << 25;
    hash = hash.wrapping_add(hash >> 6);

    if hash == 0 || hash == u32::MAX || hash == u32::MAX - 1 {
        hash = CG3_HASH_SEED;
    }

    hash
}

// [spec:cg3:def:inlines.cg3.hash-value-fn]
// [spec:cg3:sem:inlines.cg3.hash-value-fn]
// The self-contained 65599-style integer mixer. NOTE: the required shared-API
// signature drops the C++ default arg (`h = CG3_HASH_SEED`); callers pass the
// seed explicitly. The SuperFastHash-based alternative in the C++ is commented
// out and NOT used.
pub fn hash_value(value: u32, seed: u32) -> u32 {
    let mut h = seed;
    if h == 0 {
        h = CG3_HASH_SEED;
    }
    // h = c + (h<<6) + (h<<16) - h   (== c + h*65599, mod 2^32)
    h = value
        .wrapping_add(h << 6)
        .wrapping_add(h << 16)
        .wrapping_sub(h);
    if h == 0 || h == u32::MAX || h == u32::MAX - 1 {
        h = CG3_HASH_SEED;
    }
    h
}

// size_t-width analog of the integer mixer. There is NO C++ counterpart in
// inlines.hpp (the source only mixes uint32_t); this is provided per the
// shared-API contract for callers that hash at pointer width. Because the
// shifts are over `usize` (64-bit) this does NOT produce the same values as the
// u32 mixer — it is a parallel, not a literal translation.
pub fn hash_value_sz(value: usize, seed: usize) -> usize {
    let mut h = seed;
    if h == 0 {
        h = CG3_HASH_SEED as usize;
    }
    h = value
        .wrapping_add(h << 6)
        .wrapping_add(h << 16)
        .wrapping_sub(h);
    if h == 0 || h == usize::MAX || h == usize::MAX - 1 {
        h = CG3_HASH_SEED as usize;
    }
    h
}

// C++ `hash_value(const UChar*, hash, len)` / `hash_value(const UString&, h)`:
// routes to the UTF-16 code-unit SuperFastHash overload, NOT the byte one.
// Tag/text hashes feed hash-ordered containers (tries, sorted output order),
// so UTF-16 unit hashing is required for output parity with the C++ — verified
// against T_Append/T_Substitute/T_Unification/T_Variables golden diffs.
pub fn hash_value_ustring(str: &str, hash: u32) -> u32 {
    let mut h = hash;
    if h == 0 {
        h = CG3_HASH_SEED;
    }
    let units: Vec<u16> = str.encode_utf16().collect();
    super_fast_hash_u16(&units, h)
}

/// C++ `SuperFastHash(const UChar* data, size_t len, uint32_t hash)` — the
/// UTF-16 code-unit overload (src/inlines.hpp:174). Two 16-bit units per main
/// loop iteration; `rem = len & 1` single-unit tail; same avalanche + reserved
/// remap as the byte overload. `hash == 0` degenerates to `len` (as in C++).
pub fn super_fast_hash_u16(data: &[u16], hash: u32) -> u32 {
    let len = data.len();
    let mut hash = if hash == 0 { len as u32 } else { hash };
    if len == 0 {
        return 0;
    }
    let rem = len & 1;
    let mut i = 0usize;
    let mut n = len >> 1;
    while n > 0 {
        hash = hash.wrapping_add(data[i] as u32);
        let tmp = ((data[i + 1] as u32) << 11) ^ hash;
        hash = (hash << 16) ^ tmp;
        i += 2;
        hash = hash.wrapping_add(hash >> 11);
        n -= 1;
    }
    if rem == 1 {
        hash = hash.wrapping_add(data[i] as u32);
        hash ^= hash << 11;
        hash = hash.wrapping_add(hash >> 17);
    }
    hash ^= hash << 3;
    hash = hash.wrapping_add(hash >> 5);
    hash ^= hash << 4;
    hash = hash.wrapping_add(hash >> 17);
    hash ^= hash << 25;
    hash = hash.wrapping_add(hash >> 6);
    if hash == 0 || hash == u32::MAX || hash == u32::MAX - 1 {
        hash = CG3_HASH_SEED;
    }
    hash
}

// [spec:cg3:def:inlines.cg3.hash-ustring]
pub struct hash_ustring;

impl hash_ustring {
    // [spec:cg3:def:inlines.cg3.hash-ustring.operator-fn]
    // [spec:cg3:sem:inlines.cg3.hash-ustring.operator-fn]
    // C++ `operator()` -> ported to a `call` method (Rust has no call operator
    // overloading for arbitrary self). Forces the seed to CG3_HASH_SEED and
    // hashes the string's bytes; return type widened to usize (size_t).
    pub fn call(&self, str: &UString) -> usize {
        hash_value_ustring(str, 0) as usize
    }

    // Sibling `operator()(const UStringView&)`.
    pub fn call_view(&self, str: UStringView) -> usize {
        hash_value_ustring(str, 0) as usize
    }
}
