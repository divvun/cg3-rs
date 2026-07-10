//! Core type aliases for the cg3 port.
//!
//! **UTF-8 throughout.** The C++ `UString` / `UChar` (ICU UTF-16) become Rust
//! `String` / `char` (Unicode scalars). There is no UTF-16 representation;
//! byte- vs codepoint-level scanning is decided per call site during
//! translation. Regex (Wave 2) uses the `regex` crate with parity checks.

// [spec:cg3:def:stdafx.cg3.u-string]
/// C++ `UString` (`std::basic_string<UChar>`) → owned UTF-8 string.
pub type UString = String;

// [spec:cg3:def:stdafx.cg3.u-string-view]
/// C++ `UStringView` (`std::basic_string_view<UChar>`) → borrowed UTF-8 slice.
pub type UStringView<'a> = &'a str;

// [spec:cg3:def:stdafx.cg3.u-string-vector]
/// C++ `UStringVector` (`std::vector<UString>`) → vector of owned UTF-8 strings.
pub type UStringVector = Vec<String>;

// [spec:cg3:def:stdafx.cg3.uint32-vector]
/// C++ `uint32Vector` (`std::vector<uint32_t>`).
pub type Uint32Vector = Vec<u32>;

// [spec:cg3:def:stdafx.cg3.flags-t]
/// C++ `flags_t` (`boost::dynamic_bitset<>`). Stand-in: a growable bit vector as
/// `Vec<bool>` for Wave 2; a packed bitset (or the `bitvec` crate) is a Wave 4
/// concern. Used for per-rule/per-set active masks (e.g. `Grammar::sets_any`).
pub type flags_t = Vec<bool>;

/// The UTF-8 analog of ICU's `UChar`: a single Unicode scalar value.
///
/// The C++ `UChar` is a UTF-16 code unit; in this UTF-8 port a `char` is the
/// closest faithful unit for the code that treats it as "one character".
/// Code that scanned raw `UChar*` buffers becomes `&str` / `char` iteration.
pub type UChar = char;
