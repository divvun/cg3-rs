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

mod hashing;
mod io;
mod misc;
mod scan;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
pub use hashing::*;
pub use io::*;
pub use misc::*;
pub use scan::*;

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
