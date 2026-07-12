//! Port of `src/Grammar.hpp` / `src/Grammar.cpp` — the central owner type.
//!
//! **CORE TYPE-SKELETON pass.** This file defines only the `Grammar` struct and
//! its member typedefs; the method bodies (`addSet`, `reindex`, ...) land in a
//! later pass. A manual [`Default`] impl stands in for the C++
//! `Grammar() = default;` so the one non-zero member initializer
//! (`mapping_prefix = '@'`) is preserved faithfully.
//!
//! ## Arena / pointer model
//! The `Grammar` OWNS the static grammar objects in `crate::arena::Arena<T>`
//! slabs; everything else that was a raw `T*`/`std::vector<T*>` becomes a typed
//! index (`TagId`/`SetId`/`RuleId`/`CtxId`) or `Vec<…Id>` / `Option<…Id>`:
//!   * `single_tags_list` (`std::vector<Tag*>`)      → `Arena<Tag>` (indexed by `TagId`)
//!   * `sets_list`        (`std::vector<Set*>`)       → `Arena<Set>` (indexed by `SetId`)
//!   * `rule_by_number`   (`RuleVector`)              → `Arena<Rule>` (indexed by `RuleId`)
//!   * `contexts_arena`   (ADDED — port infra)        → `Arena<ContextualTest>` (`CtxId` storage)
//!
//! ## Map representation choice
//! Mirroring the C++ container distinction:
//!   * C++ `flat_unordered_map<…>` fields  → [`crate::flat_unordered_map::FlatUnorderedMap`]
//!     (`single_tags`, `sets_by_name`, `set_alias`, `anchors`).
//!   * C++ `std::unordered_map<…>` fields → `std::collections::HashMap`
//!     (`sets_by_contents`, `set_name_seeds`, `templates`, `contexts`,
//!     `rules_by_set`, `rules_by_tag`, `sets_by_tag`).
//!   * C++ `bc::flat_map<…>` (`parentheses`) → `std::collections::BTreeMap`
//!     (sorted associative container; closest std analog — no flat_map module).
//!
//! ## Sibling-module dependency
//! Full compilation requires the sibling modules created in parallel:
//! `crate::tag::Tag`, `crate::set::Set`, `crate::rule::Rule`,
//! `crate::contextual_test::ContextualTest`. Until those land + are wired into
//! `lib.rs`, this module will not resolve those paths.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::arena::{Arena, CtxId, RuleId, SetId, TagId};
use crate::flat_unordered_map::{FlatUnorderedMap, Uint32FlatHashMap};
use crate::interval_vector::uint32IntervalVector;
use crate::sorted_vector::{sorted_vector, uint32SortedVector};
use crate::types::{SetNumber, UChar, UString, Uint32Vector, flags_t};

// Sibling grammar-object types (created by parallel agents). Aliased locally so
// the arena declarations read against a stable name.
use crate::contextual_test::ContextualTest;
use crate::rule::Rule;
use crate::set::Set;
use crate::tag::Tag;

// --- Method-pass imports (added with the fn bodies) ---
use std::io::Read;

use crate::inlines::{cg3_quit, hash_value_ustring, is_internal, is_textual, ui32};
use crate::rule::{RF_CAPTURE_UNIF, RF_KEEPORDER};
use crate::set::{
    MASK_ST_UNIFY, ST_ANY, ST_CHILD_UNIFY, ST_SET_UNIFY, ST_SPECIAL, ST_STATIC, ST_TAG_UNIFY,
    ST_USED,
};
use crate::strings::KEYWORDS;
use crate::tag::{
    T_ANY, T_CASE_INSENSITIVE, T_FAILFAST, T_MAPPING, T_SPECIAL, T_TEXTUAL, T_VARSTRING, TagList,
    TagVector, TagVectorSet, fill_tagvector,
};
use crate::tag_trie::{
    trie_delete, trie_get_tag_list, trie_get_tag_list_append, trie_get_tags, trie_get_tags_into,
    trie_has_type, trie_insert, trie_singular, trie_t,
};

// ---------------------------------------------------------------------------
// Local string / operator constants.
//
// These live in `src/Strings.hpp` (annotated there) but are out of scope for
// the `crate::strings` port (which covers only `KEYWORDS`). Because this pass
// may edit ONLY `grammar.rs`, they are reproduced here verbatim as local
// stand-ins (same precedent as the local ICU/scanf stand-ins in `tag.rs` /
// `set.rs`). To reconcile: move to `crate::strings` when that module grows.
const STR_DELIMITSET: &str = "_S_DELIMITERS_";
const STR_SOFTDELIMITSET: &str = "_S_SOFT_DELIMITERS_";
const STR_TEXTDELIMITSET: &str = "_S_TEXT_DELIMITERS_";
const STR_GPREFIX: &str = "_G_";
const STR_POSITIVE: &str = "POSITIVE";
const STR_NEGATIVE: &str = "NEGATIVE";
const STR_DUMMY: &str = "__CG3_DUMMY_STRINGBIT__";

// C++ `enum { ... S_OR = 3, S_PLUS, S_MINUS, ... }` (Strings.hpp). Only the two
// operators `addSet`/`appendToSet` reference are reproduced here.
const S_OR: u32 = 3;
const S_MINUS: u32 = 5;

// [spec:cg3:def:grammar.cg3.grammar.contexts-t]
/// C++ `typedef std::unordered_map<uint32_t, ContextualTest*> contexts_t`.
/// The `ContextualTest*` value becomes a `CtxId` into `Grammar::contexts_arena`.
///
/// `BTreeMap`, not `HashMap`: iteration order feeds the `.cg3b` context-record
/// order (and GrammarWriter's template output). C++ `unordered_map` order is a
/// stdlib artifact (libc++ vs libstdc++ already differ); key order makes OUR
/// output deterministic across runs and builds. The reader is order-agnostic.
pub type contexts_t = BTreeMap<u32, CtxId>;

// [spec:cg3:def:grammar.cg3.grammar.set-name-seeds-t]
/// C++ `typedef std::unordered_map<UString, uint32_t, hash_ustring> set_name_seeds_t`.
/// UTF-8 `String` keys; `hash_ustring` collapses into the std hasher.
pub type set_name_seeds_t = HashMap<UString, u32>;

// [spec:cg3:def:grammar.cg3.grammar.static-sets-t]
/// C++ `typedef std::vector<UString> static_sets_t`.
pub type static_sets_t = Vec<UString>;

// [spec:cg3:def:grammar.cg3.grammar.regex-tags-t]
/// C++ `typedef std::set<URegularExpression*> regex_tags_t`.
///
/// NOTE: each `URegularExpression*` is owned by exactly one `Tag` (`tag->regexp`,
/// inserted in `reindex`). No standalone regex type exists in the port yet
/// (`regex`-crate wiring is a later concern), so the set is keyed by the owning
/// tag's `TagId`; the compiled regex is reached via that tag. To reconcile once
/// `Tag::regexp` is defined.
pub type regex_tags_t = BTreeSet<TagId>;

// [spec:cg3:def:grammar.cg3.grammar.icase-tags-t]
/// C++ `typedef TagSortedVector icase_tags_t` (`sorted_vector<Tag*, compare_Tag>`).
///
/// NOTE: the custom `compare_Tag` comparator (orders by tag content) is not yet
/// ported; this uses the default `Less` ordering over `TagId`. To reconcile when
/// `compare_Tag` lands.
pub type icase_tags_t = sorted_vector<TagId>;

// [spec:cg3:def:grammar.cg3.grammar.rules-by-set-t]
/// C++ `typedef std::unordered_map<uint32_t, uint32IntervalVector> rules_by_set_t`.
pub type rules_by_set_t = HashMap<u32, uint32IntervalVector>;

// [spec:cg3:def:grammar.cg3.grammar.rules-by-tag-t]
/// C++ `typedef std::unordered_map<uint32_t, uint32IntervalVector> rules_by_tag_t`.
pub type rules_by_tag_t = HashMap<u32, uint32IntervalVector>;

// [spec:cg3:def:grammar.cg3.grammar.sets-by-tag-t]
/// C++ `typedef std::unordered_map<uint32_t, boost::dynamic_bitset<>> sets_by_tag_t`.
/// The `dynamic_bitset` value becomes `crate::types::flags_t`.
pub type sets_by_tag_t = HashMap<u32, flags_t>;

// [spec:cg3:def:grammar.cg3.grammar.parentheses-t]
/// C++ `typedef bc::flat_map<uint32_t, uint32_t> parentheses_t`.
/// Represented as `BTreeMap` (sorted associative container).
pub type parentheses_t = BTreeMap<u32, u32>;

// [spec:cg3:def:grammar.cg3.grammar]
/// The parsed/loaded grammar: owner of all static tags, sets, rules and
/// contextual tests, plus every runtime lookup index built by `reindex`.
pub struct Grammar {
    /// Wave-4 grammar-owned PRNG state for `Set::set_name`'s `to == 0`
    /// fallback (the C++ used the process-global libc `rand()`). Non-zero
    /// xorshift32 state, stepped by [`crate::set::rand_step`].
    pub rand_state: u32,

    // --- feature / mode flags ---
    pub has_dep: bool,
    pub has_bag_of_tags: bool,
    pub has_relations: bool,
    pub has_encl_final: bool,
    pub has_protect: bool,
    pub is_binary: bool,
    pub sub_readings_ltr: bool,
    pub ordered: bool,
    pub addcohort_attach: bool,

    // --- sizes / counters ---
    pub grammar_size: usize,
    pub num_tags: usize,
    pub mapping_prefix: UChar,
    pub lines: u32,
    pub verbosity_level: u32,
    pub total_time: f64,

    // --- command-line argument capture ---
    pub cmdargs: String,
    pub cmdargs_override: String,

    // --- tags ---
    /// Owned tag arena (was `std::vector<Tag*> single_tags_list`); `TagId` indexes it.
    pub single_tags_list: Arena<Tag>,
    /// C++ `Taguint32HashMap` (`flat_unordered_map<uint32_t, Tag*>`): hash → tag.
    pub single_tags: FlatUnorderedMap<u32, TagId>,

    // --- sets ---
    /// Owned set arena (was `std::vector<Set*> sets_list`); `SetId` indexes it.
    pub sets_list: Arena<Set>,
    /// The C++ `std::vector<Set*> sets_list` ORDER: maps a DENSE set number to
    /// its arena id (`sets_list_order[s.number] == s` for every listed set,
    /// including the dummy at position 0). Maintained by `allocate_dummy_set`
    /// (front-insert), `add_set_to_list` (push + number = len-1), `reindex`
    /// (`resize(1)`), and the binary reader. Port infrastructure (the arena keeps
    /// ownership; this is the numbered view).
    pub sets_list_order: Vec<SetId>,
    /// C++ `SetSet sets_all` (`sorted_vector<Set*>`): ownership registry of every
    /// allocated set. NOTE: default `Less` orders by `SetId` (was pointer order).
    pub sets_all: sorted_vector<SetId>,
    /// C++ `uint32FlatHashMap sets_by_name`: name-hash → (content-hash | set-number).
    pub sets_by_name: Uint32FlatHashMap,
    pub set_name_seeds: set_name_seeds_t,
    /// C++ `Setuint32HashMap sets_by_contents` (`std::unordered_map<uint32_t, Set*>`):
    /// content-hash → set.
    ///
    /// `BTreeMap`, not `HashMap`: reindex iterates this to assign dense set
    /// numbers (→ `.cg3b` set-record order). Key order = deterministic output
    /// across runs/builds; C++ stdlibs already disagree among themselves.
    pub sets_by_contents: BTreeMap<u32, SetId>,
    /// C++ `uint32FlatHashMap set_alias`: alias name-hash → real name-hash.
    pub set_alias: Uint32FlatHashMap,
    /// C++ `SetSet maybe_used_sets`.
    pub maybe_used_sets: sorted_vector<SetId>,

    pub static_sets: static_sets_t,

    pub regex_tags: regex_tags_t,
    pub icase_tags: icase_tags_t,

    // --- contextual tests ---
    /// Owned contextual-test arena (ADDED — port infra) backing every `CtxId`.
    pub contexts_arena: Arena<ContextualTest>,
    pub templates: contexts_t,
    pub contexts: contexts_t,

    // --- runtime indexes ---
    pub rules_by_set: rules_by_set_t,
    pub rules_by_tag: rules_by_tag_t,
    pub sets_by_tag: sets_by_tag_t,

    /// C++ `uint32IntervalVector* rules_any` — cached `rules_by_tag[tag_any]`.
    pub rules_any: Option<uint32IntervalVector>,
    /// C++ `boost::dynamic_bitset<>* sets_any` — cached `sets_by_tag[tag_any]`.
    pub sets_any: Option<flags_t>,

    // --- delimiter sets (nullable `Set*`) ---
    pub delimiters: Option<SetId>,
    pub soft_delimiters: Option<SetId>,
    pub text_delimiters: Option<SetId>,

    pub tag_any: u32,
    /// C++ `uint32Vector preferred_targets` (tag hashes).
    pub preferred_targets: Uint32Vector,
    /// C++ `uint32SortedVector reopen_mappings`.
    pub reopen_mappings: uint32SortedVector,
    pub parentheses: parentheses_t,
    pub parentheses_reverse: parentheses_t,

    /// C++ `uint32Vector sections`.
    pub sections: Uint32Vector,
    /// C++ `uint32FlatHashMap anchors`: anchor name-hash → rule position.
    pub anchors: Uint32FlatHashMap,

    // --- rules ---
    /// Owned rule arena (was `RuleVector rule_by_number`); `RuleId` indexes it,
    /// and a rule's `number` is its index here.
    pub rule_by_number: Arena<Rule>,
    /// C++ `RuleVector before_sections` (rules for section -1).
    pub before_sections: Vec<RuleId>,
    /// C++ `RuleVector rules` (rules for numbered sections).
    pub rules: Vec<RuleId>,
    /// C++ `RuleVector after_sections` (rules for section -2).
    pub after_sections: Vec<RuleId>,
    /// C++ `RuleVector null_section` (rules for section -3).
    pub null_section: Vec<RuleId>,
    /// C++ `RuleVector wf_rules` (wordform-scoped rules).
    pub wf_rules: Vec<RuleId>,
}

impl Default for Grammar {
    /// Faithful analog of the C++ `Grammar() = default;`: every member takes its
    /// zero/empty value except `mapping_prefix`, whose C++ member initializer is
    /// `'@'`.
    fn default() -> Self {
        Grammar {
            rand_state: 1,
            has_dep: false,
            has_bag_of_tags: false,
            has_relations: false,
            has_encl_final: false,
            has_protect: false,
            is_binary: false,
            sub_readings_ltr: false,
            ordered: false,
            addcohort_attach: false,
            grammar_size: 0,
            num_tags: 0,
            mapping_prefix: '@',
            lines: 0,
            verbosity_level: 0,
            total_time: 0.0,
            cmdargs: String::new(),
            cmdargs_override: String::new(),
            single_tags_list: Arena::new(),
            single_tags: FlatUnorderedMap::default(),
            sets_list: Arena::new(),
            sets_list_order: Vec::new(),
            sets_all: sorted_vector::new(),
            sets_by_name: Uint32FlatHashMap::default(),
            set_name_seeds: set_name_seeds_t::default(),
            sets_by_contents: BTreeMap::default(),
            set_alias: Uint32FlatHashMap::default(),
            maybe_used_sets: sorted_vector::new(),
            static_sets: static_sets_t::default(),
            regex_tags: regex_tags_t::default(),
            icase_tags: sorted_vector::new(),
            contexts_arena: Arena::new(),
            templates: contexts_t::default(),
            contexts: contexts_t::default(),
            rules_by_set: rules_by_set_t::default(),
            rules_by_tag: rules_by_tag_t::default(),
            sets_by_tag: sets_by_tag_t::default(),
            rules_any: None,
            sets_any: None,
            delimiters: None,
            soft_delimiters: None,
            text_delimiters: None,
            tag_any: 0,
            preferred_targets: Uint32Vector::default(),
            reopen_mappings: uint32SortedVector::new(),
            parentheses: parentheses_t::default(),
            parentheses_reverse: parentheses_t::default(),
            sections: Uint32Vector::default(),
            anchors: Uint32FlatHashMap::default(),
            rule_by_number: Arena::new(),
            before_sections: Vec::new(),
            rules: Vec::new(),
            after_sections: Vec::new(),
            null_section: Vec::new(),
            wf_rules: Vec::new(),
        }
    }
}

// ===========================================================================
// Method bodies (Wave 2 translate pass). Ported literally, bug-for-bug, from
// `src/Grammar.cpp` / `src/Grammar.hpp`; each fn carries its `[spec:cg3:def]` +
// `[spec:cg3:sem]` ids verbatim.
//
// ARENA MODEL. Every C++ member fn is an `&mut self` (or `&self`) method here.
// The four static arenas (`single_tags_list`, `sets_list`, `rule_by_number`,
// `contexts_arena`) are the sole owners; `Tag*`/`Set*`/`Rule*`/`ContextualTest*`
// become `TagId`/`SetId`/`RuleId`/`CtxId` (arena index), nullable → `Option`.
//
// SET-NUMBER RECONCILIATION (documented once, applies to `addSetToList`/`reindex`
// and every `sets_list[number]` access): the C++ `sets_list` is a *vector* that
// reindex rebuilds so `set->number` == the vector index, and post-reindex code
// reaches a set only by that number (`sets_list[number]`). The port keeps the
// `Arena<Set>` (indexed by `SetId`) as the owner and mirrors the C++ vector as
// `sets_list_order: Vec<SetId>`: `add_set_to_list` pushes and assigns
// `number = sets_list_order.len() - 1` exactly like C++, so numbers are the
// DENSE 0..k DFS order (dummy at position 0) and match the C++ on-disk/binary
// numbering. C++ `sets_list[n]` (n a set number) maps to
// `sets_list[sets_list_order[n].0]` (see `set_id_by_number`/`set_by_number`);
// `SetId.0` (the arena slot) is only a storage index and is NOT the number.
// The dummy set still occupies `SetId(0)` (allocated first via
// `allocateDummySet`) and position 0 of `sets_list_order` (front-insert).
// ===========================================================================

// [spec:cg3:def:grammar.cg3.grammar.grammar-fn]
// [spec:cg3:sem:grammar.cg3.grammar.grammar-fn]
/// C++ `~Grammar()`. The C++ dtor manually `delete`s every owned object
/// (sets_list → destroySet, sets_all, single_tags, rule_by_number, contexts).
/// In the arena port each of those lives inside an `Arena<T>` that the derived
/// drop glue tears down automatically when the `Grammar` drops (each arena drops
/// its slots; `Set::drop` runs `trie_delete`). This explicit `Drop` is therefore
/// a documented no-op. DIVERGENCE: the C++ note that `templates` can leak (used
/// templates retained during reindex, not also owned via `contexts`) does NOT
/// occur here — every `ContextualTest` is owned once by `contexts_arena`, so no
/// double-free and no leak.
impl Drop for Grammar {
    fn drop(&mut self) {}
}

impl Grammar {
    // [spec:cg3:def:grammar.cg3.grammar.allocate-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.allocate-set-fn]
    /// `new Set` → arena alloc; inserted into the `sets_all` ownership registry.
    pub fn allocate_set(&mut self) -> SetId {
        let id = SetId(self.sets_list.alloc(Set::default()));
        self.sets_all.insert(id);
        id
    }

    // [spec:cg3:def:grammar.cg3.grammar.destroy-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.destroy-set-fn]
    /// `sets_all.erase(set); delete set` → erase from the registry, then free the
    /// arena slot (which runs the `Set` drop glue). Erasing first prevents a
    /// later double-free in the dtor sweep (moot in the port, but faithful).
    pub fn destroy_set(&mut self, set: SetId) {
        self.sets_all.erase(set);
        self.sets_list.free_slot(set.0);
    }

    // [spec:cg3:def:grammar.cg3.grammar.get-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.get-set-fn]
    /// Resolves `which` as a content hash, else a name hash (with the seeded
    /// name-collision recursion). `nullptr` → `None`.
    pub fn get_set(&self, which: u32) -> Option<SetId> {
        if let Some(&sid) = self.sets_by_contents.get(&which) {
            return Some(sid);
        }
        // else: treat `which` as a name hash.
        let chash = {
            let it = self.sets_by_name.find(which);
            if it == self.sets_by_name.end() {
                return None;
            }
            it.get().1 // sets_by_name[which] == a content hash
        };
        let candidate = match self.sets_by_contents.get(&chash) {
            Some(&c) => c,
            None => return None,
        };
        // set_name_seeds keyed by the candidate set's name.
        let seed = {
            let cand_name = &self.sets_list[candidate.0].name;
            self.set_name_seeds.get(cand_name).copied()
        };
        match seed {
            // getSet(iter->second + iter2->second) — re-resolve with seed folded.
            Some(s) => self.get_set(chash.wrapping_add(s)),
            None => Some(candidate),
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.undef-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.undef-set-fn]
    /// Pulls the set(s) named `_name` (and its `$$`/`&&` unify variants) out of
    /// the name index, renaming each to an internal numeric name. Returns the
    /// LAST-prefix ("") result (the plain-named set) — the `$$`/`&&` variants are
    /// mutated as a side effect but never returned (quirk reproduced).
    pub fn undef_set(&mut self, name_: &str) -> Option<SetId> {
        let mut tset: Option<SetId> = None;
        let pfxs = ["$$", "&&", ""];
        for pfx in pfxs {
            let name = format!("{pfx}{name_}");
            let mut nhash = hash_value_ustring(&name, 0);
            tset = self.get_set(nhash);
            if let Some(t) = tset {
                let to = ui32(self.sets_by_contents.len());
                self.sets_list[t.0].set_name(to, &mut self.rand_state);
            }
            if let Some(&seed) = self.set_name_seeds.get(&name) {
                nhash = nhash.wrapping_add(seed);
                self.set_name_seeds.remove(&name);
            }
            if self.sets_by_name.contains(nhash) {
                self.sets_by_name.erase(nhash);
            }
        }
        tset
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-set-to-list-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-set-to-list-fn]
    /// Depth-first numbers a used set (and its components). Guard: only when
    /// `number == 0` (not yet numbered) AND `s` is not the set at
    /// `sets_list[0]` (the reserved dummy, position 0 of `sets_list_order`).
    /// Children are numbered first. `sets_list.push_back(s);
    /// s->number = UI32(sets_list.size()-1)` → push onto `sets_list_order` and
    /// assign the dense push-back position (see the reconciliation note).
    pub fn add_set_to_list(&mut self, s: SetId) {
        if self.sets_list[s.0].number == SetNumber(0) {
            // C++ guard `sets_list.empty() || sets_list[0] != s`.
            if self.sets_list_order.is_empty() || self.sets_list_order[0] != s {
                let sets = self.sets_list[s.0].sets.clone();
                if !sets.is_empty() {
                    for sit in sets {
                        // C++ addSetToList(getSet(sit)); getSet null → deref crash.
                        let child = self.get_set(sit).unwrap();
                        self.add_set_to_list(child);
                    }
                }
                self.sets_list_order.push(s);
                self.sets_list[s.0].number = SetNumber(ui32(self.sets_list_order.len() - 1));
            }
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.allocate-dummy-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.allocate-dummy-set-fn]
    /// Reserved dummy set at `sets_list` index 0. `setName(STR_DUMMY)` uses the
    /// (string) `setName` overload (assign name directly) — not ported on `Set`,
    /// so the assign is inlined. `sets_list.insert(begin(), set_c)` (front-insert)
    /// → front-insert into `sets_list_order` (the arena keeps the dummy at its
    /// slot, `SetId(0)`, allocated first). `number = MAX` marks it always-used /
    /// never-renumbered (reindex later resets it to 0).
    pub fn allocate_dummy_set(&mut self) {
        let set_c = self.allocate_set();
        self.sets_list[set_c.0].line = 0;
        // setName(STR_DUMMY): non-empty string → name = STR_DUMMY.
        self.sets_list[set_c.0].name = STR_DUMMY.to_string();
        let t = self.allocate_tag(STR_DUMMY);
        self.add_tag_to_set(t, set_c);
        let set_c = self.add_set(set_c);
        self.sets_list[set_c.0].number = SetNumber(u32::MAX);
        // sets_list.insert(sets_list.begin(), set_c)
        self.sets_list_order.insert(0, set_c);
    }

    // [spec:cg3:def:grammar.cg3.grammar.allocate-rule-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.allocate-rule-fn]
    /// `new Rule` → a fresh, unregistered `Rule` VALUE (by-value reconciliation,
    /// same as the tag build-then-intern flow): the caller populates it and hands
    /// it to `add_rule`, which assigns the number + arena slot. (C++ returned a
    /// heap `Rule*`; the port has no id until `add_rule` interns it.)
    pub fn allocate_rule(&self) -> Rule {
        Rule::default()
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-rule-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-rule-fn]
    /// `rule->number = UI32(rule_by_number.size()); rule_by_number.push_back(rule)`
    /// → number is the arena slot the rule is about to occupy (rules are never
    /// freed, so `capacity() == size()`), then alloc. `RuleId.0 == number`.
    pub fn add_rule(&mut self, mut rule: Rule) -> RuleId {
        rule.number = ui32(self.rule_by_number.capacity());
        RuleId(self.rule_by_number.alloc(rule))
    }

    // [spec:cg3:def:grammar.cg3.grammar.destroy-rule-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.destroy-rule-fn]
    /// `delete rule` → free the arena slot. Does not remove it from any index.
    pub fn destroy_rule(&mut self, rule: RuleId) {
        self.rule_by_number.free_slot(rule.0);
    }

    /// C++ no-arg `Tag* Grammar::allocateTag() { return new Tag; }` (unannotated
    /// in the spec). By-value reconciliation: returns a fresh `Tag` for the
    /// build-then-`add_tag` flow.
    pub fn allocate_tag_new(&self) -> Tag {
        Tag::default()
    }

    // [spec:cg3:def:grammar.cg3.grammar.allocate-tag-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.allocate-tag-fn]
    /// Interns a tag from raw text. Empty / leading-`(` texts are hard errors
    /// (`CG3Quit(1)`; the `u_fprintf` diagnostic is deferred I/O). Fast path: an
    /// un-seeded slot whose text matches is returned directly. Otherwise a fresh
    /// `Tag` is `parse_tag_raw`'d and interned via `add_tag`.
    pub fn allocate_tag(&mut self, txt: &str) -> TagId {
        let first = txt.chars().next().unwrap_or('\0');
        if first == '\0' {
            // "Error: Empty tag on line <lines>! Forgot to fill in a ()?"
            cg3_quit(1, Some(file!()), self.lines);
        }
        if first == '(' {
            // "Error: Tag '<txt>' cannot start with ( on line <lines>! ..."
            cg3_quit(1, Some(file!()), self.lines);
        }
        let thash = hash_value_ustring(txt, 0);
        // Fast path: only the un-seeded slot is checked.
        let fast = {
            let it = self.single_tags.find(thash);
            if it != self.single_tags.end() {
                Some(it.get().1)
            } else {
                None
            }
        };
        if let Some(tid) = fast {
            let existing = &self.single_tags_list[tid.0];
            if !existing.tag.is_empty() && existing.tag == txt {
                return tid;
            }
        }
        let mut tag = Tag::default();
        crate::tag::parse_tag_raw(&mut tag, txt, self);
        self.add_tag(tag)
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-tag-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-tag-fn]
    /// Interns a `Tag` (by value) into `single_tags_list` (arena) + `single_tags`
    /// (hash → id), deduplicating by hash+text with the 0..9999 seed probe.
    /// `t == tag` (pointer identity) can never hold for a fresh by-value tag, so
    /// only the text-equality dedup applies; the read-only probe is split from the
    /// insert so the incoming `tag` moves exactly once (after the loop).
    pub fn add_tag(&mut self, mut tag: Tag) -> TagId {
        let hash = tag.rehash();
        let mut existing: Option<TagId> = None;
        let mut chosen_seed: Option<u32> = None;
        let mut seed = 0u32;
        while seed < 10000 {
            let ih = hash.wrapping_add(seed);
            let found: Option<TagId> = {
                let it = self.single_tags.find(ih.get());
                if it != self.single_tags.end() {
                    Some(it.get().1)
                } else {
                    None
                }
            };
            match found {
                Some(t_id) => {
                    // C++ `t->tag == tag->tag`: duplicate parked at a seeded slot.
                    // (`hash += seed; return single_tags[hash]` == returning t_id.)
                    if self.single_tags_list[t_id.0].tag == tag.tag {
                        existing = Some(t_id);
                        break;
                    }
                    // else: hash collision, different text — keep probing.
                }
                None => {
                    chosen_seed = Some(seed);
                    break;
                }
            }
            seed += 1;
        }

        if let Some(t_id) = existing {
            // `delete tag` — the incoming value is dropped at end of scope.
            return t_id;
        }

        let seed = chosen_seed.expect("addTag: seed space exhausted");
        // verbosity_level>0 && seed hash-seed warning: deferred I/O.
        tag.seed = seed;
        let new_hash = tag.rehash(); // rehash folds seed → base+seed == ih.
        let idx = self.single_tags_list.alloc(tag);
        self.single_tags_list[idx].number = idx; // UI32(size-1)
        self.single_tags.insert((new_hash.get(), TagId(idx)));
        TagId(idx)
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-tag-to-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-tag-to-set-fn]
    /// Adds a single tag to `set` as a length-1 trie path + type flags. A failfast
    /// tag (which is also `T_SPECIAL`) lands in BOTH `ff_tags` and `trie_special`
    /// (quirk reproduced). Read the tag type into a local before mutating the set
    /// (both live inside `self`).
    pub fn add_tag_to_set(&mut self, rtag: TagId, set: SetId) {
        let rtype = self.single_tags_list[rtag.0].r#type;
        let s = self.sets_list.get_mut(set.0);
        if rtype.intersects(T_ANY) {
            s.r#type |= ST_ANY;
        }
        if rtype.intersects(T_FAILFAST) {
            s.ff_tags.insert(rtag);
        }
        if rtype.intersects(T_SPECIAL) {
            s.r#type |= ST_SPECIAL;
            s.trie_special.entry(rtag).or_default().terminal = true;
        } else {
            s.trie.entry(rtag).or_default().terminal = true;
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.destroy-tag-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.destroy-tag-fn]
    /// `delete tag` → free the arena slot. Does not unregister from `single_tags`.
    pub fn destroy_tag(&mut self, tag: TagId) {
        self.single_tags_list.free_slot(tag.0);
    }

    // [spec:cg3:def:grammar.cg3.grammar.allocate-contextual-test-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.allocate-contextual-test-fn]
    /// `new ContextualTest` → arena alloc, returning its `CtxId`. Not registered
    /// in `contexts`; the caller interns it later via `add_contextual_test`.
    /// (Unlike tags/rules, contexts use the by-id form because they hold internal
    /// `CtxId` references — `linked`/`ors`/`tmpl` — that must live in the arena.)
    pub fn allocate_contextual_test(&mut self) -> CtxId {
        CtxId(self.contexts_arena.alloc(ContextualTest::default()))
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-contextual-test-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-contextual-test-fn]
    /// Interns a `ContextualTest` into `contexts`, deduplicating structurally
    /// equal tests. `nullptr` → `None`. Recursively interns `linked` and each
    /// `ors` entry (NOT `tmpl`), then linear-probes seeds 0..999.
    pub fn add_contextual_test(&mut self, t: Option<CtxId>) -> Option<CtxId> {
        let t = t?;
        ContextualTest::rehash(&mut self.contexts_arena, t);

        // t->linked = addContextualTest(t->linked)
        let linked = self.contexts_arena[t.0].linked;
        let new_linked = self.add_contextual_test(linked);
        self.contexts_arena[t.0].linked = new_linked;

        // for (auto& it : t->ors) it = addContextualTest(it)
        let ors = self.contexts_arena[t.0].ors.clone();
        let mut new_ors: Vec<CtxId> = Vec::with_capacity(ors.len());
        for it in ors {
            // ors entries are non-null → the result is always Some.
            new_ors.push(self.add_contextual_test(Some(it)).unwrap());
        }
        self.contexts_arena[t.0].ors = new_ors;

        let base = self.contexts_arena[t.0].hash;
        let mut result = t;
        let mut seed = 0u32;
        while seed < 1000 {
            let key = base.wrapping_add(seed);
            match self.contexts.get(&key).copied() {
                None => {
                    self.contexts.insert(key, t);
                    self.contexts_arena[t.0].hash = key; // t->hash += seed
                    self.contexts_arena[t.0].seed = seed;
                    // verbosity_level>1 && seed hash-seed warning: deferred I/O.
                    result = t;
                    break;
                }
                Some(cit) => {
                    if cit == t {
                        result = t;
                        break;
                    }
                    let eq = {
                        let a = &self.contexts_arena[t.0];
                        let b = &self.contexts_arena[cit.0];
                        a.equals(b, &self.contexts_arena)
                    };
                    if eq {
                        // delete t; t = cit->second
                        self.contexts_arena.free_slot(t.0);
                        result = cit;
                        break;
                    }
                }
            }
            seed += 1;
        }
        Some(result)
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-template-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-template-fn]
    /// Registers a named template. Keyed purely by `hash_value(name)` with no
    /// seed/collision handling (quirk: a genuine name-hash collision misreports as
    /// a redefinition). Redefinition → `CG3Quit(1)` (diagnostic deferred).
    pub fn add_template(&mut self, test: CtxId, name: &str) {
        let cn = hash_value_ustring(name, 0);
        if self.templates.contains_key(&cn) {
            cg3_quit(1, Some(file!()), self.lines);
        }
        self.templates.insert(cn, test);
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-anchor-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-anchor-fn]
    /// Registers a named section anchor. `primary` re-definition of an existing
    /// anchor → `CG3Quit(1)`. `at > rule_by_number.size()` (strict `>`) clamps to
    /// the size. Stores only when the anchor did NOT already exist (non-primary
    /// re-adds silently keep the old position — quirk reproduced).
    pub fn add_anchor(&mut self, to: &str, mut at: u32, primary: bool) {
        let ah = {
            let tid = self.allocate_tag(to);
            self.single_tags_list[tid.0].hash
        };
        let exists = self.anchors.contains(ah.get());
        if primary && exists {
            // "Error: Redefinition attempt for anchor '<to>' on line <lines>!"
            cg3_quit(1, Some(file!()), self.lines);
        }
        if at > self.rule_by_number.capacity() {
            // "Warning: No corresponding rule available for anchor ...": deferred.
            at = ui32(self.rule_by_number.capacity());
        }
        if !exists {
            self.anchors.insert((ah.get(), at));
        }
    }
}

impl Grammar {
    // [spec:cg3:def:grammar.cg3.grammar.add-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-set-fn]
    /// Registers a fully-built set, canonicalizing by content and by name, and
    /// returns the canonical `SetId` (the C++ `Set*& to` in/out reference — the
    /// caller reassigns). Steps: delimiter capture, SET→LIST folding, fail-fast
    /// splitting, name registration (the always-break-once loop), content
    /// registration. `getNonEmpty()` (`trie` if non-empty else `trie_special`) is
    /// inlined (not ported on `Set`).
    pub fn add_set(&mut self, mut to: SetId) -> SetId {
        let name = self.sets_list[to.0].name.clone();

        // (1) Delimiter capture (only when the slot is still null).
        if self.delimiters.is_none() && name == STR_DELIMITSET {
            self.delimiters = Some(to);
        } else if self.soft_delimiters.is_none() && name == STR_SOFTDELIMITSET {
            self.soft_delimiters = Some(to);
        } else if self.text_delimiters.is_none() && name == STR_TEXTDELIMITSET {
            self.text_delimiters = Some(to);
        }
        // (2) verbosity_level>0 && name[0]=='T' && name[1]==':' warning: deferred I/O.

        // (3) SET→LIST folding.
        let to_sets = self.sets_list[to.0].sets.clone();
        let to_type = self.sets_list[to.0].r#type;
        if !to_sets.is_empty() && !to_type.intersects(ST_TAG_UNIFY | ST_CHILD_UNIFY | ST_SET_UNIFY)
        {
            let to_set_ops = self.sets_list[to.0].set_ops.clone();
            let mut all_tags = true;
            for i in 0..to_sets.len() {
                if i > 0 && to_set_ops[i - 1] != S_OR {
                    all_tags = false;
                    break;
                }
                let s = self.get_set(to_sets[i]).unwrap();
                if !self.sets_list[s.0].sets.is_empty() {
                    all_tags = false;
                    break;
                }
                if !self.sets_list[s.0].trie.is_empty()
                    && !self.sets_list[s.0].trie_special.is_empty()
                {
                    all_tags = false;
                    break;
                }
                // getNonEmpty().size() != 1 || !trie_singular(getNonEmpty())
                let sset = &self.sets_list[s.0];
                let ne = if !sset.trie.is_empty() {
                    &sset.trie
                } else {
                    &sset.trie_special
                };
                if ne.len() != 1 || !trie_singular(ne) {
                    all_tags = false;
                    break;
                }
            }

            if all_tags {
                for &i in &to_sets {
                    let s = self.get_set(i).unwrap();
                    self.maybe_used_sets.insert(s);
                    // tv = trie_getTagList(s->getNonEmpty())
                    let ne = {
                        let sset = &self.sets_list[s.0];
                        if !sset.trie.is_empty() {
                            sset.trie.clone()
                        } else {
                            sset.trie_special.clone()
                        }
                    };
                    let tv = trie_get_tag_list(&ne, self);
                    if tv.len() == 1 {
                        self.add_tag_to_set(tv[0], to);
                    } else {
                        let mut special = false;
                        for &tag in &tv {
                            if self.single_tags_list[tag.0].r#type.intersects(T_SPECIAL) {
                                special = true;
                                break;
                            }
                        }
                        let node = self.sets_list.get_mut(to.0);
                        if special {
                            trie_insert(&mut node.trie_special, &tv, 0);
                        } else {
                            trie_insert(&mut node.trie, &tv, 0);
                        }
                    }
                }
                {
                    let node = self.sets_list.get_mut(to.0);
                    node.sets.clear();
                    node.set_ops.clear();
                }
                Set::reindex(self, to);
                // verbosity_level>1 "SET ... changed to a LIST": deferred I/O.
            }
        }

        // (4) Fail-fast splitting.
        let (ff_len, trie_sz, trie_sp_sz) = {
            let s = &self.sets_list[to.0];
            (s.ff_tags.size(), s.trie.len(), s.trie_special.len())
        };
        if ff_len != 0 && ff_len < (trie_sz + trie_sp_sz) {
            let positive = self.allocate_set();
            let negative = self.allocate_set();

            self.sets_list[positive.0].name = format!("{STR_GPREFIX}{name}_{STR_POSITIVE}");
            self.sets_list[negative.0].name = format!("{STR_GPREFIX}{name}_{STR_NEGATIVE}");

            // positive->trie.swap(to->trie); positive->trie_special.swap(...):
            // positive is fresh (empty), so `take` from `to` is an equivalent swap.
            let to_trie = std::mem::take(&mut self.sets_list.get_mut(to.0).trie);
            let to_trie_sp = std::mem::take(&mut self.sets_list.get_mut(to.0).trie_special);
            self.sets_list.get_mut(positive.0).trie = to_trie;
            self.sets_list.get_mut(positive.0).trie_special = to_trie_sp;

            let ff: Vec<TagId> = self.sets_list[to.0].ff_tags.iter().copied().collect();
            for iter in ff {
                {
                    let ptrie = &mut self.sets_list.get_mut(positive.0).trie_special;
                    let do_erase = if let Some(node) = ptrie.get_mut(&iter) {
                        if node.terminal {
                            if let Some(sub) = node.trie.as_mut() {
                                trie_delete(sub);
                            }
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if do_erase {
                        ptrie.remove(&iter);
                    }
                }
                // Tag copy with T_FAILFAST cleared, re-interned, added to negative.
                let mut tagcopy = self.single_tags_list[iter.0].clone();
                tagcopy.r#type &= !T_FAILFAST;
                let tid = self.add_tag(tagcopy);
                self.add_tag_to_set(tid, negative);
            }

            Set::reindex(self, positive);
            Set::reindex(self, negative);
            let positive = self.add_set(positive);
            let negative = self.add_set(negative);
            let pos_hash = self.sets_list[positive.0].hash;
            let neg_hash = self.sets_list[negative.0].hash;

            {
                let node = self.sets_list.get_mut(to.0);
                node.ff_tags.clear();
                node.sets.push(pos_hash);
                node.sets.push(neg_hash);
                node.set_ops.push(S_MINUS);
            }
            Set::reindex(self, to);
            // verbosity_level>1 "LIST ... was split into two sets": deferred I/O.
        }

        // (5) Name registration — the `for(;;){...break;}` that runs at most once
        // and is skipped entirely for internal names (quirk reproduced).
        let chash = Set::rehash(self, to);
        if !is_internal(&name) {
            let mut nhash = hash_value_ustring(&name, 0);
            let mut skip = false;
            {
                let sb = self.sets_by_name.find(nhash);
                if sb != self.sets_by_name.end() {
                    let content_hash = sb.get().1;
                    let a = self.sets_by_contents[&content_hash];
                    let a_hash = self.sets_list[a.0].hash;
                    let to_hash = self.sets_list[to.0].hash;
                    if a == to || a_hash == to_hash {
                        skip = true;
                    }
                }
            }
            if !skip {
                if let Some(&seed) = self.set_name_seeds.get(&name) {
                    nhash = nhash.wrapping_add(seed);
                }
                if !self.sets_by_name.contains(nhash) {
                    self.sets_by_name.insert((nhash, chash));
                } else {
                    let existing_content = {
                        let sb = self.sets_by_name.find(nhash);
                        sb.get().1
                    };
                    let a = self.sets_by_contents[&existing_content];
                    let a_hash = self.sets_list[a.0].hash;
                    if chash != a_hash {
                        let a_name = self.sets_list[a.0].name.clone();
                        if a_name == name {
                            // "Error: Set ... already defined ..."
                            cg3_quit(1, Some(file!()), self.lines);
                        }
                        let mut seed = 0u32;
                        while seed < 1000 {
                            if !self.sets_by_name.contains(nhash.wrapping_add(seed)) {
                                // verbosity warn deferred
                                self.set_name_seeds.insert(name.clone(), seed);
                                self.sets_by_name.insert((nhash.wrapping_add(seed), chash));
                                break;
                            }
                            seed += 1;
                        }
                    }
                }
            }
        }

        // (6) Content registration.
        if !self.sets_by_contents.contains_key(&chash) {
            self.sets_by_contents.insert(chash, to);
        } else {
            let a = self.sets_by_contents[&chash];
            if a != to {
                Set::reindex(self, a);
                Set::reindex(self, to);
                let mask = ST_SPECIAL | ST_TAG_UNIFY | ST_CHILD_UNIFY | ST_SET_UNIFY;
                let (at, ao, asz, atr, asp) = {
                    let s = &self.sets_list[a.0];
                    (
                        s.r#type & mask,
                        s.set_ops.len(),
                        s.sets.len(),
                        s.trie.len(),
                        s.trie_special.len(),
                    )
                };
                let (tt, to_, tsz, ttr, tsp) = {
                    let s = &self.sets_list[to.0];
                    (
                        s.r#type & mask,
                        s.set_ops.len(),
                        s.sets.len(),
                        s.trie.len(),
                        s.trie_special.len(),
                    )
                };
                if at != tt || ao != to_ || asz != tsz || atr != ttr || asp != tsp {
                    // "Error: Content hash collision between set ..."
                    cg3_quit(1, Some(file!()), self.lines);
                }
                self.destroy_set(to);
            }
        }
        to = self.sets_by_contents[&chash];
        to
    }

    // [spec:cg3:def:grammar.cg3.grammar.append-to-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.append-to-set-fn]
    /// Implements LIST `+=`: extends the already-defined set named `to->name` with
    /// the new content in `to`, returning the merged canonical `SetId`. Precondition
    /// (quirk): `undefSet` must find that set — a `None` would crash in `addSet`
    /// (`to->name` deref); reproduced via `unwrap`. Delimiter capture at the end
    /// overwrites unconditionally (unlike `addSet`).
    pub fn append_to_set(&mut self, mut to: SetId) -> SetId {
        let to_name = self.sets_list[to.0].name.clone();
        let to_line = self.sets_list[to.0].line;

        // (1) Pull the currently-registered set out of the name index.
        let tset = self.undef_set(&to_name).unwrap();
        // (2) Re-register it under fresh keys.
        let tset = self.add_set(tset);

        if !self.sets_list[tset.0].sets.is_empty() {
            let first_hash = self.sets_list[tset.0].sets[0];
            let fset = self.get_set(first_hash).unwrap();
            let fname = self.sets_list[fset.0].name.clone();
            // NOT a generated positive half → wrap-in-OR.
            let is_positive_split =
                fname.find(STR_GPREFIX) == Some(0) && fname.find(STR_POSITIVE).is_some();
            if !is_positive_split {
                let ns = self.allocate_set();
                self.sets_list[ns.0].name = to_name.clone(); // ns->setName(to->name)
                self.sets_list[ns.0].line = to_line;

                let newname = ui32(self.sets_by_contents.len() + 1);
                self.sets_list[to.0].set_name(newname, &mut self.rand_state);
                to = self.add_set(to);

                let tset_hash = self.sets_list[tset.0].hash;
                let to_hash = self.sets_list[to.0].hash;
                {
                    let node = self.sets_list.get_mut(ns.0);
                    node.sets.push(tset_hash);
                    node.sets.push(to_hash);
                    node.set_ops.push(S_OR);
                }
                to = ns;
            } else {
                // positive-minus-negative split: copy the positive half back into
                // `to`, then re-establish the negative half's failfast tags.
                let set0 = self.get_set(first_hash).unwrap();
                let ptrie = self.sets_list[set0.0].trie.clone();
                let tvs = trie_get_tags(&ptrie, self);
                for tv in &tvs {
                    trie_insert(&mut self.sets_list.get_mut(to.0).trie, tv, 0);
                }
                let ptrie_sp = self.sets_list[set0.0].trie_special.clone();
                let tvs = trie_get_tags(&ptrie_sp, self);
                for tv in &tvs {
                    trie_insert(&mut self.sets_list.get_mut(to.0).trie_special, tv, 0);
                }

                let second_hash = self.sets_list[tset.0].sets[1];
                let set1 = self.get_set(second_hash).unwrap();
                let ntrie = self.sets_list[set1.0].trie.clone();
                let ntrie_sp = self.sets_list[set1.0].trie_special.clone();
                let tva = [
                    trie_get_tag_list(&ntrie, self),
                    trie_get_tag_list(&ntrie_sp, self),
                ];
                for tv in &tva {
                    for &t in tv {
                        let mut tagcopy = self.single_tags_list[t.0].clone();
                        tagcopy.r#type |= T_FAILFAST;
                        let tid = self.add_tag(tagcopy);
                        self.add_tag_to_set(tid, to);
                    }
                }
            }
        } else {
            // plain LIST: merge tries + ff_tags into `to`.
            let ttrie = self.sets_list[tset.0].trie.clone();
            let tvs = trie_get_tags(&ttrie, self);
            for tv in &tvs {
                trie_insert(&mut self.sets_list.get_mut(to.0).trie, tv, 0);
            }
            let ttrie_sp = self.sets_list[tset.0].trie_special.clone();
            let tvs = trie_get_tags(&ttrie_sp, self);
            for tv in &tvs {
                trie_insert(&mut self.sets_list.get_mut(to.0).trie_special, tv, 0);
            }
            let ff: Vec<TagId> = self.sets_list[tset.0].ff_tags.as_slice().to_vec();
            for t in ff {
                self.sets_list.get_mut(to.0).ff_tags.insert(t);
            }
        }

        // (5) Register the merged/renamed set.
        to = self.add_set(to);

        // (6) Delimiter capture (unconditional overwrite).
        let final_name = self.sets_list[to.0].name.clone();
        if final_name == STR_DELIMITSET {
            self.delimiters = Some(to);
        } else if final_name == STR_SOFTDELIMITSET {
            self.soft_delimiters = Some(to);
        } else if final_name == STR_TEXTDELIMITSET {
            self.text_delimiters = Some(to);
        }
        to
    }

    // [spec:cg3:def:grammar.cg3.grammar.get-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.get-tags-fn]
    /// Collects the set's tag combinations into `rv`. Recurses over component sets
    /// (resolved by content hash via `getSet`; other operators than OR are ignored,
    /// per the source ToDo) then appends this set's own trie paths via the shared
    /// `tv` buffer (the sort-then-pop quirk lives in `trie_get_tags_into`).
    pub fn get_tags(&self, set: SetId, rv: &mut TagVectorSet) {
        let sets = self.sets_list[set.0].sets.clone();
        for s in sets {
            let child = self.get_set(s).unwrap(); // *getSet(s), null → crash
            self.get_tags(child, rv);
        }
        let trie = self.sets_list[set.0].trie.clone();
        let trie_special = self.sets_list[set.0].trie_special.clone();
        let mut tv: TagVector = TagVector::new();
        trie_get_tags_into(&trie, rv, &mut tv, self);
        tv.clear();
        trie_get_tags_into(&trie_special, rv, &mut tv, self);
    }

    /// C++ one-arg overload `TagList getTagList_Any(const Set&) const`: delegates
    /// to the two-arg form with a fresh `TagList`.
    pub fn get_tag_list_any_ret(&self, set: SetId) -> TagList {
        let mut the_tags = TagList::new();
        self.get_tag_list_any(set, &mut the_tags);
        the_tags
    }

    // [spec:cg3:def:grammar.cg3.grammar.get-tag-list-any-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.get-tag-list-any-fn]
    /// Collects all tags of a set into `theTags`. (a) unify set → CLEAR + push the
    /// single `tag_any` tag; (b) composite → recurse over `sets_list[iter]`
    /// (treating `sets` entries as NUMBERS, i.e. post-reindex `SetId`s); (c) leaf →
    /// flatten both tries (every key at every depth, incl. non-terminals).
    pub fn get_tag_list_any(&self, set: SetId, the_tags: &mut TagList) {
        let ty = self.sets_list[set.0].r#type;
        if ty.intersects(ST_SET_UNIFY | ST_TAG_UNIFY) {
            the_tags.clear();
            // single_tags.find(tag_any)->second — null-deref crash if absent.
            let tid = {
                let it = self.single_tags.find(self.tag_any);
                it.get().1
            };
            the_tags.push(tid);
        } else if !self.sets_list[set.0].sets.is_empty() {
            let sets = self.sets_list[set.0].sets.clone();
            for iter in sets {
                // getTagList_Any(*sets_list[iter]) — `iter` is a set NUMBER.
                self.get_tag_list_any(self.set_id_by_number(SetNumber(iter)), the_tags);
            }
        } else {
            let trie = self.sets_list[set.0].trie.clone();
            let trie_special = self.sets_list[set.0].trie_special.clone();
            trie_get_tag_list_append(&trie, the_tags, self);
            trie_get_tag_list_append(&trie_special, the_tags, self);
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.remove-numeric-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn]
    /// Returns the hash of a variant of set `s` with all `T_NUMERICAL` tags
    /// removed, building a new `_G_<name>_B_` set only when something was actually
    /// removed. Composite and leaf cases per the spec; `ntags` is a
    /// `BTreeMap<TagVector, bool>` (C++ `std::map<TagVector, bool>`).
    pub fn remove_numeric_tags(&mut self, s: u32) -> u32 {
        let mut set = self.get_set(s).unwrap();
        let is_composite = !self.sets_list[set.0].sets.is_empty();
        if is_composite {
            let mut did = false;
            let mut sets = self.sets_list[set.0].sets.clone();
            for idx in 0..sets.len() {
                let i = sets[idx];
                let ns = self.remove_numeric_tags(i);
                if ns == 0 {
                    // set = getSet(i); "Error: ... branch resulted in set ... empty!"
                    cg3_quit(1, Some(file!()), self.lines);
                }
                if ns != i {
                    sets[idx] = ns;
                    did = true;
                }
            }
            if did {
                let ns_id = self.allocate_set();
                let (ty, line, mut nm, set_ops) = {
                    let src = &self.sets_list[set.0];
                    (src.r#type, src.line, src.name.clone(), src.set_ops.clone())
                };
                nm = format!("{STR_GPREFIX}{nm}_B_");
                {
                    let dst = self.sets_list.get_mut(ns_id.0);
                    dst.r#type = ty;
                    dst.line = line;
                    dst.name = nm;
                    dst.sets = sets;
                    dst.set_ops = set_ops;
                }
                set = self.add_set(ns_id);
            }
        } else {
            let mut did = false;
            let mut ntags: BTreeMap<TagVector, bool> = BTreeMap::new();
            let tries = [
                self.sets_list[set.0].trie.clone(),
                self.sets_list[set.0].trie_special.clone(),
            ];
            for tr in &tries {
                if tr.is_empty() {
                    continue;
                }
                let ctags = trie_get_tags(tr, self);
                for it in &ctags {
                    let mut special = false;
                    let mut tags: TagVector = TagVector::new();
                    fill_tagvector(self, it, &mut tags, &mut did, &mut special);
                    if !tags.is_empty() {
                        ntags.insert(tags, special);
                    }
                }
            }
            let ff: Vec<TagId> = self.sets_list[set.0].ff_tags.as_slice().to_vec();
            if !ff.is_empty() {
                let mut special = false;
                let mut tags: TagVector = TagVector::new();
                fill_tagvector(self, &ff, &mut tags, &mut did, &mut special);
                if !tags.is_empty() {
                    ntags.insert(tags, special);
                }
            }
            if did {
                if ntags.is_empty() {
                    let tid = {
                        let it = self.single_tags.find(self.tag_any);
                        it.get().1
                    };
                    ntags.insert(vec![tid], true);
                    // verbosity_level>0 "Set ... was empty ... C branch": deferred.
                }
                let ns_id = self.allocate_set();
                let (ty, line, mut nm) = {
                    let src = &self.sets_list[set.0];
                    (src.r#type, src.line, src.name.clone())
                };
                nm = format!("{STR_GPREFIX}{nm}_B_");
                {
                    let dst = self.sets_list.get_mut(ns_id.0);
                    dst.r#type = ty;
                    dst.line = line;
                    dst.name = nm;
                }
                for (tagvec, special) in &ntags {
                    if *special {
                        if tagvec.len() == 1
                            && self.single_tags_list[tagvec[0].0]
                                .r#type
                                .intersects(T_FAILFAST)
                        {
                            self.sets_list.get_mut(ns_id.0).ff_tags.insert(tagvec[0]);
                        } else {
                            let dst = &mut self.sets_list.get_mut(ns_id.0).trie_special;
                            trie_insert(dst, tagvec, 0);
                        }
                    } else {
                        let dst = &mut self.sets_list.get_mut(ns_id.0).trie;
                        trie_insert(dst, tagvec, 0);
                    }
                }
                set = self.add_set(ns_id);
            }
        }
        self.sets_list[set.0].hash
    }
}

impl Grammar {
    /// The C++ `sets_list` VECTOR (the numbered used-set list) in dense number
    /// order: position 0 is the dummy, positions 1..k the sets numbered by
    /// `addSetToList`. Unused sets stay in the arena but are not listed. Not a
    /// manifest symbol — port infrastructure.
    fn used_set_ids(&self) -> Vec<SetId> {
        self.sets_list_order.clone()
    }

    /// C++ `grammar->sets_list[number]` — resolves a DENSE set number to its
    /// arena id via `sets_list_order`. Panics on an out-of-range number (the C++
    /// vector-index UB analog). Not a manifest symbol — port infrastructure.
    pub fn set_id_by_number(&self, n: SetNumber) -> SetId {
        self.sets_list_order[n.get() as usize]
    }

    /// C++ `*grammar->sets_list[number]` — borrow of the set with DENSE number
    /// `n`. Not a manifest symbol — port infrastructure.
    pub fn set_by_number(&self, n: SetNumber) -> &Set {
        &self.sets_list[self.sets_list_order[n.get() as usize].0]
    }

    // [spec:cg3:def:grammar.cg3.grammar.index-tag-to-rule-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.index-tag-to-rule-fn]
    /// Inserts rule number `r` into `rules_by_tag[t]` (creating the entry).
    pub fn index_tag_to_rule(&mut self, t: u32, r: u32) {
        self.rules_by_tag.entry(t).or_default().insert(r);
    }

    // [spec:cg3:def:grammar.cg3.grammar.index-tag-to-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.index-tag-to-set-fn]
    /// Sets bit `r` (a set number) in `sets_by_tag[t]`, first creating + resizing
    /// the bitset to `sets_list.size()` (== `sets_list_order.len()`) if absent.
    pub fn index_tag_to_set(&mut self, t: u32, r: u32) {
        if !self.sets_by_tag.contains_key(&t) {
            let mut bs: flags_t = Vec::new();
            bs.resize(self.sets_list_order.len(), false);
            self.sets_by_tag.insert(t, bs);
        }
        self.sets_by_tag.get_mut(&t).unwrap()[r as usize] = true;
    }

    // [spec:cg3:def:grammar.cg3.grammar.index-set-to-rule-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.index-set-to-rule-fn]
    /// Records which tags trigger rule number `r` through target set `s`. Special
    /// / tag-unify sets also index `tag_any`. Descends into BOTH tries and every
    /// child (unlike `indexSets`, it does NOT stop for special sets).
    pub fn index_set_to_rule(&mut self, r: u32, s: SetId) {
        let ty = self.sets_list[s.0].r#type;
        if ty.intersects(ST_SPECIAL | ST_TAG_UNIFY) {
            let ta = self.tag_any;
            self.index_tag_to_rule(ta, r);
        }
        let trie = self.sets_list[s.0].trie.clone();
        let trie_special = self.sets_list[s.0].trie_special.clone();
        trie_index_to_rule(&trie, self, r);
        trie_index_to_rule(&trie_special, self, r);
        let sets = self.sets_list[s.0].sets.clone();
        for i in sets {
            // indexSetToRule(r, sets_list[i]) — `i` is a set NUMBER.
            let child = self.set_id_by_number(SetNumber(i));
            self.index_set_to_rule(r, child);
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.index-sets-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.index-sets-fn]
    /// Maps each tag hash to the bitset of set numbers containing it. Special /
    /// tag-unify sets index `tag_any` and RETURN immediately (no trie/child
    /// descent — the key difference from `indexSetToRule`).
    pub fn index_sets(&mut self, r: u32, s: SetId) {
        let ty = self.sets_list[s.0].r#type;
        if ty.intersects(ST_SPECIAL | ST_TAG_UNIFY) {
            let ta = self.tag_any;
            self.index_tag_to_set(ta, r);
            return;
        }
        let trie = self.sets_list[s.0].trie.clone();
        let trie_special = self.sets_list[s.0].trie_special.clone();
        trie_index_to_set(&trie, self, r);
        trie_index_to_set(&trie_special, self, r);
        let sets = self.sets_list[s.0].sets.clone();
        for i in sets {
            // indexSets(r, sets_list[i]) — `i` is a set NUMBER.
            let child = self.set_id_by_number(SetNumber(i));
            self.index_sets(r, child);
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.set-adjust-sets-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.set-adjust-sets-fn]
    /// Rewrites `s->sets` from content hashes to set numbers, recursively, once
    /// per set (`ST_USED` is the visited marker, cleared on entry). No presence
    /// check on the content-hash lookup (C++ UB → HashMap index panic).
    pub fn set_adjust_sets(&mut self, s: SetId) {
        if !self.sets_list[s.0].r#type.intersects(ST_USED) {
            return;
        }
        self.sets_list.get_mut(s.0).r#type &= !ST_USED;
        let sets = self.sets_list[s.0].sets.clone();
        let mut new_sets = Vec::with_capacity(sets.len());
        for i in &sets {
            let set = self.sets_by_contents[i]; // find(i)->second — no end-check.
            new_sets.push(self.sets_list[set.0].number.get());
            self.set_adjust_sets(set);
        }
        self.sets_list.get_mut(s.0).sets = new_sets;
    }

    // [spec:cg3:def:grammar.cg3.grammar.context-adjust-target-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.context-adjust-target-fn]
    /// Rewrites a test's set references from content hash to set number,
    /// recursively, once per test (`is_used` doubles as the visited marker).
    pub fn context_adjust_target(&mut self, test: CtxId) {
        if !self.contexts_arena[test.0].is_used {
            return;
        }
        self.contexts_arena[test.0].is_used = false;
        let (target, barrier, cbarrier) = {
            let t = &self.contexts_arena[test.0];
            (t.target, t.barrier, t.cbarrier)
        };
        if target.get() != 0 {
            let set = self.sets_by_contents[&target.get()];
            self.contexts_arena[test.0].target = self.sets_list[set.0].number;
        }
        if barrier.get() != 0 {
            let set = self.sets_by_contents[&barrier.get()];
            self.contexts_arena[test.0].barrier = self.sets_list[set.0].number;
        }
        if cbarrier.get() != 0 {
            let set = self.sets_by_contents[&cbarrier.get()];
            self.contexts_arena[test.0].cbarrier = self.sets_list[set.0].number;
        }
        let (ors, tmpl, linked) = {
            let t = &self.contexts_arena[test.0];
            (t.ors.clone(), t.tmpl, t.linked)
        };
        for tor in ors {
            self.context_adjust_target(tor);
        }
        if let Some(t) = tmpl {
            self.context_adjust_target(t);
        }
        if let Some(l) = linked {
            self.context_adjust_target(l);
        }
    }

    // [spec:cg3:def:contextual-test.cg3.contextual-test.mark-used-fn]
    // [spec:cg3:sem:contextual-test.cg3.contextual-test.mark-used-fn]
    /// C++ `void ContextualTest::markUsed(Grammar&)`. INLINED HERE (not on
    /// `ContextualTest`): the sibling deferred it because it recurses into
    /// `Grammar::getSet(..)->markUsed(..)`. Ported faithfully as a private
    /// `Grammar` method; `is_used` guards against re-processing. `getSet` null →
    /// deref crash (reproduced via `unwrap`).
    fn context_mark_used(&mut self, test: CtxId) {
        if self.contexts_arena[test.0].is_used {
            return;
        }
        self.contexts_arena[test.0].is_used = true;
        let (target, barrier, cbarrier, tmpl, ors, linked) = {
            let t = &self.contexts_arena[test.0];
            (
                t.target,
                t.barrier,
                t.cbarrier,
                t.tmpl,
                t.ors.clone(),
                t.linked,
            )
        };
        if target.get() != 0 {
            let s = self.get_set(target.get()).unwrap();
            Set::mark_used(self, s);
        }
        if barrier.get() != 0 {
            let s = self.get_set(barrier.get()).unwrap();
            Set::mark_used(self, s);
        }
        if cbarrier.get() != 0 {
            let s = self.get_set(cbarrier.get()).unwrap();
            Set::mark_used(self, s);
        }
        if let Some(t) = tmpl {
            self.context_mark_used(t);
        }
        for o in ors {
            self.context_mark_used(o);
        }
        if let Some(l) = linked {
            self.context_mark_used(l);
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.reindex-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.reindex-fn]
    /// Core finalization pass (21 steps). Marks used sets/tags/contexts, numbers
    /// the sets, builds every runtime index, and rewrites hash-based refs into
    /// number-based ones. All `u_fprintf` diagnostics are deferred I/O; the
    /// `used_tags` dump still `exit(0)`s the process (flagged quirk). See the
    /// SET-NUMBER RECONCILIATION note for the `sets_list`/`number` handling.
    pub fn reindex(
        &mut self,
        unused_sets: bool,
        used_tags: bool,
    ) -> Result<(), crate::error::Cg3Error> {
        // (1) Reset set state.
        let all_content_sets: Vec<SetId> = self.sets_by_contents.values().copied().collect();
        for sid in &all_content_sets {
            let s = self.sets_list.get_mut(sid.0);
            if s.number == SetNumber(u32::MAX) {
                s.r#type |= ST_USED;
                continue;
            }
            if !s.r#type.intersects(ST_STATIC) {
                s.r#type &= !ST_USED;
            }
            s.number = SetNumber(0);
        }

        // (2) Static sets.
        let static_sets = self.static_sets.clone();
        for sset in &static_sets {
            let sh = hash_value_ustring(sset, 0);
            if self.set_alias.contains(sh) {
                // "Error: Static set ... is an alias ..."
                crate::error::emit_cg3quit_line(file!(), self.lines);
                return Err(crate::error::Cg3Error::fatal(1, None));
            }
            let s = match self.get_set(sh) {
                Some(s) => s,
                None => continue, // verbosity warn deferred
            };
            if &self.sets_list[s.0].name != sset {
                self.sets_list.get_mut(s.0).name = sset.clone();
            }
            Set::mark_used(self, s);
            self.sets_list.get_mut(s.0).r#type |= ST_STATIC;
        }

        // (3) Clear/reset containers.
        self.set_alias.clear(0);
        self.sets_by_name.clear(0);
        self.rules.clear();
        self.before_sections.clear();
        self.after_sections.clear();
        self.null_section.clear();
        self.sections.clear();
        if !self.is_binary {
            // sets_list.resize(1); sets_list[0]->number = 0 — keep only the dummy
            // in the numbered order and reset its number. Guarded so a dummy-less
            // grammar does not panic (C++ would UB).
            self.sets_list_order.truncate(1);
            if let Some(&d0) = self.sets_list_order.first() {
                self.sets_list.get_mut(d0.0).number = SetNumber(0);
            }
        }
        self.set_name_seeds.clear();
        self.sets_any = None;
        self.rules_any = None;

        // Snapshot of every live tag id (arena order == tag number order).
        let all_tag_ids: Vec<TagId> = (0..self.single_tags_list.capacity())
            .filter(|&i| self.single_tags_list.try_get(i).is_some())
            .map(TagId)
            .collect();

        // (4) Populate regex_tags / icase_tags; markUsed varstring sets.
        for tid in &all_tag_ids {
            let (has_regexp, is_txt, is_icase, vs) = {
                let t = &self.single_tags_list[tid.0];
                (
                    t.regexp.is_some(),
                    is_textual(&t.tag),
                    t.r#type.intersects(T_CASE_INSENSITIVE),
                    t.vs_sets.clone(),
                )
            };
            if has_regexp && !is_txt {
                // regex_tags keyed by owning TagId (skeleton note).
                self.regex_tags.insert(*tid);
            }
            if is_icase && !is_txt {
                self.icase_tags.insert(*tid);
            }
            if self.is_binary {
                continue;
            }
            if let Some(vs) = vs {
                for sit in vs {
                    Set::mark_used(self, sit);
                }
            }
        }

        // (5) Propagate T_TEXTUAL (regex find + icase compare).
        let regex_tag_ids: Vec<TagId> = self.regex_tags.iter().copied().collect();
        let icase_tag_ids: Vec<TagId> = self.icase_tags.iter().copied().collect();
        for tid in &all_tag_ids {
            if self.single_tags_list[tid.0].r#type.intersects(T_TEXTUAL) {
                continue;
            }
            let ttext = self.single_tags_list[tid.0].tag.clone();
            let mut textual = false;
            for rid in &regex_tag_ids {
                if let Some(re) = &self.single_tags_list[rid.0].regexp {
                    // uregex_find(-1) == unanchored search == Regex::is_match.
                    if re.is_match(&ttext) {
                        textual = true;
                    }
                }
            }
            for iid in &icase_tag_ids {
                let itext = &self.single_tags_list[iid.0].tag;
                if ux_str_case_compare(&ttext, itext) {
                    textual = true;
                }
            }
            if textual {
                self.single_tags_list.get_mut(tid.0).r#type |= T_TEXTUAL;
            }
        }

        // (6) Mark parenthesis + preferred-target tags used.
        let parens: Vec<(u32, u32)> = self.parentheses.iter().map(|(&a, &b)| (a, b)).collect();
        for (a, b) in parens {
            let ta = {
                let it = self.single_tags.find(a);
                it.get().1
            };
            self.single_tags_list.get_mut(ta.0).mark_used();
            let tb = {
                let it = self.single_tags.find(b);
                it.get().1
            };
            self.single_tags_list.get_mut(tb.0).mark_used();
        }
        let pref: Vec<u32> = self.preferred_targets.clone();
        for it in pref {
            let t = {
                let iter = self.single_tags.find(it);
                iter.get().1
            };
            self.single_tags_list.get_mut(t.0).mark_used();
        }

        // (7) Rule pre-pass.
        let all_rule_ids: Vec<RuleId> = (0..self.rule_by_number.capacity())
            .filter(|&i| self.rule_by_number.try_get(i).is_some())
            .map(RuleId)
            .collect();
        for rid in &all_rule_ids {
            let (
                wordform,
                rtype,
                target,
                childset1,
                childset2,
                maplist,
                sublist,
                dep_target,
                tests,
                dep_tests,
            ) = {
                let r = &self.rule_by_number[rid.0];
                (
                    r.wordform,
                    r.r#type,
                    r.target,
                    r.childset1,
                    r.childset2,
                    r.maplist,
                    r.sublist,
                    r.dep_target,
                    r.tests.clone(),
                    r.dep_tests.clone(),
                )
            };
            if wordform.is_some() {
                self.wf_rules.push(*rid);
            }
            if rtype == KEYWORDS::K_PROTECT {
                self.has_protect = true;
            }
            if self.is_binary {
                continue;
            }
            {
                let s = self.get_set(target.get()).unwrap();
                Set::mark_used(self, s);
            }
            if childset1.get() != 0 {
                let s = self.get_set(childset1.get()).unwrap();
                Set::mark_used(self, s);
            }
            if childset2.get() != 0 {
                let s = self.get_set(childset2.get()).unwrap();
                Set::mark_used(self, s);
            }
            if let Some(m) = maplist {
                Set::mark_used(self, m);
            }
            if let Some(sl) = sublist {
                Set::mark_used(self, sl);
            }
            if let Some(dt) = dep_target {
                self.context_mark_used(dt);
            }
            for it in &tests {
                self.context_mark_used(*it);
            }
            for it in &dep_tests {
                self.context_mark_used(*it);
            }
        }

        // (8) Delimiter markUsed; filter templates / contexts.
        if !self.is_binary {
            if let Some(d) = self.delimiters {
                Set::mark_used(self, d);
            }
            if let Some(d) = self.soft_delimiters {
                Set::mark_used(self, d);
            }
            if let Some(d) = self.text_delimiters {
                Set::mark_used(self, d);
            }

            let templates: Vec<(u32, CtxId)> =
                self.templates.iter().map(|(&k, &v)| (k, v)).collect();
            let mut tosave: contexts_t = contexts_t::default();
            for (k, v) in templates {
                if self.contexts_arena[v.0].is_used {
                    tosave.insert(k, v);
                }
            }
            self.templates = tosave;

            let contexts: Vec<(u32, CtxId)> = self.contexts.iter().map(|(&k, &v)| (k, v)).collect();
            let mut tosave2: contexts_t = contexts_t::default();
            for (k, v) in contexts {
                if self.contexts_arena[v.0].is_used {
                    tosave2.insert(k, v);
                } else {
                    self.contexts_arena.free_slot(v.0); // delete cntx.second
                }
            }
            self.contexts = tosave2;
        }

        // (9) Unused-sets diagnostic (ux_stdout): deferred I/O; no state change.
        if unused_sets {
            // "Unused sets:" ... "End of unused sets." — deferred I/O.
        }

        // (10) Build sets_list (depth-first numbering) for used sets.
        let content_sets: Vec<SetId> = self.sets_by_contents.values().copied().collect();
        for sid in content_sets {
            if self.sets_list[sid.0].r#type.intersects(ST_USED) {
                self.add_set_to_list(sid);
            }
        }

        // (11) Recompute mapping flag over single_tags.
        let mp = self.mapping_prefix;
        for tid in &all_tag_ids {
            let first = self.single_tags_list[tid.0]
                .tag
                .chars()
                .next()
                .unwrap_or('\0');
            let t = self.single_tags_list.get_mut(tid.0);
            if first == mp {
                t.r#type |= T_MAPPING;
            } else {
                t.r#type &= !T_MAPPING;
            }
        }

        // (12) reindex + setAdjustSets + indexSets over sets_list.
        let sl_ids = self.used_set_ids();
        if !self.is_binary {
            for &sid in &sl_ids {
                Set::reindex(self, sid);
            }
            for &sid in &sl_ids {
                self.set_adjust_sets(sid);
            }
        }
        for &sid in &sl_ids {
            let num = self.sets_list[sid.0].number.get();
            self.index_sets(num, sid);
        }

        // (13) Rule finalization.
        let mut sects = uint32SortedVector::new();
        for rid in &all_rule_ids {
            let (section, number, target) = {
                let r = &self.rule_by_number[rid.0];
                (r.section, r.number, r.target)
            };
            if section == -1 {
                self.before_sections.push(*rid);
            } else if section == -2 {
                self.after_sections.push(*rid);
            } else if section == -3 {
                self.null_section.push(*rid);
            } else {
                sects.insert(section as u32);
                self.rules.push(*rid);
            }

            if target.get() != 0 {
                let set = if self.is_binary {
                    self.sets_list_order[target.get() as usize] // sets_list[rule->target]
                } else {
                    let s = self.sets_by_contents[&target.get()];
                    let num = self.sets_list[s.0].number;
                    self.rule_by_number.get_mut(rid.0).target = num;
                    s
                };
                self.index_set_to_rule(number, set);
                let rtarget = self.rule_by_number[rid.0].target;
                self.rules_by_set.entry(rtarget.get()).or_default().insert(number);
            } else {
                // "Warning: Rule on line ... had no target": deferred I/O.
            }

            let (maplist, sublist, childset1, childset2, dep_target, tests, dep_tests) = {
                let r = &self.rule_by_number[rid.0];
                (
                    r.maplist,
                    r.sublist,
                    r.childset1,
                    r.childset2,
                    r.dep_target,
                    r.tests.clone(),
                    r.dep_tests.clone(),
                )
            };
            let mut cap = false;
            if let Some(m) = maplist {
                if self.sets_list[m.0].r#type.intersects(ST_CHILD_UNIFY) {
                    cap = true;
                }
            }
            if let Some(sl) = sublist {
                if self.sets_list[sl.0].r#type.intersects(ST_CHILD_UNIFY) {
                    cap = true;
                }
            }
            if cap {
                self.rule_by_number.get_mut(rid.0).flags |= RF_CAPTURE_UNIF;
            }
            if self.is_binary {
                continue;
            }
            if childset1.get() != 0 {
                let s = self.sets_by_contents[&childset1.get()];
                let n = self.sets_list[s.0].number;
                self.rule_by_number.get_mut(rid.0).childset1 = n;
            }
            if childset2.get() != 0 {
                let s = self.sets_by_contents[&childset2.get()];
                let n = self.sets_list[s.0].number;
                self.rule_by_number.get_mut(rid.0).childset2 = n;
            }
            if let Some(dt) = dep_target {
                self.context_adjust_target(dt);
            }
            for test in &tests {
                self.context_adjust_target(*test);
            }
            for test in &dep_tests {
                self.context_adjust_target(*test);
            }
        }

        // (14) Fill sections contiguously 0..=sects.back().
        if !sects.empty() {
            let back = sects.back();
            for i in 0..=back {
                self.sections.push(i);
            }
        }

        // (15) Cache any-tag indexes (clone: the port fields own a copy, not a
        // pointer into the map — see the `sets_any`/`rules_any` field docs).
        let ta = self.tag_any;
        if let Some(bs) = self.sets_by_tag.get(&ta).cloned() {
            self.sets_any = Some(bs);
        }
        if let Some(iv) = self.rules_by_tag.get(&ta).cloned() {
            self.rules_any = Some(iv);
        }

        // (16) Clear sets_by_contents (sets henceforth referenced by number).
        self.sets_by_contents.clear();

        // (17) Re-register static-set names by NUMBER.
        for &to in &sl_ids {
            if self.sets_list[to.0].r#type.intersects(ST_STATIC) {
                let nm = self.sets_list[to.0].name.clone();
                let nhash = hash_value_ustring(&nm, 0);
                let cnum = self.sets_list[to.0].number;
                if !self.sets_by_name.contains(nhash) {
                    self.sets_by_name.insert((nhash, cnum.get()));
                } else {
                    let existing_num = {
                        let sb = self.sets_by_name.find(nhash);
                        sb.get().1
                    };
                    // sets_list[existing_num]->number (existing_num is a number).
                    let a_sid = self.sets_list_order[existing_num as usize];
                    let a_num = self.sets_list[a_sid.0].number;
                    if cnum != a_num {
                        let a_name = self.sets_list[a_sid.0].name.clone();
                        if a_name == nm {
                            // "Error: Static set ... already defined ..."
                            crate::error::emit_cg3quit_line(file!(), self.lines);
                            return Err(crate::error::Cg3Error::fatal(1, None));
                        }
                        let mut seed = 0u32;
                        while seed < 1000 {
                            if !self.sets_by_name.contains(nhash.wrapping_add(seed)) {
                                self.set_name_seeds.insert(nm.clone(), seed);
                                self.sets_by_name.insert((nhash.wrapping_add(seed), cnum.get()));
                                break;
                            }
                            seed += 1;
                        }
                    }
                }
            }
        }

        // (18) sets_vstr: transitively contains varstring tags (fixpoint).
        // boost::dynamic_bitset<> sets_vstr(sets_list.size()) — indexed by dense
        // set numbers.
        let cap_sz = self.sets_list_order.len();
        let mut sets_vstr: Vec<bool> = vec![false; cap_sz];
        let mut did = true;
        while did {
            did = false;
            for &set in &sl_ids {
                let num = self.sets_list[set.0].number.get() as usize;
                if sets_vstr[num] {
                    continue;
                }
                let sets = self.sets_list[set.0].sets.clone();
                for iset in &sets {
                    if sets_vstr[*iset as usize] {
                        sets_vstr[num] = true;
                        did = true;
                        break;
                    }
                }
                let has_vs = {
                    let trie = self.sets_list[set.0].trie.clone();
                    let trie_special = self.sets_list[set.0].trie_special.clone();
                    trie_has_type(&trie, T_VARSTRING, self)
                        || trie_has_type(&trie_special, T_VARSTRING, self)
                };
                if has_vs {
                    sets_vstr[num] = true;
                    did = true;
                }
            }
        }

        // (19) nk: contexts that use unification or varstrings (fixpoint).
        let context_ids: Vec<CtxId> = self.contexts.values().copied().collect();
        let mut nk: BTreeSet<CtxId> = BTreeSet::new();
        let mut did = true;
        while did {
            did = false;
            for &t in &context_ids {
                if nk.contains(&t) {
                    continue;
                }
                let (tmpl, linked, target, barrier, cbarrier) = {
                    let ct = &self.contexts_arena[t.0];
                    (ct.tmpl, ct.linked, ct.target, ct.barrier, ct.cbarrier)
                };
                if let Some(tm) = tmpl {
                    if nk.contains(&tm) {
                        if nk.insert(t) {
                            did = true;
                        }
                        continue;
                    }
                }
                if let Some(l) = linked {
                    if nk.contains(&l) {
                        if nk.insert(t) {
                            did = true;
                        }
                        continue;
                    }
                }
                if target.get() != 0 && self.set_by_number(target).r#type.intersects(MASK_ST_UNIFY) {
                    if nk.insert(t) {
                        did = true;
                    }
                    continue;
                }
                if target.get() != 0 && sets_vstr[target.get() as usize] {
                    if nk.insert(t) {
                        did = true;
                    }
                    continue;
                }
                if barrier.get() != 0 && self.set_by_number(barrier).r#type.intersects(MASK_ST_UNIFY)
                {
                    if nk.insert(t) {
                        did = true;
                    }
                    continue;
                }
                if barrier.get() != 0 && sets_vstr[barrier.get() as usize] {
                    if nk.insert(t) {
                        did = true;
                    }
                    continue;
                }
                if cbarrier.get() != 0
                    && self
                        .set_by_number(cbarrier)
                        .r#type
                        .intersects(MASK_ST_UNIFY)
                {
                    if nk.insert(t) {
                        did = true;
                    }
                    continue;
                }
                if cbarrier.get() != 0 && sets_vstr[cbarrier.get() as usize] {
                    if nk.insert(t) {
                        did = true;
                    }
                    continue;
                }
            }
        }

        // (20) Auto-KEEPORDER.
        for rid in &all_rule_ids {
            let (flags, sublist, maplist, dep_target, tests, dep_tests) = {
                let r = &self.rule_by_number[rid.0];
                (
                    r.flags,
                    r.sublist,
                    r.maplist,
                    r.dep_target,
                    r.tests.clone(),
                    r.dep_tests.clone(),
                )
            };
            if flags.intersects(RF_KEEPORDER) {
                continue;
            }
            let mut needs = false;
            if let Some(sl) = sublist {
                if sets_vstr[self.sets_list[sl.0].number.get() as usize] {
                    needs = true;
                }
            }
            if let Some(m) = maplist {
                if sets_vstr[self.sets_list[m.0].number.get() as usize] {
                    needs = true;
                }
            }
            if let Some(dt) = dep_target {
                if nk.contains(&dt) {
                    needs = true;
                }
            }
            for cntx in &tests {
                if nk.contains(cntx) {
                    needs = true;
                }
            }
            for cntx in &dep_tests {
                if nk.contains(cntx) {
                    needs = true;
                }
            }
            if needs {
                self.rule_by_number.get_mut(rid.0).flags |= RF_KEEPORDER;
            }
        }

        // (21) used_tags dump → exit(0) (flagged quirk: terminates). Wave 4:
        // returned as a Cg3Error carrying exit code 0; the binaries convert it
        // to the exit.
        if used_tags {
            // for tag in single_tags with T_USED: print toUString(true) to
            // ux_stdout — deferred I/O.
            return Err(crate::error::Cg3Error::fatal(0, None));
        }

        Ok(())
    }
}

// [spec:cg3:def:grammar.cg3.trie-index-to-rule-fn]
// [spec:cg3:sem:grammar.cg3.trie-index-to-rule-fn]
/// Free fn. Recursively maps every tag (at every depth, incl. non-terminals) to
/// rule number `r` via `grammar.indexTagToRule(tag->hash, r)`. The `trie` must be
/// an EXTERNAL copy (callers clone the set's trie out first) so it does not alias
/// the `&mut Grammar` borrow.
pub fn trie_index_to_rule(trie: &trie_t, grammar: &mut Grammar, r: u32) {
    for (k, node) in trie.iter() {
        let h = grammar.single_tags_list[k.0].hash;
        grammar.index_tag_to_rule(h.get(), r);
        if let Some(sub) = &node.trie {
            trie_index_to_rule(sub, grammar, r);
        }
    }
}

// [spec:cg3:def:grammar.cg3.trie-index-to-set-fn]
// [spec:cg3:sem:grammar.cg3.trie-index-to-set-fn]
/// Free fn. Identical shape to `trie_index_to_rule` but sets bit `r` (a set
/// number) in `sets_by_tag[tag->hash]` for every tag in the trie.
pub fn trie_index_to_set(trie: &trie_t, grammar: &mut Grammar, r: u32) {
    for (k, node) in trie.iter() {
        let h = grammar.single_tags_list[k.0].hash;
        grammar.index_tag_to_set(h.get(), r);
        if let Some(sub) = &node.trie {
            trie_index_to_set(sub, grammar, r);
        }
    }
}

// [spec:cg3:def:grammar.cg3.trie-unserialize-fn]
// [spec:cg3:sem:grammar.cg3.trie-unserialize-fn]
/// Free fn. Deserializes a tag-trie from a binary-grammar stream (mirrors
/// `trie_serialize`). Per entry: BE `u32` tag index → key `TagId(index)` (the
/// arena index IS the tag number; C++ dereferenced `single_tags_list[index]` to
/// obtain the `Tag*` used as the flat_map key), BE `u8` terminal flag, BE `u32`
/// child count (recurse if non-zero). No bounds check on the tag index (arena
/// index panics on OOB, matching the C++ UB).
pub fn trie_unserialize<R: Read>(
    trie: &mut trie_t,
    input: &mut R,
    grammar: &Grammar,
    num_tags: u32,
) {
    for _ in 0..num_tags {
        let u32tmp: u32 = crate::inlines::read_be(input);
        // Parity: C++ indexes single_tags_list[u32tmp] (OOB → UB); the key IS
        // TagId(u32tmp) since arena index == tag number.
        let _tag = &grammar.single_tags_list[u32tmp];
        let node = trie.entry(TagId(u32tmp)).or_default();

        let u8tmp: u8 = crate::inlines::read_be(input);
        node.terminal = u8tmp != 0;

        let child_count: u32 = crate::inlines::read_be(input);
        if child_count != 0 {
            if node.trie.is_none() {
                node.trie = Some(Box::new(trie_t::new()));
            }
            trie_unserialize(
                node.trie.as_deref_mut().unwrap(),
                input,
                grammar,
                child_count,
            );
        }
    }
}

/// `uextras.hpp` `ux_strCaseCompare(a, b)` (ICU `u_strCaseCompare`,
/// `U_FOLD_CASE_DEFAULT`) — true on full case-fold equality. Approximated with
/// Unicode simple lowercase folding (same stand-in as `tag.rs`; ICU-vs-Rust
/// parity risk for non-ASCII). Local because `uextras` is not yet ported and this
/// pass edits only `grammar.rs`.
fn ux_str_case_compare(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}
