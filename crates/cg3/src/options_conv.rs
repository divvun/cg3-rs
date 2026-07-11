//! Port of `src/options_conv.hpp` (`OptionsConv::options_conv`).
//!
//! The cg-conv CLI option enum ([`OPTIONS`]) and its `UOption` table (plus the
//! default/override copies). Like [`crate::options`], each table entry is
//! indexed by its matching [`OPTIONS`] enumerator, so the table order matches
//! the enum order.
//!
//! The C++ header does `using ::Options::UOption;` (and the `UOPT_*` values) —
//! cg-conv reuses the exact same [`UOption`] type as vislcg3. The port mirrors
//! that by importing it from [`crate::options`].
//!
//! ## Global-vs-function (NOTE)
//! `options_conv.hpp` defines `options_conv` and its four `inline auto` copies
//! (`options_default`, `options_override`, `grammar_options_default`,
//! `grammar_options_override`) as mutable globals; as in [`crate::options`],
//! they are exposed here as constructor functions returning fresh arrays.

use crate::options::{UOPT_NO_ARG, UOPT_OPTIONAL_ARG, UOPT_REQUIRES_ARG, UOption};
use crate::types::UChar;

/// Local four/three-field `UOption` aggregate-init helpers, mirroring the ones
/// in [`crate::options`] (kept private there).
fn uo(long: &'static str, short: UChar, has_arg: u8, desc: &'static str) -> UOption {
    UOption {
        long_name: Some(long),
        short_name: short,
        has_arg,
        description: desc.to_string(),
        does_occur: false,
        value: String::new(),
    }
}
fn uo3(long: &'static str, short: UChar, has_arg: u8) -> UOption {
    UOption {
        long_name: Some(long),
        short_name: short,
        has_arg,
        description: String::new(),
        does_occur: false,
        value: String::new(),
    }
}

// [spec:cg3:def:options-conv.options-conv.options]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OPTIONS {
    HELP1,
    HELP2,
    MAPPING_PREFIX,
    IN_AUTO,
    IN_CG,
    IN_CG2,
    IN_NICELINE,
    IN_APERTIUM,
    IN_FST,
    IN_PLAIN,
    IN_JSONL,
    IN_BINARY,
    ADD_TAGS,
    OUT_CG,
    OUT_CG2,
    OUT_APERTIUM,
    OUT_FST,
    OUT_MATXIN,
    OUT_NICELINE,
    OUT_PLAIN,
    OUT_JSONL,
    OUT_BINARY,
    FST_WFACTOR,
    FST_WTAG,
    SUB_DELIMITER,
    SUB_RTL,
    SUB_LTR,
    ORDERED,
    PARSE_DEP,
    DEP_DELIMIT,
    UNICODE_TAGS,
    PIPE_DELETED,
    NO_BREAK,
    NUM_OPTIONS_CONV,
}

/// `std::array<UOption, NUM_OPTIONS_CONV>`.
pub type options_conv_t = [UOption; OPTIONS::NUM_OPTIONS_CONV as usize];

/// The base `OptionsConv::options_conv` table (indexed by [`OPTIONS`]).
pub fn options_conv() -> options_conv_t {
    [
        uo("help", 'h', UOPT_NO_ARG, "shows this help"),
        uo("?", '?', UOPT_NO_ARG, "shows this help"),
        uo(
            "prefix",
            'p',
            UOPT_REQUIRES_ARG,
            "sets the mapping prefix; defaults to @",
        ),
        uo(
            "in-auto",
            'u',
            UOPT_NO_ARG,
            "auto-detect input format (default)",
        ),
        uo("in-cg", 'c', UOPT_NO_ARG, "sets input format to CG"),
        uo3("v", 'v', UOPT_NO_ARG),
        uo(
            "in-niceline",
            'n',
            UOPT_NO_ARG,
            "sets input format to Niceline CG",
        ),
        uo(
            "in-apertium",
            'a',
            UOPT_NO_ARG,
            "sets input format to Apertium",
        ),
        uo("in-fst", 'f', UOPT_NO_ARG, "sets input format to HFST/XFST"),
        uo(
            "in-plain",
            'x',
            UOPT_NO_ARG,
            "sets input format to plain text",
        ),
        uo(
            "in-jsonl",
            'j',
            UOPT_NO_ARG,
            "sets input format to JSONL (experimental, specs below)",
        ),
        uo(
            "in-binary",
            'z',
            UOPT_NO_ARG,
            "sets input format to binary (experimental)",
        ),
        uo(
            "add-tags",
            '\0',
            UOPT_NO_ARG,
            "adds minimal analysis to readings (implies -x)",
        ),
        uo(
            "out-cg",
            'C',
            UOPT_NO_ARG,
            "sets output format to CG (default)",
        ),
        uo3("V", 'V', UOPT_NO_ARG),
        uo(
            "out-apertium",
            'A',
            UOPT_NO_ARG,
            "sets output format to Apertium",
        ),
        uo(
            "out-fst",
            'F',
            UOPT_NO_ARG,
            "sets output format to HFST/XFST",
        ),
        uo(
            "out-matxin",
            'M',
            UOPT_NO_ARG,
            "sets output format to Matxin",
        ),
        uo(
            "out-niceline",
            'N',
            UOPT_NO_ARG,
            "sets output format to Niceline CG",
        ),
        uo(
            "out-plain",
            'X',
            UOPT_NO_ARG,
            "sets output format to plain text",
        ),
        uo(
            "out-jsonl",
            'J',
            UOPT_NO_ARG,
            "sets output format to JSONL (experimental, specs below)",
        ),
        uo(
            "out-binary",
            'Z',
            UOPT_NO_ARG,
            "sets output format to binary (experimental)",
        ),
        uo(
            "wfactor",
            'W',
            UOPT_REQUIRES_ARG,
            "FST weight factor (defaults to 1.0)",
        ),
        uo(
            "wtag",
            '\0',
            UOPT_REQUIRES_ARG,
            "FST weight tag prefix (defaults to W)",
        ),
        uo(
            "sub-delim",
            'S',
            UOPT_REQUIRES_ARG,
            "FST sub-reading delimiters (defaults to #)",
        ),
        uo(
            "rtl",
            'r',
            UOPT_NO_ARG,
            "sets sub-reading direction to RTL (default)",
        ),
        uo("ltr", 'l', UOPT_NO_ARG, "sets sub-reading direction to LTR"),
        uo("ordered", 'o', UOPT_NO_ARG, "tag order matters mode"),
        uo(
            "parse-dep",
            'D',
            UOPT_NO_ARG,
            "parse dependency (defaults to treating as normal tags)",
        ),
        uo(
            "dep-delimit",
            '\0',
            UOPT_OPTIONAL_ARG,
            "delimit windows based on dependency; defaults to 10",
        ),
        uo(
            "unicode-tags",
            '\0',
            UOPT_NO_ARG,
            "outputs Unicode code points for things like ->",
        ),
        uo(
            "deleted",
            '\0',
            UOPT_NO_ARG,
            "read deleted readings as such, instead of as text",
        ),
        uo(
            "no-break",
            'B',
            UOPT_NO_ARG,
            "inhibits any extra whitespace in output",
        ),
    ]
}

// `inline auto options_default = options_conv;` and the other copies.
pub fn options_default() -> options_conv_t {
    options_conv()
}
pub fn options_override() -> options_conv_t {
    options_conv()
}
pub fn grammar_options_default() -> options_conv_t {
    options_conv()
}
pub fn grammar_options_override() -> options_conv_t {
    options_conv()
}
