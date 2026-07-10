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
//!   `&data[4]`). `buf` is reconstructed via `slice::from_raw_parts` from a
//!   `grammarbufs` entry so it does NOT borrow `self` — letting every parse
//!   method take `&mut self` + `buf` + `pos` without a borrow conflict. The char
//!   data of each `grammarbufs` entry is heap-stable and never mutated after
//!   creation (only new buffers are pushed), so the shared `buf` slice never
//!   aliases a live `&mut` into the same data — faithful to the C++ raw pointers
//!   into stable `unique_ptr<UString>` buffers.
//! * **Errors / exceptions.** The C++ `error(...)` is `[[noreturn]]` and throws
//!   an `int` caught by the per-statement `try/catch(int)` in `parseFromUChar`
//!   (which recovers by skipping to the next line). Ported with `panic_any`
//!   ([`ParseError`]) + [`catch_unwind`] in `parse_from_u_char`; a non-`ParseError`
//!   payload is re-raised. `incErrorCount`'s `>= 10` bail is `cg3_quit` (exit).
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

#![allow(clippy::too_many_arguments)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};

use regex::Regex;

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::ast::{Ast, ASTHelper, ASTType};
use crate::contextual_test::{
    GSR_SPECIALS, MASK_POS_SCAN, POS_64BIT, POS_ABSOLUTE, POS_ACTIVE, POS_ALL, POS_ATTACH_TO,
    POS_BAG_OF_TAGS, POS_CAREFUL, POS_DEP_CHILD, POS_DEP_DEEP, POS_DEP_GLOB, POS_DEP_PARENT,
    POS_DEP_SIBLING, POS_INACTIVE, POS_JUMP, POS_LEFT, POS_LEFTMOST, POS_LEFT_PAR, POS_LOOK_DELAYED,
    POS_LOOK_DELETED, POS_LOOK_IGNORED, POS_MARK_SET, POS_NEGATE, POS_NO_BARRIER, POS_NONE,
    POS_NOT, POS_NO_PASS_ORIGIN, POS_NUMERIC_BRANCH, POS_PASS_ORIGIN, POS_RELATION, POS_RIGHT,
    POS_RIGHTMOST, POS_RIGHT_PAR, POS_SCANALL, POS_SCANFIRST, POS_SELF, POS_SPAN_BOTH,
    POS_SPAN_LEFT, POS_SPAN_RIGHT, POS_TMPL_OVERRIDE, POS_UNKNOWN, POS_WITH, POS_JUMP_POS,
    copy_cntx,
};
use crate::grammar::Grammar;
use crate::igrammar_parser::IGrammarParser;
use crate::inlines::{
    backtonl, cg3_quit, hash_value_ustring, isnl, isspace, skipln, skipto, skiptows, skipws,
    ui32,
};
use crate::rule::{
    FLAGS_COUNT, FLAGS_EXCLS, RF_AFTER, RF_ALLOWLOOP, RF_BEFORE, RF_DELAYED, RF_ENCL_ANY,
    RF_ENCL_FINAL, RF_ENCL_INNER, RF_ENCL_OUTER, RF_IGNORED, RF_IMMEDIATE, RF_ITERATE, RF_KEEPORDER,
    RF_LOOKDELAYED, RF_LOOKDELETED, RF_LOOKIGNORED, RF_NEAREST, RF_NOCHILD, RF_NOITERATE,
    RF_REMEMBERX, RF_RESETX, RF_REVERSE, RF_SAFE, RF_UNMAPLAST, RF_UNSAFE, RF_VARYORDER,
    RF_WITHCHILD, Rule,
};
use crate::set::{
    ST_CHILD_UNIFY, ST_ORDERED, ST_SET_UNIFY, ST_TAG_UNIFY, Set,
};
use crate::strings::KEYWORDS;
use crate::tag::{
    T_ANY, T_ATTACHTO, T_BASEFORM, T_CASE_INSENSITIVE, T_ENCL, T_FAILFAST, T_LOCAL_VARIABLE, T_MARK,
    T_META, T_PAR_LEFT, T_PAR_RIGHT, T_REGEXP, T_REGEXP_ANY, T_REGEXP_LINE, T_SAME_BASIC, T_SET,
    T_SPECIAL, T_TARGET, T_VARIABLE, T_VARSTRING, T_VSTR, T_WORDFORM, Tag, TagVector, TagVectorSet,
    compare_tag_vector,
};
use crate::tag_trie::{trie_get_tags, trie_insert};
use crate::sorted_vector::uint32SortedVector;
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
const STR_UU_C: [&str; 9] = ["_C1_", "_C2_", "_C3_", "_C4_", "_C5_", "_C6_", "_C7_", "_C8_", "_C9_"];
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
const G_FLAGS: [&str; 34] = [
    "NEAREST", "ALLOWLOOP", "DELAYED", "IMMEDIATE", "LOOKDELETED", "LOOKDELAYED", "UNSAFE", "SAFE",
    "REMEMBERX", "RESETX", "KEEPORDER", "VARYORDER", "ENCL_INNER", "ENCL_OUTER", "ENCL_FINAL",
    "ENCL_ANY", "ALLOWCROSS", "WITHCHILD", "NOCHILD", "ITERATE", "NOITERATE", "UNMAPLAST", "REVERSE",
    "SUB", "OUTPUT", "CAPTURE_UNIF", "REPEAT", "BEFORE", "AFTER", "IGNORED", "LOOKIGNORED",
    "NOMAPPED", "NOPARENT", "DETACH",
];

/// C++ `keywords[KEYWORD_COUNT]` (72). Indexed by `KEYWORDS as usize`.
const KEYWORDS_STR: [&str; 72] = [
    "__CG3_DUMMY_KEYWORD__", "SETS", "LIST", "SET", "DELIMITERS", "SOFT-DELIMITERS",
    "PREFERRED-TARGETS", "MAPPING-PREFIX", "MAPPINGS", "CONSTRAINTS", "CORRECTIONS", "SECTION",
    "BEFORE-SECTIONS", "AFTER-SECTIONS", "NULL-SECTION", "ADD", "MAP", "REPLACE", "SELECT", "REMOVE",
    "IFF", "APPEND", "SUBSTITUTE", "START", "END", "ANCHOR", "EXECUTE", "JUMP", "REMVARIABLE",
    "SETVARIABLE", "DELIMIT", "MATCH", "SETPARENT", "SETCHILD", "ADDRELATION", "SETRELATION",
    "REMRELATION", "ADDRELATIONS", "SETRELATIONS", "REMRELATIONS", "TEMPLATE", "MOVE", "MOVE-AFTER",
    "MOVE-BEFORE", "SWITCH", "REMCOHORT", "STATIC-SETS", "UNMAP", "COPY", "ADDCOHORT",
    "ADDCOHORT-AFTER", "ADDCOHORT-BEFORE", "EXTERNAL", "EXTERNAL-ONCE", "EXTERNAL-ALWAYS", "OPTIONS",
    "STRICT-TAGS", "REOPEN-MAPPINGS", "SUBREADINGS", "SPLITCOHORT", "PROTECT", "UNPROTECT",
    "MERGECOHORTS", "RESTORE", "WITH", "OLIST", "OSET", "CMDARGS", "CMDARGS-OVERRIDE", "COPYCOHORT",
    "REMPARENT", "SWITCHPARENT",
];

/// C++ `flag_excls[]` — the 9 mutually-exclusive rule-flag groups.
const FLAG_EXCLS_GROUPS: [u64; 9] = [
    RF_NEAREST | RF_ALLOWLOOP,
    RF_DELAYED | RF_IMMEDIATE | RF_IGNORED,
    RF_UNSAFE | RF_SAFE,
    RF_REMEMBERX | RF_RESETX,
    RF_KEEPORDER | RF_VARYORDER,
    RF_ENCL_INNER | RF_ENCL_OUTER | RF_ENCL_FINAL | RF_ENCL_ANY,
    RF_WITHCHILD | RF_NOCHILD,
    RF_ITERATE | RF_NOITERATE,
    RF_BEFORE | RF_AFTER,
];

/// C++ `struct flags_t { rule_flags_t flags = 0; int32_t sub_reading = 0; }`
/// (Strings.hpp). Not `crate::types::flags_t` (a bitset); this is the rule-flag
/// return payload of `parseRuleFlags`.
#[derive(Clone, Copy, Default)]
struct flags_t {
    flags: u64,
    sub_reading: i32,
}

/// Panic payload for the `error(...)` / `catch(int)` control flow.
struct ParseError(#[allow(dead_code)] i32);

// ---------------------------------------------------------------------------
// Free helpers.
// ---------------------------------------------------------------------------

#[inline]
fn slen(s: &str) -> usize {
    s.chars().count()
}

/// Raw begin/end pointer into the source buffer for AST nodes.
#[inline]
fn pptr(buf: &[char], pos: usize) -> *const char {
    unsafe { buf.as_ptr().add(pos) }
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
    crate::inlines::is_icase(buf, pos, &ucv, &lcv)
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
    if !(trie_empty && trie_sp_empty && (st & (ST_TAG_UNIFY | ST_SET_UNIFY | ST_CHILD_UNIFY)) == 0) {
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
                    if grammar.single_tags_list[tag.0].r#type & (T_FAILFAST | T_REGEXP_LINE) != 0 {
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

// [spec:cg3:def:textual-parser.cg3.freq-sorter]
/// Translation-unit-local comparator functor `struct freq_sorter` (C++
/// `TextualParser.cpp` ~line 114). Holds a reference to the tag→frequency map
/// (`bc::flat_map<Tag*, size_t>` → `&BTreeMap<TagId, usize>` in the arena model)
/// and orders highest-frequency-first for cheap trie compression. In the port the
/// single live use is the inlined `tv.sort_by(...)` in `do_grammar_actions`; this
/// faithful reproduction carries the three manifest ids and is exercised via the
/// `operator()` equivalent `compare`.
#[allow(dead_code)]
struct freq_sorter<'a> {
    tag_freq: &'a BTreeMap<TagId, usize>,
}

#[allow(dead_code)]
impl<'a> freq_sorter<'a> {
    // [spec:cg3:def:textual-parser.cg3.freq-sorter.freq-sorter-fn]
    // [spec:cg3:sem:textual-parser.cg3.freq-sorter.freq-sorter-fn]
    /// `freq_sorter(const bc::flat_map<Tag*, size_t>& tag_freq)` — stores the map
    /// by reference in the member `tag_freq`; empty body.
    fn new(tag_freq: &'a BTreeMap<TagId, usize>) -> Self {
        freq_sorter { tag_freq }
    }

    // [spec:cg3:def:textual-parser.cg3.freq-sorter.operator-fn]
    // [spec:cg3:sem:textual-parser.cg3.freq-sorter.operator-fn]
    /// `bool operator()(Tag* a, Tag* b) const` — sorts highest-frequency-first:
    /// returns `tag_freq[a] > tag_freq[b]` (dereferences `find(...)->second` with
    /// no end-check, so both keys must be present). Used with `std::sort`.
    fn compare(&self, a: TagId, b: TagId) -> bool {
        self.tag_freq[&a] > self.tag_freq[&b]
    }
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
type deferred_t = HashMap<CtxId, (usize, String)>;

// [spec:cg3:def:textual-parser.cg3.textual-parser]
pub struct TextualParser {
    pub grammar: Grammar,
    pub filebase: String,
    pub strict_tags: uint32SortedVector,
    pub list_tags: uint32SortedVector,
    nearbuf: [char; 32],
    verbosity_level: u32,
    sets_counter: u32,
    seen_mapping_prefix: u32,
    section_flags: flags_t,
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
    cur_grammar: *const char,
    cur_grammar_n: u32,
    num_grammars: u32,
    deferred_tmpls: deferred_t,
    grammarbufs: Vec<Vec<char>>,
    error_counter: i32,
    /// Signals the `END` directive breaking the `parseFromUChar` loop.
    parse_end_break: bool,
    pub nrules: Option<Regex>,
    pub nrules_inv: Option<Regex>,
    /// Wave-4 owned AST builder (C++ `thread_local parse_ast/ast/cur_ast`).
    ast: Ast,
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
            strict_tags: uint32SortedVector::new(),
            list_tags: uint32SortedVector::new(),
            nearbuf: ['\0'; 32],
            verbosity_level: 0,
            sets_counter: 100,
            seen_mapping_prefix: 0,
            section_flags: flags_t::default(),
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
            cur_grammar: std::ptr::null(),
            cur_grammar_n: 0,
            num_grammars: 0,
            deferred_tmpls: HashMap::new(),
            grammarbufs: Vec::new(),
            error_counter: 0,
            parse_end_break: false,
            nrules: None,
            nrules_inv: None,
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
            let _ = write!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            let _ = write!(out, "<!-- l is line -->\n");
            let _ = write!(
                out,
                "<!-- b is begin, e is end - both are absolute UTF-16 code unit offsets (not code point) in the file -->\n"
            );
            let _ = write!(out, "<!-- u is the deduplicated objects' unique identifier -->\n");
            crate::ast::print_ast(out, root.cs[0].b, 0, &root.cs[0]);
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.inc-error-count-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.inc-error-count-fn]
    fn inc_error_count(&mut self) -> ! {
        let _ = std::io::stderr().flush();
        self.error_counter += 1;
        if self.error_counter >= 10 {
            tracing::error!("{}: Too many errors - giving up...", self.filebase);
            cg3_quit(1, None, 0);
        }
        panic::panic_any(ParseError(self.error_counter))
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.error-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.error-fn]
    /// The `(str, const UChar* p)` error overload (the near-context form the vast
    /// majority of sites use). `near` is the buffer tail at `p`. Diagnostics text
    /// is simplified; the control flow (near-context copy + `incErrorCount`) is
    /// faithful.
    pub fn error_near(&mut self, near: &[char]) -> ! {
        ux_bufcpy(&mut self.nearbuf, Some(near), 20);
        let nb: String = self.nearbuf.iter().take_while(|&&c| c != '\0').collect();
        tracing::error!("{}: Error on line {} near `{}`!", self.filebase, self.grammar.lines, nb);
        self.inc_error_count()
    }

    /// The `(str)` overload (no near-context; used by the SUB:* guard).
    fn error_bare(&mut self) -> ! {
        tracing::error!("{}: Error on line {}!", self.filebase, self.grammar.lines);
        self.inc_error_count()
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.add-tag-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.add-tag-fn]
    pub fn add_tag(&mut self, tag: Tag) -> TagId {
        self.grammar.add_tag(tag)
    }

    /// C++ `MAYBE_QUOTED(n, p)` macro: skip a `"..."` span if `n` is at a quote.
    fn maybe_quoted(&mut self, buf: &[char], n: &mut usize, near_pos: usize) {
        if buf[*n] == '"' {
            *n += 1;
            crate::inlines::skipto_nospan(buf, n, '"');
            if buf[*n] != '"' {
                self.error_near(&buf[near_pos..]);
            }
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-tag-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-fn]
    pub fn parse_tag(&mut self, to: &str, near: &[char]) -> TagId {
        let tag = crate::parser_helpers::parse_tag(to, near, self, true);
        let (ty, tagstr, plain) = {
            let t = &self.grammar.single_tags_list[tag.0];
            (t.r#type, t.tag.clone(), t.plain_hash)
        };
        if ty & T_VARSTRING != 0 && !tagstr.contains('{') && !tagstr.contains('$') {
            self.error_near(near); // "Varstring tag had no variables"
        }
        if !self.strict_tags.empty() && self.strict_tags.count(plain) == 0 {
            if ty
                & (T_ANY
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
                    | T_SAME_BASIC)
                != 0
            {
                // Always allow...
            } else if tagstr == ">>>" || tagstr == "<<<" {
                // Always allow >>> and <<<
            } else if ty & (T_REGEXP | T_REGEXP_ANY) != 0 {
                if self.strict_regex {
                    self.error_near(near);
                }
            } else if ty & T_CASE_INSENSITIVE != 0 {
                if self.strict_icase {
                    self.error_near(near);
                }
            } else if ty & T_WORDFORM != 0 {
                if self.strict_wforms {
                    self.error_near(near);
                }
            } else if ty & T_BASEFORM != 0 {
                if self.strict_bforms {
                    self.error_near(near);
                }
            } else if tagstr.starts_with('<') && tagstr.ends_with('>') {
                if self.strict_second {
                    self.error_near(near);
                }
            } else {
                self.error_near(near);
            }
        }
        tag
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-fn]
    pub fn parse_set(&mut self, name: &str, near: &[char]) -> SetId {
        crate::parser_helpers::parse_set(name, near, self)
    }
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-tag-list-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-tag-list-fn]
    fn parse_tag_list(&mut self, buf: &[char], pos: &mut usize, s: SetId, ordered: bool) {
        let mut taglists: BTreeSet<TagVector> = BTreeSet::new();
        let mut tag_freq: BTreeMap<TagId, usize> = BTreeMap::new();

        while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
            self.grammar.lines += skipws(buf, pos, ';', ')', false);
            if buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                let mut tags: TagVector = TagVector::new();
                if buf[*pos] == '(' {
                    *pos += 1;
                    self.grammar.lines += skipws(buf, pos, ';', ')', false);
                    while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                        let mut n = *pos;
                        self.maybe_quoted(buf, &mut n, *pos);
                        self.grammar.lines += skiptows(buf, &mut n, ')', true, false);
                        let token: String = buf[*pos..n].iter().collect();
                        let t = self.parse_tag(&token, &buf[*pos..]);
                        tags.push(t);
                        *pos = n;
                        self.grammar.lines += skipws(buf, pos, ';', ')', false);
                    }
                    if buf[*pos] != ')' {
                        self.error_near(&buf[*pos..]);
                    }
                    *pos += 1;
                } else {
                    let mut n = *pos;
                    self.maybe_quoted(buf, &mut n, *pos);
                    self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
                    let token: String = buf[*pos..n].iter().collect();
                    let t = self.parse_tag(&token, &buf[*pos..]);
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
                if self.grammar.single_tags_list[tag.0].r#type & T_SPECIAL != 0 {
                    special = true;
                    break;
                }
            }
            if special {
                trie_insert(&mut self.grammar.sets_list.get_mut(s.0).trie_special, &tv, 0);
            } else {
                trie_insert(&mut self.grammar.sets_list.get_mut(s.0).trie, &tv, 0);
            }
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-inline-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-fn]
    fn parse_set_inline(&mut self, buf: &[char], pos: &mut usize, s: Option<SetId>) -> SetId {
        let mut s = s;
        let mut set_ops: Vec<u32> = Vec::new();
        let mut sets: Vec<u32> = Vec::new();

        let mut wantop = false;
        while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
            self.grammar.lines += skipws(buf, pos, ';', ')', false);
            if buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                if !wantop {
                    if buf[*pos] == '(' {
                        if self.no_isets && buf[*pos + 1] != '*' {
                            self.error_near(&buf[*pos..]);
                        }
                        let n_open = *pos;
                        *pos += 1;
                        let set_c = self.grammar.allocate_set();
                        self.grammar.sets_list[set_c.0].r#type |= ST_ORDERED;
                        self.grammar.sets_list[set_c.0].line = self.grammar.lines;
                        let nm = self.sets_counter;
                        self.sets_counter += 1;
                        self.grammar.sets_list.get_mut(set_c.0).set_name(nm, &mut self.grammar.rand_state);
                        let mut tags: TagVector = TagVector::new();

                        while buf[*pos] != '\0' && buf[*pos] != ';' && buf[*pos] != ')' {
                            self.grammar.lines += skipws(buf, pos, ';', ')', false);
                            let mut n = *pos;
                            self.maybe_quoted(buf, &mut n, *pos);
                            self.grammar.lines += skiptows(buf, &mut n, ')', true, false);
                            let token: String = buf[*pos..n].iter().collect();
                            let t = self.parse_tag(&token, &buf[*pos..]);
                            tags.push(t);
                            *pos = n;
                            self.grammar.lines += skipws(buf, pos, ';', ')', false);
                        }
                        if buf[*pos] != ')' {
                            self.error_near(&buf[*pos..]);
                        }
                        *pos += 1;

                        if tags.is_empty() {
                            self.error_near(&buf[n_open..]);
                        } else if tags.len() == 1 {
                            self.grammar.add_tag_to_set(tags[0], set_c);
                        } else {
                            let mut special = false;
                            for &tag in &tags {
                                if self.grammar.single_tags_list[tag.0].r#type & T_SPECIAL != 0 {
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

                        let set_c = self.grammar.add_set(set_c);
                        let h = self.grammar.sets_list[set_c.0].hash;
                        sets.push(h);
                    } else {
                        let mut n = *pos;
                        self.grammar.lines += skiptows(buf, &mut n, ')', true, false);
                        while buf[n - 1] == ',' || buf[n - 1] == ']' {
                            n -= 1;
                        }
                        let token: String = buf[*pos..n].iter().collect();
                        let tmp = self.parse_set(&token, &buf[*pos..]);
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
                        self.grammar.sets_list.get_mut(set_c.0).set_name(nm, &mut self.grammar.rand_state);

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
                                if self.grammar.single_tags_list[tag.0].r#type & T_SPECIAL != 0 {
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

                        let set_c = self.grammar.add_set(set_c);
                        let h = self.grammar.sets_list[set_c.0].hash;
                        sets.push(h);
                    }

                    wantop = true;
                } else {
                    let mut n = *pos;
                    if buf[n] == '\\' && isspace(buf[n + 1]) {
                        n += 1;
                    } else {
                        self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
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
                self.error_near(&buf[*pos..]);
            }
        }

        if s.is_none() && sets.is_empty() {
            self.error_near(&buf[*pos..]);
        }

        if s.is_none() && sets.len() == 1 {
            s = Some(self.grammar.get_set(*sets.last().unwrap()).unwrap());
        } else {
            let sid = match s {
                Some(sid) => sid,
                None => self.grammar.allocate_set(),
            };
            std::mem::swap(&mut self.grammar.sets_list.get_mut(sid.0).sets, &mut sets);
            std::mem::swap(&mut self.grammar.sets_list.get_mut(sid.0).set_ops, &mut set_ops);
            s = Some(sid);
        }

        s.unwrap()
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-set-inline-wrapper-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-set-inline-wrapper-fn]
    fn parse_set_inline_wrapper(&mut self, buf: &[char], pos: &mut usize) -> SetId {
        let tmplines = self.grammar.lines;
        let s = self.parse_set_inline(buf, pos, None);
        if self.grammar.sets_list[s.0].line == 0 {
            self.grammar.sets_list[s.0].line = tmplines;
        }
        if self.grammar.sets_list[s.0].name.is_empty() {
            let nm = self.sets_counter;
            self.sets_counter += 1;
            self.grammar.sets_list.get_mut(s.0).set_name(nm, &mut self.grammar.rand_state);
        }
        self.grammar.add_set(s)
    }
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-position-fn]
    fn parse_contextual_test_position(&mut self, buf: &[char], pos: &mut usize, t: CtxId) {
        let mut negative = false;
        let mut had_digits = false;

        let n = *pos;

        let mut posb: u64 = self.grammar.contexts_arena[t.0].pos;
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
            if buf[*pos] == 'c' && (posb & POS_DEP_CHILD != 0) {
                posb &= !POS_DEP_CHILD;
                posb |= POS_DEP_GLOB;
                *pos += 1;
            }
            if buf[*pos] == 'p' {
                posb |= POS_DEP_PARENT;
                *pos += 1;
            }
            if buf[*pos] == 'p' && (posb & POS_DEP_PARENT != 0) {
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
                skiptows(buf, &mut nn, '(', false, false);
                let token: String = buf[*pos..nn].iter().collect();
                let tag = self.parse_tag(&token, &buf[*pos..]);
                self.grammar.contexts_arena[t.0].relation =
                    self.grammar.single_tags_list[tag.0].hash;
                *pos = nn;
            }
            if buf[*pos] == 'r' {
                posb |= POS_RIGHT;
                *pos += 1;
            }
            if buf[*pos] == 'r' && (posb & POS_RIGHT != 0) {
                posb &= !POS_RIGHT;
                posb |= POS_RIGHTMOST;
                *pos += 1;
            }
            if buf[*pos] == 'l' {
                posb |= POS_LEFT;
                *pos += 1;
            }
            if buf[*pos] == 'l' && (posb & POS_LEFT != 0) {
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
                    jump_pos = POS_JUMP_POS::JUMP_ATTACH as i8;
                    *pos += 2;
                } else if buf[*pos + 1] == 'T' {
                    posb |= POS_JUMP;
                    jump_pos = POS_JUMP_POS::JUMP_TARGET as i8;
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
                    offset_sub = GSR_SPECIALS::GSR_ANY as i32;
                    *pos += 2;
                }
                if buf[*pos] == '*' {
                    offset_sub = GSR_SPECIALS::GSR_ANY as i32;
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

        if self.self_no_barrier && (posb & POS_SELF != 0) {
            if posb & POS_NO_BARRIER != 0 {
                posb &= !POS_NO_BARRIER;
            } else {
                posb |= POS_NO_BARRIER;
            }
        }

        if (posb & (POS_DEP_CHILD | POS_DEP_SIBLING) != 0)
            && (posb & (POS_SCANFIRST | POS_SCANALL) != 0)
        {
            posb &= !POS_SCANFIRST;
            posb &= !POS_SCANALL;
            posb |= POS_DEP_DEEP;
        }

        if tries >= 100 {
            self.error_near(&buf[n..]); // unknown specifier
        } else if tries >= 20 {
            tracing::warn!("{}: Warning: Position took many loops.", self.filebase);
        }
        if !isspace(buf[*pos]) {
            self.error_near(&buf[n..]); // garbage data
        }
        if *pos - n == 1 && (buf[n] == 'o' || buf[n] == 'O') {
            self.error_near(&buf[n..]); // stand-alone o/O
        }

        if had_digits {
            if posb & (POS_DEP_CHILD | POS_DEP_SIBLING | POS_DEP_PARENT) != 0 {
                self.error_near(&buf[n..]);
            }
            if posb & (POS_LEFT_PAR | POS_RIGHT_PAR) != 0 {
                self.error_near(&buf[n..]);
            }
            if posb & POS_RELATION != 0 {
                self.error_near(&buf[n..]);
            }
        }
        if (posb & POS_BAG_OF_TAGS != 0)
            && ((posb
                & !(POS_BAG_OF_TAGS
                    | POS_NOT
                    | POS_NEGATE
                    | POS_SPAN_BOTH
                    | POS_SPAN_LEFT
                    | POS_SPAN_RIGHT)
                != 0)
                || had_digits)
        {
            self.error_near(&buf[n..]);
        }
        if (posb & POS_DEP_PARENT != 0)
            && (posb & POS_DEP_GLOB == 0)
            && (posb & (POS_LEFTMOST | POS_RIGHTMOST) != 0)
        {
            self.error_near(&buf[n..]);
        }
        if (posb & POS_PASS_ORIGIN != 0) && (posb & POS_NO_PASS_ORIGIN != 0) {
            self.error_near(&buf[n..]);
        }
        if (posb & POS_LEFT_PAR != 0) && (posb & POS_RIGHT_PAR != 0) {
            self.error_near(&buf[n..]);
        }
        if (posb & POS_ALL != 0) && (posb & POS_NONE != 0) {
            self.error_near(&buf[n..]);
        }
        if (posb & POS_UNKNOWN != 0) && (posb != POS_UNKNOWN || had_digits) {
            self.error_near(&buf[n..]);
        }
        if (posb & POS_SCANALL != 0) && (posb & POS_NOT != 0) {
            tracing::warn!("{}: Warning: mixing NOT and ** ...", self.filebase);
        }

        if posb > POS_64BIT {
            posb |= POS_64BIT;
        }

        self.grammar.contexts_arena[t.0].pos = posb;
        self.grammar.contexts_arena[t.0].jump_pos = jump_pos;
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
        self.grammar.lines += skiptows(buf, &mut n, ')', false, false);
        let name: String = buf[*pos..n].iter().collect();
        let cn = hash_value_ustring(&name, 0);
        // Placeholder: hold the name-hash in `tmpl` (C++ reinterpret_cast<CT*>(cn)).
        self.grammar.contexts_arena[t_cur.0].tmpl = Some(CtxId(cn));
        let tmpl_data = (self.grammar.lines as usize, name);
        *pos = n;
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        tmpl_data
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn]
    fn parse_contextual_test_list(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        rule_flags: Option<u64>,
        in_tmpl: bool,
    ) -> CtxId {
        let ot = self.grammar.allocate_contextual_test();
        let mut t_cur = ot;
        self.grammar.contexts_arena[ot.0].line = self.grammar.lines;

        let mut tmpl_data: Option<(usize, String)> = None;

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_TEXTNEGATE) {
            *pos += slen(STR_TEXTNEGATE);
            self.grammar.contexts_arena[ot.0].pos |= POS_NEGATE;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_ALL) {
            *pos += slen(STR_ALL);
            self.grammar.contexts_arena[ot.0].pos |= POS_ALL;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_NONE) {
            *pos += slen(STR_NONE);
            self.grammar.contexts_arena[ot.0].pos |= POS_NONE;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_TEXTNOT) {
            *pos += slen(STR_TEXTNOT);
            self.grammar.contexts_arena[ot.0].pos |= POS_NOT;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        // Peek the token up to '('.
        let mut n_peek = *pos;
        self.grammar.lines += skiptows(buf, &mut n_peek, '(', false, false);
        let token: String = buf[*pos..n_peek].iter().collect();

        if ux_is_empty(&token) {
            // (1) Inline template.
            if self.no_itmpls {
                self.error_near(&buf[*pos..]);
            }
            *pos = n_peek;
            loop {
                if buf[*pos] != '(' {
                    self.error_near(&buf[*pos..]);
                }
                *pos += 1;
                let ored = self.parse_contextual_test_list(buf, pos, rule_flags, true);
                *pos += 1;
                self.grammar.contexts_arena[t_cur.0].ors.push(ored);
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                if simplecasecmp(buf, *pos, STR_OR) {
                    *pos += slen(STR_OR);
                } else {
                    break;
                }
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if self.grammar.contexts_arena[t_cur.0].ors.len() == 1 && self.verbosity_level > 0 {
                tracing::warn!("{}: Warning: inline template ...", self.filebase);
            }
        } else if token.starts_with('[') {
            // (2) Template shorthand [set, set, ...].
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.grammar.contexts_arena[t_cur.0].offset = 1;
            self.grammar.contexts_arena[t_cur.0].target = self.grammar.sets_list[s.0].hash;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            while buf[*pos] == ',' {
                *pos += 1;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                let lnk = self.grammar.allocate_contextual_test();
                let s2 = self.parse_set_inline_wrapper(buf, pos);
                self.grammar.contexts_arena[lnk.0].offset = 1;
                self.grammar.contexts_arena[lnk.0].target = self.grammar.sets_list[s2.0].hash;
                self.grammar.contexts_arena[t_cur.0].linked = Some(lnk);
                t_cur = lnk;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if buf[*pos] != ']' {
                self.error_near(&buf[*pos..]);
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
                self.parse_contextual_test_position(buf, pos, t_cur);
                *pos = n_peek;
                let pb = self.grammar.contexts_arena[t_cur.0].pos;
                if pb & (POS_DEP_CHILD | POS_DEP_PARENT | POS_DEP_SIBLING) != 0 {
                    self.grammar.has_dep = true;
                }
                if pb & POS_RELATION != 0 {
                    self.grammar.has_relations = true;
                }
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }

            if goto_template || (buf[*pos] == 'T' && buf[*pos + 1] == ':') {
                if !goto_template {
                    self.grammar.contexts_arena[t_cur.0].pos |= POS_TMPL_OVERRIDE;
                }
                tmpl_data = Some(self.parse_template_ref_body(buf, pos, t_cur));
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            } else {
                let s = self.parse_set_inline_wrapper(buf, pos);
                self.grammar.contexts_arena[t_cur.0].target = self.grammar.sets_list[s.0].hash;
            }

            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if simplecasecmp(buf, *pos, STR_CBARRIER) {
                *pos += slen(STR_CBARRIER);
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                let s = self.parse_set_inline_wrapper(buf, pos);
                self.grammar.contexts_arena[t_cur.0].cbarrier = self.grammar.sets_list[s.0].hash;
            }
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if simplecasecmp(buf, *pos, STR_BARRIER) {
                *pos += slen(STR_BARRIER);
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                let s = self.parse_set_inline_wrapper(buf, pos);
                self.grammar.contexts_arena[t_cur.0].barrier = self.grammar.sets_list[s.0].hash;
            }
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

            let (barrier, cbarrier, pb) = {
                let c = &self.grammar.contexts_arena[t_cur.0];
                (c.barrier, c.cbarrier, c.pos)
            };
            if (barrier != 0 || cbarrier != 0) && (pb & (MASK_POS_SCAN | POS_SELF) == 0) {
                tracing::warn!("{}: Warning: Barriers only make sense for scanning or self tests.", self.filebase);
                self.grammar.contexts_arena[t_cur.0].barrier = 0;
                self.grammar.contexts_arena[t_cur.0].cbarrier = 0;
            }
        }

        let mut linked = false;
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_AND) {
            self.error_near(&buf[*pos..]); // AND deprecated
        }
        if simplecasecmp(buf, *pos, STR_LINK) {
            *pos += slen(STR_LINK);
            linked = true;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        if linked {
            let l = self.parse_contextual_test_list(buf, pos, rule_flags, in_tmpl);
            self.grammar.contexts_arena[t_cur.0].linked = Some(l);
            if self.grammar.contexts_arena[t_cur.0].pos & POS_NONE != 0 {
                self.error_near(&buf[*pos..]); // LINK from a NONE test
            }
        } else if !in_tmpl
            && (self.grammar.contexts_arena[t_cur.0].pos & POS_SCANALL != 0)
            && (self.grammar.contexts_arena[t_cur.0].pos & POS_CAREFUL == 0)
        {
            tracing::warn!("{}: Warning: ** without LINK or C doesn't make sense.", self.filebase);
        }

        if let Some(rf) = rule_flags {
            if rf & RF_LOOKDELETED != 0 {
                self.grammar.contexts_arena[t_cur.0].pos |= POS_LOOK_DELETED;
            }
            if rf & RF_LOOKDELAYED != 0 {
                self.grammar.contexts_arena[t_cur.0].pos |= POS_LOOK_DELAYED;
            }
            if rf & RF_LOOKIGNORED != 0 {
                self.grammar.contexts_arena[t_cur.0].pos |= POS_LOOK_IGNORED;
            }
        }

        let t = self.grammar.add_contextual_test(Some(ot)).unwrap();
        // profiler skipped.
        if self.grammar.contexts_arena[t.0].tmpl.is_some() {
            if let Some(td) = tmpl_data {
                self.deferred_tmpls.insert(t, td);
            }
        }

        t
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-tests-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-tests-fn]
    fn parse_contextual_tests(&mut self, buf: &[char], pos: &mut usize, rule: &mut Rule) {
        let rf = rule.flags;
        let t = self.parse_contextual_test_list(buf, pos, Some(rf), false);
        if self.option_vislcg_compat && (self.grammar.contexts_arena[t.0].pos & POS_NOT != 0) {
            self.grammar.contexts_arena[t.0].pos &= !POS_NOT;
            self.grammar.contexts_arena[t.0].pos |= POS_NEGATE;
        }
        Rule::add_contextual_test(t, &mut rule.tests);
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-dependency-tests-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-dependency-tests-fn]
    fn parse_contextual_dependency_tests(&mut self, buf: &[char], pos: &mut usize, rule: &mut Rule) {
        let rf = rule.flags;
        let t = self.parse_contextual_test_list(buf, pos, Some(rf), false);
        if self.option_vislcg_compat && (self.grammar.contexts_arena[t.0].pos & POS_NOT != 0) {
            self.grammar.contexts_arena[t.0].pos &= !POS_NOT;
            self.grammar.contexts_arena[t.0].pos |= POS_NEGATE;
        }
        Rule::add_contextual_test(t, &mut rule.dep_tests);
    }
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-rule-flags-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-flags-fn]
    fn parse_rule_flags(&mut self, buf: &[char], pos: &mut usize) -> flags_t {
        let mut rv = flags_t::default();

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        let lp = *pos;
        let mut setflag = true;
        while setflag {
            setflag = false;
            for i in 0..FLAGS_COUNT {
                let op = *pos;
                if simplecasecmp(buf, *pos, G_FLAGS[i]) {
                    *pos += slen(G_FLAGS[i]);
                    rv.flags |= 1u64 << i;
                    setflag = true;

                    let mut undo = false;
                    if i == FL_SUB {
                        if buf[*pos] != ':' {
                            undo = true;
                        } else {
                            *pos += 1;
                            let mut n = *pos;
                            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
                            let token: String = buf[*pos..n].iter().collect();
                            *pos = n;
                            if token.chars().next() == Some('*') {
                                rv.sub_reading = GSR_SPECIALS::GSR_ANY as i32;
                            } else {
                                rv.sub_reading = scan_d(&token);
                            }
                        }
                    }

                    if !undo && buf[*pos] != '(' && buf[*pos] != ';' && !isspace(buf[*pos]) {
                        undo = true;
                    }

                    if undo {
                        rv.flags &= !(1u64 << i);
                        *pos = op;
                        setflag = false;
                        break;
                    }
                }
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                if buf[*pos] == '(' || buf[*pos] == 'T' || buf[*pos] == 't' || buf[*pos] == ';' {
                    setflag = false;
                    break;
                }
            }
            if rv.flags & (RF_WITHCHILD | RF_NOCHILD | RF_BEFORE | RF_AFTER) != 0 {
                break;
            }
        }

        for excl in FLAG_EXCLS_GROUPS {
            let bits = rv.flags & excl;
            if bits.count_ones() > 1 {
                self.error_near(&buf[lp..]);
            }
        }

        if rv.flags & RF_UNMAPLAST != 0 && rv.flags & RF_SAFE != 0 {
            self.error_near(&buf[lp..]);
        }

        if rv.flags & RF_UNMAPLAST != 0 {
            rv.flags |= RF_UNSAFE;
        }
        if rv.flags & RF_REMEMBERX != 0 {
            rv.flags |= RF_KEEPORDER;
        }
        if rv.flags & RF_ENCL_FINAL != 0 {
            self.grammar.has_encl_final = true;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        rv
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.maybe-parse-rule-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.maybe-parse-rule-fn]
    fn maybe_parse_rule(&mut self, buf: &[char], pos: &mut usize) -> bool {
        let p = *pos;
        // Longer names first; order-sensitive where one is a prefix of another.
        if is_icase_kw(buf, p, "ADDRELATIONS", "addrelations") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_ADDRELATIONS);
        } else if is_icase_kw(buf, p, "SETRELATIONS", "setrelations") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SETRELATIONS);
        } else if is_icase_kw(buf, p, "REMRELATIONS", "remrelations") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_REMRELATIONS);
        } else if is_icase_kw(buf, p, "ADDRELATION", "addrelation") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_ADDRELATION);
        } else if is_icase_kw(buf, p, "SETRELATION", "setrelation") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SETRELATION);
        } else if is_icase_kw(buf, p, "REMRELATION", "remrelation") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_REMRELATION);
        } else if is_icase_kw(buf, p, "SETVARIABLE", "setvariable") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SETVARIABLE);
        } else if is_icase_kw(buf, p, "REMVARIABLE", "remvariable") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_REMVARIABLE);
        } else if is_icase_kw(buf, p, "SETPARENT", "setparent") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SETPARENT);
        } else if is_icase_kw(buf, p, "SETCHILD", "setchild") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SETCHILD);
        } else if is_icase_kw(buf, p, "REMPARENT", "remparent") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_REMPARENT);
        } else if is_icase_kw(buf, p, "SWITCHPARENT", "switchparent") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SWITCHPARENT);
        } else if is_icase_kw(buf, p, "RESTORE", "restore") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_RESTORE);
        } else if is_icase_kw(buf, p, "IFF", "iff") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_IFF);
        } else if is_icase_kw(buf, p, "MAP", "map") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_MAP);
        } else if is_icase_kw(buf, p, "ADD", "add") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_ADD);
        } else if is_icase_kw(buf, p, "APPEND", "append") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_APPEND);
        } else if is_icase_kw(buf, p, "SELECT", "select") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SELECT);
        } else if is_icase_kw(buf, p, "REMOVE", "remove") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_REMOVE);
        } else if is_icase_kw(buf, p, "REPLACE", "replace") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_REPLACE);
        } else if is_icase_kw(buf, p, "SUBSTITUTE", "substitute") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SUBSTITUTE);
        } else if is_icase_kw(buf, p, "COPYCOHORT", "copycohort") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_COPYCOHORT);
        } else if is_icase_kw(buf, p, "COPY", "copy") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_COPY);
        } else if is_icase_kw(buf, p, "UNMAP", "unmap") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_UNMAP);
        } else if is_icase_kw(buf, p, "PROTECT", "protect") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_PROTECT);
        } else if is_icase_kw(buf, p, "UNPROTECT", "unprotect") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_UNPROTECT);
        } else if is_icase_kw(buf, p, "DELIMIT", "delimit") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_DELIMIT);
        } else if is_icase_kw(buf, p, "JUMP", "jump") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_JUMP);
        } else if is_icase_kw(buf, p, "MOVE", "move") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_MOVE);
        } else if is_icase_kw(buf, p, "SWITCH", "switch") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SWITCH);
        } else if is_icase_kw(buf, p, "EXECUTE", "execute") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_EXECUTE);
        } else if is_icase_kw(buf, p, "EXTERNAL", "external") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_EXTERNAL);
        } else if is_icase_kw(buf, p, "REMCOHORT", "remcohort") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_REMCOHORT);
        } else if is_icase_kw(buf, p, "ADDCOHORT", "addcohort") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_ADDCOHORT);
        } else if is_icase_kw(buf, p, "SPLITCOHORT", "splitcohort") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_SPLITCOHORT);
        } else if is_icase_kw(buf, p, "MERGECOHORTS", "mergecohorts") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_MERGECOHORTS);
        } else if is_icase_kw(buf, p, "RESTORE", "restore") != 0 {
            // Dead duplicate branch (never reached — the first RESTORE wins).
            self.parse_rule(buf, pos, KEYWORDS::K_RESTORE);
        } else if is_icase_kw(buf, p, "WITH", "with") != 0 {
            self.parse_rule(buf, pos, KEYWORDS::K_WITH);
        } else {
            return false;
        }
        true
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-anchorish-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-anchorish-fn]
    fn parse_anchorish(&mut self, buf: &[char], pos: &mut usize, rule_flags: bool) {
        if buf[*pos] != ':' {
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            let name: String = buf[*pos..n].iter().collect();
            if !self.only_sets {
                let at = ui32(self.grammar.rule_by_number.capacity());
                self.grammar.add_anchor(&name, at, true);
            }
            *pos = n;
        }

        self.grammar.lines += skipws(buf, pos, ':', '\0', false);
        if rule_flags && buf[*pos] == ':' {
            *pos += 1;
            self.section_flags = self.parse_rule_flags(buf, pos);
        }

        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }
}

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.add-rule-to-grammar-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.add-rule-to-grammar-fn]
    fn add_rule_to_grammar(&mut self, mut rule: Rule) {
        if self.in_nested_rule {
            rule.section = -3;
            let rid = self.grammar.add_rule(rule);
            self.nested_subrules.push(rid);
        } else if self.in_section {
            rule.section = self.grammar.sections.len() as i32 - 1;
            self.grammar.add_rule(rule);
        } else if self.in_after_sections {
            rule.section = -2;
            self.grammar.add_rule(rule);
        } else if self.in_null_section {
            rule.section = -3;
            self.grammar.add_rule(rule);
        } else {
            rule.section = -1;
            self.grammar.add_rule(rule);
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-rule-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-fn]
    fn parse_rule(&mut self, buf: &[char], pos: &mut usize, key: KEYWORDS) {
        let mut rule = self.grammar.allocate_rule();
        rule.line = self.grammar.lines;
        rule.r#type = key;

        // Leading wordform.
        let mut lp = *pos;
        backtonl(buf, &mut lp);
        self.grammar.lines += skipws(buf, &mut lp, '\0', '\0', false);
        if lp != *pos && lp < *pos {
            let mut n = lp;
            self.maybe_quoted(buf, &mut n, lp);
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            let token: String = buf[lp..n].iter().collect();
            let wform = self.parse_tag(&token, &buf[lp..]);
            rule.wordform = Some(wform);
        }

        *pos += slen(KEYWORDS_STR[key as usize]);
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        if buf[*pos] == ':' {
            *pos += 1;
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '(', false, false);
            let name: String = buf[*pos..n].iter().collect();
            if name.is_empty() {
                tracing::warn!("{}: Warning: Rule had : but no name.", self.filebase);
            } else {
                rule.set_name(Some(&name));
            }
            *pos = n;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        if key == KEYWORDS::K_EXTERNAL {
            if simplecasecmp(buf, *pos, STR_ONCE) {
                *pos += slen(STR_ONCE);
                rule.r#type = KEYWORDS::K_EXTERNAL_ONCE;
            } else if simplecasecmp(buf, *pos, STR_ALWAYS) {
                *pos += slen(STR_ALWAYS);
                rule.r#type = KEYWORDS::K_EXTERNAL_ALWAYS;
            } else {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

            let mut n = *pos;
            if buf[n] == '"' {
                n += 1;
                crate::inlines::skipto_nospan(buf, &mut n, '"');
                if buf[n] != '"' {
                    self.error_near(&buf[*pos..]);
                }
            }
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            let cmd: String = if buf[*pos] == '"' {
                // strip surrounding quotes
                buf[*pos + 1..n - 1].iter().collect()
            } else {
                buf[*pos..n].iter().collect()
            };
            let ext = self.grammar.allocate_tag(&cmd);
            rule.varname = self.grammar.single_tags_list[ext.0].hash;
            *pos = n;
        }

        let flags = self.parse_rule_flags(buf, pos);
        rule.flags = flags.flags;
        rule.sub_reading = flags.sub_reading;

        if self.section_flags.flags != 0 {
            for i in 0..FLAGS_COUNT {
                let f = 1u64 << i;
                if (self.section_flags.flags & f) != 0 && (rule.flags & FLAGS_EXCLS[i]) == 0 {
                    rule.flags |= f;
                }
            }
        }
        if self.section_flags.sub_reading != 0 && rule.sub_reading == 0 {
            rule.sub_reading = self.section_flags.sub_reading;
        }

        if rule.flags & (RF_ITERATE | RF_NOITERATE) == 0
            && key != KEYWORDS::K_SELECT
            && key != KEYWORDS::K_REMOVE
            && key != KEYWORDS::K_IFF
            && key != KEYWORDS::K_DELIMIT
            && key != KEYWORDS::K_REMCOHORT
            && key != KEYWORDS::K_MOVE
            && key != KEYWORDS::K_SWITCH
        {
            rule.flags |= RF_NOITERATE;
        }
        if key == KEYWORDS::K_UNMAP && rule.flags & (RF_SAFE | RF_UNSAFE) == 0 {
            rule.flags |= RF_SAFE;
        }
        if key == KEYWORDS::K_SETPARENT && rule.flags & (RF_SAFE | RF_UNSAFE) == 0 {
            rule.flags |= if self.safe_setparent { RF_SAFE } else { RF_UNSAFE };
        }

        if rule.flags & RF_WITHCHILD != 0 {
            self.grammar.has_dep = true;
            let s = self.parse_set_inline_wrapper(buf, pos);
            rule.childset1 = self.grammar.sets_list[s.0].hash;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        } else if rule.flags & RF_NOCHILD != 0 {
            rule.childset1 = 0;
        }

        lp = *pos;
        if key == KEYWORDS::K_SUBSTITUTE || key == KEYWORDS::K_EXECUTE {
            let saved = self.no_isets;
            self.no_isets = false;
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.no_isets = saved;
            Set::reindex(&mut self.grammar, s);
            rule.sublist = Some(s);
            if self.grammar.sets_list[s.0].empty() {
                self.error_near(&buf[lp..]);
            }
            if !is_mapping_list(&self.grammar, s) {
                self.error_near(&buf[lp..]);
            }
        }

        if rule.sub_reading == GSR_SPECIALS::GSR_ANY as i32
            && (key == KEYWORDS::K_MAP
                || key == KEYWORDS::K_ADD
                || key == KEYWORDS::K_REPLACE
                || key == KEYWORDS::K_SUBSTITUTE
                || key == KEYWORDS::K_COPY
                || key == KEYWORDS::K_COPYCOHORT)
        {
            self.error_bare();
        }

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        lp = *pos;
        if matches!(
            key,
            KEYWORDS::K_MAP
                | KEYWORDS::K_ADD
                | KEYWORDS::K_REPLACE
                | KEYWORDS::K_APPEND
                | KEYWORDS::K_SUBSTITUTE
                | KEYWORDS::K_COPY
                | KEYWORDS::K_COPYCOHORT
                | KEYWORDS::K_ADDRELATIONS
                | KEYWORDS::K_ADDRELATION
                | KEYWORDS::K_SETRELATIONS
                | KEYWORDS::K_SETRELATION
                | KEYWORDS::K_REMRELATIONS
                | KEYWORDS::K_REMRELATION
                | KEYWORDS::K_SETVARIABLE
                | KEYWORDS::K_REMVARIABLE
                | KEYWORDS::K_ADDCOHORT
                | KEYWORDS::K_JUMP
                | KEYWORDS::K_SPLITCOHORT
                | KEYWORDS::K_MERGECOHORTS
                | KEYWORDS::K_RESTORE
        ) {
            let saved = self.no_isets;
            self.no_isets = false;
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.no_isets = saved;
            Set::reindex(&mut self.grammar, s);
            rule.maplist = Some(s);
            if self.grammar.sets_list[s.0].empty() {
                self.error_near(&buf[lp..]);
            }
            if !is_mapping_list(&self.grammar, s) {
                self.error_near(&buf[lp..]);
            }
        }

        let mut copy_except = false;
        if (key == KEYWORDS::K_COPY || key == KEYWORDS::K_COPYCOHORT || key == KEYWORDS::K_REPLACE)
            && simplecasecmp(buf, *pos, STR_EXCEPT)
        {
            *pos += slen(STR_EXCEPT);
            copy_except = true;
        }

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        lp = *pos;
        if key == KEYWORDS::K_ADDRELATIONS
            || key == KEYWORDS::K_SETRELATIONS
            || key == KEYWORDS::K_REMRELATIONS
            || key == KEYWORDS::K_SETVARIABLE
            || copy_except
        {
            let saved = self.no_isets;
            self.no_isets = false;
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.no_isets = saved;
            Set::reindex(&mut self.grammar, s);
            rule.sublist = Some(s);
            if self.grammar.sets_list[s.0].empty() {
                self.error_near(&buf[lp..]);
            }
            if !is_mapping_list(&self.grammar, s) {
                self.error_near(&buf[lp..]);
            }
        }

        if key == KEYWORDS::K_ADDCOHORT {
            if simplecasecmp(buf, *pos, STR_AFTER) {
                *pos += slen(STR_AFTER);
                rule.r#type = KEYWORDS::K_ADDCOHORT_AFTER;
            } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                *pos += slen(STR_BEFORE);
                rule.r#type = KEYWORDS::K_ADDCOHORT_BEFORE;
            } else {
                self.error_near(&buf[*pos..]);
            }
        }

        if key == KEYWORDS::K_ADD
            || key == KEYWORDS::K_MAP
            || key == KEYWORDS::K_SUBSTITUTE
            || key == KEYWORDS::K_COPY
            || key == KEYWORDS::K_COPYCOHORT
        {
            if simplecasecmp(buf, *pos, STR_AFTER) {
                *pos += slen(STR_AFTER);
                rule.flags |= RF_AFTER;
            } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                *pos += slen(STR_BEFORE);
                rule.flags |= RF_BEFORE;
            }
            if key != KEYWORDS::K_COPYCOHORT && (rule.flags & (RF_BEFORE | RF_AFTER) != 0) {
                let s = self.parse_set_inline_wrapper(buf, pos);
                rule.childset1 = self.grammar.sets_list[s.0].hash;
            }
        }

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_TARGET) {
            *pos += slen(STR_TARGET);
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        if simplecasecmp(buf, *pos, G_FLAGS[FL_WITHCHILD]) {
            *pos += slen(G_FLAGS[FL_WITHCHILD]);
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.grammar.has_dep = true;
            rule.flags |= RF_WITHCHILD;
            rule.flags &= !RF_NOCHILD;
            rule.childset1 = self.grammar.sets_list[s.0].hash;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        } else if simplecasecmp(buf, *pos, G_FLAGS[FL_NOCHILD]) {
            *pos += slen(G_FLAGS[FL_NOCHILD]);
            rule.flags |= RF_NOCHILD;
            rule.flags &= !RF_WITHCHILD;
            rule.childset1 = 0;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        }

        let s = self.parse_set_inline_wrapper(buf, pos);
        rule.target = self.grammar.sets_list[s.0].hash;

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_IF) {
            *pos += slen(STR_IF);
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        while buf[*pos] != '\0' && buf[*pos] == '(' {
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            self.parse_contextual_tests(buf, pos, &mut rule);
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if buf[*pos] != ')' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        }

        if matches!(
            key,
            KEYWORDS::K_SETPARENT
                | KEYWORDS::K_SETCHILD
                | KEYWORDS::K_ADDRELATIONS
                | KEYWORDS::K_ADDRELATION
                | KEYWORDS::K_SETRELATIONS
                | KEYWORDS::K_SETRELATION
                | KEYWORDS::K_REMRELATIONS
                | KEYWORDS::K_REMRELATION
                | KEYWORDS::K_MOVE
                | KEYWORDS::K_SWITCH
                | KEYWORDS::K_MERGECOHORTS
                | KEYWORDS::K_COPYCOHORT
        ) {
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if key == KEYWORDS::K_MOVE {
                if simplecasecmp(buf, *pos, STR_AFTER) {
                    *pos += slen(STR_AFTER);
                    rule.r#type = KEYWORDS::K_MOVE_AFTER;
                } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                    *pos += slen(STR_BEFORE);
                    rule.r#type = KEYWORDS::K_MOVE_BEFORE;
                } else {
                    self.error_near(&buf[*pos..]);
                }
            } else if key == KEYWORDS::K_SWITCH || key == KEYWORDS::K_MERGECOHORTS {
                if simplecasecmp(buf, *pos, STR_WITH) {
                    *pos += slen(STR_WITH);
                } else {
                    self.error_near(&buf[*pos..]);
                }
            } else if simplecasecmp(buf, *pos, STR_TO) {
                *pos += slen(STR_TO);
            } else if simplecasecmp(buf, *pos, STR_FROM) {
                *pos += slen(STR_FROM);
                rule.flags |= RF_REVERSE;
            } else {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

            if key == KEYWORDS::K_COPYCOHORT && (rule.flags & RF_REVERSE == 0) {
                if simplecasecmp(buf, *pos, STR_AFTER) {
                    *pos += slen(STR_AFTER);
                    rule.flags |= RF_AFTER;
                } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                    *pos += slen(STR_BEFORE);
                    rule.flags |= RF_BEFORE;
                }
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }

            if key == KEYWORDS::K_MOVE || key == KEYWORDS::K_COPYCOHORT {
                if simplecasecmp(buf, *pos, G_FLAGS[FL_WITHCHILD]) {
                    *pos += slen(G_FLAGS[FL_WITHCHILD]);
                    self.grammar.has_dep = true;
                    let s = self.parse_set_inline_wrapper(buf, pos);
                    rule.childset2 = self.grammar.sets_list[s.0].hash;
                    self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                } else if simplecasecmp(buf, *pos, G_FLAGS[FL_NOCHILD]) {
                    *pos += slen(G_FLAGS[FL_NOCHILD]);
                    rule.childset2 = 0;
                    self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                }
            }

            lp = *pos;
            while buf[*pos] != '\0' && buf[*pos] == '(' {
                *pos += 1;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                self.parse_contextual_dependency_tests(buf, pos, &mut rule);
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                if buf[*pos] != ')' {
                    self.error_near(&buf[*pos..]);
                }
                *pos += 1;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if rule.dep_tests.is_empty() {
                self.error_near(&buf[lp..]);
            }
            if key != KEYWORDS::K_MERGECOHORTS {
                rule.dep_target = rule.dep_tests.back().copied();
                rule.dep_tests.pop_back();
            }
        }
        if key == KEYWORDS::K_SETPARENT
            || key == KEYWORDS::K_SETCHILD
            || key == KEYWORDS::K_SPLITCOHORT
            || key == KEYWORDS::K_MERGECOHORTS
        {
            self.grammar.has_dep = true;
        }
        if key == KEYWORDS::K_SETRELATION
            || key == KEYWORDS::K_SETRELATIONS
            || key == KEYWORDS::K_ADDRELATION
            || key == KEYWORDS::K_ADDRELATIONS
            || key == KEYWORDS::K_REMRELATION
            || key == KEYWORDS::K_REMRELATIONS
            || key == KEYWORDS::K_MERGECOHORTS
        {
            self.grammar.has_relations = true;
        }
        if key == KEYWORDS::K_COPYCOHORT && (rule.flags & (RF_BEFORE | RF_AFTER) == 0) {
            rule.flags |= RF_AFTER;
        }

        if rule.flags & RF_REMEMBERX == 0 {
            let mut found = false;
            if let Some(dt) = rule.dep_target {
                let c = &self.grammar.contexts_arena[dt.0];
                if c.pos & POS_JUMP != 0 && c.jump_pos == POS_JUMP_POS::JUMP_MARK as i8 {
                    found = true;
                }
            }
            if !found {
                for &it in rule.tests.iter() {
                    let c = &self.grammar.contexts_arena[it.0];
                    if c.pos & POS_JUMP != 0 && c.jump_pos == POS_JUMP_POS::JUMP_MARK as i8 {
                        found = true;
                        break;
                    }
                }
                for &it in rule.dep_tests.iter() {
                    let c = &self.grammar.contexts_arena[it.0];
                    if c.pos & POS_JUMP != 0 && c.jump_pos == POS_JUMP_POS::JUMP_MARK as i8 {
                        found = true;
                        break;
                    }
                }
            }
            if found {
                rule.flags |= RF_REMEMBERX | RF_KEEPORDER;
            }
        }

        if key == KEYWORDS::K_WITH {
            rule.flags |= RF_KEEPORDER;
            self.grammar.lines += skipws(buf, pos, '{', ';', false);
            if buf[*pos] == '{' {
                *pos += 1;
                let prev_in_nested = self.in_nested_rule;
                let prev_sub = std::mem::take(&mut self.nested_subrules);
                self.in_nested_rule = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                loop {
                    if !self.maybe_parse_rule(buf, pos) {
                        self.error_near(&buf[*pos..]);
                    }
                    self.grammar.lines += skipws(buf, pos, '}', ';', false);
                    if buf[*pos] == ';' {
                        *pos += 1;
                        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                    }
                    if buf[*pos] == '}' {
                        break;
                    }
                }
                *pos += 1;
                rule.sub_rules = std::mem::take(&mut self.nested_subrules);
                self.nested_subrules = prev_sub;
                self.in_nested_rule = prev_in_nested;
            }
        }

        rule.reverse_contextual_tests();

        let mut destroy = self.only_sets;
        if let Some(re) = &self.nrules {
            // UNANCHORED search over the rule name.
            if !re.is_match(&rule.name) {
                destroy = true;
            }
        }
        if let Some(re) = &self.nrules_inv {
            if re.is_match(&rule.name) {
                destroy = true;
            }
        }

        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            tracing::warn!("{}: Warning: Expected closing ; after previous rule!", self.filebase);
        }

        if destroy {
            // `destroyRule` on a heap Rule*; the port's local value is just dropped.
        } else {
            self.add_rule_to_grammar(rule);
        }
    }
}

impl TextualParser {
    /// One iteration of the `parseFromUChar` main loop (the C++ `try { ... }`
    /// body): progress print, leading `SKIPWS`, and the keyword dispatch chain.
    fn parse_directive(&mut self, buf: &[char], pos: &mut usize, fname: &str) {
        let p0 = *pos;
        if self.verbosity_level > 0 && self.grammar.lines % 500 == 0 {
            tracing::info!("Parsing line {}", self.grammar.lines);
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        let _ = p0;

        if is_icase_kw(buf, *pos, "DELIMITERS", "delimiters") != 0 {
            if self.grammar.delimiters.is_some() {
                self.error_near(&buf[*pos..]);
            }
            let d = self.grammar.allocate_set();
            self.grammar.sets_list[d.0].line = self.grammar.lines;
            self.grammar.sets_list[d.0].name = STR_DELIMITSET.to_string();
            self.grammar.delimiters = Some(d);
            *pos += 10;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.parse_tag_list(buf, pos, d, false);
            let d = self.grammar.add_set(d);
            self.grammar.delimiters = Some(d);
            if self.grammar.sets_list[d.0].empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "SOFT-DELIMITERS", "soft-delimiters") != 0 {
            if self.grammar.soft_delimiters.is_some() {
                self.error_near(&buf[*pos..]);
            }
            let d = self.grammar.allocate_set();
            self.grammar.sets_list[d.0].line = self.grammar.lines;
            self.grammar.sets_list[d.0].name = STR_SOFTDELIMITSET.to_string();
            self.grammar.soft_delimiters = Some(d);
            *pos += 15;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.parse_tag_list(buf, pos, d, false);
            let d = self.grammar.add_set(d);
            self.grammar.soft_delimiters = Some(d);
            if self.grammar.sets_list[d.0].empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "TEXT-DELIMITERS", "text-delimiters") != 0 {
            if self.grammar.text_delimiters.is_some() {
                self.error_near(&buf[*pos..]);
            }
            let d = self.grammar.allocate_set();
            self.grammar.sets_list[d.0].line = self.grammar.lines;
            self.grammar.sets_list[d.0].name = STR_TEXTDELIMITSET.to_string();
            self.grammar.text_delimiters = Some(d);
            *pos += 15;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.parse_tag_list(buf, pos, d, false);
            let d = self.grammar.add_set(d);
            self.grammar.text_delimiters = Some(d);
            if self.grammar.sets_list[d.0].empty() {
                self.error_near(&buf[*pos..]);
            }
            let mut the_tags = crate::tag::TagList::new();
            let trie = self.grammar.sets_list[d.0].trie.clone();
            let trie_sp = self.grammar.sets_list[d.0].trie_special.clone();
            crate::tag_trie::trie_get_tag_list_append(&trie, &mut the_tags, &self.grammar);
            crate::tag_trie::trie_get_tag_list_append(&trie_sp, &mut the_tags, &self.grammar);
            for tag in the_tags {
                if self.grammar.single_tags_list[tag.0].r#type & T_REGEXP == 0 {
                    self.error_bare();
                }
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "MAPPING-PREFIX", "mapping-prefix") != 0 {
            if self.seen_mapping_prefix != 0 {
                self.inc_error_count();
            }
            self.seen_mapping_prefix = self.grammar.lines;
            *pos += 14;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, ';', false, false);
            let token: String = buf[*pos..n].iter().collect();
            *pos = n;
            self.grammar.mapping_prefix = token.chars().next().unwrap_or('\0');
            if self.grammar.mapping_prefix == '\0' {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "PREFERRED-TARGETS", "preferred-targets") != 0 {
            *pos += 17;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                let h = self.grammar.single_tags_list[t.0].hash;
                self.grammar.preferred_targets.push(h);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if self.grammar.preferred_targets.is_empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "REOPEN-MAPPINGS", "reopen-mappings") != 0 {
            *pos += 15;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                let h = self.grammar.single_tags_list[t.0].hash;
                self.grammar.reopen_mappings.insert(h);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if self.grammar.reopen_mappings.empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "STATIC-SETS", "static-sets") != 0 {
            *pos += 11;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let name: String = buf[*pos..n].iter().collect();
                self.grammar.static_sets.push(name);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if self.grammar.static_sets.is_empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if let Some(icn) = self.match_cmdargs(buf, *pos) {
            *pos += icn;
            self.grammar.lines += skipws(buf, pos, '+', '\0', false);
            if buf[*pos] != '+' || buf[*pos + 1] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 2;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let s = *pos;
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            let args: String = buf[s..*pos].iter().collect();
            if icn == 16 {
                self.grammar.cmdargs_override = args;
            } else {
                self.grammar.cmdargs = args;
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "UNDEF-SETS", "undef-sets") != 0 {
            *pos += 10;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut did = false;
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let name: String = buf[*pos..n].iter().collect();
                if self.grammar.undef_set(&name).is_none() {
                    tracing::warn!("{}: Warning: Set {} wasn't defined.", self.filebase, name);
                }
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                did = true;
            }
            if !did {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "SETS", "sets") != 0 {
            *pos += 4;
        } else if is_icase_kw(buf, *pos, "LIST-TAGS", "list-tags") != 0 {
            *pos += 9;
            self.grammar.lines += skipws(buf, pos, '+', '\0', false);
            if buf[*pos] != '+' || buf[*pos + 1] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 2;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut tmp = uint32SortedVector::new();
            self.list_tags.swap(&mut tmp);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                tmp.insert(self.grammar.single_tags_list[t.0].hash);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if tmp.empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
            self.list_tags.swap(&mut tmp);
        } else if is_icase_kw(buf, *pos, "LIST", "list") != 0
            || is_icase_kw(buf, *pos, "OLIST", "olist") != 0
        {
            self.parse_list(buf, pos);
        } else if is_icase_kw(buf, *pos, "SET", "set") != 0 {
            self.parse_set_def(buf, pos);
        } else if is_icase_kw(buf, *pos, "MAPPINGS", "mappings") != 0 {
            *pos += 8;
            self.section_before(buf, pos);
        } else if is_icase_kw(buf, *pos, "CORRECTIONS", "corrections") != 0 {
            *pos += 11;
            self.section_before(buf, pos);
        } else if is_icase_kw(buf, *pos, "BEFORE-SECTIONS", "before-sections") != 0 {
            *pos += 15;
            self.section_before(buf, pos);
        } else if is_icase_kw(buf, *pos, "SECTION", "section") != 0 {
            *pos += 7;
            self.section_numbered(buf, pos);
        } else if is_icase_kw(buf, *pos, "CONSTRAINTS", "constraints") != 0 {
            *pos += 11;
            self.section_numbered(buf, pos);
        } else if is_icase_kw(buf, *pos, "AFTER-SECTIONS", "after-sections") != 0 {
            *pos += 14;
            if !self.only_sets {
                self.in_before_sections = false;
                self.in_section = false;
                self.in_after_sections = true;
                self.in_null_section = false;
            }
            self.maybe_anchorish(buf, pos);
        } else if is_icase_kw(buf, *pos, "NULL-SECTION", "null-section") != 0 {
            *pos += 12;
            if !self.only_sets {
                self.in_before_sections = false;
                self.in_section = false;
                self.in_after_sections = false;
                self.in_null_section = true;
            }
            self.maybe_anchorish(buf, pos);
        } else if is_icase_kw(buf, *pos, "SUBREADINGS", "subreadings") != 0 {
            *pos += 11;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if buf[*pos] == 'L' || buf[*pos] == 'l' {
                self.grammar.sub_readings_ltr = true;
            } else if buf[*pos] == 'R' || buf[*pos] == 'r' {
                self.grammar.sub_readings_ltr = false;
            } else {
                self.error_near(&buf[*pos..]);
            }
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            *pos = n;
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "OPTIONS", "options") != 0 {
            self.parse_options(buf, pos);
        } else if is_icase_kw(buf, *pos, "STRICT-TAGS", "strict-tags") != 0 {
            *pos += 11;
            self.grammar.lines += skipws(buf, pos, '+', '\0', false);
            if buf[*pos] != '+' || buf[*pos + 1] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 2;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut tmp = uint32SortedVector::new();
            self.strict_tags.swap(&mut tmp);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                tmp.insert(self.grammar.single_tags_list[t.0].hash);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if tmp.empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
            self.strict_tags.swap(&mut tmp);
        } else if is_icase_kw(buf, *pos, "ANCHOR", "anchor") != 0 {
            *pos += 6;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            self.parse_anchorish(buf, pos, false);
        } else if is_icase_kw(buf, *pos, "INCLUDE", "include") != 0 {
            self.parse_include(buf, pos, fname);
        } else if is_icase_kw(buf, *pos, "TEMPLATE", "template") != 0 {
            let line = self.grammar.lines;
            *pos += 8;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            let name: String = buf[*pos..n].iter().collect();
            *pos = n;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            let saved = self.no_itmpls;
            self.no_itmpls = false;
            let t = self.parse_contextual_test_list(buf, pos, None, true);
            self.no_itmpls = saved;
            self.grammar.contexts_arena[t.0].line = line;
            self.grammar.add_template(t, &name);
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "PARENTHESES", "parentheses") != 0 {
            self.parse_parentheses(buf, pos);
        } else if is_icase_kw(buf, *pos, "END", "end") != 0 {
            if (isnl(buf[*pos - 1]) || isspace(buf[*pos - 1]))
                && (buf[*pos + 3] == '\0' || isnl(buf[*pos + 3]) || isspace(buf[*pos + 3]))
            {
                // break the whole loop: signalled by leaving pos at the NUL.
                self.parse_end_break = true;
                return;
            }
            *pos += 1;
        } else if self.maybe_parse_rule(buf, pos) {
            // Has to happen last (so MAPPINGS is not parsed as MAP PINGS).
        } else {
            let n = *pos;
            if buf[*pos] == ';' || buf[*pos] == '"' {
                if buf[*pos] == '"' {
                    *pos += 1;
                    crate::inlines::skipto_nospan(buf, pos, '"');
                    if buf[*pos] != '"' {
                        self.error_near(&buf[n..]);
                    }
                }
                self.grammar.lines += skiptows(buf, pos, '\0', false, false);
            }
            if buf[*pos] != '\0'
                && buf[*pos] != ';'
                && buf[*pos] != '"'
                && !isnl(buf[*pos])
                && !isspace(buf[*pos])
            {
                self.error_near(&buf[*pos..]);
            }
            if isnl(buf[*pos]) {
                self.grammar.lines += 1;
            }
            *pos += 1;
        }
    }
}

impl TextualParser {
    fn match_cmdargs(&self, buf: &[char], pos: usize) -> Option<usize> {
        let a = is_icase_kw(buf, pos, "CMDARGS-OVERRIDE", "cmdargs-override");
        if a != 0 {
            return Some(a);
        }
        let b = is_icase_kw(buf, pos, "CMDARGS", "cmdargs");
        if b != 0 {
            return Some(b);
        }
        None
    }

    fn maybe_anchorish(&mut self, buf: &[char], pos: &mut usize) {
        let mut s = *pos;
        skipln(buf, &mut s);
        skipws(buf, &mut s, '\0', '\0', false);
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if *pos != s {
            self.parse_anchorish(buf, pos, true);
        }
    }

    fn section_before(&mut self, buf: &[char], pos: &mut usize) {
        if !self.only_sets {
            self.in_before_sections = true;
            self.in_section = false;
            self.in_after_sections = false;
            self.in_null_section = false;
        }
        self.maybe_anchorish(buf, pos);
    }

    fn section_numbered(&mut self, buf: &[char], pos: &mut usize) {
        if !self.only_sets {
            let l = self.grammar.lines;
            self.grammar.sections.push(l);
            self.in_before_sections = false;
            self.in_section = true;
            self.in_after_sections = false;
            self.in_null_section = false;
        }
        self.maybe_anchorish(buf, pos);
    }

    fn parse_list(&mut self, buf: &[char], pos: &mut usize) {
        let sset = self.grammar.allocate_set();
        self.grammar.sets_list[sset.0].line = self.grammar.lines;
        let mut ordered = false;
        if buf[*pos] == 'O' {
            *pos += 1;
            ordered = true;
        }
        *pos += 4;
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        let mut n = *pos;
        self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
        while buf[n - 1] == ',' || buf[n - 1] == ']' {
            n -= 1;
        }
        let name: String = buf[*pos..n].iter().collect();
        self.grammar.sets_list[sset.0].name = name.clone();
        *pos = n;
        self.grammar.lines += skipws(buf, pos, '=', '\0', false);
        let mut append = false;
        if buf[*pos] == '+' && buf[*pos + 1] == '=' {
            let aset = self.grammar.get_set(hash_value_ustring(&name, 0));
            if aset.is_none() {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            append = true;
        }
        if buf[*pos] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 1;
        self.parse_tag_list(buf, pos, sset, ordered);
        Set::rehash(&mut self.grammar, sset);
        let sset = if append {
            self.grammar.append_to_set(sset)
        } else {
            self.grammar.add_set(sset)
        };
        if self.grammar.sets_list[sset.0].empty() {
            self.error_near(&buf[*pos..]);
        }
        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    fn parse_set_def(&mut self, buf: &[char], pos: &mut usize) {
        let s0 = self.grammar.allocate_set();
        self.grammar.sets_list[s0.0].line = self.grammar.lines;
        *pos += 3;
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        let mut n = *pos;
        self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
        while buf[n - 1] == ',' || buf[n - 1] == ']' {
            n -= 1;
        }
        let name: String = buf[*pos..n].iter().collect();
        self.grammar.sets_list[s0.0].name = name.clone();
        let sh = hash_value_ustring(&name, 0);
        *pos = n;
        self.grammar.lines += skipws(buf, pos, '=', '\0', false);
        if buf[*pos] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 1;

        let saved = self.no_isets;
        self.no_isets = false;
        self.parse_set_inline(buf, pos, Some(s0));
        self.no_isets = saved;

        Set::rehash(&mut self.grammar, s0);
        let mut s = s0;
        let chash = self.grammar.sets_list[s0.0].hash;
        let existing = self.grammar.get_set(chash);
        if existing.is_some() {
            // verbosity dup warning skipped
        } else if self.grammar.sets_list[s0.0].sets.len() == 1
            && (self.grammar.sets_list[s0.0].r#type & ST_TAG_UNIFY == 0)
        {
            let back = *self.grammar.sets_list[s0.0].sets.last().unwrap();
            let tmp = self.grammar.get_set(back).unwrap();
            self.grammar.maybe_used_sets.insert(tmp);
            let th = self.grammar.sets_list[tmp.0].hash;
            self.grammar.set_alias.insert((sh, th));
            self.grammar.destroy_set(s0);
            s = tmp;
        }
        let s = self.grammar.add_set(s);
        if self.grammar.sets_list[s.0].empty() {
            self.error_near(&buf[*pos..]);
        }
        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    fn parse_options(&mut self, buf: &[char], pos: &mut usize) {
        *pos += 7;
        self.grammar.lines += skipws(buf, pos, '+', '\0', false);
        if buf[*pos] != '+' || buf[*pos + 1] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 2;
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        while buf[*pos] != ';' {
            let mut found = false;
            // No `break` between checks — reproduces the C++ multi-match loop.
            if simplecasecmp(buf, *pos, STR_NO_ISETS) {
                *pos += slen(STR_NO_ISETS);
                self.no_isets = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_NO_ITMPLS) {
                *pos += slen(STR_NO_ITMPLS);
                self.no_itmpls = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_WFORMS) {
                *pos += slen(STR_STRICT_WFORMS);
                self.strict_wforms = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_BFORMS) {
                *pos += slen(STR_STRICT_BFORMS);
                self.strict_bforms = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_SECOND) {
                *pos += slen(STR_STRICT_SECOND);
                self.strict_second = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_REGEX) {
                *pos += slen(STR_STRICT_REGEX);
                self.strict_regex = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_ICASE) {
                *pos += slen(STR_STRICT_ICASE);
                self.strict_icase = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_SELF_NO_BARRIER) {
                *pos += slen(STR_SELF_NO_BARRIER);
                self.self_no_barrier = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_ORDERED) {
                *pos += slen(STR_ORDERED);
                self.grammar.ordered = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_ADDCOHORT_ATTACH) {
                *pos += slen(STR_ADDCOHORT_ATTACH);
                self.grammar.addcohort_attach = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_SAFE_SETPARENT) {
                *pos += slen(STR_SAFE_SETPARENT);
                self.safe_setparent = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                found = true;
            }
            if !found {
                self.error_near(&buf[*pos..]);
            }
        }

        if self.grammar.addcohort_attach {
            self.grammar.has_dep = true;
        }
        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    fn parse_parentheses(&mut self, buf: &[char], pos: &mut usize) {
        *pos += 11;
        self.grammar.lines += skipws(buf, pos, '=', '\0', false);
        if buf[*pos] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 1;
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        while buf[*pos] != '\0' && buf[*pos] != ';' {
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '(', true, false);
            if buf[n] != '(' {
                self.error_near(&buf[*pos..]);
            }
            n += 1;
            self.grammar.lines += skipws(buf, &mut n, '\0', '\0', false);
            *pos = n;
            self.maybe_quoted(buf, &mut n, *pos);
            self.grammar.lines += skiptows(buf, &mut n, ')', true, false);
            let ltok: String = buf[*pos..n].iter().collect();
            let left = self.parse_tag(&ltok, &buf[*pos..]);
            self.grammar.lines += skipws(buf, &mut n, '\0', '\0', false);
            *pos = n;
            if buf[*pos] == ')' {
                self.error_near(&buf[*pos..]);
            }
            self.maybe_quoted(buf, &mut n, *pos);
            self.grammar.lines += skiptows(buf, &mut n, ')', true, false);
            let rtok: String = buf[*pos..n].iter().collect();
            let right = self.parse_tag(&rtok, &buf[*pos..]);
            self.grammar.lines += skipws(buf, &mut n, '\0', '\0', false);
            *pos = n;
            if buf[*pos] != ')' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

            let lh = self.grammar.single_tags_list[left.0].hash;
            let rh = self.grammar.single_tags_list[right.0].hash;
            self.grammar.parentheses.insert(lh, rh);
            self.grammar.parentheses_reverse.insert(rh, lh);
        }
        if self.grammar.parentheses.is_empty() {
            self.error_near(&buf[*pos..]);
        }
        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    fn parse_include(&mut self, buf: &[char], pos: &mut usize, fname: &str) {
        *pos += 7;
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        let mut local_only_sets = self.only_sets;
        if simplecasecmp(buf, *pos, STR_STATIC) && isspace(buf[*pos + slen(STR_STATIC)]) {
            *pos += slen(STR_STATIC);
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            local_only_sets = true;
        }

        let mut n = *pos;
        self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
        let incname: String = buf[*pos..n].iter().collect();
        *pos = n;
        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }

        let mut abspath = incname.clone();
        if abspath.contains('~') || abspath.contains('$') || abspath.contains('*') {
            abspath = shell_expand(&abspath);
        }
        if !abspath.starts_with('/') {
            let dir = ux_dirname(fname);
            abspath = format!("{dir}{abspath}");
        }
        let mut bytes = match std::fs::read(&abspath) {
            Ok(b) => b,
            Err(_) => match std::fs::read(&incname) {
                Ok(b) => {
                    abspath = incname.clone();
                    b
                }
                Err(e) => {
                    tracing::error!(
                        "{}: Error: Cannot stat {} due to error {} - bailing out!",
                        self.filebase, abspath, e
                    );
                    cg3_quit(1, None, 0);
                }
            },
        };
        if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
            bytes.drain(0..3);
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut data: Vec<char> = vec!['\0'; 4];
        data.extend(text.chars());
        data.extend(std::iter::repeat('\0').take(40));
        self.grammarbufs.push(data);
        let gi2 = self.grammarbufs.len() - 1;

        let saved_lines = self.grammar.lines;
        let saved_filebase = std::mem::take(&mut self.filebase);
        let saved_cur_grammar = self.cur_grammar;
        let saved_cur_grammar_n = self.cur_grammar_n;
        let saved_only = self.only_sets;
        let saved_end = self.parse_end_break;
        self.only_sets = local_only_sets;
        self.parse_from_u_char(gi2, abspath);
        self.parse_end_break = saved_end;
        self.only_sets = saved_only;
        self.cur_grammar_n = saved_cur_grammar_n;
        self.cur_grammar = saved_cur_grammar;
        self.filebase = saved_filebase;
        self.grammar.lines = saved_lines;
    }

    fn make_magic_set(&mut self, name: &str) -> SetId {
        let set_c = self.grammar.allocate_set();
        self.grammar.sets_list[set_c.0].line = 0;
        self.grammar.sets_list[set_c.0].name = name.to_string();
        let t = self.parse_tag(name, &[]);
        self.grammar.add_tag_to_set(t, set_c);
        self.grammar.add_set(set_c)
    }

    fn resolve_varstring(&mut self, tid: TagId) {
        let tagstr = self.grammar.single_tags_list[tid.0].tag.clone();
        let mut tbuf: Vec<char> = vec!['\0'];
        tbuf.extend(tagstr.chars());
        tbuf.extend(std::iter::repeat('\0').take(4));
        let mut p = 1usize;
        loop {
            skipto(&tbuf, &mut p, '{');
            if tbuf[p] != '\0' {
                let mut n = p;
                skipto(&tbuf, &mut n, '}');
                if tbuf[n] != '\0' {
                    self.grammar.single_tags_list[tid.0].allocate_vs_sets();
                    self.grammar.single_tags_list[tid.0].allocate_vs_names();
                    p += 1;
                    let theset: String = tbuf[p..n].iter().collect();
                    let tmp = self.parse_set(&theset, &tbuf[p..]);
                    let setname = self.grammar.sets_list[tmp.0].name.clone();
                    self.grammar.single_tags_list[tid.0].vs_sets.as_mut().unwrap().push(tmp);
                    let old = format!("{{{setname}}}");
                    self.grammar.single_tags_list[tid.0].vs_names.as_mut().unwrap().push(old);
                    p = n;
                    p += 1;
                }
            }
            if tbuf[p] == '\0' {
                break;
            }
        }
    }

    fn numeric_branch_split(&mut self) {
        let mut sets_cache: BTreeMap<u32, u32> = BTreeMap::new();
        loop {
            let found = self.grammar.contexts.iter().find_map(|(&k, &v)| {
                if self.grammar.contexts_arena[v.0].pos & POS_NUMERIC_BRANCH != 0 {
                    Some((k, v))
                } else {
                    None
                }
            });
            let (key, unsafec) = match found {
                Some(x) => x,
                None => break,
            };
            self.grammar.contexts.remove(&key);

            let target = self.grammar.contexts_arena[unsafec.0].target;
            if !sets_cache.contains_key(&target) {
                let stripped = self.grammar.remove_numeric_tags(target);
                sets_cache.insert(target, stripped);
            }
            self.grammar.contexts_arena[unsafec.0].pos &= !POS_NUMERIC_BRANCH;

            let safec = self.grammar.allocate_contextual_test();
            {
                let src = self.grammar.contexts_arena[unsafec.0].clone();
                copy_cntx(&src, &mut self.grammar.contexts_arena[safec.0]);
            }
            self.grammar.contexts_arena[safec.0].pos |= POS_CAREFUL;
            self.grammar.contexts_arena[safec.0].target = sets_cache[&target];

            let tmp = unsafec;
            let unsafec2 = self.grammar.add_contextual_test(Some(unsafec)).unwrap();
            let safec2 = self.grammar.add_contextual_test(Some(safec)).unwrap();

            let orc = self.grammar.allocate_contextual_test();
            self.grammar.contexts_arena[orc.0].ors.push(safec2);
            self.grammar.contexts_arena[orc.0].ors.push(unsafec2);
            let orc = self.grammar.add_contextual_test(Some(orc)).unwrap();

            let ctx_ids: Vec<CtxId> = self.grammar.contexts.values().copied().collect();
            for v in ctx_ids {
                if self.grammar.contexts_arena[v.0].linked == Some(tmp) {
                    self.grammar.contexts_arena[v.0].linked = Some(orc);
                }
            }
            let rule_ids: Vec<RuleId> = (0..self.grammar.rule_by_number.capacity())
                .filter(|&i| self.grammar.rule_by_number.try_get(i).is_some())
                .map(RuleId)
                .collect();
            for rid in rule_ids {
                if self.grammar.rule_by_number[rid.0].dep_target == Some(tmp) {
                    self.grammar.rule_by_number.get_mut(rid.0).dep_target = Some(orc);
                }
                let tests: Vec<CtxId> =
                    self.grammar.rule_by_number[rid.0].tests.iter().copied().collect();
                for (i, t) in tests.iter().enumerate() {
                    if *t == tmp {
                        self.grammar.rule_by_number.get_mut(rid.0).tests[i] = orc;
                    }
                }
                let dep_tests: Vec<CtxId> =
                    self.grammar.rule_by_number[rid.0].dep_tests.iter().copied().collect();
                for (i, t) in dep_tests.iter().enumerate() {
                    if *t == tmp {
                        self.grammar.rule_by_number.get_mut(rid.0).dep_tests[i] = orc;
                    }
                }
            }
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-from-u-char-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-from-u-char-fn]
    fn parse_from_u_char(&mut self, gi: usize, fname: String) {
        let (ptr, len) = {
            let g = &self.grammarbufs[gi];
            (g.as_ptr(), g.len())
        };
        // SAFETY: the char data of `grammarbufs[gi]` is heap-stable and never
        // mutated after creation; the decoupled slice never aliases a live `&mut`
        // into that data. Faithful to the C++ raw pointers into stable buffers.
        let buf: &[char] = unsafe { std::slice::from_raw_parts(ptr, len) };

        if len <= 4 || buf[4] == '\0' {
            tracing::error!("{}: Error: Input is empty - cannot continue!", fname);
            cg3_quit(1, None, 0);
        }

        let id = {
            self.num_grammars += 1;
            self.num_grammars
        };
        self.cur_grammar = unsafe { buf.as_ptr().add(4) };
        self.cur_grammar_n = id;
        let mut pos = 4usize;
        self.grammar.lines = 1;
        let mut ast_grammar =
            ASTHelper::new(&mut self.ast, ASTType::AST_Grammar, self.grammar.lines as usize, pptr(buf, 4));
        self.filebase = basename(Some(&fname)).to_string();
        self.parse_end_break = false;

        while buf[pos] != '\0' {
            let r = panic::catch_unwind(AssertUnwindSafe(|| {
                self.parse_directive(buf, &mut pos, &fname);
            }));
            if let Err(e) = r {
                if e.is::<ParseError>() {
                    self.grammar.lines += skipln(buf, &mut pos);
                } else {
                    panic::resume_unwind(e);
                }
            }
            if self.parse_end_break {
                break;
            }
        }

        ast_grammar.close_id(&mut self.ast, pptr(buf, pos), id);
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-grammar-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn]
    fn parse_grammar_data(&mut self, gi: usize) -> i32 {
        // 1. START anchor at rule 0.
        self.grammar.add_anchor(KEYWORDS_STR[KEYWORDS::K_START as usize], 0, true);
        // 2. Magic * tag.
        let tany = self.parse_tag(STR_ASTERIK, &[]);
        self.grammar.tag_any = self.grammar.single_tags_list[tany.0].hash;
        // 3. Dummy set.
        self.grammar.allocate_dummy_set();
        // 4. Magic sets.
        self.make_magic_set(STR_UU_TARGET);
        self.make_magic_set(STR_UU_MARK);
        self.make_magic_set(STR_UU_ATTACHTO);
        let s_left = self.make_magic_set(STR_UU_LEFT);
        let s_right = self.make_magic_set(STR_UU_RIGHT);
        self.make_magic_set(STR_UU_ENCL);
        {
            let set_c = self.grammar.allocate_set();
            self.grammar.sets_list[set_c.0].line = 0;
            self.grammar.sets_list[set_c.0].name = STR_UU_PAREN.to_string();
            self.grammar.sets_list[set_c.0].set_ops.push(S_OR);
            let lh = self.grammar.sets_list[s_left.0].hash;
            let rh = self.grammar.sets_list[s_right.0].hash;
            self.grammar.sets_list[set_c.0].sets.push(lh);
            self.grammar.sets_list[set_c.0].sets.push(rh);
            self.grammar.add_set(set_c);
        }
        self.make_magic_set(STR_UU_SAME_BASIC);
        for i in 0..9 {
            self.make_magic_set(STR_UU_C[i]);
        }

        // 5. Parse the grammar text.
        let fname = self.filename.clone();
        self.parse_from_u_char(gi, fname);

        // 6. END anchor at the last rule number.
        let end_at = ui32(self.grammar.rule_by_number.capacity().wrapping_sub(1));
        self.grammar.add_anchor(KEYWORDS_STR[KEYWORDS::K_END as usize], end_at, true);

        // 7. Named-rule anchors.
        let rule_ids: Vec<RuleId> = (0..self.grammar.rule_by_number.capacity())
            .filter(|&i| self.grammar.rule_by_number.try_get(i).is_some())
            .map(RuleId)
            .collect();
        for rid in &rule_ids {
            let (name, number) = {
                let r = &self.grammar.rule_by_number[rid.0];
                (r.name.clone(), r.number)
            };
            if !name.is_empty() {
                self.grammar.add_anchor(&name, number, false);
            }
        }

        // 8. Validate JUMP rules.
        for rid in &rule_ids {
            let (rtype, maplist) = {
                let r = &self.grammar.rule_by_number[rid.0];
                (r.r#type, r.maplist)
            };
            if rtype == KEYWORDS::K_JUMP {
                let maplist = maplist.unwrap();
                let to = self.grammar.get_tag_list_any_ret(maplist)[0];
                let (tty, thash) = {
                    let t = &self.grammar.single_tags_list[to.0];
                    (t.r#type, t.hash)
                };
                if tty & T_SPECIAL != 0 {
                    continue;
                }
                if self.grammar.anchors.find(thash) == self.grammar.anchors.end() {
                    tracing::error!("Error: JUMP could not find anchor.");
                    self.error_counter += 1;
                }
            }
        }

        // 9. Varstring set resolution + T_REGEXP_LINE ordered.
        let tag_ids: Vec<TagId> = (0..self.grammar.single_tags_list.capacity())
            .filter(|&i| self.grammar.single_tags_list.try_get(i).is_some())
            .map(TagId)
            .collect();
        for tid in &tag_ids {
            let ty = self.grammar.single_tags_list[tid.0].r#type;
            if ty & T_REGEXP_LINE != 0 {
                self.grammar.ordered = true;
            }
            if ty & T_VARSTRING == 0 {
                continue;
            }
            self.resolve_varstring(*tid);
        }

        // 10. Resolve deferred template refs.
        let deferred: Vec<(CtxId, (usize, String))> =
            self.deferred_tmpls.iter().map(|(&k, v)| (k, v.clone())).collect();
        for (t, (line, name)) in deferred {
            let cn = hash_value_ustring(&name, 0);
            if !self.grammar.templates.contains_key(&cn) {
                tracing::error!(
                    "{}: Error: Unknown template '{}' referenced on line {}!",
                    self.filebase, name, line
                );
                self.error_counter += 1;
                continue;
            }
            let real = self.grammar.templates[&cn];
            self.grammar.contexts_arena[t.0].tmpl = Some(real);
        }

        // 11. Numeric-branch splitting.
        self.numeric_branch_split();

        // 12. num_tags.
        self.grammar.num_tags = self.grammar.single_tags_list.capacity() as usize;

        self.error_counter
    }

    /// C++ `int parse_grammar(const char* buffer, size_t length)` (UTF-8 memory
    /// buffer). Builds the `data` buffer (4 leading NULs + text + NUL padding),
    /// then runs the private `parse_grammar(data)` driver.
    pub fn parse_grammar_utf8(&mut self, buffer: &[u8]) -> i32 {
        self.filename = "<utf8-memory>".to_string();
        self.filebase = "<utf8-memory>".to_string();
        self.grammar.grammar_size = buffer.len();
        let text = String::from_utf8_lossy(buffer);
        let mut data: Vec<char> = vec!['\0'; 4];
        data.extend(text.chars());
        data.extend(std::iter::repeat('\0').take(40));
        self.grammarbufs.push(data);
        let gi = self.grammarbufs.len() - 1;
        self.parse_grammar_data(gi)
    }
}

/// Best-effort `wordexp` stand-in for the INCLUDE path (`~`/`$`/`*`). Only `~`
/// (home) is expanded; env-var / glob expansion is a deliberate simplification
/// (documented). The C++ uses `wordexp(WRDE_NOCMD|WRDE_UNDEF)`.
fn shell_expand(s: &str) -> String {
    let mut out = s.to_string();
    if out == "~" || out.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            out = out.replacen('~', &home, 1);
        }
    }
    out
}

impl IGrammarParser for TextualParser {
    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    /// Reconciliation: `TextualParser` builds into its OWN `self.grammar`; the
    /// caller's `&mut Grammar` is swapped in for the duration so the result lands
    /// there (faithful to the C++ `result` being the `Grammar&` handed at ctor).
    fn parse_grammar(&mut self, grammar: &mut Grammar, input: &[u8]) -> i32 {
        std::mem::swap(&mut self.grammar, grammar);
        let rv = self.parse_grammar_utf8(input);
        std::mem::swap(&mut self.grammar, grammar);
        rv
    }

    fn set_compatible(&mut self, compat: bool) {
        self.option_vislcg_compat = compat;
    }

    fn set_verbosity(&mut self, level: u32) {
        self.verbosity_level = level;
    }

    fn get_grammar(&self) -> &Grammar {
        &self.grammar
    }
}
