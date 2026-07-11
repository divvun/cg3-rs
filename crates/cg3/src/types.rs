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

// ---------------------------------------------------------------------------
// Wave 4 (w4-sentinels-newtypes): domain newtypes over the raw `uint32_t`
// identity values that the C++ passed around untyped. `#[repr(transparent)]`
// keeps them layout-identical to `u32`, and `.get()` / `.0` extract the raw
// value at the wire read/write boundary so the `.cg3b` / pipe / JSONL byte
// streams are unchanged. They make "a tag hash", "a set number", and "a global
// cohort number" distinct types so they can't be crossed by accident.
// ---------------------------------------------------------------------------
macro_rules! value_newtype {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
        #[repr(transparent)]
        pub struct $name(pub u32);

        impl $name {
            /// The underlying `u32` (for wire I/O and raw-keyed containers).
            #[inline]
            pub const fn get(self) -> u32 {
                self.0
            }

            /// Wrapping `+ n`, staying in the domain (C++ `++global_number`).
            #[inline]
            pub const fn wrapping_add(self, n: u32) -> Self {
                Self(self.0.wrapping_add(n))
            }
        }

        impl std::fmt::Display for $name {
            #[inline]
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

value_newtype!(
    /// A tag's `SuperFastHash` value (C++ `Tag::hash`, `Reading::baseform`, …).
    /// `0` is never a real hash, so `Option<TagHash>` models "no tag".
    TagHash
);
value_newtype!(
    /// A set's dense list number (C++ `Set::number`), as serialized in `.cg3b`.
    SetNumber
);
value_newtype!(
    /// A cohort's monotonically-increasing global number (C++
    /// `Cohort::global_number` / `dep_self` / `dep_parent`).
    GlobalNumber
);
