//! Port of `src/inlines.hpp` — foundation inline helpers.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). Names are snake_cased.
//! Quirks, off-by-ones and wraparound are reproduced faithfully; idiomatic
//! cleanups are deferred to Wave 4.
//!
//! ## Porting-representation decisions (apply throughout this file)
//! * The C++ pointer-walking helpers take `Char*& p` (a reference to a raw
//!   pointer into a NUL-terminated buffer). They are ported over
//!   `(p: &[char], pos: &mut usize)` — a full buffer slice plus an absolute
//!   cursor. `*p` becomes `p[*pos]`, `++p`/`--p` become `*pos += 1`/`*pos -= 1`,
//!   and `p[-1]`/`p[c+1]` become `p[*pos - 1]`/`p[*pos + c + 1]`. Callers MUST
//!   pass the whole buffer (so backward reads land inside it) and MUST include a
//!   `'\0'` terminator (the loops stop on `*p == 0`). Just like the C++ there is
//!   NO lower-bound check: an underflowing `*pos - a` panics here where the C++
//!   would read out of bounds — same precondition, different failure mode.
//! * `Char` is instantiated concretely as `char` (our `UChar`), matching the
//!   UTF-16 `UChar` text buffers of the original but over Unicode scalars.
//! * Byte IO (`readRaw`/`writeRaw`/`readBE`…): the C++ `std::istream&` /
//!   `std::ostream&` become `std::io::Read` / `Write`. The generic byte plumbing
//!   goes through the [`ByteOrdered`] trait. Reads are return-style (the C++
//!   `readRaw(S&, T&)` in-place form is folded into a returning function). Short
//!   reads/writes: `read_exact`/`write_all` errors are swallowed (the C++
//!   ignores failbit); on a short read the value is left zero-filled (the C++
//!   leaves it partially written — minor deviation, noted).
//! * The narrowing-cast helpers (`si8`…`dbl`) are generic over the [`Prim`]
//!   trait (a stand-in for C++ `static_cast` on arithmetic types). NOTE: Rust's
//!   `as` from float saturates for out-of-range inputs, whereas C++
//!   `static_cast` is UB/implementation-defined there; for in-range values they
//!   agree. `constexpr` becomes a plain fn (const trait methods are unstable),
//!   except `make_64` which stays `const fn`.
//! * ICU is not available in Wave 2. Because our `UString` is already UTF-8, the
//!   `u_strToUTF8`/`u_strFromUTF8` transcoding collapses to identity over the
//!   string's bytes, so the UTF-8 read/write helpers keep the exact on-disk
//!   format (length prefix + UTF-8 bytes) with no external crate. `u_isalnum`,
//!   `u_isWhitespace` are approximated with Rust's Unicode tables (parity risk
//!   noted at each site). `isalpha`/`isdigit` use the C "C"-locale semantics
//!   (no libc, no crate).
//! * No external crate is required (std only): `to_be_bytes`/`from_le_bytes`/…
//!   for endianness, hand-ported musl `frexp`/`scalbn` for `ldexp`.

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::types::{UChar, UString, UStringView};
use std::io::{Read, Write};

// ---------------------------------------------------------------------------
// Numeric-cast scaffolding: the `Prim` trait stands in for C++ static_cast over
// arithmetic types so the SIn/UIn/DBL/UIZ helpers can be generic.
// ---------------------------------------------------------------------------

pub trait Prim: Copy {
    fn as_i8(self) -> i8;
    fn as_i32(self) -> i32;
    fn as_i64(self) -> i64;
    fn as_u8(self) -> u8;
    fn as_u16(self) -> u16;
    fn as_u32(self) -> u32;
    fn as_u64(self) -> u64;
    fn as_usize(self) -> usize;
    fn as_f64(self) -> f64;
}

macro_rules! impl_prim {
    ($($t:ty),*) => {$(
        impl Prim for $t {
            #[inline] fn as_i8(self) -> i8 { self as i8 }
            #[inline] fn as_i32(self) -> i32 { self as i32 }
            #[inline] fn as_i64(self) -> i64 { self as i64 }
            #[inline] fn as_u8(self) -> u8 { self as u8 }
            #[inline] fn as_u16(self) -> u16 { self as u16 }
            #[inline] fn as_u32(self) -> u32 { self as u32 }
            #[inline] fn as_u64(self) -> u64 { self as u64 }
            #[inline] fn as_usize(self) -> usize { self as usize }
            #[inline] fn as_f64(self) -> f64 { self as f64 }
        }
    )*};
}
impl_prim!(i8, u8, i16, u16, i32, u32, i64, u64, isize, usize, f32, f64);

// [spec:cg3:def:inlines.si8-fn]
// [spec:cg3:sem:inlines.si8-fn]
#[inline]
pub fn si8<T: Prim>(t: T) -> i8 {
    t.as_i8()
}

// [spec:cg3:def:inlines.si32-fn]
// [spec:cg3:sem:inlines.si32-fn]
#[inline]
pub fn si32<T: Prim>(t: T) -> i32 {
    t.as_i32()
}

// [spec:cg3:def:inlines.si64-fn]
// [spec:cg3:sem:inlines.si64-fn]
#[inline]
pub fn si64<T: Prim>(t: T) -> i64 {
    t.as_i64()
}

// [spec:cg3:def:inlines.ui8-fn]
// [spec:cg3:sem:inlines.ui8-fn]
#[inline]
pub fn ui8<T: Prim>(t: T) -> u8 {
    t.as_u8()
}

// [spec:cg3:def:inlines.ui16-fn]
// [spec:cg3:sem:inlines.ui16-fn]
#[inline]
pub fn ui16<T: Prim>(t: T) -> u16 {
    t.as_u16()
}

// [spec:cg3:def:inlines.ui32-fn]
// [spec:cg3:sem:inlines.ui32-fn]
#[inline]
pub fn ui32<T: Prim>(t: T) -> u32 {
    t.as_u32()
}

// [spec:cg3:def:inlines.ui64-fn]
// [spec:cg3:sem:inlines.ui64-fn]
#[inline]
pub fn ui64<T: Prim>(t: T) -> u64 {
    t.as_u64()
}

// [spec:cg3:def:inlines.dbl-fn]
// [spec:cg3:sem:inlines.dbl-fn]
#[inline]
pub fn dbl<T: Prim>(t: T) -> f64 {
    t.as_f64()
}

// [spec:cg3:def:inlines.uiz-fn]
// [spec:cg3:sem:inlines.uiz-fn]
#[inline]
pub fn uiz<T: Prim>(t: T) -> usize {
    t.as_usize()
}

// [spec:cg3:def:inlines.voidp-fn]
// [spec:cg3:sem:inlines.voidp-fn]
// Casts a (typically pointer) value to `void*`. Ported over a raw pointer;
// casting the pointer is safe (dereferencing it is not).
#[inline]
pub fn voidp<T>(t: *mut T) -> *mut core::ffi::c_void {
    t as *mut core::ffi::c_void
}

pub const NUMERIC_MIN: f64 = -(1i64 << 48) as f64;
pub const NUMERIC_MAX: f64 = ((1i64 << 48) - 1) as f64;

// [spec:cg3:def:inlines.cg3.hash-value-fn] (shared constant)
pub const CG3_HASH_SEED: u32 = 705577479;

// [spec:cg3:def:inlines.cg3.make-64-fn]
// [spec:cg3:sem:inlines.cg3.make-64-fn]
#[inline]
pub const fn make_64(hi: u32, low: u32) -> u64 {
    ((hi as u64) << 32) | (low as u64)
}

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

// ---------------------------------------------------------------------------
// Byte IO: readRaw/writeRaw/readBE/writeBE/readLE/writeLE over Read/Write.
// ---------------------------------------------------------------------------

pub trait ByteOrdered: Copy {
    fn to_be_vec(self) -> Vec<u8>;
    fn to_le_vec(self) -> Vec<u8>;
    fn to_ne_vec(self) -> Vec<u8>;
    fn from_be_slice(b: &[u8]) -> Self;
    fn from_le_slice(b: &[u8]) -> Self;
    fn from_ne_slice(b: &[u8]) -> Self;
    fn byte_size() -> usize;
}

macro_rules! impl_byte_ordered {
    ($($t:ty),*) => {$(
        impl ByteOrdered for $t {
            #[inline] fn to_be_vec(self) -> Vec<u8> { self.to_be_bytes().to_vec() }
            #[inline] fn to_le_vec(self) -> Vec<u8> { self.to_le_bytes().to_vec() }
            #[inline] fn to_ne_vec(self) -> Vec<u8> { self.to_ne_bytes().to_vec() }
            #[inline] fn from_be_slice(b: &[u8]) -> Self {
                let mut a = [0u8; std::mem::size_of::<$t>()];
                a.copy_from_slice(&b[..std::mem::size_of::<$t>()]);
                Self::from_be_bytes(a)
            }
            #[inline] fn from_le_slice(b: &[u8]) -> Self {
                let mut a = [0u8; std::mem::size_of::<$t>()];
                a.copy_from_slice(&b[..std::mem::size_of::<$t>()]);
                Self::from_le_bytes(a)
            }
            #[inline] fn from_ne_slice(b: &[u8]) -> Self {
                let mut a = [0u8; std::mem::size_of::<$t>()];
                a.copy_from_slice(&b[..std::mem::size_of::<$t>()]);
                Self::from_ne_bytes(a)
            }
            #[inline] fn byte_size() -> usize { std::mem::size_of::<$t>() }
        }
    )*};
}
impl_byte_ordered!(u8, i8, u16, i16, u32, i32, u64, i64, usize, isize);

// [spec:cg3:def:inlines.cg3.write-raw-fn]
// [spec:cg3:sem:inlines.cg3.write-raw-fn]
// Writes the raw object representation (host byte order), no conversion.
#[inline]
pub fn write_raw<T: ByteOrdered, W: Write>(stream: &mut W, value: T) {
    let _ = stream.write_all(&value.to_ne_vec());
}

// [spec:cg3:def:inlines.cg3.read-raw-fn]
// [spec:cg3:sem:inlines.cg3.read-raw-fn]
// Reads sizeof(T) bytes verbatim into a T (host byte order). Ported to
// return-by-value (the C++ `readRaw(S&, T&)` filled an out-param). On a short
// read the returned value is zero-filled (C++ leaves it partially written).
#[inline]
pub fn read_raw<T: ByteOrdered, R: Read>(stream: &mut R) -> T {
    let mut buf = vec![0u8; T::byte_size()];
    let _ = stream.read_exact(&mut buf);
    T::from_ne_slice(&buf)
}

// Generic `writeBE<T>`: native_to_big + writeRaw. (Unspecced in inlines.md but
// the specialization for double delegates to it.)
#[inline]
pub fn write_be<T: ByteOrdered, W: Write>(stream: &mut W, value: T) {
    let _ = stream.write_all(&value.to_be_vec());
}

// Generic `readBE<T>`: readRaw + big_to_native.
#[inline]
pub fn read_be<T: ByteOrdered, R: Read>(stream: &mut R) -> T {
    let mut buf = vec![0u8; T::byte_size()];
    let _ = stream.read_exact(&mut buf);
    T::from_be_slice(&buf)
}

// [spec:cg3:def:inlines.cg3.write-le-fn]
// [spec:cg3:sem:inlines.cg3.write-le-fn]
#[inline]
pub fn write_le<T: ByteOrdered, W: Write>(stream: &mut W, value: T) {
    let _ = stream.write_all(&value.to_le_vec());
}

// [spec:cg3:def:inlines.cg3.read-le-fn]
// [spec:cg3:sem:inlines.cg3.read-le-fn]
#[inline]
pub fn read_le<T: ByteOrdered, R: Read>(stream: &mut R) -> T {
    let mut buf = vec![0u8; T::byte_size()];
    let _ = stream.read_exact(&mut buf);
    T::from_le_slice(&buf)
}

// hand-ported musl frexp: returns (mantissa in [0.5,1), exponent).
fn frexp(x: f64) -> (f64, i32) {
    let mut y = x.to_bits();
    let ee = ((y >> 52) & 0x7ff) as i32;
    if ee == 0 {
        if x != 0.0 {
            // 0x1p64
            let (x1, e1) = frexp(x * f64::from_bits(0x43f0000000000000));
            return (x1, e1 - 64);
        }
        return (x, 0);
    } else if ee == 0x7ff {
        // inf / nan: exponent is unspecified in C; return 0.
        return (x, 0);
    }
    let e = ee - 0x3fe;
    y &= 0x800fffffffffffff;
    y |= 0x3fe0000000000000;
    (f64::from_bits(y), e)
}

// hand-ported musl scalbn == ldexp: value * 2^n.
fn ldexp(x: f64, mut n: i32) -> f64 {
    let mut y = x;
    if n > 1023 {
        y *= f64::from_bits(0x7fe0000000000000); // 0x1p1023
        n -= 1023;
        if n > 1023 {
            y *= f64::from_bits(0x7fe0000000000000);
            n -= 1023;
            if n > 1023 {
                n = 1023;
            }
        }
    } else if n < -1022 {
        // 0x1p-1022 * 0x1p53
        y *= f64::from_bits(0x0010000000000000) * f64::from_bits(0x4340000000000000);
        n += 1022 - 53;
        if n < -1022 {
            y *= f64::from_bits(0x0010000000000000) * f64::from_bits(0x4340000000000000);
            n += 1022 - 53;
            if n < -1022 {
                n = -1022;
            }
        }
    }
    let u = ((0x3ff + n) as u64) << 52;
    y * f64::from_bits(u)
}

// [spec:cg3:def:inlines.cg3.write-be-fn]
// [spec:cg3:sem:inlines.cg3.write-be-fn]
// C++ full specialization `writeBE(ostream, double)`. Rust cannot specialize a
// generic fn, so the double case is a distinct name. Encodes big-endian
// mantissa (8 bytes) then exponent (4 bytes) = 12 bytes.
pub fn write_be_f64<W: Write>(stream: &mut W, value: f64) {
    let (m, exp) = frexp(value);
    let mant64 = ui64(si64(dbl(i64::MAX) * m));
    let exp32 = ui32(exp);
    write_be(stream, mant64);
    write_be(stream, exp32);
}

// [spec:cg3:def:inlines.cg3.read-be-fn]
// [spec:cg3:sem:inlines.cg3.read-be-fn]
// C++ full specialization `readBE<double>`; distinct name in Rust. Inverse of
// write_be_f64: reads 8-byte BE mantissa then 4-byte BE exponent (12 bytes).
pub fn read_be_f64<R: Read>(stream: &mut R) -> f64 {
    let mant64: u64 = read_be(stream);
    let exp = read_be::<i32, R>(stream) as i32;

    let value = dbl(si64(mant64)) / dbl(i64::MAX);

    ldexp(value, exp)
}

// ---------------------------------------------------------------------------
// UTF-8 length-prefixed IO. ICU transcoding collapses to identity because our
// UString is already UTF-8; the on-disk format (length prefix + UTF-8 bytes) is
// preserved exactly, including the 16-bit prefix (>65535-byte strings wrap) and
// the host-endian vs little-endian prefix distinction.
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.write-utf8-raw-fn]
// [spec:cg3:sem:inlines.cg3.write-utf8-raw-fn]
// The (UChar*, len) and UString overloads collapse to a single &str entry since
// UString is UTF-8. Length prefix written RAW (host byte order).
pub fn write_utf8_raw<W: Write>(output: &mut W, str: &str) {
    let buffer = str.as_bytes();
    let olen = buffer.len() as i32; // u_strToUTF8 is identity here
    let cs = ui16(olen); // UI16 truncation quirk preserved
    write_raw(output, cs);
    let _ = output.write_all(&buffer[..cs as usize]);
}

// [spec:cg3:def:inlines.cg3.write-utf8-le-fn]
// [spec:cg3:sem:inlines.cg3.write-utf8-le-fn]
// Length prefix written LITTLE-ENDIAN.
pub fn write_utf8_le<W: Write>(output: &mut W, str: &str) {
    let buffer = str.as_bytes();
    let olen = buffer.len() as i32;
    let cs = ui16(olen);
    write_le(output, cs);
    let _ = output.write_all(&buffer[..cs as usize]);
}

// [spec:cg3:def:inlines.cg3.read-utf8-raw-fn]
// [spec:cg3:sem:inlines.cg3.read-utf8-raw-fn]
// Length prefix read RAW (host byte order). ICU decode with ignored status ->
// from_utf8_lossy (malformed bytes -> U+FFFD, matching ICU's substitution).
pub fn read_utf8_raw<R: Read>(input: &mut R) -> UString {
    let len: u16 = read_raw(input);
    let mut buffer = vec![0u8; len as usize];
    let _ = input.read_exact(&mut buffer);
    String::from_utf8_lossy(&buffer).into_owned()
}

// [spec:cg3:def:inlines.cg3.read-utf8-le-fn]
// [spec:cg3:sem:inlines.cg3.read-utf8-le-fn]
// Length prefix read LITTLE-ENDIAN, decoded into the out-param `rv`.
pub fn read_utf8_le<R: Read>(input: &mut R, rv: &mut UString) {
    let len: u16 = read_le(input);
    let mut buffer = vec![0u8; len as usize];
    let _ = input.read_exact(&mut buffer);
    rv.clear();
    *rv = String::from_utf8_lossy(&buffer).into_owned();
}

// Returning convenience overload (`readUTF8_LE(S&) -> UString`).
pub fn read_utf8_le_ret<R: Read>(input: &mut R) -> UString {
    let mut rv = UString::new();
    read_utf8_le(input, &mut rv);
    rv
}

// ---------------------------------------------------------------------------
// Character predicates
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.isdelim-fn]
// [spec:cg3:sem:inlines.cg3.isdelim-fn]
#[inline]
pub fn isdelim(c: UChar) -> bool {
    c == '(' || c == ')' || c == '+' || c == '-' || c == '*' || c == '/' || c == '^' || c == '%' || c == '='
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

// ---------------------------------------------------------------------------
// Misc utilities
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.cg3-quit-fn]
// [spec:cg3:sem:inlines.cg3.cg3-quit-fn]
// [[noreturn]] -> `-> !`. Prints the diagnostic to stderr iff file is Some and
// line != 0, then exits with code `c`.
pub fn cg3_quit(c: i32, file: Option<&str>, line: u32) -> ! {
    if let Some(file) = file {
        if line != 0 {
            eprintln!("CG3Quit triggered from {} line {}.", file, line);
        }
    }
    std::process::exit(c)
}

// [spec:cg3:def:inlines.cg3.usv-fn]
// [spec:cg3:sem:inlines.cg3.usv-fn]
// Only the `USV(UString&)` overload is ported (it simply returns a view); the
// `USV(UnicodeString&)` overload requires ICU and is omitted in Wave 2.
#[inline]
pub fn usv(str: &UString) -> UStringView<'_> {
    str
}

// [spec:cg3:def:inlines.cg3.size-fn]
// [spec:cg3:sem:inlines.cg3.size-fn]
#[inline]
pub const fn size<T, const N: usize>(_: &[T; N]) -> usize {
    N
}

// A container that can report empty-ness and clear itself (stand-in for the C++
// template's implicit `.empty()`/`.clear()` requirement). Other modules may
// impl this for their own container types.
pub trait Clearable {
    fn is_empty_c(&self) -> bool;
    fn clear_c(&mut self);
}

impl<T> Clearable for Vec<T> {
    #[inline]
    fn is_empty_c(&self) -> bool {
        self.is_empty()
    }
    #[inline]
    fn clear_c(&mut self) {
        self.clear();
    }
}

impl Clearable for String {
    #[inline]
    fn is_empty_c(&self) -> bool {
        self.is_empty()
    }
    #[inline]
    fn clear_c(&mut self) {
        self.clear();
    }
}

// [spec:cg3:def:inlines.cg3.clear-fn]
// [spec:cg3:sem:inlines.cg3.clear-fn]
#[inline]
pub fn clear<C: Clearable>(c: &mut C) {
    if !c.is_empty_c() {
        c.clear_c();
    }
}

// [spec:cg3:def:inlines.cg3.is-textual-fn]
// [spec:cg3:sem:inlines.cg3.is-textual-fn]
// Ported over bytes (AsRef<[u8]>): the delimiters compared are all ASCII, so a
// byte-level front/back check is faithful. Panics on empty `s` (C++ front()/
// back() on empty is UB). PARITY: if the last char is multibyte and non-ASCII,
// the last byte != '"'/'>' — same result as the C++ (last code unit != '"').
#[inline]
pub fn is_textual<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    let front = s[0];
    let back = s[s.len() - 1];
    (front == b'"' && back == b'"') || (front == b'<' && back == b'>')
}

// [spec:cg3:def:inlines.cg3.is-internal-fn]
// [spec:cg3:sem:inlines.cg3.is-internal-fn]
#[inline]
pub fn is_internal<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    s[0] == b'_' && s[1] == b'G' && s[2] == b'_'
}

// [spec:cg3:def:inlines.cg3.is-cg3b-fn]
// [spec:cg3:sem:inlines.cg3.is-cg3b-fn]
#[inline]
pub fn is_cg3b<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    s[0] == b'C' && s[1] == b'G' && s[2] == b'3' && s[3] == b'B'
}

// [spec:cg3:def:inlines.cg3.is-cg3bsf-fn]
// [spec:cg3:sem:inlines.cg3.is-cg3bsf-fn]
#[inline]
pub fn is_cg3bsf<S: AsRef<[u8]>>(s: S) -> bool {
    let s = s.as_ref();
    s[0] == b'C' && s[1] == b'G' && s[2] == b'B' && s[3] == b'F'
}

// [spec:cg3:def:inlines.cg3.insert-if-exists-fn]
// [spec:cg3:sem:inlines.cg3.insert-if-exists-fn]
// boost::dynamic_bitset -> Vec<bool> stand-in (bit i == vec[i]); no bitset type
// nor boost/external crate exists in Wave 2. Grows `cont` (zero-fill) then ORs.
pub fn insert_if_exists(cont: &mut Vec<bool>, other: Option<&Vec<bool>>) {
    if let Some(other) = other {
        if !other.is_empty() {
            let newlen = cont.len().max(other.len());
            cont.resize(newlen, false);
            for (i, &bit) in other.iter().enumerate() {
                if bit {
                    cont[i] = true;
                }
            }
        }
    }
}

// [spec:cg3:def:inlines.cg3.g-app-set-opts-ranged-fn]
// [spec:cg3:sem:inlines.cg3.g-app-set-opts-ranged-fn]
// Parses comma-separated numbers/inclusive ranges. `value` (C++ `const char*`)
// -> `&str`; scanning is over its bytes with hand-ported atoi/strchr. `cont` is
// a Vec<u32>. Inclusive ranges use Rust's `low..=high` (empty when high < low,
// matching the uint32 `low <= high` false case for e.g. "3-1").
pub fn g_app_set_opts_ranged(value: &str, cont: &mut Vec<u32>, fill: bool) {
    let vb = value.as_bytes();
    cont.clear();
    let mut had_range = false;

    let mut comma = 0usize;
    loop {
        let low = atoi(vb, comma).unsigned_abs();
        let mut high = low;
        let delim = strchr(vb, comma, b'-');
        let nextc = strchr(vb, comma, b',');
        if let Some(d) = delim {
            if nextc.is_none() || nextc.unwrap() > d {
                had_range = true;
                high = atoi(vb, d + 1).unsigned_abs();
            }
        }
        for v in low..=high {
            cont.push(v);
        }

        // do-while: (comma = strchr(comma,',')) != 0 && ++comma && *comma != 0
        let c = match strchr(vb, comma, b',') {
            Some(c) => c,
            None => break,
        };
        comma = c + 1;
        if !(comma < vb.len() && vb[comma] != 0) {
            break;
        }
    }

    if cont.len() == 1 && !had_range && fill {
        let val = cont[0];
        cont.clear();
        for i in 1..=val {
            cont.push(i);
        }
    }
}

// C `atoi` starting at `start`: skip leading whitespace, optional sign, digits.
fn atoi(s: &[u8], mut i: usize) -> i32 {
    while i < s.len() && (s[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    let mut sign: i64 = 1;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        if s[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let mut n: i64 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        n = n * 10 + (s[i] - b'0') as i64;
        i += 1;
    }
    (sign * n) as i32
}

// C `strchr(s + start, ch)`: index of first `ch` at/after `start`, else None.
fn strchr(s: &[u8], start: usize, ch: u8) -> Option<usize> {
    s[start..].iter().position(|&b| b == ch).map(|p| start + p)
}

// ---------------------------------------------------------------------------
// RAII guards
// ---------------------------------------------------------------------------

// [spec:cg3:def:inlines.cg3.swapper]
pub struct swapper<'a, T> {
    cond: bool,
    a: &'a mut T,
    b: &'a mut T,
}

impl<'a, T> swapper<'a, T> {
    // [spec:cg3:def:inlines.cg3.swapper.swapper-fn]
    // [spec:cg3:sem:inlines.cg3.swapper.swapper-fn]
    pub fn new(cond: bool, a: &'a mut T, b: &'a mut T) -> swapper<'a, T> {
        if cond {
            std::mem::swap(a, b);
        }
        swapper { cond, a, b }
    }
}

impl<'a, T> Drop for swapper<'a, T> {
    fn drop(&mut self) {
        if self.cond {
            std::mem::swap(&mut *self.a, &mut *self.b);
        }
    }
}

// [spec:cg3:def:inlines.cg3.swapper-false]
// The C++ nests a `swapper<bool>` over an internal `val`, which is
// self-referential (swp borrows val) and cannot be expressed in safe Rust. The
// equivalent NET effect is implemented directly: while alive (cond true), `b`
// is held at false and restored to its original value on drop.
pub struct swapper_false<'a> {
    cond: bool,
    b: &'a mut bool,
    old: bool,
}

impl<'a> swapper_false<'a> {
    // [spec:cg3:def:inlines.cg3.swapper-false.swapper-false-fn]
    // [spec:cg3:sem:inlines.cg3.swapper-false.swapper-false-fn]
    pub fn new(cond: bool, b: &'a mut bool) -> swapper_false<'a> {
        let old = *b;
        if cond {
            *b = false;
        }
        swapper_false { cond, b, old }
    }
}

impl<'a> Drop for swapper_false<'a> {
    fn drop(&mut self) {
        if self.cond {
            *self.b = self.old;
        }
    }
}

// [spec:cg3:def:inlines.cg3.uncond-swap]
pub struct uncond_swap<'a, T> {
    a: &'a mut T,
    b: T,
}

impl<'a, T> uncond_swap<'a, T> {
    // [spec:cg3:def:inlines.cg3.uncond-swap.uncond-swap-fn]
    // [spec:cg3:sem:inlines.cg3.uncond-swap.uncond-swap-fn]
    // `b` is taken by value (a copy/move); after construction `a` holds the
    // passed value and `b_` holds a's original.
    pub fn new(a: &'a mut T, mut b: T) -> uncond_swap<'a, T> {
        std::mem::swap(a, &mut b);
        uncond_swap { a, b }
    }
}

impl<'a, T> Drop for uncond_swap<'a, T> {
    fn drop(&mut self) {
        std::mem::swap(&mut *self.a, &mut self.b);
    }
}

// Provides `++`/`--` for the inc_dec counter guard.
pub trait Incrementable {
    fn increment(&mut self);
    fn decrement(&mut self);
}

macro_rules! impl_incrementable {
    ($($t:ty),*) => {$(
        impl Incrementable for $t {
            #[inline] fn increment(&mut self) { *self += 1; }
            #[inline] fn decrement(&mut self) { *self -= 1; }
        }
    )*};
}
impl_incrementable!(i8, u8, i16, u16, i32, u32, i64, u64, isize, usize);

// [spec:cg3:def:inlines.cg3.inc-dec]
pub struct inc_dec<'a, T: Incrementable> {
    p: Option<&'a mut T>,
}

impl<'a, T: Incrementable> inc_dec<'a, T> {
    pub fn new() -> inc_dec<'a, T> {
        inc_dec { p: None }
    }

    // [spec:cg3:def:inlines.cg3.inc-dec.inc-fn]
    // [spec:cg3:sem:inlines.cg3.inc-dec.inc-fn]
    pub fn inc(&mut self, pt: &'a mut T) {
        pt.increment();
        self.p = Some(pt);
    }
}

impl<'a, T: Incrementable> Default for inc_dec<'a, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T: Incrementable> Drop for inc_dec<'a, T> {
    // [spec:cg3:def:inlines.cg3.inc-dec.inc-dec-fn]
    // [spec:cg3:sem:inlines.cg3.inc-dec.inc-dec-fn]
    fn drop(&mut self) {
        if let Some(p) = self.p.as_mut() {
            p.decrement();
        }
    }
}

// [spec:cg3:def:inlines.cg3.scope-guard]
// `std::function<void()>` -> `Box<dyn FnMut() + 'a>`. The C++ "empty func +
// good -> throws bad_function_call" case does not arise: a callable is always
// supplied at construction.
pub struct scope_guard<'a> {
    func: Box<dyn FnMut() + 'a>,
    good: bool,
}

impl<'a> scope_guard<'a> {
    pub fn new<F: FnMut() + 'a>(func: F) -> scope_guard<'a> {
        scope_guard {
            func: Box::new(func),
            good: true,
        }
    }

    // [spec:cg3:def:inlines.cg3.scope-guard.set-fn]
    // [spec:cg3:sem:inlines.cg3.scope-guard.set-fn]
    // C++ default `val = true`; caller passes explicitly (no Rust default args).
    pub fn set(&mut self, val: bool) {
        self.good = val;
    }
}

impl<'a> Drop for scope_guard<'a> {
    // [spec:cg3:def:inlines.cg3.scope-guard.scope-guard-fn]
    // [spec:cg3:sem:inlines.cg3.scope-guard.scope-guard-fn]
    fn drop(&mut self) {
        if self.good {
            (self.func)();
        }
    }
}

// ---------------------------------------------------------------------------
// Linked-list reverse, reversed-range iteration, erase, make_array, concat
// ---------------------------------------------------------------------------

// A node linked through a public `->next` raw pointer.
pub trait Linked {
    fn next(&self) -> *mut Self;
    fn set_next(&mut self, n: *mut Self);
}

// [spec:cg3:def:inlines.cg3.reverse-fn]
// [spec:cg3:sem:inlines.cg3.reverse-fn]
// In-place singly-linked-list reversal. Inherently raw-pointer work, so `unsafe`.
///
/// # Safety
/// `head` must be null or a valid pointer to a `Linked` node whose `->next`
/// chain is valid and non-cyclic.
pub unsafe fn reverse<T: Linked>(mut head: *mut T) -> *mut T {
    let mut nr: *mut T = std::ptr::null_mut();
    while !head.is_null() {
        let next = unsafe { (*head).next() };
        unsafe { (*head).set_next(nr) };
        nr = head;
        head = next;
    }
    nr
}

// [spec:cg3:def:inlines.cg3.reversed]
pub struct Reversed<'a, T: ?Sized> {
    pub t: &'a T,
}

// [spec:cg3:def:inlines.cg3.reversed-fn]
// [spec:cg3:sem:inlines.cg3.reversed-fn]
pub fn reversed<T: ?Sized>(c: &T) -> Reversed<'_, T> {
    Reversed { t: c }
}

// [spec:cg3:def:inlines.cg3.begin-fn]
// [spec:cg3:sem:inlines.cg3.begin-fn]
// C++ ADL `begin(Reversed<T>)` -> `std::rbegin`. Rust returns a reverse
// iterator over the wrapped container.
pub fn begin<'a, T>(c: Reversed<'a, T>) -> std::iter::Rev<<&'a T as IntoIterator>::IntoIter>
where
    &'a T: IntoIterator,
    <&'a T as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    c.t.into_iter().rev()
}

// [spec:cg3:def:inlines.cg3.end-fn]
// [spec:cg3:sem:inlines.cg3.end-fn]
// C++ ADL `end(Reversed<T>)` -> `std::rend`. Rust has no separate rend type in
// this scheme; an exhausted reverse iterator stands in for the past-the-reverse-
// end position. Prefer the `IntoIterator` impl (below) for real iteration.
pub fn end<'a, T>(c: Reversed<'a, T>) -> std::iter::Rev<<&'a T as IntoIterator>::IntoIter>
where
    &'a T: IntoIterator,
    <&'a T as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    let mut it = c.t.into_iter().rev();
    while it.next().is_some() {}
    it
}

// Makes `for x in reversed(&container)` iterate in reverse (the actual intent of
// the C++ begin/end ADL pair).
impl<'a, T: ?Sized> IntoIterator for Reversed<'a, T>
where
    &'a T: IntoIterator,
    <&'a T as IntoIterator>::IntoIter: DoubleEndedIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = std::iter::Rev<<&'a T as IntoIterator>::IntoIter>;
    fn into_iter(self) -> Self::IntoIter {
        self.t.into_iter().rev()
    }
}

// [spec:cg3:def:inlines.cg3.erase-fn]
// [spec:cg3:sem:inlines.cg3.erase-fn]
// Erase-remove idiom over Vec<T>: removes ALL elements equal to `val`.
#[inline]
pub fn erase<T: PartialEq>(cont: &mut Vec<T>, val: &T) {
    cont.retain(|x| x != val);
}

// [spec:cg3:def:inlines.cg3.make-array-helper-fn]
// [spec:cg3:sem:inlines.cg3.make-array-helper-fn]
// The C++ compile-time `std::index_sequence` expansion becomes `array::from_fn`,
// which calls `f(0), f(1), ..., f(N-1)` in order.
#[inline]
pub fn make_array_helper<const N: usize, R, F: Fn(usize) -> R>(f: F) -> [R; N] {
    std::array::from_fn(|i| f(i))
}

// [spec:cg3:def:inlines.cg3.make-array-fn]
// [spec:cg3:sem:inlines.cg3.make-array-fn]
#[inline]
pub fn make_array<const N: usize, R, F: Fn(usize) -> R>(f: F) -> [R; N] {
    make_array_helper::<N, R, F>(f)
}

pub mod details {
    // [spec:cg3:def:inlines.cg3.details.concat-fn]
    // [spec:cg3:sem:inlines.cg3.details.concat-fn]
    // C++ variadic recursion -> a slice of pieces appended in order.
    pub fn _concat(msg: &mut String, args: &[&str]) {
        for a in args {
            msg.push_str(a);
        }
    }
}

// [spec:cg3:def:inlines.cg3.concat-fn]
// [spec:cg3:sem:inlines.cg3.concat-fn]
// Variadic string builder. Rust has no variadics: the first argument is `value`
// and the remaining pieces are passed as a slice `args`.
pub fn concat(value: &str, args: &[&str]) -> String {
    let mut msg = String::from(value);
    details::_concat(&mut msg, args);
    msg
}
