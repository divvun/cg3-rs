//! Port of the specced parts of `src/Strings.hpp`.
//!
//! `Strings.hpp` is mostly keyword enums plus name-string tables. The `KEYWORDS`
//! enum is covered by the spec (`docs/spec/port/src/Strings.md`) and ported
//! verbatim; the remaining enums, name tables (`keywords[]`, `g_flags[]`,
//! `stringbits[]`), the `STR_*` string constants, and the size/misc constants
//! are ported here as data so the rest of the crate can reference their exact
//! literal text and indexing.

// [spec:cg3:def:strings.cg3.keywords]
// C++ `enum KEYWORDS : uint32_t { K_IGNORE, ..., KEYWORD_COUNT }`.
// `K_IGNORE` is 0 and each subsequent variant auto-increments; `KEYWORD_COUNT`
// is the trailing sentinel equal to the number of real keywords (72).
// The exact `K_*` identifiers are preserved (they name grammar keywords), so
// the non-camel-case lint is allowed on this type.
#[allow(non_camel_case_types)]
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KEYWORDS {
    K_IGNORE = 0,
    K_SETS,
    K_LIST,
    K_SET,
    K_DELIMITERS,
    K_SOFT_DELIMITERS,
    K_PREFERRED_TARGETS,
    K_MAPPING_PREFIX,
    K_MAPPINGS,
    K_CONSTRAINTS,
    K_CORRECTIONS,
    K_SECTION,
    K_BEFORE_SECTIONS,
    K_AFTER_SECTIONS,
    K_NULL_SECTION,
    K_ADD,
    K_MAP,
    K_REPLACE,
    K_SELECT,
    K_REMOVE,
    K_IFF,
    K_APPEND,
    K_SUBSTITUTE,
    K_START,
    K_END,
    K_ANCHOR,
    K_EXECUTE,
    K_JUMP,
    K_REMVARIABLE,
    K_SETVARIABLE,
    K_DELIMIT,
    K_MATCH,
    K_SETPARENT,
    K_SETCHILD,
    K_ADDRELATION,
    K_SETRELATION,
    K_REMRELATION,
    K_ADDRELATIONS,
    K_SETRELATIONS,
    K_REMRELATIONS,
    K_TEMPLATE,
    K_MOVE,
    K_MOVE_AFTER,
    K_MOVE_BEFORE,
    K_SWITCH,
    K_REMCOHORT,
    K_STATIC_SETS,
    K_UNMAP,
    K_COPY,
    K_ADDCOHORT,
    K_ADDCOHORT_AFTER,
    K_ADDCOHORT_BEFORE,
    K_EXTERNAL,
    K_EXTERNAL_ONCE,
    K_EXTERNAL_ALWAYS,
    K_OPTIONS,
    K_STRICT_TAGS,
    K_REOPEN_MAPPINGS,
    K_SUBREADINGS,
    K_SPLITCOHORT,
    K_PROTECT,
    K_UNPROTECT,
    K_MERGECOHORTS,
    K_RESTORE,
    K_WITH,
    K_OLIST,
    K_OSET,
    K_CMDARGS,
    K_CMDARGS_OVERRIDE,
    K_COPYCOHORT,
    K_REMPARENT,
    K_SWITCHPARENT,
    KEYWORD_COUNT,
}

/// Every `KEYWORDS` variant indexed by discriminant — the safe lookup
/// backing `keywords_from_u32` (no transmute).
pub const KEYWORDS_BY_ID: [KEYWORDS; KEYWORDS::KEYWORD_COUNT as usize] = [
    KEYWORDS::K_IGNORE,
    KEYWORDS::K_SETS,
    KEYWORDS::K_LIST,
    KEYWORDS::K_SET,
    KEYWORDS::K_DELIMITERS,
    KEYWORDS::K_SOFT_DELIMITERS,
    KEYWORDS::K_PREFERRED_TARGETS,
    KEYWORDS::K_MAPPING_PREFIX,
    KEYWORDS::K_MAPPINGS,
    KEYWORDS::K_CONSTRAINTS,
    KEYWORDS::K_CORRECTIONS,
    KEYWORDS::K_SECTION,
    KEYWORDS::K_BEFORE_SECTIONS,
    KEYWORDS::K_AFTER_SECTIONS,
    KEYWORDS::K_NULL_SECTION,
    KEYWORDS::K_ADD,
    KEYWORDS::K_MAP,
    KEYWORDS::K_REPLACE,
    KEYWORDS::K_SELECT,
    KEYWORDS::K_REMOVE,
    KEYWORDS::K_IFF,
    KEYWORDS::K_APPEND,
    KEYWORDS::K_SUBSTITUTE,
    KEYWORDS::K_START,
    KEYWORDS::K_END,
    KEYWORDS::K_ANCHOR,
    KEYWORDS::K_EXECUTE,
    KEYWORDS::K_JUMP,
    KEYWORDS::K_REMVARIABLE,
    KEYWORDS::K_SETVARIABLE,
    KEYWORDS::K_DELIMIT,
    KEYWORDS::K_MATCH,
    KEYWORDS::K_SETPARENT,
    KEYWORDS::K_SETCHILD,
    KEYWORDS::K_ADDRELATION,
    KEYWORDS::K_SETRELATION,
    KEYWORDS::K_REMRELATION,
    KEYWORDS::K_ADDRELATIONS,
    KEYWORDS::K_SETRELATIONS,
    KEYWORDS::K_REMRELATIONS,
    KEYWORDS::K_TEMPLATE,
    KEYWORDS::K_MOVE,
    KEYWORDS::K_MOVE_AFTER,
    KEYWORDS::K_MOVE_BEFORE,
    KEYWORDS::K_SWITCH,
    KEYWORDS::K_REMCOHORT,
    KEYWORDS::K_STATIC_SETS,
    KEYWORDS::K_UNMAP,
    KEYWORDS::K_COPY,
    KEYWORDS::K_ADDCOHORT,
    KEYWORDS::K_ADDCOHORT_AFTER,
    KEYWORDS::K_ADDCOHORT_BEFORE,
    KEYWORDS::K_EXTERNAL,
    KEYWORDS::K_EXTERNAL_ONCE,
    KEYWORDS::K_EXTERNAL_ALWAYS,
    KEYWORDS::K_OPTIONS,
    KEYWORDS::K_STRICT_TAGS,
    KEYWORDS::K_REOPEN_MAPPINGS,
    KEYWORDS::K_SUBREADINGS,
    KEYWORDS::K_SPLITCOHORT,
    KEYWORDS::K_PROTECT,
    KEYWORDS::K_UNPROTECT,
    KEYWORDS::K_MERGECOHORTS,
    KEYWORDS::K_RESTORE,
    KEYWORDS::K_WITH,
    KEYWORDS::K_OLIST,
    KEYWORDS::K_OSET,
    KEYWORDS::K_CMDARGS,
    KEYWORDS::K_CMDARGS_OVERRIDE,
    KEYWORDS::K_COPYCOHORT,
    KEYWORDS::K_REMPARENT,
    KEYWORDS::K_SWITCHPARENT,
];


// C++ `enum : uint32_t` of set-operator / string-bit codes. Sparse: several
// values are explicitly assigned and the gaps are unnamed. These index into
// `STRINGBITS` below.
pub const S_IGNORE: u32 = 0;
pub const S_OR: u32 = 3;
pub const S_PLUS: u32 = 4;
pub const S_MINUS: u32 = 5;
pub const S_FAILFAST: u32 = 8;
pub const S_SET_DIFF: u32 = 9;
pub const S_SET_ISECT_U: u32 = 10;
pub const S_SET_SYMDIFF_U: u32 = 11;

// C++ `enum : uint32_t` of rule flags. This must be kept in lock-step with
// `Rule.hpp`'s `RULE_FLAGS`. These index into `G_FLAGS` below.
pub const FL_NEAREST: u32 = 0;
pub const FL_ALLOWLOOP: u32 = 1;
pub const FL_DELAYED: u32 = 2;
pub const FL_IMMEDIATE: u32 = 3;
pub const FL_LOOKDELETED: u32 = 4;
pub const FL_LOOKDELAYED: u32 = 5;
pub const FL_UNSAFE: u32 = 6;
pub const FL_SAFE: u32 = 7;
pub const FL_REMEMBERX: u32 = 8;
pub const FL_RESETX: u32 = 9;
pub const FL_KEEPORDER: u32 = 10;
pub const FL_VARYORDER: u32 = 11;
pub const FL_ENCL_INNER: u32 = 12;
pub const FL_ENCL_OUTER: u32 = 13;
pub const FL_ENCL_FINAL: u32 = 14;
pub const FL_ENCL_ANY: u32 = 15;
pub const FL_ALLOWCROSS: u32 = 16;
pub const FL_WITHCHILD: u32 = 17;
pub const FL_NOCHILD: u32 = 18;
pub const FL_ITERATE: u32 = 19;
pub const FL_NOITERATE: u32 = 20;
pub const FL_UNMAPLAST: u32 = 21;
pub const FL_REVERSE: u32 = 22;
pub const FL_SUB: u32 = 23;
pub const FL_OUTPUT: u32 = 24;
pub const FL_CAPTURE_UNIF: u32 = 25;
pub const FL_REPEAT: u32 = 26;
pub const FL_BEFORE: u32 = 27;
pub const FL_AFTER: u32 = 28;
pub const FL_IGNORED: u32 = 29;
pub const FL_LOOKIGNORED: u32 = 30;
pub const FL_NOMAPPED: u32 = 31;
pub const FL_NOPARENT: u32 = 32;
pub const FL_DETACH: u32 = 33;
pub const FLAGS_COUNT: usize = 34;

/// Rule-flag names, indexed by the `FL_*` constants. (`g_flags[FLAGS_COUNT]`)
pub static G_FLAGS: [&str; FLAGS_COUNT] = [
    "NEAREST",
    "ALLOWLOOP",
    "DELAYED",
    "IMMEDIATE",
    "LOOKDELETED",
    "LOOKDELAYED",
    "UNSAFE",
    "SAFE",
    "REMEMBERX",
    "RESETX",
    "KEEPORDER",
    "VARYORDER",
    "ENCL_INNER",
    "ENCL_OUTER",
    "ENCL_FINAL",
    "ENCL_ANY",
    "ALLOWCROSS",
    "WITHCHILD",
    "NOCHILD",
    "ITERATE",
    "NOITERATE",
    "UNMAPLAST",
    "REVERSE",
    "SUB",
    "OUTPUT",
    "CAPTURE_UNIF",
    "REPEAT",
    "BEFORE",
    "AFTER",
    "IGNORED",
    "LOOKIGNORED",
    "NOMAPPED",
    "NOPARENT",
    "DETACH",
];

/// Keyword name strings, indexed by the `KEYWORDS` enum values.
/// (`keywords[KEYWORD_COUNT]`)
pub static KEYWORDS_STR: [&str; KEYWORDS::KEYWORD_COUNT as usize] = [
    "__CG3_DUMMY_KEYWORD__",
    "SETS",
    "LIST",
    "SET",
    "DELIMITERS",
    "SOFT-DELIMITERS",
    "PREFERRED-TARGETS",
    "MAPPING-PREFIX",
    "MAPPINGS",
    "CONSTRAINTS",
    "CORRECTIONS",
    "SECTION",
    "BEFORE-SECTIONS",
    "AFTER-SECTIONS",
    "NULL-SECTION",
    "ADD",
    "MAP",
    "REPLACE",
    "SELECT",
    "REMOVE",
    "IFF",
    "APPEND",
    "SUBSTITUTE",
    "START",
    "END",
    "ANCHOR",
    "EXECUTE",
    "JUMP",
    "REMVARIABLE",
    "SETVARIABLE",
    "DELIMIT",
    "MATCH",
    "SETPARENT",
    "SETCHILD",
    "ADDRELATION",
    "SETRELATION",
    "REMRELATION",
    "ADDRELATIONS",
    "SETRELATIONS",
    "REMRELATIONS",
    "TEMPLATE",
    "MOVE",
    "MOVE-AFTER",
    "MOVE-BEFORE",
    "SWITCH",
    "REMCOHORT",
    "STATIC-SETS",
    "UNMAP",
    "COPY",
    "ADDCOHORT",
    "ADDCOHORT-AFTER",
    "ADDCOHORT-BEFORE",
    "EXTERNAL",
    "EXTERNAL-ONCE",
    "EXTERNAL-ALWAYS",
    "OPTIONS",
    "STRICT-TAGS",
    "REOPEN-MAPPINGS",
    "SUBREADINGS",
    "SPLITCOHORT",
    "PROTECT",
    "UNPROTECT",
    "MERGECOHORTS",
    "RESTORE",
    "WITH",
    "OLIST",
    "OSET",
    "CMDARGS",
    "CMDARGS-OVERRIDE",
    "COPYCOHORT",
    "REMPARENT",
    "SWITCHPARENT",
];

/// Set-op / string-bit names, indexed by the `S_*` constants. (`stringbits[]`)
pub static STRINGBITS: [&str; 9] = [
    "",   // S_IGNORE
    "",   //
    "",   //
    "OR", // S_OR
    "+",  // S_PLUS
    "-",  // S_MINUS
    "",   //
    "",   //
    "^",  // S_FAILFAST
];

// The `STR_*` string constants. These are compared against grammar/stream input
// and printed verbatim, so the exact literal text is load-bearing.
pub const STR_TARGET: &str = "TARGET";
pub const STR_AND: &str = "AND";
pub const STR_IF: &str = "IF";
pub const STR_OR: &str = "OR";
pub const STR_TEXTNOT: &str = "NOT";
pub const STR_TEXTNEGATE: &str = "NEGATE";
pub const STR_ALL: &str = "ALL";
pub const STR_NONE: &str = "NONE";
pub const STR_LINK: &str = "LINK";
pub const STR_TO: &str = "TO";
pub const STR_FROM: &str = "FROM";
pub const STR_AFTER: &str = "AFTER";
pub const STR_BEFORE: &str = "BEFORE";
pub const STR_WITH: &str = "WITH";
pub const STR_ONCE: &str = "ONCE";
pub const STR_ALWAYS: &str = "ALWAYS";
pub const STR_EXCEPT: &str = "EXCEPT";
pub const STR_STATIC: &str = "STATIC";
pub const STR_ASTERIK: &str = "*";
pub const STR_BARRIER: &str = "BARRIER";
pub const STR_CBARRIER: &str = "CBARRIER";
pub const STR_CMD_FLUSH: &str = "<STREAMCMD:FLUSH>";
pub const STR_CMD_EXIT: &str = "<STREAMCMD:EXIT>";
pub const STR_CMD_IGNORE: &str = "<STREAMCMD:IGNORE>";
pub const STR_CMD_RESUME: &str = "<STREAMCMD:RESUME>";
pub const STR_CMD_SETVAR: &str = "<STREAMCMD:SETVAR:";
pub const STR_CMD_REMVAR: &str = "<STREAMCMD:REMVAR:";
pub const STR_DELIMITSET: &str = "_S_DELIMITERS_";
pub const STR_SOFTDELIMITSET: &str = "_S_SOFT_DELIMITERS_";
pub const STR_TEXTDELIMITSET: &str = "_S_TEXT_DELIMITERS_";
pub const STR_TEXTDELIM_DEFAULT: &str = "/(^|\\n)</s/r";
pub const STR_BEGINTAG: &str = ">>>";
pub const STR_ENDTAG: &str = "<<<";
pub const STR_UU_LEFT: &str = "_LEFT_";
pub const STR_UU_RIGHT: &str = "_RIGHT_";
pub const STR_UU_PAREN: &str = "_PAREN_";
pub const STR_UU_TARGET: &str = "_TARGET_";
pub const STR_UU_MARK: &str = "_MARK_";
pub const STR_UU_ATTACHTO: &str = "_ATTACHTO_";
pub const STR_UU_ENCL: &str = "_ENCL_";
pub const STR_UU_SAME_BASIC: &str = "_SAME_BASIC_";
pub const STR_UU_C1: &str = "_C1_";
pub const STR_UU_C2: &str = "_C2_";
pub const STR_UU_C3: &str = "_C3_";
pub const STR_UU_C4: &str = "_C4_";
pub const STR_UU_C5: &str = "_C5_";
pub const STR_UU_C6: &str = "_C6_";
pub const STR_UU_C7: &str = "_C7_";
pub const STR_UU_C8: &str = "_C8_";
pub const STR_UU_C9: &str = "_C9_";
pub const STR_RXTEXT_ANY: &str = "<.*>";
pub const STR_RXBASE_ANY: &str = "\".*\"";
pub const STR_RXWORD_ANY: &str = "\"<.*>\"";
pub const STR_VS1_RAW: &str = "$1";
pub const STR_VS2_RAW: &str = "$2";
pub const STR_VS3_RAW: &str = "$3";
pub const STR_VS4_RAW: &str = "$4";
pub const STR_VS5_RAW: &str = "$5";
pub const STR_VS6_RAW: &str = "$6";
pub const STR_VS7_RAW: &str = "$7";
pub const STR_VS8_RAW: &str = "$8";
pub const STR_VS9_RAW: &str = "$9";
pub const STR_VS1: &str = "\u{01}1";
pub const STR_VS2: &str = "\u{01}2";
pub const STR_VS3: &str = "\u{01}3";
pub const STR_VS4: &str = "\u{01}4";
pub const STR_VS5: &str = "\u{01}5";
pub const STR_VS6: &str = "\u{01}6";
pub const STR_VS7: &str = "\u{01}7";
pub const STR_VS8: &str = "\u{01}8";
pub const STR_VS9: &str = "\u{01}9";
pub const STR_VSU_LOWER_RAW: &str = "%u";
pub const STR_VSU_UPPER_RAW: &str = "%U";
pub const STR_VSL_LOWER_RAW: &str = "%l";
pub const STR_VSL_UPPER_RAW: &str = "%L";
pub const STR_VSU_LOWER: &str = "\u{01}u";
pub const STR_VSU_UPPER: &str = "\u{01}U";
pub const STR_VSL_LOWER: &str = "\u{01}l";
pub const STR_VSL_UPPER: &str = "\u{01}L";
pub const STR_GPREFIX: &str = "_G_";
pub const STR_POSITIVE: &str = "POSITIVE";
pub const STR_NEGATIVE: &str = "NEGATIVE";
pub const STR_NO_ISETS: &str = "no-inline-sets";
pub const STR_NO_ITMPLS: &str = "no-inline-templates";
pub const STR_STRICT_WFORMS: &str = "strict-wordforms";
pub const STR_STRICT_BFORMS: &str = "strict-baseforms";
pub const STR_STRICT_SECOND: &str = "strict-secondary";
pub const STR_STRICT_REGEX: &str = "strict-regex";
pub const STR_STRICT_ICASE: &str = "strict-icase";
pub const STR_SELF_NO_BARRIER: &str = "self-no-barrier";
pub const STR_ORDERED: &str = "ordered";
pub const STR_ADDCOHORT_ATTACH: &str = "addcohort-attach";
pub const STR_SAFE_SETPARENT: &str = "safe-setparent";
pub const STR_DUMMY: &str = "__CG3_DUMMY_STRINGBIT__";

pub const CG3_BUFFER_SIZE: usize = 8192;
pub const NUM_GBUFFERS: usize = 1;
pub const NUM_CBUFFERS: usize = 1;

pub const NOT_SIGN: char = '\u{00AC}';
