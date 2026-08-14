//! Port of `src/TextualParser.cpp` / `src/TextualParser.hpp` — the recursive
//! descent parser that turns CG-3 text grammar into a [`Grammar`]
//! (spec `docs/spec/port/src/TextualParser.md`).
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2, translate pass).
//!
//! ## Representation decisions
//! * **Cursor model.** The C++ walks a NUL-terminated `UChar*& p`. Ported over a
//!   decoupled `buf: &[char]` slice plus a `pos: &mut usize` cursor (the same
//!   convention as `crate::inlines`). The buffer is the whole grammar source
//!   (4 leading NULs + text + trailing NUL padding); `pos` starts at 4 (C++
//!   `&data[4]`). Each `grammarbufs` entry is a shared, immutable
//!   [`SrcBuf`](crate::ast::SrcBuf) (`Rc<[char]>`); `parse_from_u_char` clones
//!   the handle (a refcount bump) into an owned local, so `buf` does NOT borrow
//!   `self` — letting every parse method take `&mut self` + `buf` + `pos`
//!   without a borrow conflict (and letting `#include` push new buffers while a
//!   parse is in flight). The char data is never mutated after creation, so the
//!   shared handle is faithful to the C++ raw pointers into stable
//!   `unique_ptr<UString>` buffers.
//! * **Errors.** The C++ `error(...)` is `[[noreturn]]` and throws an `int`
//!   caught per statement by `parseFromUChar`, which recovers by skipping to the
//!   next line. Here [`TextualParser::error_near`] RETURNS the error and every
//!   call site propagates it with `?` — see `[dec:cg3:results-not-unwinding]`.
//!   The one frame that cannot use `?` is `parse_from_u_char`'s directive loop,
//!   which records the error and carries on, because a recoverable parse error
//!   is resumable and a bad grammar must report all of them
//!   (`[spec:cg3:req:errors.parse-reports-all]`). `incErrorCount`'s `>= 10` bail
//!   is [`MAX_PARSE_ERRORS`], which now stops the loop instead of exiting.
//! * **AST.** `parse_ast` (the `--dump-ast` capture) is gated on the thread-local
//!   in `crate::ast`. SCOPE REDUCTION: only the root `AST_Grammar` node is opened
//!   here (via [`ASTHelper`]); the ~120 inner `AST_OPEN`/`AST_CLOSE` sites are
//!   NOT reproduced — they are a debug-only feature, `crate::ast` exposes no
//!   `cur_ast->b = …` setter (and this pass may not edit it), and reproducing
//!   them would multiply compile-surface for no parsing-semantics gain. Documented
//!   deviation; `print_ast` itself is ported.
//! * **Profiler.** The C++ `Profiler* profiler` is always null in the port (no
//!   Profiler module); every `if (profiler)` block is skipped.
//! * **`gbuffers[0]` scratch.** The shared UString token scratch becomes a local
//!   `String` per extraction.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

use regex::Regex;

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::ast::{ASTHelper, ASTType, Ast};
use crate::contextual_test::{
    GsrSpecials, MASK_POS_SCAN, POS_64BIT, POS_ABSOLUTE, POS_ACTIVE, POS_ALL, POS_ATTACH_TO,
    POS_BAG_OF_TAGS, POS_CAREFUL, POS_DEP_CHILD, POS_DEP_DEEP, POS_DEP_GLOB, POS_DEP_PARENT,
    POS_DEP_SIBLING, POS_INACTIVE, POS_JUMP, POS_LEFT, POS_LEFT_PAR, POS_LEFTMOST,
    POS_LOOK_DELAYED, POS_LOOK_DELETED, POS_LOOK_IGNORED, POS_MARK_SET, POS_NEGATE, POS_NO_BARRIER,
    POS_NO_PASS_ORIGIN, POS_NONE, POS_NOT, POS_NUMERIC_BRANCH, POS_PASS_ORIGIN, POS_RELATION,
    POS_RIGHT, POS_RIGHT_PAR, POS_RIGHTMOST, POS_SCANALL, POS_SCANFIRST, POS_SELF, POS_SPAN_BOTH,
    POS_SPAN_LEFT, POS_SPAN_RIGHT, POS_TMPL_OVERRIDE, POS_UNKNOWN, POS_WITH, PosJumpPos,
};
use crate::grammar::Grammar;
use crate::inlines::{hash_value_ustring, isspace, skiptows_chars, skipws_chars, ui32};
use crate::rule::{
    FLAGS_COUNT, RF_AFTER, RF_ALLOWLOOP, RF_BEFORE, RF_DELAYED, RF_ENCL_ANY, RF_ENCL_FINAL,
    RF_ENCL_INNER, RF_ENCL_OUTER, RF_IGNORED, RF_IMMEDIATE, RF_ITERATE, RF_KEEPORDER,
    RF_LOOKDELAYED, RF_LOOKDELETED, RF_LOOKIGNORED, RF_NEAREST, RF_NOCHILD, RF_NOITERATE,
    RF_REMEMBERX, RF_RESETX, RF_SAFE, RF_UNMAPLAST, RF_UNSAFE, RF_VARYORDER, RF_WITHCHILD, Rule,
};
use crate::set::{ST_CHILD_UNIFY, ST_ORDERED, ST_SET_UNIFY, ST_TAG_UNIFY};
use crate::sorted_vector::Uint32SortedVector;
use crate::strings::Keywords;
use crate::tag::{
    T_ANY, T_ATTACHTO, T_BASEFORM, T_CASE_INSENSITIVE, T_ENCL, T_FAILFAST, T_LOCAL_VARIABLE,
    T_MARK, T_META, T_PAR_LEFT, T_PAR_RIGHT, T_REGEXP, T_REGEXP_ANY, T_REGEXP_LINE, T_SAME_BASIC,
    T_SET, T_SPECIAL, T_TARGET, T_VARIABLE, T_VARSTRING, T_VSTR, T_WORDFORM, Tag, TagVector,
    TagVectorSet, compare_tag_vector,
};
use crate::tag_trie::{trie_get_tags, trie_insert};
use crate::types::SetNumber;
mod driver;
mod rules;
use crate::uextras::{S_IGNORE, basename, ux_bufcpy, ux_dirname, ux_is_empty, ux_is_set_op};

// ---------------------------------------------------------------------------
// Local constants (canonical home `Strings.hpp`; reproduced verbatim, same
// precedent as `grammar.rs` / `parser_helpers.rs`).
// ---------------------------------------------------------------------------

// enum : uint32_t { S_IGNORE, S_OR=3, S_PLUS, S_MINUS, S_FAILFAST=8, S_SET_DIFF,
// S_SET_ISECT_U, S_SET_SYMDIFF_U }  (as u32, stored in `set_ops`).
const S_OR: u32 = 3;
const S_SET_DIFF: u32 = 9;
const S_SET_ISECT_U: u32 = 10;
const S_SET_SYMDIFF_U: u32 = 11;

// FL_* indices used explicitly.
const FL_WITHCHILD: usize = 17;
const FL_NOCHILD: usize = 18;
const FL_SUB: usize = 23;

const STR_ASTERIK: &str = "*";
const STR_DELIMITSET: &str = "_S_DELIMITERS_";
const STR_SOFTDELIMITSET: &str = "_S_SOFT_DELIMITERS_";
const STR_TEXTDELIMITSET: &str = "_S_TEXT_DELIMITERS_";
const STR_UU_LEFT: &str = "_LEFT_";
const STR_UU_RIGHT: &str = "_RIGHT_";
const STR_UU_PAREN: &str = "_PAREN_";
const STR_UU_TARGET: &str = "_TARGET_";
const STR_UU_MARK: &str = "_MARK_";
const STR_UU_ATTACHTO: &str = "_ATTACHTO_";
const STR_UU_ENCL: &str = "_ENCL_";
const STR_UU_SAME_BASIC: &str = "_SAME_BASIC_";
const STR_UU_C: [&str; 9] = [
    "_C1_", "_C2_", "_C3_", "_C4_", "_C5_", "_C6_", "_C7_", "_C8_", "_C9_",
];
const STR_TEXTNOT: &str = "NOT";
const STR_TEXTNEGATE: &str = "NEGATE";
const STR_ALL: &str = "ALL";
const STR_NONE: &str = "NONE";
const STR_OR: &str = "OR";
const STR_AND: &str = "AND";
const STR_LINK: &str = "LINK";
const STR_BARRIER: &str = "BARRIER";
const STR_CBARRIER: &str = "CBARRIER";
const STR_TARGET: &str = "TARGET";
const STR_IF: &str = "IF";
const STR_TO: &str = "TO";
const STR_FROM: &str = "FROM";
const STR_AFTER: &str = "AFTER";
const STR_BEFORE: &str = "BEFORE";
const STR_WITH: &str = "WITH";
const STR_ONCE: &str = "ONCE";
const STR_ALWAYS: &str = "ALWAYS";
const STR_EXCEPT: &str = "EXCEPT";
const STR_STATIC: &str = "STATIC";
const STR_NO_ISETS: &str = "no-inline-sets";
const STR_NO_ITMPLS: &str = "no-inline-templates";
const STR_STRICT_WFORMS: &str = "strict-wordforms";
const STR_STRICT_BFORMS: &str = "strict-baseforms";
const STR_STRICT_SECOND: &str = "strict-secondary";
const STR_STRICT_REGEX: &str = "strict-regex";
const STR_STRICT_ICASE: &str = "strict-icase";
const STR_SELF_NO_BARRIER: &str = "self-no-barrier";
const STR_ORDERED: &str = "ordered";
const STR_ADDCOHORT_ATTACH: &str = "addcohort-attach";
const STR_SAFE_SETPARENT: &str = "safe-setparent";

/// C++ `g_flags[FLAGS_COUNT]` — the rule-flag keyword names (index == FL_*).
const G_FLAGS: [&str; FLAGS_COUNT] = [
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

/// C++ `keywords[KEYWORD_COUNT]` (72). Indexed by `KEYWORDS as usize`.
const KEYWORDS_STR: [&str; 72] = [
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

/// C++ `flag_excls[]` — the 9 mutually-exclusive rule-flag groups.
const FLAG_EXCLS_GROUPS: [crate::rule::RuleFlags; 9] = [
    RF_NEAREST.union(RF_ALLOWLOOP),
    RF_DELAYED.union(RF_IMMEDIATE).union(RF_IGNORED),
    RF_UNSAFE.union(RF_SAFE),
    RF_REMEMBERX.union(RF_RESETX),
    RF_KEEPORDER.union(RF_VARYORDER),
    RF_ENCL_INNER
        .union(RF_ENCL_OUTER)
        .union(RF_ENCL_FINAL)
        .union(RF_ENCL_ANY),
    RF_WITHCHILD.union(RF_NOCHILD),
    RF_ITERATE.union(RF_NOITERATE),
    RF_BEFORE.union(RF_AFTER),
];

/// C++ `struct flags_t { rule_flags_t flags = 0; int32_t sub_reading = 0; }`
/// (Strings.hpp). Not `crate::types::flags_t` (a bitset); this is the rule-flag
/// return payload of `parseRuleFlags`.
#[derive(Clone, Copy, Default)]
struct DynBitset {
    flags: crate::rule::RuleFlags,
    sub_reading: i32,
}

/// How many recoverable errors a parse reports before giving up. The C++
/// `incErrorCount` bailed at the same count with `CG3Quit(1)`.
pub(crate) const MAX_PARSE_ERRORS: usize = 10;

/// What every parser function returns: a value, or the recoverable error that
/// stopped it. The C++ signalled these by throwing.
pub(crate) type ParseResult<T = ()> = Result<T, crate::error::ParseError>;

// ---------------------------------------------------------------------------
// Free helpers.
// ---------------------------------------------------------------------------

#[inline]
fn slen(s: &str) -> usize {
    s.chars().count()
}

/// `ux_simplecasecmp(p, STR.data(), STR.size())`.
fn simplecasecmp(buf: &[char], pos: usize, s: &str) -> bool {
    let bc: Vec<char> = s.chars().collect();
    crate::uextras::ux_simplecasecmp(&buf[pos..], &bc, bc.len())
}

/// `IS_ICASE(p, "UPPER", "lower")` → matched length, else 0.
fn is_icase_kw(buf: &[char], pos: usize, uc: &str, lc: &str) -> usize {
    let ucv: Vec<char> = uc.chars().chain(std::iter::once('\0')).collect();
    let lcv: Vec<char> = lc.chars().chain(std::iter::once('\0')).collect();
    crate::inlines::is_icase_chars(buf, pos, &ucv, &lcv)
}

/// ICU `u_isdigit` (decimal-digit category), approximated with Rust's Unicode
/// numeric table (parity with `<= '9'` ASCII loops is exact for 0-9).
#[inline]
fn u_isdigit(c: char) -> bool {
    c.is_numeric()
}

/// `u_sscanf(s, "%d", &out)`: leading optional sign + decimal digits.
fn scan_d(s: &str) -> i32 {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let mut sign = 1i64;
    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
        if chars[i] == '-' {
            sign = -1;
        }
        i += 1;
    }
    let mut n = 0i64;
    while i < chars.len() && chars[i].is_ascii_digit() {
        n = n * 10 + (chars[i] as i64 - '0' as i64);
        i += 1;
    }
    (sign * n) as i32
}

// [spec:cg3:def:textual-parser.cg3.is-mapping-list-fn]
// [spec:cg3:sem:textual-parser.cg3.is-mapping-list-fn]
fn is_mapping_list(grammar: &Grammar, s: SetId) -> bool {
    let mut is_list = true;
    let st = grammar.sets_list[s.0].r#type;
    let trie_empty = grammar.sets_list[s.0].trie.is_empty();
    let trie_sp_empty = grammar.sets_list[s.0].trie_special.is_empty();
    if !(trie_empty
        && trie_sp_empty
        && !st.intersects(ST_TAG_UNIFY | ST_SET_UNIFY | ST_CHILD_UNIFY))
    {
        let tries = [
            grammar.sets_list[s.0].trie.clone(),
            grammar.sets_list[s.0].trie_special.clone(),
        ];
        for trie in &tries {
            if trie.is_empty() {
                continue;
            }
            let ctags = trie_get_tags(trie, grammar);
            for it in &ctags {
                for &tag in it {
                    if grammar.single_tags_list[tag.0]
                        .r#type
                        .intersects(T_FAILFAST | T_REGEXP_LINE)
                    {
                        return false;
                    }
                }
            }
        }
        return is_list;
    }
    let set_ops = grammar.sets_list[s.0].set_ops.clone();
    for op in set_ops {
        if op != S_OR {
            is_list = false;
            break;
        }
    }
    let sets = grammar.sets_list[s.0].sets.clone();
    for i in sets {
        let set = grammar.get_set(i).unwrap();
        if !is_mapping_list(grammar, set) {
            is_list = false;
            break;
        }
    }
    is_list
}

/// Collect a `TagVectorSet` into a `Vec<TagVector>` ordered by `compare_TagVector`
/// (hash order) — the order `std::set_*` require (the port's `BTreeSet` is
/// `Vec<TagId>`-ordered instead; documented deviation in `tag.rs`).
fn sorted_tvs(grammar: &Grammar, s: &TagVectorSet) -> Vec<TagVector> {
    let mut v: Vec<TagVector> = s.iter().cloned().collect();
    v.sort_by(|x, y| {
        if compare_tag_vector(grammar, x, y) {
            std::cmp::Ordering::Less
        } else if compare_tag_vector(grammar, y, x) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    v
}

fn merge_intersection(g: &Grammar, a: &[TagVector], b: &[TagVector]) -> Vec<TagVector> {
    let mut r = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if compare_tag_vector(g, &a[i], &b[j]) {
            i += 1;
        } else if compare_tag_vector(g, &b[j], &a[i]) {
            j += 1;
        } else {
            r.push(a[i].clone());
            i += 1;
            j += 1;
        }
    }
    r
}

fn merge_symdiff(g: &Grammar, a: &[TagVector], b: &[TagVector]) -> Vec<TagVector> {
    let mut r = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if compare_tag_vector(g, &a[i], &b[j]) {
            r.push(a[i].clone());
            i += 1;
        } else if compare_tag_vector(g, &b[j], &a[i]) {
            r.push(b[j].clone());
            j += 1;
        } else {
            i += 1;
            j += 1;
        }
    }
    r.extend_from_slice(&a[i..]);
    r.extend_from_slice(&b[j..]);
    r
}

/// a \ b (elements of `a` not in `b`).
fn merge_difference(g: &Grammar, a: &[TagVector], b: &[TagVector]) -> Vec<TagVector> {
    let mut r = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if compare_tag_vector(g, &a[i], &b[j]) {
            r.push(a[i].clone());
            i += 1;
        } else if compare_tag_vector(g, &b[j], &a[i]) {
            j += 1;
        } else {
            i += 1;
            j += 1;
        }
    }
    r.extend_from_slice(&a[i..]);
    r
}

// [spec:cg3:def:textual-parser.cg3.textual-parser.deferred-t]
/// `typedef std::unordered_map<ContextualTest*, std::pair<size_t, UString>>
/// deferred_t` (TextualParser.hpp:86). The `ContextualTest*` key becomes the arena
/// `CtxId`, `UString` becomes `String`; maps a deferred template context to its
/// `(line, name)` for late resolution.
type DeferredTests = HashMap<CtxId, (usize, String)>;

// [spec:cg3:def:textual-parser.cg3.textual-parser]
pub struct TextualParser {
    pub grammar: Grammar,
    pub filebase: String,
    pub strict_tags: Uint32SortedVector,
    pub list_tags: Uint32SortedVector,
    nearbuf: [char; 32],
    verbosity_level: u32,
    sets_counter: u32,
    seen_mapping_prefix: u32,
    section_flags: DynBitset,
    option_vislcg_compat: bool,
    in_section: bool,
    in_before_sections: bool,
    in_after_sections: bool,
    in_null_section: bool,
    in_nested_rule: bool,
    no_isets: bool,
    no_itmpls: bool,
    strict_wforms: bool,
    strict_bforms: bool,
    strict_second: bool,
    strict_regex: bool,
    strict_icase: bool,
    self_no_barrier: bool,
    safe_setparent: bool,
    only_sets: bool,
    /// Collector for the WITH-block subrules (the C++ `nested_rule->sub_rules`
    /// target). See `add_rule_to_grammar`.
    nested_subrules: Vec<RuleId>,
    filename: String,
    /// Shared handle to the grammar buffer currently being parsed — the buffer
    /// each AST node opened here records for its span. Replaces the C++
    /// `cur_grammar` raw pointer (which the port never dereferenced: profiler
    /// spans are computed from `pos` offsets directly, so the pointer's only
    /// live role was buffer identity for AST nodes). Saved/restored around
    /// `#include` exactly as the C++ pointer was.
    cur_grammar_buf: crate::ast::SrcBuf,
    cur_grammar_n: u32,
    num_grammars: u32,
    deferred_tmpls: DeferredTests,
    grammarbufs: Vec<crate::ast::SrcBuf>,
    /// Every recoverable parse error found, in source order —
    /// `[spec:cg3:req:errors.parse-reports-all]`. Replaces the C++
    /// `error_counter`, which was thrown and never read.
    errors: Vec<crate::error::ParseError>,
    /// Signals the `END` directive breaking the `parseFromUChar` loop.
    parse_end_break: bool,
    pub nrules: Option<Regex>,
    pub nrules_inv: Option<Regex>,
    /// Wave-4 owned AST builder (C++ `thread_local parse_ast/ast/cur_ast`).
    ast: Ast,
    /// C++ `Profiler* profiler` (raw pointer to main's Profiler) — OWNED here:
    /// the driver moves the profiler in before `parse_grammar` and takes it
    /// back afterwards.
    pub profiler: Option<crate::profiler::Profiler>,
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.textual-parser-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.textual-parser-fn]
    /// C++ `TextualParser(Grammar& res, std::ostream& ux_err, bool _dump_ast)`.
    /// The port OWNS its `Grammar`; the C++ error-stream arg becomes stderr.
    pub fn new(grammar: Grammar, dump_ast: bool) -> TextualParser {
        TextualParser {
            ast: Ast::new(dump_ast),
            grammar,
            filebase: String::new(),
            strict_tags: Uint32SortedVector::new(),
            list_tags: Uint32SortedVector::new(),
            nearbuf: ['\0'; 32],
            verbosity_level: 0,
            sets_counter: 100,
            seen_mapping_prefix: 0,
            section_flags: DynBitset::default(),
            option_vislcg_compat: false,
            in_section: false,
            in_before_sections: true,
            in_after_sections: false,
            in_null_section: false,
            in_nested_rule: false,
            no_isets: false,
            no_itmpls: false,
            strict_wforms: false,
            strict_bforms: false,
            strict_second: false,
            strict_regex: false,
            strict_icase: false,
            self_no_barrier: false,
            safe_setparent: false,
            only_sets: false,
            nested_subrules: Vec::new(),
            filename: String::new(),
            cur_grammar_buf: crate::ast::SrcBuf::from([] as [char; 0]),
            cur_grammar_n: 0,
            num_grammars: 0,
            deferred_tmpls: HashMap::new(),
            grammarbufs: Vec::new(),
            errors: Vec::new(),
            parse_end_break: false,
            nrules: None,
            nrules_inv: None,
            profiler: None,
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.set-compatible-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.set-compatible-fn]
    pub fn set_compatible(&mut self, f: bool) {
        self.option_vislcg_compat = f;
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.set-verbosity-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.set-verbosity-fn]
    pub fn set_verbosity(&mut self, level: u32) {
        self.verbosity_level = level;
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.get-grammar-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.get-grammar-fn]
    pub fn get_grammar(&self) -> &Grammar {
        &self.grammar
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.print-ast-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.print-ast-fn]
    pub fn print_ast(&self, out: &mut dyn Write) {
        {
            let root = self.ast.root();
            if root.cs.is_empty() {
                return;
            }
            let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
            let _ = writeln!(out, "<!-- l is line -->");
            let _ = writeln!(
                out,
                "<!-- b is begin, e is end - both are absolute UTF-16 code unit offsets (not code point) in the file -->"
            );
            let _ = writeln!(
                out,
                "<!-- u is the deduplicated objects' unique identifier -->"
            );
            crate::ast::print_ast(out, root.cs[0].b, 0, &root.cs[0]);
        }
    }

    /// How many recoverable errors have been recorded.
    pub(crate) fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Take the accumulated errors, leaving the parser empty.
    pub(crate) fn take_errors(&mut self) -> Vec<crate::error::ParseError> {
        std::mem::take(&mut self.errors)
    }

    /// Record a recoverable error and report it, exactly once.
    ///
    /// The C++ printed at the raise site and threw; recording is now the single
    /// point where an error becomes final, so it is also the single point that
    /// prints.
    pub(crate) fn record(&mut self, e: crate::error::ParseError) {
        tracing::error!("{e}");
        self.errors.push(e);
    }

    /// Build a parse error at the current position.
    pub(crate) fn parse_error_at(
        &self,
        near: String,
        kind: crate::error::ParseErrorKind,
    ) -> crate::error::ParseError {
        crate::error::ParseError {
            file: self.filebase.clone(),
            line: self.grammar.lines,
            near,
            kind,
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.error-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.error-fn]
    /// The `(str, const UChar* p)` error overload (the near-context form the
    /// vast majority of sites use). `near` is the buffer tail at `p`.
    ///
    /// Returns the error rather than diverging: the C++ `throw` is now a
    /// `return Err(..)` at each call site — `[dec:cg3:results-not-unwinding]`.
    // [spec:cg3:req:errors.result-primary]
    pub fn error_near(&mut self, near: &[char]) -> crate::error::ParseError {
        ux_bufcpy(&mut self.nearbuf, Some(near), 20);
        let near_text: String = self.nearbuf.iter().take_while(|&&c| c != '\0').collect();
        self.parse_error_at(near_text, crate::error::ParseErrorKind::Syntax)
    }

    /// The `(str)` overload (no near-context; used by the SUB:* guard).
    fn error_bare(&mut self) -> crate::error::ParseError {
        self.parse_error_at(String::new(), crate::error::ParseErrorKind::Syntax)
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.add-tag-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.add-tag-fn]
    pub fn add_tag(&mut self, tag: Tag) -> TagId {
        self.grammar.add_tag(tag)
    }

    /// C++ `MAYBE_QUOTED(n, p)` macro: skip a `"..."` span if `n` is at a quote.
    fn maybe_quoted(&mut self, buf: &[char], n: &mut usize, near_pos: usize) -> ParseResult {
        if buf[*n] == '"' {
            *n += 1;
            crate::inlines::skipto_nospan_chars(buf, n, '"');
            if buf[*n] != '"' {
                return Err(self.error_near(&buf[near_pos..]));
            }
        }
        Ok(())
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-tag-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-fn]
    pub fn parse_tag(&mut self, to: &str, near: &[char]) -> ParseResult<TagId> {
        let tag = crate::parser_helpers::parse_tag(to, near, self, true)?;
        let (ty, tagstr, plain) = {
            let t = &self.grammar.single_tags_list[tag.0];
            (t.r#type, t.tag.clone(), t.plain_hash)
        };
        if ty.intersects(T_VARSTRING) && !tagstr.contains('{') && !tagstr.contains('$') {
            return Err(self.error_near(near)); // "Varstring tag had no variables"
        }
        if !self.strict_tags.empty() && self.strict_tags.count(plain.get()) == 0 {
            if ty.intersects(
                T_ANY
                    | T_VARSTRING
                    | T_VSTR
                    | T_META
                    | T_VARIABLE
                    | T_LOCAL_VARIABLE
                    | T_SET
                    | T_PAR_LEFT
                    | T_PAR_RIGHT
                    | T_ENCL
                    | T_TARGET
                    | T_MARK
                    | T_ATTACHTO
                    | T_SAME_BASIC,
            ) {
                // Always allow...
            } else if tagstr == ">>>" || tagstr == "<<<" {
                // Always allow >>> and <<<
            } else if ty.intersects(T_REGEXP | T_REGEXP_ANY) {
                if self.strict_regex {
                    return Err(self.error_near(near));
                }
            } else if ty.intersects(T_CASE_INSENSITIVE) {
                if self.strict_icase {
                    return Err(self.error_near(near));
                }
            } else if ty.intersects(T_WORDFORM) {
                if self.strict_wforms {
                    return Err(self.error_near(near));
                }
            } else if ty.intersects(T_BASEFORM) {
                if self.strict_bforms {
                    return Err(self.error_near(near));
                }
            } else if tagstr.starts_with('<') && tagstr.ends_with('>') {
                if self.strict_second {
                    return Err(self.error_near(near));
                }
            } else {
                return Err(self.error_near(near));
            }
        }
        Ok(tag)
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-fn]
    pub fn parse_set(&mut self, name: &str, near: &[char]) -> ParseResult<SetId> {
        crate::parser_helpers::parse_set(name, near, self)
    }
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-tag-list-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-list-fn]
    fn parse_tag_list(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        s: SetId,
        ordered: bool,
    ) -> ParseResult {
        let mut taglists: BTreeSet<TagVector> = BTreeSet::new();
        let mut tag_freq: BTreeMap<TagId, usize> = BTreeMap::new();

        while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
            self.grammar.lines += skipws_chars(buf, pos, ';', ')', false);
            if buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                let mut tags: TagVector = TagVector::new();
                if buf[*pos] == '(' {
                    *pos += 1;
                    self.grammar.lines += skipws_chars(buf, pos, ';', ')', false);
                    while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                        let mut n = *pos;
                        self.maybe_quoted(buf, &mut n, *pos)?;
                        self.grammar.lines += skiptows_chars(buf, &mut n, ')', true, false);
                        let token: String = buf[*pos..n].iter().collect();
                        let t = self.parse_tag(&token, &buf[*pos..])?;
                        tags.push(t);
                        *pos = n;
                        self.grammar.lines += skipws_chars(buf, pos, ';', ')', false);
                    }
                    if buf[*pos] != ')' {
                        return Err(self.error_near(&buf[*pos..]));
                    }
                    *pos += 1;
                } else {
                    let mut n = *pos;
                    self.maybe_quoted(buf, &mut n, *pos)?;
                    self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
                    let token: String = buf[*pos..n].iter().collect();
                    let t = self.parse_tag(&token, &buf[*pos..])?;
                    tags.push(t);
                    *pos = n;
                }

                if !ordered {
                    // sort + uniq the tags (by Tag::hash)
                    tags.sort_by(|&a, &b| {
                        self.grammar.single_tags_list[a.0]
                            .hash
                            .cmp(&self.grammar.single_tags_list[b.0].hash)
                    });
                    tags.dedup_by(|a, b| {
                        self.grammar.single_tags_list[a.0].hash
                            == self.grammar.single_tags_list[b.0].hash
                    });
                }
                if taglists.insert(tags.clone()) {
                    for &t in &tags {
                        *tag_freq.entry(t).or_insert(0) += 1;
                    }
                }
            }
        }

        for tvc in &taglists {
            if tvc.len() == 1 {
                self.grammar.add_tag_to_set(tvc[0], s);
                continue;
            }
            let mut tv: TagVector = tvc.clone();
            if !ordered {
                // Sort tags by frequency, high-to-low (cheap trie compression).
                tv.sort_by(|&a, &b| tag_freq[&b].cmp(&tag_freq[&a]));
            }
            let mut special = false;
            for &tag in &tv {
                if self.grammar.single_tags_list[tag.0]
                    .r#type
                    .intersects(T_SPECIAL)
                {
                    special = true;
                    break;
                }
            }
            if special {
                trie_insert(
                    &mut self.grammar.sets_list.get_mut(s.0).trie_special,
                    &tv,
                    0,
                );
            } else {
                trie_insert(&mut self.grammar.sets_list.get_mut(s.0).trie, &tv, 0);
            }
        }
        Ok(())
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-inline-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-fn]
    fn parse_set_inline(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        s: Option<SetId>,
    ) -> ParseResult<SetId> {
        let mut s = s;
        let mut set_ops: Vec<u32> = Vec::new();
        let mut sets: Vec<u32> = Vec::new();

        let mut wantop = false;
        while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
            self.grammar.lines += skipws_chars(buf, pos, ';', ')', false);
            if buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                if !wantop {
                    if buf[*pos] == '(' {
                        if self.no_isets && buf[*pos + 1] != '*' {
                            return Err(self.error_near(&buf[*pos..]));
                        }
                        let n_open = *pos;
                        *pos += 1;
                        let set_c = self.grammar.allocate_set();
                        self.grammar.sets_list[set_c.0].r#type |= ST_ORDERED;
                        self.grammar.sets_list[set_c.0].line = self.grammar.lines;
                        let nm = self.sets_counter;
                        self.sets_counter += 1;
                        self.grammar
                            .sets_list
                            .get_mut(set_c.0)
                            .set_name(nm, &mut self.grammar.rand_state);
                        let mut tags: TagVector = TagVector::new();

                        while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                            self.grammar.lines += skipws_chars(buf, pos, ';', ')', false);
                            let mut n = *pos;
                            self.maybe_quoted(buf, &mut n, *pos)?;
                            self.grammar.lines += skiptows_chars(buf, &mut n, ')', true, false);
                            let token: String = buf[*pos..n].iter().collect();
                            let t = self.parse_tag(&token, &buf[*pos..])?;
                            tags.push(t);
                            *pos = n;
                            self.grammar.lines += skipws_chars(buf, pos, ';', ')', false);
                        }
                        if buf[*pos] != ')' {
                            return Err(self.error_near(&buf[*pos..]));
                        }
                        *pos += 1;

                        if tags.is_empty() {
                            return Err(self.error_near(&buf[n_open..]));
                        } else if tags.len() == 1 {
                            self.grammar.add_tag_to_set(tags[0], set_c);
                        } else {
                            let mut special = false;
                            for &tag in &tags {
                                if self.grammar.single_tags_list[tag.0]
                                    .r#type
                                    .intersects(T_SPECIAL)
                                {
                                    special = true;
                                    break;
                                }
                            }
                            if special {
                                trie_insert(
                                    &mut self.grammar.sets_list.get_mut(set_c.0).trie_special,
                                    &tags,
                                    0,
                                );
                            } else {
                                trie_insert(
                                    &mut self.grammar.sets_list.get_mut(set_c.0).trie,
                                    &tags,
                                    0,
                                );
                            }
                        }

                        let set_c = self.grammar.add_set(set_c)?;
                        let h = self.grammar.sets_list[set_c.0].hash;
                        sets.push(h);
                    } else {
                        let mut n = *pos;
                        self.grammar.lines += skiptows_chars(buf, &mut n, ')', true, false);
                        while buf[n - 1] == ',' || buf[n - 1] == ']' {
                            n -= 1;
                        }
                        let token: String = buf[*pos..n].iter().collect();
                        let tmp = self.parse_set(&token, &buf[*pos..])?;
                        let sh = self.grammar.sets_list[tmp.0].hash;
                        sets.push(sh);
                        *pos = n;
                    }

                    // Eager binary set operators.
                    if !set_ops.is_empty()
                        && (*set_ops.last().unwrap() == S_SET_DIFF
                            || *set_ops.last().unwrap() == S_SET_ISECT_U
                            || *set_ops.last().unwrap() == S_SET_SYMDIFF_U)
                    {
                        let sa = self.grammar.get_set(sets[sets.len() - 1]).unwrap();
                        let mut a: TagVectorSet = TagVectorSet::new();
                        self.grammar.get_tags(sa, &mut a);
                        let sb = self.grammar.get_set(sets[sets.len() - 2]).unwrap();
                        let mut b: TagVectorSet = TagVectorSet::new();
                        self.grammar.get_tags(sb, &mut b);

                        let av = sorted_tvs(&self.grammar, &a);
                        let bv = sorted_tvs(&self.grammar, &b);
                        let op = *set_ops.last().unwrap();
                        let r: Vec<TagVector> = if op == S_SET_ISECT_U {
                            merge_intersection(&self.grammar, &av, &bv)
                        } else if op == S_SET_SYMDIFF_U {
                            merge_symdiff(&self.grammar, &av, &bv)
                        } else {
                            // S_SET_DIFF: (b,a) because order matters → b \ a.
                            merge_difference(&self.grammar, &bv, &av)
                        };

                        set_ops.pop();
                        sets.pop();
                        sets.pop();

                        let set_c = self.grammar.allocate_set();
                        self.grammar.sets_list[set_c.0].line = self.grammar.lines;
                        let nm = self.sets_counter;
                        self.sets_counter += 1;
                        self.grammar
                            .sets_list
                            .get_mut(set_c.0)
                            .set_name(nm, &mut self.grammar.rand_state);

                        let mut tag_freq: BTreeMap<TagId, usize> = BTreeMap::new();
                        for tags in &r {
                            for &t in tags {
                                *tag_freq.entry(t).or_insert(0) += 1;
                            }
                        }
                        for tv in &r {
                            if tv.len() == 1 {
                                self.grammar.add_tag_to_set(tv[0], set_c);
                                continue;
                            }
                            let mut tvm: TagVector = tv.clone();
                            tvm.sort_by(|&a, &b| tag_freq[&b].cmp(&tag_freq[&a]));
                            let mut special = false;
                            for &tag in &tvm {
                                if self.grammar.single_tags_list[tag.0]
                                    .r#type
                                    .intersects(T_SPECIAL)
                                {
                                    special = true;
                                    break;
                                }
                            }
                            if special {
                                trie_insert(
                                    &mut self.grammar.sets_list.get_mut(set_c.0).trie_special,
                                    &tvm,
                                    0,
                                );
                            } else {
                                trie_insert(
                                    &mut self.grammar.sets_list.get_mut(set_c.0).trie,
                                    &tvm,
                                    0,
                                );
                            }
                        }

                        let set_c = self.grammar.add_set(set_c)?;
                        let h = self.grammar.sets_list[set_c.0].hash;
                        sets.push(h);
                    }

                    wantop = true;
                } else {
                    let mut n = *pos;
                    if buf[n] == '\\' && isspace(buf[n + 1]) {
                        n += 1;
                    } else {
                        self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
                    }
                    let token: String = buf[*pos..n].iter().collect();
                    let sop = ux_is_set_op(&token);
                    if sop != S_IGNORE {
                        set_ops.push(sop as u32);
                        wantop = false;
                        *pos = n;
                    } else {
                        break;
                    }
                }
            } else if !wantop {
                return Err(self.error_near(&buf[*pos..]));
            }
        }

        if s.is_none() && sets.is_empty() {
            return Err(self.error_near(&buf[*pos..]));
        }

        if s.is_none() && sets.len() == 1 {
            s = Some(self.grammar.get_set(*sets.last().unwrap()).unwrap());
        } else {
            let sid = match s {
                Some(sid) => sid,
                None => self.grammar.allocate_set(),
            };
            std::mem::swap(&mut self.grammar.sets_list.get_mut(sid.0).sets, &mut sets);
            std::mem::swap(
                &mut self.grammar.sets_list.get_mut(sid.0).set_ops,
                &mut set_ops,
            );
            s = Some(sid);
        }

        Ok(s.unwrap())
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-inline-wrapper-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-wrapper-fn]
    fn parse_set_inline_wrapper(&mut self, buf: &[char], pos: &mut usize) -> ParseResult<SetId> {
        let tmplines = self.grammar.lines;
        let s = self.parse_set_inline(buf, pos, None)?;
        if self.grammar.sets_list[s.0].line == 0 {
            self.grammar.sets_list[s.0].line = tmplines;
        }
        if self.grammar.sets_list[s.0].name.is_empty() {
            let nm = self.sets_counter;
            self.sets_counter += 1;
            self.grammar
                .sets_list
                .get_mut(s.0)
                .set_name(nm, &mut self.grammar.rand_state);
        }
        self.grammar.add_set(s)
    }
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn]
    fn parse_contextual_test_position(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        t: CtxId,
    ) -> ParseResult {
        let mut negative = false;
        let mut had_digits = false;

        let n = *pos;

        let mut posb = self.grammar.contexts_arena[t.0].pos;
        let mut offset: i32 = self.grammar.contexts_arena[t.0].offset;
        let mut jump_pos: i8 = self.grammar.contexts_arena[t.0].jump_pos;

        // Run of INDEPENDENT ifs; order matters (quirk reproduced).
        let mut tries = 0usize;
        while buf[*pos] != ' ' && buf[*pos] != '(' && buf[*pos] != '/' && tries < 100 {
            if buf[*pos] == '*' && buf[*pos + 1] == '*' {
                posb |= POS_SCANALL;
                *pos += 2;
            }
            if buf[*pos] == '*' {
                posb |= POS_SCANFIRST;
                *pos += 1;
            }
            if buf[*pos] == 'C' {
                posb |= POS_CAREFUL;
                *pos += 1;
            }
            if buf[*pos] == 'c' {
                posb |= POS_DEP_CHILD;
                *pos += 1;
            }
            if buf[*pos] == 'c' && (posb.intersects(POS_DEP_CHILD)) {
                posb &= !POS_DEP_CHILD;
                posb |= POS_DEP_GLOB;
                *pos += 1;
            }
            if buf[*pos] == 'p' {
                posb |= POS_DEP_PARENT;
                *pos += 1;
            }
            if buf[*pos] == 'p' && (posb.intersects(POS_DEP_PARENT)) {
                posb |= POS_DEP_GLOB;
                *pos += 1;
            }
            if buf[*pos] == 's' {
                posb |= POS_DEP_SIBLING;
                *pos += 1;
            }
            if buf[*pos] == 'S' {
                posb |= POS_SELF;
                *pos += 1;
            }
            if buf[*pos] == 'N' {
                posb |= POS_NO_BARRIER;
                *pos += 1;
            }
            if buf[*pos] == '<' {
                posb |= POS_SPAN_LEFT;
                *pos += 1;
            }
            if buf[*pos] == '>' {
                posb |= POS_SPAN_RIGHT;
                *pos += 1;
            }
            if buf[*pos] == 'W' {
                posb |= POS_SPAN_BOTH;
                *pos += 1;
            }
            if buf[*pos] == '@' {
                posb |= POS_ABSOLUTE;
                *pos += 1;
            }
            if buf[*pos] == 'O' {
                posb |= POS_NO_PASS_ORIGIN;
                *pos += 1;
            }
            if buf[*pos] == 'o' {
                posb |= POS_PASS_ORIGIN;
                *pos += 1;
            }
            if buf[*pos] == 'L' {
                posb |= POS_LEFT_PAR;
                *pos += 1;
            }
            if buf[*pos] == 'R' {
                posb |= POS_RIGHT_PAR;
                *pos += 1;
            }
            if buf[*pos] == 'X' {
                posb |= POS_MARK_SET;
                *pos += 1;
            }
            if buf[*pos] == 'x' {
                posb |= POS_JUMP;
                *pos += 1;
            }
            if buf[*pos] == 'D' {
                posb |= POS_LOOK_DELETED;
                *pos += 1;
            }
            if buf[*pos] == 'd' {
                posb |= POS_LOOK_DELAYED;
                *pos += 1;
            }
            if buf[*pos] == 'I' {
                posb |= POS_LOOK_IGNORED;
                *pos += 1;
            }
            if buf[*pos] == 'A' {
                posb |= POS_ATTACH_TO;
                *pos += 1;
            }
            if buf[*pos] == 'w' {
                posb |= POS_WITH;
                *pos += 1;
            }
            if buf[*pos] == '?' {
                posb |= POS_UNKNOWN;
                *pos += 1;
            }
            if buf[*pos] == 'f' {
                posb |= POS_NUMERIC_BRANCH;
                *pos += 1;
            }
            if buf[*pos] == 'T' {
                posb |= POS_ACTIVE;
                *pos += 1;
            }
            if buf[*pos] == 't' {
                posb |= POS_INACTIVE;
                *pos += 1;
            }
            if buf[*pos] == 'B' {
                self.grammar.has_bag_of_tags = true;
                posb |= POS_BAG_OF_TAGS;
                *pos += 1;
            }
            if buf[*pos] == '-' {
                negative = true;
                *pos += 1;
            }
            if u_isdigit(buf[*pos]) {
                had_digits = true;
                while buf[*pos] >= '0' && buf[*pos] <= '9' {
                    offset = (offset * 10) + (buf[*pos] as i32 - '0' as i32);
                    *pos += 1;
                }
            }
            if buf[*pos] == 'r' && buf[*pos + 1] == ':' {
                posb |= POS_RELATION;
                *pos += 2;
                let mut nn = *pos;
                skiptows_chars(buf, &mut nn, '(', false, false);
                let token: String = buf[*pos..nn].iter().collect();
                let tag = self.parse_tag(&token, &buf[*pos..])?;
                self.grammar.contexts_arena[t.0].relation =
                    self.grammar.single_tags_list[tag.0].hash.get();
                *pos = nn;
            }
            if buf[*pos] == 'r' {
                posb |= POS_RIGHT;
                *pos += 1;
            }
            if buf[*pos] == 'r' && (posb.intersects(POS_RIGHT)) {
                posb &= !POS_RIGHT;
                posb |= POS_RIGHTMOST;
                *pos += 1;
            }
            if buf[*pos] == 'l' {
                posb |= POS_LEFT;
                *pos += 1;
            }
            if buf[*pos] == 'l' && (posb.intersects(POS_LEFT)) {
                posb &= !POS_LEFT;
                posb |= POS_LEFTMOST;
                *pos += 1;
            }
            if buf[*pos] == 'j' {
                if buf[*pos + 1] == 'M' {
                    posb |= POS_JUMP;
                    *pos += 2;
                } else if buf[*pos + 1] == 'A' {
                    posb |= POS_JUMP;
                    jump_pos = PosJumpPos::JumpAttach as i8;
                    *pos += 2;
                } else if buf[*pos + 1] == 'T' {
                    posb |= POS_JUMP;
                    jump_pos = PosJumpPos::JumpTarget as i8;
                    *pos += 2;
                } else if buf[*pos + 1] == 'C' && u_isdigit(buf[*pos + 2]) {
                    *pos += 2;
                    posb |= POS_JUMP;
                    jump_pos = (buf[*pos] as i32 - '0' as i32) as i8;
                    *pos += 1;
                }
            }
            tries += 1;
        }

        if negative {
            offset = -offset.abs();
        }
        self.grammar.contexts_arena[t.0].offset = offset;

        let mut offset_sub: i32 = self.grammar.contexts_arena[t.0].offset_sub;
        if buf[*pos] == '/' {
            *pos += 1;
            let mut negative2 = false;
            let mut tries2 = 0usize;
            while buf[*pos] != ' ' && buf[*pos] != '(' && tries2 < 100 {
                if buf[*pos] == '*' && buf[*pos + 1] == '*' {
                    offset_sub = GsrSpecials::GsrAny as i32;
                    *pos += 2;
                }
                if buf[*pos] == '*' {
                    offset_sub = GsrSpecials::GsrAny as i32;
                    *pos += 1;
                }
                if buf[*pos] == '-' {
                    negative2 = true;
                    *pos += 1;
                }
                if u_isdigit(buf[*pos]) {
                    while buf[*pos] >= '0' && buf[*pos] <= '9' {
                        offset_sub = (offset_sub * 10) + (buf[*pos] as i32 - '0' as i32);
                        *pos += 1;
                    }
                }
                tries2 += 1;
            }
            if negative2 {
                offset_sub = -offset_sub.abs();
            }
        }
        self.grammar.contexts_arena[t.0].offset_sub = offset_sub;

        if self.self_no_barrier && (posb.intersects(POS_SELF)) {
            if posb.intersects(POS_NO_BARRIER) {
                posb &= !POS_NO_BARRIER;
            } else {
                posb |= POS_NO_BARRIER;
            }
        }

        if (posb.intersects(POS_DEP_CHILD | POS_DEP_SIBLING))
            && (posb.intersects(POS_SCANFIRST | POS_SCANALL))
        {
            posb &= !POS_SCANFIRST;
            posb &= !POS_SCANALL;
            posb |= POS_DEP_DEEP;
        }

        if tries >= 100 {
            return Err(self.error_near(&buf[n..])); // unknown specifier
        } else if tries >= 20 {
            tracing::warn!("{}: Warning: Position took many loops.", self.filebase);
        }
        if !isspace(buf[*pos]) {
            return Err(self.error_near(&buf[n..])); // garbage data
        }
        if *pos - n == 1 && (buf[n] == 'o' || buf[n] == 'O') {
            return Err(self.error_near(&buf[n..])); // stand-alone o/O
        }

        if had_digits {
            if posb.intersects(POS_DEP_CHILD | POS_DEP_SIBLING | POS_DEP_PARENT) {
                return Err(self.error_near(&buf[n..]));
            }
            if posb.intersects(POS_LEFT_PAR | POS_RIGHT_PAR) {
                return Err(self.error_near(&buf[n..]));
            }
            if posb.intersects(POS_RELATION) {
                return Err(self.error_near(&buf[n..]));
            }
        }
        if (posb.intersects(POS_BAG_OF_TAGS))
            && (posb.intersects(
                !(POS_BAG_OF_TAGS
                    | POS_NOT
                    | POS_NEGATE
                    | POS_SPAN_BOTH
                    | POS_SPAN_LEFT
                    | POS_SPAN_RIGHT),
            ) || had_digits)
        {
            return Err(self.error_near(&buf[n..]));
        }
        if (posb.intersects(POS_DEP_PARENT))
            && (!posb.intersects(POS_DEP_GLOB))
            && (posb.intersects(POS_LEFTMOST | POS_RIGHTMOST))
        {
            return Err(self.error_near(&buf[n..]));
        }
        if (posb.intersects(POS_PASS_ORIGIN)) && (posb.intersects(POS_NO_PASS_ORIGIN)) {
            return Err(self.error_near(&buf[n..]));
        }
        if (posb.intersects(POS_LEFT_PAR)) && (posb.intersects(POS_RIGHT_PAR)) {
            return Err(self.error_near(&buf[n..]));
        }
        if (posb.intersects(POS_ALL)) && (posb.intersects(POS_NONE)) {
            return Err(self.error_near(&buf[n..]));
        }
        if (posb.intersects(POS_UNKNOWN)) && (posb != POS_UNKNOWN || had_digits) {
            return Err(self.error_near(&buf[n..]));
        }
        if (posb.intersects(POS_SCANALL)) && (posb.intersects(POS_NOT)) {
            tracing::warn!("{}: Warning: mixing NOT and ** ...", self.filebase);
        }

        if posb.bits() > POS_64BIT.bits() {
            posb |= POS_64BIT;
        }

        self.grammar.contexts_arena[t.0].pos = posb;
        self.grammar.contexts_arena[t.0].jump_pos = jump_pos;
        Ok(())
    }
}

impl TextualParser {
    /// The `label_parseTemplateRef` body: read the template name, stash the
    /// name-hash placeholder in `tmpl`, and return `(line, name)` for deferral.
    fn parse_template_ref_body(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        t_cur: CtxId,
    ) -> (usize, String) {
        *pos += 2;
        let mut n = *pos;
        self.grammar.lines += skiptows_chars(buf, &mut n, ')', false, false);
        let name: String = buf[*pos..n].iter().collect();
        let cn = hash_value_ustring(&name, 0);
        // Placeholder: hold the name-hash in `tmpl` (C++ reinterpret_cast<CT*>(cn)).
        self.grammar.contexts_arena[t_cur.0].tmpl = Some(CtxId(cn));
        let tmpl_data = (self.grammar.lines as usize, name);
        *pos = n;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        tmpl_data
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn]
    fn parse_contextual_test_list(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        rule_flags: Option<crate::rule::RuleFlags>,
        in_tmpl: bool,
    ) -> ParseResult<CtxId> {
        // C++ `AST_OPEN(Context)` — also the profiler's context span start.
        let ast_ctx_b = *pos;
        let mut ast_context = ASTHelper::new(
            &mut self.ast,
            ASTType::AstContext,
            self.grammar.lines as usize,
            *pos,
            self.cur_grammar_buf.clone(),
        );
        let ot = self.grammar.allocate_contextual_test();
        let mut t_cur = ot;
        self.grammar.contexts_arena[ot.0].line = self.grammar.lines;

        let mut tmpl_data: Option<(usize, String)> = None;

        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_TEXTNEGATE) {
            *pos += slen(STR_TEXTNEGATE);
            self.grammar.contexts_arena[ot.0].pos |= POS_NEGATE;
        }
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_ALL) {
            *pos += slen(STR_ALL);
            self.grammar.contexts_arena[ot.0].pos |= POS_ALL;
        }
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_NONE) {
            *pos += slen(STR_NONE);
            self.grammar.contexts_arena[ot.0].pos |= POS_NONE;
        }
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_TEXTNOT) {
            *pos += slen(STR_TEXTNOT);
            self.grammar.contexts_arena[ot.0].pos |= POS_NOT;
        }
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        // Peek the token up to '('.
        let mut n_peek = *pos;
        self.grammar.lines += skiptows_chars(buf, &mut n_peek, '(', false, false);
        let token: String = buf[*pos..n_peek].iter().collect();

        if ux_is_empty(&token) {
            // (1) Inline template.
            if self.no_itmpls {
                return Err(self.error_near(&buf[*pos..]));
            }
            *pos = n_peek;
            loop {
                if buf[*pos] != '(' {
                    return Err(self.error_near(&buf[*pos..]));
                }
                *pos += 1;
                let ored = self.parse_contextual_test_list(buf, pos, rule_flags, true)?;
                *pos += 1;
                self.grammar.contexts_arena[t_cur.0].ors.push(ored);
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                if simplecasecmp(buf, *pos, STR_OR) {
                    *pos += slen(STR_OR);
                } else {
                    break;
                }
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            }
            if self.grammar.contexts_arena[t_cur.0].ors.len() == 1 && self.verbosity_level > 0 {
                tracing::warn!("{}: Warning: inline template ...", self.filebase);
            }
        } else if token.starts_with('[') {
            // (2) Template shorthand [set, set, ...].
            *pos += 1;
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            let s = self.parse_set_inline_wrapper(buf, pos)?;
            self.grammar.contexts_arena[t_cur.0].offset = 1;
            self.grammar.contexts_arena[t_cur.0].target =
                SetNumber(self.grammar.sets_list[s.0].hash);
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            while buf[*pos] == ',' {
                *pos += 1;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                let lnk = self.grammar.allocate_contextual_test();
                let s2 = self.parse_set_inline_wrapper(buf, pos)?;
                self.grammar.contexts_arena[lnk.0].offset = 1;
                self.grammar.contexts_arena[lnk.0].target =
                    SetNumber(self.grammar.sets_list[s2.0].hash);
                self.grammar.contexts_arena[t_cur.0].linked = Some(lnk);
                t_cur = lnk;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            }
            if buf[*pos] != ']' {
                return Err(self.error_near(&buf[*pos..]));
            }
            *pos += 1;
        } else {
            // (3) T: template-ref peek, OR (4) a normal test. `goto_template`
            // reproduces the `goto label_parseTemplateRef` (skips position + first
            // SKIPWS, lands at the template-ref body, then continues to barriers).
            let goto_template = {
                let tc: Vec<char> = token.chars().collect();
                !tc.is_empty() && tc[0] == 'T' && tc.get(1) == Some(&':')
            };

            if !goto_template {
                self.parse_contextual_test_position(buf, pos, t_cur)?;
                *pos = n_peek;
                let pb = self.grammar.contexts_arena[t_cur.0].pos;
                if pb.intersects(POS_DEP_CHILD | POS_DEP_PARENT | POS_DEP_SIBLING) {
                    self.grammar.has_dep = true;
                }
                if pb.intersects(POS_RELATION) {
                    self.grammar.has_relations = true;
                }
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            }

            if goto_template || (buf[*pos] == 'T' && buf[*pos + 1] == ':') {
                if !goto_template {
                    self.grammar.contexts_arena[t_cur.0].pos |= POS_TMPL_OVERRIDE;
                }
                tmpl_data = Some(self.parse_template_ref_body(buf, pos, t_cur));
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            } else {
                let s = self.parse_set_inline_wrapper(buf, pos)?;
                self.grammar.contexts_arena[t_cur.0].target =
                    SetNumber(self.grammar.sets_list[s.0].hash);
            }

            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            if simplecasecmp(buf, *pos, STR_CBARRIER) {
                *pos += slen(STR_CBARRIER);
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                let s = self.parse_set_inline_wrapper(buf, pos)?;
                self.grammar.contexts_arena[t_cur.0].cbarrier =
                    SetNumber(self.grammar.sets_list[s.0].hash);
            }
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            if simplecasecmp(buf, *pos, STR_BARRIER) {
                *pos += slen(STR_BARRIER);
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                let s = self.parse_set_inline_wrapper(buf, pos)?;
                self.grammar.contexts_arena[t_cur.0].barrier =
                    SetNumber(self.grammar.sets_list[s.0].hash);
            }
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

            let (barrier, cbarrier, pb) = {
                let c = &self.grammar.contexts_arena[t_cur.0];
                (c.barrier, c.cbarrier, c.pos)
            };
            if (barrier.get() != 0 || cbarrier.get() != 0)
                && (!pb.intersects(MASK_POS_SCAN | POS_SELF))
            {
                tracing::warn!(
                    "{}: Warning: Barriers only make sense for scanning or self tests.",
                    self.filebase
                );
                self.grammar.contexts_arena[t_cur.0].barrier = SetNumber(0);
                self.grammar.contexts_arena[t_cur.0].cbarrier = SetNumber(0);
            }
        }

        let mut linked = false;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_AND) {
            return Err(self.error_near(&buf[*pos..])); // AND deprecated
        }
        if simplecasecmp(buf, *pos, STR_LINK) {
            *pos += slen(STR_LINK);
            linked = true;
        }
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        if linked {
            let l = self.parse_contextual_test_list(buf, pos, rule_flags, in_tmpl)?;
            self.grammar.contexts_arena[t_cur.0].linked = Some(l);
            if self.grammar.contexts_arena[t_cur.0]
                .pos
                .intersects(POS_NONE)
            {
                return Err(self.error_near(&buf[*pos..])); // LINK from a NONE test
            }
        } else if !in_tmpl
            && (self.grammar.contexts_arena[t_cur.0]
                .pos
                .intersects(POS_SCANALL))
            && (!self.grammar.contexts_arena[t_cur.0]
                .pos
                .intersects(POS_CAREFUL))
        {
            tracing::warn!(
                "{}: Warning: ** without LINK or C doesn't make sense.",
                self.filebase
            );
        }

        if let Some(rf) = rule_flags {
            if rf.intersects(RF_LOOKDELETED) {
                self.grammar.contexts_arena[t_cur.0].pos |= POS_LOOK_DELETED;
            }
            if rf.intersects(RF_LOOKDELAYED) {
                self.grammar.contexts_arena[t_cur.0].pos |= POS_LOOK_DELAYED;
            }
            if rf.intersects(RF_LOOKIGNORED) {
                self.grammar.contexts_arena[t_cur.0].pos |= POS_LOOK_IGNORED;
            }
        }

        let t = self.grammar.add_contextual_test(Some(ot)).unwrap();
        if let Some(prof) = self.profiler.as_mut() {
            // profiler->addContext(t->hash, cur_grammar_n,
            //                      cur_ast->b - cur_grammar, p - cur_grammar)
            let th = self.grammar.contexts_arena[t.0].hash;
            prof.add_context(th, self.cur_grammar_n, ast_ctx_b - 4, *pos - 4);
        }
        if self.grammar.contexts_arena[t.0].tmpl.is_some()
            && let Some(td) = tmpl_data
        {
            self.deferred_tmpls.insert(t, td);
        }

        // C++ `AST_CLOSE_ID(p, t->hash)`.
        let t_hash = self.grammar.contexts_arena[t.0].hash;
        ast_context.close_id(&mut self.ast, *pos, t_hash);

        Ok(t)
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-tests-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-tests-fn]
    fn parse_contextual_tests(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        rule: &mut Rule,
    ) -> ParseResult {
        let rf = rule.flags;
        let t = self.parse_contextual_test_list(buf, pos, Some(rf), false)?;
        if self.option_vislcg_compat && (self.grammar.contexts_arena[t.0].pos.intersects(POS_NOT)) {
            self.grammar.contexts_arena[t.0].pos &= !POS_NOT;
            self.grammar.contexts_arena[t.0].pos |= POS_NEGATE;
        }
        Rule::add_contextual_test(t, &mut rule.tests);
        Ok(())
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-dependency-tests-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-dependency-tests-fn]
    fn parse_contextual_dependency_tests(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        rule: &mut Rule,
    ) -> ParseResult {
        let rf = rule.flags;
        let t = self.parse_contextual_test_list(buf, pos, Some(rf), false)?;
        if self.option_vislcg_compat && (self.grammar.contexts_arena[t.0].pos.intersects(POS_NOT)) {
            self.grammar.contexts_arena[t.0].pos &= !POS_NOT;
            self.grammar.contexts_arena[t.0].pos |= POS_NEGATE;
        }
        Rule::add_contextual_test(t, &mut rule.dep_tests);
        Ok(())
    }
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-rule-flags-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-flags-fn]
    fn parse_rule_flags(&mut self, buf: &[char], pos: &mut usize) -> ParseResult<DynBitset> {
        let mut rv = DynBitset::default();

        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        let lp = *pos;
        let mut setflag = true;
        while setflag {
            setflag = false;
            // `i` is both the flag BIT index (`1 << i`) and the index of the
            // flag's keyword in the parallel `G_FLAGS` table.
            for (i, flag_name) in G_FLAGS.iter().enumerate() {
                let op = *pos;
                if simplecasecmp(buf, *pos, flag_name) {
                    *pos += slen(flag_name);
                    rv.flags |= crate::rule::RuleFlags::from_bits_retain(1u64 << i);
                    setflag = true;

                    let mut undo = false;
                    if i == FL_SUB {
                        if buf[*pos] != ':' {
                            undo = true;
                        } else {
                            *pos += 1;
                            let mut n = *pos;
                            self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
                            let token: String = buf[*pos..n].iter().collect();
                            *pos = n;
                            if token.starts_with('*') {
                                rv.sub_reading = GsrSpecials::GsrAny as i32;
                            } else {
                                rv.sub_reading = scan_d(&token);
                            }
                        }
                    }

                    if !undo && buf[*pos] != '(' && buf[*pos] != ';' && !isspace(buf[*pos]) {
                        undo = true;
                    }

                    if undo {
                        rv.flags &= !crate::rule::RuleFlags::from_bits_retain(1u64 << i);
                        *pos = op;
                        setflag = false;
                        break;
                    }
                }
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                if buf[*pos] == '(' || buf[*pos] == 'T' || buf[*pos] == 't' || buf[*pos] == ';' {
                    setflag = false;
                    break;
                }
            }
            if rv
                .flags
                .intersects(RF_WITHCHILD | RF_NOCHILD | RF_BEFORE | RF_AFTER)
            {
                break;
            }
        }

        for excl in FLAG_EXCLS_GROUPS {
            let bits = rv.flags & excl;
            if bits.bits().count_ones() > 1 {
                return Err(self.error_near(&buf[lp..]));
            }
        }

        if rv.flags.intersects(RF_UNMAPLAST) && rv.flags.intersects(RF_SAFE) {
            return Err(self.error_near(&buf[lp..]));
        }

        if rv.flags.intersects(RF_UNMAPLAST) {
            rv.flags |= RF_UNSAFE;
        }
        if rv.flags.intersects(RF_REMEMBERX) {
            rv.flags |= RF_KEEPORDER;
        }
        if rv.flags.intersects(RF_ENCL_FINAL) {
            self.grammar.has_encl_final = true;
        }
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        Ok(rv)
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.maybe-parse-rule-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.maybe-parse-rule-fn]
    fn maybe_parse_rule(&mut self, buf: &[char], pos: &mut usize) -> ParseResult<bool> {
        let p = *pos;
        // Longer names first; order-sensitive where one is a prefix of another.
        if is_icase_kw(buf, p, "ADDRELATIONS", "addrelations") != 0 {
            self.parse_rule(buf, pos, Keywords::KAddrelations)?;
        } else if is_icase_kw(buf, p, "SETRELATIONS", "setrelations") != 0 {
            self.parse_rule(buf, pos, Keywords::KSetrelations)?;
        } else if is_icase_kw(buf, p, "REMRELATIONS", "remrelations") != 0 {
            self.parse_rule(buf, pos, Keywords::KRemrelations)?;
        } else if is_icase_kw(buf, p, "ADDRELATION", "addrelation") != 0 {
            self.parse_rule(buf, pos, Keywords::KAddrelation)?;
        } else if is_icase_kw(buf, p, "SETRELATION", "setrelation") != 0 {
            self.parse_rule(buf, pos, Keywords::KSetrelation)?;
        } else if is_icase_kw(buf, p, "REMRELATION", "remrelation") != 0 {
            self.parse_rule(buf, pos, Keywords::KRemrelation)?;
        } else if is_icase_kw(buf, p, "SETVARIABLE", "setvariable") != 0 {
            self.parse_rule(buf, pos, Keywords::KSetvariable)?;
        } else if is_icase_kw(buf, p, "REMVARIABLE", "remvariable") != 0 {
            self.parse_rule(buf, pos, Keywords::KRemvariable)?;
        } else if is_icase_kw(buf, p, "SETPARENT", "setparent") != 0 {
            self.parse_rule(buf, pos, Keywords::KSetparent)?;
        } else if is_icase_kw(buf, p, "SETCHILD", "setchild") != 0 {
            self.parse_rule(buf, pos, Keywords::KSetchild)?;
        } else if is_icase_kw(buf, p, "REMPARENT", "remparent") != 0 {
            self.parse_rule(buf, pos, Keywords::KRemparent)?;
        } else if is_icase_kw(buf, p, "SWITCHPARENT", "switchparent") != 0 {
            self.parse_rule(buf, pos, Keywords::KSwitchparent)?;
        } else if is_icase_kw(buf, p, "RESTORE", "restore") != 0 {
            self.parse_rule(buf, pos, Keywords::KRestore)?;
        } else if is_icase_kw(buf, p, "IFF", "iff") != 0 {
            self.parse_rule(buf, pos, Keywords::KIff)?;
        } else if is_icase_kw(buf, p, "MAP", "map") != 0 {
            self.parse_rule(buf, pos, Keywords::KMap)?;
        } else if is_icase_kw(buf, p, "ADD", "add") != 0 {
            self.parse_rule(buf, pos, Keywords::KAdd)?;
        } else if is_icase_kw(buf, p, "APPEND", "append") != 0 {
            self.parse_rule(buf, pos, Keywords::KAppend)?;
        } else if is_icase_kw(buf, p, "SELECT", "select") != 0 {
            self.parse_rule(buf, pos, Keywords::KSelect)?;
        } else if is_icase_kw(buf, p, "REMOVE", "remove") != 0 {
            self.parse_rule(buf, pos, Keywords::KRemove)?;
        } else if is_icase_kw(buf, p, "REPLACE", "replace") != 0 {
            self.parse_rule(buf, pos, Keywords::KReplace)?;
        } else if is_icase_kw(buf, p, "SUBSTITUTE", "substitute") != 0 {
            self.parse_rule(buf, pos, Keywords::KSubstitute)?;
        } else if is_icase_kw(buf, p, "COPYCOHORT", "copycohort") != 0 {
            self.parse_rule(buf, pos, Keywords::KCopycohort)?;
        } else if is_icase_kw(buf, p, "COPY", "copy") != 0 {
            self.parse_rule(buf, pos, Keywords::KCopy)?;
        } else if is_icase_kw(buf, p, "UNMAP", "unmap") != 0 {
            self.parse_rule(buf, pos, Keywords::KUnmap)?;
        } else if is_icase_kw(buf, p, "PROTECT", "protect") != 0 {
            self.parse_rule(buf, pos, Keywords::KProtect)?;
        } else if is_icase_kw(buf, p, "UNPROTECT", "unprotect") != 0 {
            self.parse_rule(buf, pos, Keywords::KUnprotect)?;
        } else if is_icase_kw(buf, p, "DELIMIT", "delimit") != 0 {
            self.parse_rule(buf, pos, Keywords::KDelimit)?;
        } else if is_icase_kw(buf, p, "JUMP", "jump") != 0 {
            self.parse_rule(buf, pos, Keywords::KJump)?;
        } else if is_icase_kw(buf, p, "MOVE", "move") != 0 {
            self.parse_rule(buf, pos, Keywords::KMove)?;
        } else if is_icase_kw(buf, p, "SWITCH", "switch") != 0 {
            self.parse_rule(buf, pos, Keywords::KSwitch)?;
        } else if is_icase_kw(buf, p, "EXECUTE", "execute") != 0 {
            self.parse_rule(buf, pos, Keywords::KExecute)?;
        } else if is_icase_kw(buf, p, "EXTERNAL", "external") != 0 {
            self.parse_rule(buf, pos, Keywords::KExternal)?;
        } else if is_icase_kw(buf, p, "REMCOHORT", "remcohort") != 0 {
            self.parse_rule(buf, pos, Keywords::KRemcohort)?;
        } else if is_icase_kw(buf, p, "ADDCOHORT", "addcohort") != 0 {
            self.parse_rule(buf, pos, Keywords::KAddcohort)?;
        } else if is_icase_kw(buf, p, "SPLITCOHORT", "splitcohort") != 0 {
            self.parse_rule(buf, pos, Keywords::KSplitcohort)?;
        } else if is_icase_kw(buf, p, "MERGECOHORTS", "mergecohorts") != 0 {
            self.parse_rule(buf, pos, Keywords::KMergecohorts)?;
        } else if is_icase_kw(buf, p, "RESTORE", "restore") != 0 {
            // Dead duplicate branch (never reached — the first RESTORE wins).
            self.parse_rule(buf, pos, Keywords::KRestore)?;
        } else if is_icase_kw(buf, p, "WITH", "with") != 0 {
            self.parse_rule(buf, pos, Keywords::KWith)?;
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-anchorish-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-anchorish-fn]
    fn parse_anchorish(&mut self, buf: &[char], pos: &mut usize, rule_flags: bool) -> ParseResult {
        if buf[*pos] != ':' {
            let mut n = *pos;
            self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
            let name: String = buf[*pos..n].iter().collect();
            if !self.only_sets {
                let at = ui32(self.grammar.rule_by_number.capacity());
                self.grammar.add_anchor(&name, at, true)?;
            }
            *pos = n;
        }

        self.grammar.lines += skipws_chars(buf, pos, ':', '\0', false);
        if rule_flags && buf[*pos] == ':' {
            *pos += 1;
            self.section_flags = self.parse_rule_flags(buf, pos)?;
        }

        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            return Err(self.error_near(&buf[*pos..]));
        }
        Ok(())
    }
}
