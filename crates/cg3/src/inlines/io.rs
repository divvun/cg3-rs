//! `inlines.hpp` — byte IO (readRaw/writeRaw/readBE/writeBE/readLE/writeLE) and UTF-8 length-prefixed IO.
//!
//! Split out of the wave-2 monolithic `inlines.rs` (wave 4, w4-file-split-fmt).

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::types::UString;
use std::io::{Read, Write};

use super::*;

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
    let exp = read_be::<i32, R>(stream);

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
