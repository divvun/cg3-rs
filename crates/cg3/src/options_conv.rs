//! Port of `src/options_conv.hpp` (`OptionsConv::options_conv`).
//!
//! The cg-conv CLI option enum ([`Opt`]) and its `ArgOption` table (plus the
//! default/override copies). Like [`crate::options`], each table entry is
//! indexed by its matching [`Opt`] enumerator, so the table order matches
//! the enum order.
//!
//! cg-conv shares vislcg3's [`ArgOption`] entry type, from [`crate::options`].
//!
//! ## Global-vs-function (NOTE)
//! `options_conv.hpp` defines `options_conv` and its four `inline auto` copies
//! (`options_default`, `options_override`, `grammar_options_default`,
//! `grammar_options_override`) as mutable globals; as in [`crate::options`],
//! they are exposed here as constructor functions returning fresh arrays.

use crate::options::{ArgOption, HasArg};

// [spec:cg3:def:options-conv.options-conv.options]
/// C++ `enum OPTIONS` (`options_conv.hpp`); each variant camel-cases its C++
/// enumerator (`NUM_OPTIONS_CONV` → `NumOptionsConv`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    Help1,
    Help2,
    MappingPrefix,
    InAuto,
    InCg,
    InCg2,
    InNiceline,
    InApertium,
    InFst,
    InPlain,
    InJsonl,
    InBinary,
    AddTags,
    OutCg,
    OutCg2,
    OutApertium,
    OutFst,
    OutNiceline,
    OutPlain,
    OutJsonl,
    OutBinary,
    FstWfactor,
    FstWtag,
    SubDelimiter,
    SubRtl,
    SubLtr,
    Ordered,
    ParseDep,
    DepDelimit,
    UnicodeTags,
    PipeDeleted,
    NoBreak,
    NumOptionsConv,
}

/// C++ `options_conv_t`: one [`ArgOption`] per [`Opt`].
pub type ConvOptionsTable = [ArgOption; Opt::NumOptionsConv as usize];

/// The base `OptionsConv::options_conv` table (indexed by [`Opt`]).
pub fn options_conv() -> ConvOptionsTable {
    [
        ArgOption::new("help", 'h', HasArg::No, "shows this help"),
        ArgOption::new("?", '?', HasArg::No, "shows this help"),
        ArgOption::new(
            "prefix",
            'p',
            HasArg::Required,
            "sets the mapping prefix; defaults to @",
        ),
        ArgOption::new(
            "in-auto",
            'u',
            HasArg::No,
            "auto-detect input format (default)",
        ),
        ArgOption::new("in-cg", 'c', HasArg::No, "sets input format to CG"),
        ArgOption::hidden("v", 'v', HasArg::No),
        ArgOption::new(
            "in-niceline",
            'n',
            HasArg::No,
            "sets input format to Niceline CG",
        ),
        ArgOption::new(
            "in-apertium",
            'a',
            HasArg::No,
            "sets input format to Apertium",
        ),
        ArgOption::new("in-fst", 'f', HasArg::No, "sets input format to HFST/XFST"),
        ArgOption::new(
            "in-plain",
            'x',
            HasArg::No,
            "sets input format to plain text",
        ),
        ArgOption::new(
            "in-jsonl",
            'j',
            HasArg::No,
            "sets input format to JSONL (experimental, specs below)",
        ),
        ArgOption::new(
            "in-binary",
            'z',
            HasArg::No,
            "sets input format to binary (experimental)",
        ),
        ArgOption::new(
            "add-tags",
            '\0',
            HasArg::No,
            "adds minimal analysis to readings (implies -x)",
        ),
        ArgOption::new(
            "out-cg",
            'C',
            HasArg::No,
            "sets output format to CG (default)",
        ),
        ArgOption::hidden("V", 'V', HasArg::No),
        ArgOption::new(
            "out-apertium",
            'A',
            HasArg::No,
            "sets output format to Apertium",
        ),
        ArgOption::new(
            "out-fst",
            'F',
            HasArg::No,
            "sets output format to HFST/XFST",
        ),
        ArgOption::new(
            "out-niceline",
            'N',
            HasArg::No,
            "sets output format to Niceline CG",
        ),
        ArgOption::new(
            "out-plain",
            'X',
            HasArg::No,
            "sets output format to plain text",
        ),
        ArgOption::new(
            "out-jsonl",
            'J',
            HasArg::No,
            "sets output format to JSONL (experimental, specs below)",
        ),
        ArgOption::new(
            "out-binary",
            'Z',
            HasArg::No,
            "sets output format to binary (experimental)",
        ),
        ArgOption::new(
            "wfactor",
            'W',
            HasArg::Required,
            "FST weight factor (defaults to 1.0)",
        ),
        ArgOption::new(
            "wtag",
            '\0',
            HasArg::Required,
            "FST weight tag prefix (defaults to W)",
        ),
        ArgOption::new(
            "sub-delim",
            'S',
            HasArg::Required,
            "FST sub-reading delimiters (defaults to #)",
        ),
        ArgOption::new(
            "rtl",
            'r',
            HasArg::No,
            "sets sub-reading direction to RTL (default)",
        ),
        ArgOption::new("ltr", 'l', HasArg::No, "sets sub-reading direction to LTR"),
        ArgOption::new("ordered", 'o', HasArg::No, "tag order matters mode"),
        ArgOption::new(
            "parse-dep",
            'D',
            HasArg::No,
            "parse dependency (defaults to treating as normal tags)",
        ),
        ArgOption::new(
            "dep-delimit",
            '\0',
            HasArg::Optional,
            "delimit windows based on dependency; defaults to 10",
        ),
        ArgOption::new(
            "unicode-tags",
            '\0',
            HasArg::No,
            "outputs Unicode code points for things like ->",
        ),
        ArgOption::new(
            "deleted",
            '\0',
            HasArg::No,
            "read deleted readings as such, instead of as text",
        ),
        ArgOption::new(
            "no-break",
            'B',
            HasArg::No,
            "inhibits any extra whitespace in output",
        ),
    ]
}

// `inline auto options_default = options_conv;` and the other copies.
pub fn options_default() -> ConvOptionsTable {
    options_conv()
}
pub fn options_override() -> ConvOptionsTable {
    options_conv()
}
pub fn grammar_options_default() -> ConvOptionsTable {
    options_conv()
}
pub fn grammar_options_override() -> ConvOptionsTable {
    options_conv()
}
