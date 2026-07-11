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

/// C++ `using rule_flags_t = std::underlying_type<RULE_FLAGS>::type` (`uint64_t`)
/// — wave 4: the typed [`RuleFlags`] bitflags set.
pub type rule_flags_t = RuleFlags;

// [spec:cg3:def:rule.cg3.rule-flags]
// C++ `enum RULE_FLAGS : uint64_t`. Ported as a set of `u64` bit-flag constants
// (the faithful literal representation of a scoped bit-flag enum). These OR into
// `Rule::flags`. Must be kept in lock-step with Strings.hpp's `FL_*` / `g_flags`.
bitflags::bitflags! {
    /// C++ `RF_*` rule-flag bits over `uint64_t` (wave 4: a typed `bitflags`
    /// set instead of a bare `u64`).
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
    pub struct RuleFlags: u64 {
        const NEAREST = 1 << 0;
        const ALLOWLOOP = 1 << 1;
        const DELAYED = 1 << 2;
        const IMMEDIATE = 1 << 3;
        const LOOKDELETED = 1 << 4;
        const LOOKDELAYED = 1 << 5;
        const UNSAFE = 1 << 6;
        const SAFE = 1 << 7;
        const REMEMBERX = 1 << 8;
        const RESETX = 1 << 9;
        const KEEPORDER = 1 << 10;
        const VARYORDER = 1 << 11;
        const ENCL_INNER = 1 << 12;
        const ENCL_OUTER = 1 << 13;
        const ENCL_FINAL = 1 << 14;
        const ENCL_ANY = 1 << 15;
        const ALLOWCROSS = 1 << 16;
        const WITHCHILD = 1 << 17;
        const NOCHILD = 1 << 18;
        const ITERATE = 1 << 19;
        const NOITERATE = 1 << 20;
        const UNMAPLAST = 1 << 21;
        const REVERSE = 1 << 22;
        const SUB = 1 << 23;
        const OUTPUT = 1 << 24;
        const CAPTURE_UNIF = 1 << 25;
        const REPEAT = 1 << 26;
        const BEFORE = 1 << 27;
        const AFTER = 1 << 28;
        const IGNORED = 1 << 29;
        const LOOKIGNORED = 1 << 30;
        const NOMAPPED = 1 << 31;
        const NOPARENT = 1 << 32;
        const DETACH = 1 << 33;
    }
}

// The C++ constant names, kept so call sites read like the source.
pub const RF_NEAREST: RuleFlags = RuleFlags::NEAREST;
pub const RF_ALLOWLOOP: RuleFlags = RuleFlags::ALLOWLOOP;
pub const RF_DELAYED: RuleFlags = RuleFlags::DELAYED;
pub const RF_IMMEDIATE: RuleFlags = RuleFlags::IMMEDIATE;
pub const RF_LOOKDELETED: RuleFlags = RuleFlags::LOOKDELETED;
pub const RF_LOOKDELAYED: RuleFlags = RuleFlags::LOOKDELAYED;
pub const RF_UNSAFE: RuleFlags = RuleFlags::UNSAFE;
pub const RF_SAFE: RuleFlags = RuleFlags::SAFE;
pub const RF_REMEMBERX: RuleFlags = RuleFlags::REMEMBERX;
pub const RF_RESETX: RuleFlags = RuleFlags::RESETX;
pub const RF_KEEPORDER: RuleFlags = RuleFlags::KEEPORDER;
pub const RF_VARYORDER: RuleFlags = RuleFlags::VARYORDER;
pub const RF_ENCL_INNER: RuleFlags = RuleFlags::ENCL_INNER;
pub const RF_ENCL_OUTER: RuleFlags = RuleFlags::ENCL_OUTER;
pub const RF_ENCL_FINAL: RuleFlags = RuleFlags::ENCL_FINAL;
pub const RF_ENCL_ANY: RuleFlags = RuleFlags::ENCL_ANY;
pub const RF_ALLOWCROSS: RuleFlags = RuleFlags::ALLOWCROSS;
pub const RF_WITHCHILD: RuleFlags = RuleFlags::WITHCHILD;
pub const RF_NOCHILD: RuleFlags = RuleFlags::NOCHILD;
pub const RF_ITERATE: RuleFlags = RuleFlags::ITERATE;
pub const RF_NOITERATE: RuleFlags = RuleFlags::NOITERATE;
pub const RF_UNMAPLAST: RuleFlags = RuleFlags::UNMAPLAST;
pub const RF_REVERSE: RuleFlags = RuleFlags::REVERSE;
pub const RF_SUB: RuleFlags = RuleFlags::SUB;
pub const RF_OUTPUT: RuleFlags = RuleFlags::OUTPUT;
pub const RF_CAPTURE_UNIF: RuleFlags = RuleFlags::CAPTURE_UNIF;
pub const RF_REPEAT: RuleFlags = RuleFlags::REPEAT;
pub const RF_BEFORE: RuleFlags = RuleFlags::BEFORE;
pub const RF_AFTER: RuleFlags = RuleFlags::AFTER;
pub const RF_IGNORED: RuleFlags = RuleFlags::IGNORED;
pub const RF_LOOKIGNORED: RuleFlags = RuleFlags::LOOKIGNORED;
pub const RF_NOMAPPED: RuleFlags = RuleFlags::NOMAPPED;
pub const RF_NOPARENT: RuleFlags = RuleFlags::NOPARENT;
pub const RF_DETACH: RuleFlags = RuleFlags::DETACH;

/// Mirrors Strings.hpp's `FLAGS_COUNT` (the `FL_NEAREST..=FL_DETACH` enum tail —
/// 34 flags). Kept local to this module (the `strings` port does not export it)
/// and MUST stay in lock-step with the `RF_*` / `FL_*` lists. Sizes [`FLAGS_EXCLS`].
pub const FLAGS_COUNT: usize = 34;

// C++ `constexpr rule_flags_t flag_excls[]`: each entry ORs together a group of
// mutually-exclusive rule flags. Un-annotated in the spec (a supporting
// declaration), like the header's raw table.
const FLAG_EXCLS: [RuleFlags; 9] = [
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

// [spec:cg3:def:rule.cg3.init-flag-excls-fn]
// [spec:cg3:sem:rule.cg3.init-flag-excls-fn]
/// Maps a flag bit-index `v` to the mutual-exclusion mask it belongs to. Scans
/// [`FLAG_EXCLS`]; returns the first group whose members include bit `v`, else 0.
/// `constexpr` → `const fn` (the C++ `for` becomes an index `while`, since `for`
/// is not permitted in a `const fn`).
pub const fn init_flag_excls(v: u64) -> RuleFlags {
    let mut i = 0;
    while i < FLAG_EXCLS.len() {
        let excl = FLAG_EXCLS[i];
        if excl.bits() & (1u64 << v) != 0 {
            return excl;
        }
        i += 1;
    }
    RuleFlags::empty()
}

/// C++ `constexpr auto _flags_excls = make_array<FLAGS_COUNT>(init_flag_excls)`:
/// `_flags_excls[v]` = the mask of flags mutually exclusive with (and including)
/// flag `v`. The crate's [`crate::inlines::make_array`] is a runtime fn (unusable
/// in a `const`), so the compile-time `make_array` expansion is inlined as a const
/// block calling the `const fn` [`init_flag_excls`] for each index — same result.
pub const FLAGS_EXCLS: [RuleFlags; FLAGS_COUNT] = {
    let mut a = [RuleFlags::empty(); FLAGS_COUNT];
    let mut v = 0;
    while v < FLAGS_COUNT {
        a[v] = init_flag_excls(v as u64);
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
    pub flags: RuleFlags,
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
            flags: RuleFlags::empty(),
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
