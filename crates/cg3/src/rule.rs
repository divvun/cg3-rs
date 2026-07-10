//! Types from `src/Rule.hpp` (spec `docs/spec/port/src/Rule.md`).
//!
//! Core type-skeleton pass: only type definitions. C++ raw pointers become typed
//! arena indices (`wordform: Tag*` → `Option<TagId>`; `maplist`/`sublist: Set*` →
//! `Option<SetId>`; `dep_target: ContextualTest*` → `Option<CtxId>`; `sub_rules:
//! std::vector<Rule*>` → `Vec<RuleId>`). `uint32_t` fields that already hold
//! numeric ids (`target`, `childset1/2`, `varname`, `varvalue`) stay `u32`.
//! Method-body pass: `setName`, `addContextualTest`, `reverseContextualTests`, and
//! the `flag_excls`/`init_flag_excls`/`_flags_excls` constexpr machinery are
//! implemented here.

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::contextual_test::ContextList;
use crate::strings::KEYWORDS;
use crate::types::UString;
use std::collections::{BTreeMap, HashMap};

/// C++ `using rule_flags_t = std::underlying_type<RULE_FLAGS>::type` (`uint64_t`).
pub type rule_flags_t = u64;

// [spec:cg3:def:rule.cg3.rule-flags]
// C++ `enum RULE_FLAGS : uint64_t`. Ported as a set of `u64` bit-flag constants
// (the faithful literal representation of a scoped bit-flag enum). These OR into
// `Rule::flags`. Must be kept in lock-step with Strings.hpp's `FL_*` / `g_flags`.
pub const RF_NEAREST: rule_flags_t = 1 << 0;
pub const RF_ALLOWLOOP: rule_flags_t = 1 << 1;
pub const RF_DELAYED: rule_flags_t = 1 << 2;
pub const RF_IMMEDIATE: rule_flags_t = 1 << 3;
pub const RF_LOOKDELETED: rule_flags_t = 1 << 4;
pub const RF_LOOKDELAYED: rule_flags_t = 1 << 5;
pub const RF_UNSAFE: rule_flags_t = 1 << 6;
pub const RF_SAFE: rule_flags_t = 1 << 7;
pub const RF_REMEMBERX: rule_flags_t = 1 << 8;
pub const RF_RESETX: rule_flags_t = 1 << 9;
pub const RF_KEEPORDER: rule_flags_t = 1 << 10;
pub const RF_VARYORDER: rule_flags_t = 1 << 11;
pub const RF_ENCL_INNER: rule_flags_t = 1 << 12;
pub const RF_ENCL_OUTER: rule_flags_t = 1 << 13;
pub const RF_ENCL_FINAL: rule_flags_t = 1 << 14;
pub const RF_ENCL_ANY: rule_flags_t = 1 << 15;
pub const RF_ALLOWCROSS: rule_flags_t = 1 << 16;
pub const RF_WITHCHILD: rule_flags_t = 1 << 17;
pub const RF_NOCHILD: rule_flags_t = 1 << 18;
pub const RF_ITERATE: rule_flags_t = 1 << 19;
pub const RF_NOITERATE: rule_flags_t = 1 << 20;
pub const RF_UNMAPLAST: rule_flags_t = 1 << 21;
pub const RF_REVERSE: rule_flags_t = 1 << 22;
pub const RF_SUB: rule_flags_t = 1 << 23;
pub const RF_OUTPUT: rule_flags_t = 1 << 24;
pub const RF_CAPTURE_UNIF: rule_flags_t = 1 << 25;
pub const RF_REPEAT: rule_flags_t = 1 << 26;
pub const RF_BEFORE: rule_flags_t = 1 << 27;
pub const RF_AFTER: rule_flags_t = 1 << 28;
pub const RF_IGNORED: rule_flags_t = 1 << 29;
pub const RF_LOOKIGNORED: rule_flags_t = 1 << 30;
pub const RF_NOMAPPED: rule_flags_t = 1 << 31;
pub const RF_NOPARENT: rule_flags_t = 1 << 32;
pub const RF_DETACH: rule_flags_t = 1 << 33;

/// Mirrors Strings.hpp's `FLAGS_COUNT` (the `FL_NEAREST..=FL_DETACH` enum tail —
/// 34 flags). Kept local to this module (the `strings` port does not export it)
/// and MUST stay in lock-step with the `RF_*` / `FL_*` lists. Sizes [`FLAGS_EXCLS`].
pub const FLAGS_COUNT: usize = 34;

// C++ `constexpr rule_flags_t flag_excls[]`: each entry ORs together a group of
// mutually-exclusive rule flags. Un-annotated in the spec (a supporting
// declaration), like the header's raw table.
const FLAG_EXCLS: [rule_flags_t; 9] = [
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

// [spec:cg3:def:rule.cg3.init-flag-excls-fn]
// [spec:cg3:sem:rule.cg3.init-flag-excls-fn]
/// Maps a flag bit-index `v` to the mutual-exclusion mask it belongs to. Scans
/// [`FLAG_EXCLS`]; returns the first group whose members include bit `v`, else 0.
/// `constexpr` → `const fn` (the C++ `for` becomes an index `while`, since `for`
/// is not permitted in a `const fn`).
pub const fn init_flag_excls(v: rule_flags_t) -> rule_flags_t {
    let mut i = 0;
    while i < FLAG_EXCLS.len() {
        let excl = FLAG_EXCLS[i];
        if excl & ((1 as rule_flags_t) << v) != 0 {
            return excl;
        }
        i += 1;
    }
    0
}

/// C++ `constexpr auto _flags_excls = make_array<FLAGS_COUNT>(init_flag_excls)`:
/// `_flags_excls[v]` = the mask of flags mutually exclusive with (and including)
/// flag `v`. The crate's [`crate::inlines::make_array`] is a runtime fn (unusable
/// in a `const`), so the compile-time `make_array` expansion is inlined as a const
/// block calling the `const fn` [`init_flag_excls`] for each index — same result.
pub const FLAGS_EXCLS: [rule_flags_t; FLAGS_COUNT] = {
    let mut a = [0 as rule_flags_t; FLAGS_COUNT];
    let mut v = 0;
    while v < FLAGS_COUNT {
        a[v] = init_flag_excls(v as rule_flags_t);
        v += 1;
    }
    a
};

// [spec:cg3:def:rule.cg3.rule-vector]
/// C++ `typedef std::vector<Rule*> RuleVector`.
pub type RuleVector = Vec<RuleId>;

// [spec:cg3:def:rule.cg3.rule]
/// C++ `class Rule`.
///
/// The C++ `mutable` qualifier on `tests`/`dep_tests`/`dep_target` has no Rust
/// analog at the field level (they are plain fields here). The field `type` is
/// spelled `r#type` (Rust keyword). `Default` is hand-written rather than derived
/// because `type`'s C++ default is `K_IGNORE` and [`KEYWORDS`] has no `Default`.
#[derive(Clone, Debug)]
pub struct Rule {
    pub name: UString,
    pub wordform: Option<TagId>,
    pub target: u32,
    pub childset1: u32,
    pub childset2: u32,
    pub line: u32,
    pub number: u32,
    pub varname: u32,
    pub varvalue: u32, // ToDo: varvalue is unused
    pub flags: u64,
    pub section: i32,
    pub sub_reading: i32,
    pub r#type: KEYWORDS,
    pub maplist: Option<SetId>,
    pub sublist: Option<SetId>,
    pub sub_rules: RuleVector,
    pub tests: ContextList,
    pub dep_tests: ContextList,
    pub dep_target: Option<CtxId>,
}

// [spec:cg3:def:rule.cg3.rule.rule-fn]
// [spec:cg3:sem:rule.cg3.rule.rule-fn]
// C++ `Rule() = default`: no custom logic — every member takes its in-class
// initializer. Hand-written (not derived) only because `type`'s C++ default is
// `K_IGNORE` and [`KEYWORDS`] has no `Default`; the values below match the C++
// defaults exactly.
impl Default for Rule {
    fn default() -> Self {
        Rule {
            name: UString::new(),
            wordform: None,
            target: 0,
            childset1: 0,
            childset2: 0,
            line: 0,
            number: 0,
            varname: 0,
            varvalue: 0,
            flags: 0,
            section: 0,
            sub_reading: 0,
            r#type: KEYWORDS::K_IGNORE,
            maplist: None,
            sublist: None,
            sub_rules: RuleVector::new(),
            tests: ContextList::new(),
            dep_tests: ContextList::new(),
            dep_target: None,
        }
    }
}

impl Rule {
    // [spec:cg3:def:rule.cg3.rule.set-name-fn]
    // [spec:cg3:sem:rule.cg3.rule.set-name-fn]
    //
    // C++ `void setName(const UChar* to)`: the nullable NUL-terminated `UChar*`
    // becomes `Option<&str>` (`None` == `nullptr`). Clears `name`, then if `to` is
    // present assigns/copies it; a `None` leaves `name` empty. The `clear()` before
    // the assign is redundant but reproduced faithfully.
    pub fn set_name(&mut self, to: Option<&str>) {
        self.name.clear();
        if let Some(to) = to {
            self.name = to.to_string();
        }
    }

    // [spec:cg3:def:rule.cg3.rule.add-contextual-test-fn]
    // [spec:cg3:sem:rule.cg3.rule.add-contextual-test-fn]
    //
    // C++ `void addContextualTest(ContextualTest* to, ContextList& head)`:
    // `head.push_front(to)`. SIGNATURE: the C++ member never reads `this` — it only
    // front-inserts onto the passed `head` (one of the rule's own `tests`/
    // `dep_tests`). Ported WITHOUT a `self` receiver so callers can pass
    // `&mut rule.tests` / `&mut rule.dep_tests` without a self-aliasing borrow
    // conflict. `to` is a `CtxId`; `head` a `ContextList` (`VecDeque<CtxId>`).
    pub fn add_contextual_test(to: CtxId, head: &mut ContextList) {
        head.push_front(to);
    }

    // [spec:cg3:def:rule.cg3.rule.reverse-contextual-tests-fn]
    // [spec:cg3:sem:rule.cg3.rule.reverse-contextual-tests-fn]
    //
    // C++ `void reverseContextualTests() { tests.reverse(); dep_tests.reverse(); }`.
    // `std::list::reverse` → `VecDeque` in-place reverse via
    // `make_contiguous().reverse()` (the reason [`ContextList`] is a `VecDeque` and
    // not a `LinkedList`).
    pub fn reverse_contextual_tests(&mut self) {
        self.tests.make_contiguous().reverse();
        self.dep_tests.make_contiguous().reverse();
    }
}

// [spec:cg3:def:rule.cg3.rule-by-line-map]
/// C++ `typedef std::map<uint32_t, Rule*> RuleByLineMap`.
///
/// `std::map` is ordered → `BTreeMap` (keeps the by-line ordering).
pub type RuleByLineMap = BTreeMap<u32, RuleId>;

// [spec:cg3:def:rule.cg3.rule-by-line-hash-map]
/// C++ `typedef std::unordered_map<uint32_t, Rule*> RuleByLineHashMap`.
pub type RuleByLineHashMap = HashMap<u32, RuleId>;
