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
//!
//! ## Core / overlay split
//! A grammar lives in two phases, and each has its own type. [`GrammarCore`]
//! is the grammar: the parsers and the binary reader build it, `reindex` and
//! the relabeller finish it, the writers serialise it — all through
//! `&mut GrammarCore`, which only its owner can have. Loaded, it goes behind
//! an `Arc`, and from then on nothing can edit it.
//!
//! [`Grammar`] is one run's view of a loaded grammar: a shared core plus a thin
//! OVERLAY of the tag state the run does change. Applying a grammar can mint
//! new tags (varstrings, runtime regexes), and those are the stream's, not the
//! grammar's. [`TagStore`] is the arena AS THE RUN SEES IT — core tags below
//! `core.single_tags_list.capacity()`, the run's own above — so a `TagId` still
//! indexes one flat space. [`Grammar::single_tags`] does the same for the hash
//! index. `Grammar` derefs to the core and does not deref MUTABLY, so a run
//! cannot edit the grammar it applies.
//!
//! Interning runs in both phases, so it is written once, over [`TagSpace`],
//! which both types implement.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::arena::{Arena, CtxId, RuleId, SetId, TagId};
use crate::flat_unordered_map::{FlatUnorderedMap, Uint32FlatHashMap};
use crate::interval_vector::Uint32IntervalVector;
use crate::sorted_vector::{SortedVector, Uint32SortedVector};
use crate::strings::STR_DUMMY;
use crate::types::{DynBitset, SetNumber, Uint32Vector};

// Sibling grammar-object types (created by parallel agents). Aliased locally so
// the arena declarations read against a stable name.
use crate::contextual_test::ContextualTest;
use crate::rule::Rule;
use crate::set::Set;
use crate::tag::Tag;

// --- Method-pass imports (added with the fn bodies) ---
use crate::inlines::{hash_value_str, is_internal, ui32};
use crate::set::{ST_ANY, ST_CHILD_UNIFY, ST_SET_UNIFY, ST_SPECIAL, ST_TAG_UNIFY};
use crate::tag::{T_ANY, T_FAILFAST, T_SPECIAL, TagList, TagVectorSet};
use crate::tag_trie::{
    TagTrie, trie_delete, trie_get_tag_list, trie_get_tag_list_append, trie_get_tags, trie_insert,
    trie_singular,
};

// ---------------------------------------------------------------------------
// Local string / operator constants.
//
// These live in `src/Strings.hpp` (annotated there) but are out of scope for
// the `crate::strings` port (which covers only `KEYWORDS`). Because this pass
// may edit ONLY `grammar.rs`, they are reproduced here verbatim as local
// stand-ins (same precedent as the local scanf stand-ins in `tag.rs` /
// `set.rs`). To reconcile: move to `crate::strings` when that module grows.
mod finish;
mod walks;

const STR_DELIMITSET: &str = "_S_DELIMITERS_";
const STR_SOFTDELIMITSET: &str = "_S_SOFT_DELIMITERS_";
const STR_TEXTDELIMITSET: &str = "_S_TEXT_DELIMITERS_";
const STR_GPREFIX: &str = "_G_";
const STR_POSITIVE: &str = "POSITIVE";
const STR_NEGATIVE: &str = "NEGATIVE";

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
pub type Contexts = BTreeMap<u32, CtxId>;

// [spec:cg3:def:grammar.cg3.grammar.set-name-seeds-t]
/// C++ `set_name_seeds_t`: an unordered map from set name to seed.
/// `String` keys; the C++ custom string hasher collapses into the std hasher.
pub type SetNameSeeds = HashMap<String, u32>;

// [spec:cg3:def:grammar.cg3.grammar.static-sets-t]
/// C++ `static_sets_t`: a vector of set names.
pub type StaticSets = Vec<String>;

// [spec:cg3:def:grammar.cg3.grammar.regex-tags-t]
/// C++ `regex_tags_t`: a set of compiled-regex pointers.
///
/// NOTE: each compiled regex is owned by exactly one `Tag` (`tag->regexp`,
/// inserted in `reindex`), so the set is keyed by the owning tag's `TagId`; the
/// compiled regex is reached via that tag.
pub type RegexTags = BTreeSet<TagId>;

// [spec:cg3:def:grammar.cg3.grammar.icase-tags-t]
/// C++ `typedef TagSortedVector icase_tags_t` (`sorted_vector<Tag*, compare_Tag>`).
///
/// NOTE: the custom `compare_Tag` comparator (orders by tag content) is not yet
/// ported; this uses the default `Less` ordering over `TagId`. To reconcile when
/// `compare_Tag` lands.
pub type IcaseTags = SortedVector<TagId>;

// [spec:cg3:def:grammar.cg3.grammar.rules-by-set-t]
/// C++ `typedef std::unordered_map<uint32_t, uint32IntervalVector> rules_by_set_t`.
pub type RulesBySet = HashMap<u32, Uint32IntervalVector>;

// [spec:cg3:def:grammar.cg3.grammar.rules-by-tag-t]
/// C++ `typedef std::unordered_map<uint32_t, uint32IntervalVector> rules_by_tag_t`.
pub type RulesByTag = HashMap<u32, Uint32IntervalVector>;

// [spec:cg3:def:grammar.cg3.grammar.sets-by-tag-t]
/// C++ `typedef std::unordered_map<uint32_t, boost::dynamic_bitset<>> sets_by_tag_t`.
/// The `dynamic_bitset` value becomes `crate::types::flags_t`.
pub type SetsByTag = HashMap<u32, DynBitset>;

// [spec:cg3:def:grammar.cg3.grammar.parentheses-t]
/// C++ `typedef bc::flat_map<uint32_t, uint32_t> parentheses_t`.
/// Represented as `BTreeMap` (sorted associative container).
pub type Parentheses = BTreeMap<u32, u32>;

/// What a completed [`Grammar::reindex`] leaves its caller to do.
///
/// The C++ `exit(0)`s inside `reindex` once the `--show-tags` dump has been
/// written. That is a successful stop, and success does not travel in the error
/// channel — `[spec:cg3:req:errors.exit-codes-at-cli]` puts the decision at the
/// boundary that owns the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Reindexed {
    /// Nothing further; the grammar is ready to use.
    Done,
    /// `used_tags` was asked for and the dump has been written. The caller is
    /// done, successfully.
    DumpedTags,
}

// [spec:cg3:def:grammar.cg3.grammar]
/// The parsed/loaded grammar: owner of all static tags, sets, rules and
/// contextual tests, plus every runtime lookup index built by `reindex`.
///
/// Frozen once loaded, and shared from there on — a process applying the same
/// grammar down N pipelines holds ONE of these behind an `Arc` instead of N
/// copies, which for a real grammar is hundreds of megabytes of sets, rules and
/// contexts per pipeline. Everything a run needs to change lives beside it in
/// [`Grammar`]'s overlay instead.
pub struct GrammarCore {
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
    /// The PARSE-TIME tag count, not the live one. Runtime interning grows
    /// `single_tags_list` without touching this, so the two diverge during a
    /// run — and that is required, not tolerated: `write_binary_grammar` writes
    /// this number and then emits `0..num_tags`, so it must stay the count of
    /// tags that existed when the grammar was compiled. Runtime tags appear in
    /// no trie and must not be serialised. Reachable after a run via
    /// `vislcg3 --grammar-bin` without `--grammar-only`.
    pub num_tags: usize,
    pub mapping_prefix: char,
    pub lines: u32,
    pub verbosity_level: u32,
    pub total_time: f64,

    // --- command-line argument capture ---
    pub cmdargs: String,
    pub cmdargs_override: String,

    // [spec:cg3:req:diagnostics.source-lazy]
    /// ADDED — no C++ analog. The paths of the sources a textual parse read, in
    /// the order it read them, indexed by
    /// [`RuleProvenance::source`](crate::rule::RuleProvenance::source). Empty
    /// after a binary load, which has a companion source file instead.
    ///
    /// Paths, not text: a grammar runs to megabytes and almost no run fails, so
    /// the text is read back only when a failure needs quoting.
    pub source_names: Vec<String>,

    // [spec:cg3:req:diagnostics.source-lazy]
    /// ADDED — no C++ analog. The `.cg3b` this grammar was read from, when it
    /// was read from one by path, so a runtime failure can find the companion
    /// source file beside it. `None` for a textual load (which has
    /// [`source_names`](Self::source_names) instead) and for a binary load
    /// handed bytes with no file behind them.
    pub binary_path: Option<String>,

    // --- tags ---
    /// Owned tag arena (was `std::vector<Tag*> single_tags_list`); `TagId` indexes it.
    ///
    /// The LOAD-TIME tags only. A run reaches its tags through
    /// [`Grammar::single_tags_list`], which spans this arena and the run's own
    /// additions; this field is the lower half of that span.
    pub single_tags_list: Arena<Tag>,
    /// C++ `Taguint32HashMap` (`flat_unordered_map<uint32_t, Tag*>`): hash → tag.
    ///
    /// The LOAD-TIME entries only, and deliberately NOT named `single_tags`:
    /// [`Grammar::single_tags`] is the spanning index a run must use, and a
    /// field of that name here would let a lookup reach this half alone.
    pub tags_by_hash: FlatUnorderedMap<u32, TagId>,

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
    pub sets_all: SortedVector<SetId>,
    /// C++ `uint32FlatHashMap sets_by_name`: name-hash → (content-hash | set-number).
    pub sets_by_name: Uint32FlatHashMap,
    pub set_name_seeds: SetNameSeeds,
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
    pub maybe_used_sets: SortedVector<SetId>,

    pub static_sets: StaticSets,

    /// The LOAD-TIME regex tags, built by `reindex`. A run interns regex tags of
    /// its own, so it works from its own copy
    /// ([`Grammar::regex_tags`](Grammar#structfield.regex_tags), seeded from
    /// this one), the same way it works from its own tag type flags.
    pub regex_tags: RegexTags,
    /// The LOAD-TIME case-insensitive tags; see [`regex_tags`](Self::regex_tags).
    pub icase_tags: IcaseTags,

    // --- contextual tests ---
    /// Owned contextual-test arena (ADDED — port infra) backing every `CtxId`.
    pub contexts_arena: Arena<ContextualTest>,
    pub templates: Contexts,
    pub contexts: Contexts,

    // --- runtime indexes ---
    //
    // Built by `reindex` and NOT updated when a tag is interned at runtime.
    // That is correct, on two legs:
    //
    // 1. `single_tags` is append-only for the life of a grammar — `destroy_tag`
    //    deliberately does not unregister, and has no production caller — so the
    //    `hash + seed` probe chain is prefix-stable. The runtime interner replays
    //    exactly the chain the parser walked, and dedups on identical text. A
    //    `TagId` that is genuinely NEW is therefore text no rule or set names,
    //    and having no entry here is the same answer a fresh `reindex` would give.
    //
    // 2. The one way a runtime hash can diverge from a grammar tag of the same
    //    text is the type bits `Tag::rehash` folds in — and all eight of them are
    //    members of `MASK_TAG_SPECIAL`, so such a tag is `T_SPECIAL`, its set is
    //    `ST_SPECIAL`, and `index_sets` files it under `tag_any` without
    //    descending. It is reached through `sets_any`, never through a per-tag
    //    key here. `tag_interning_closure` pins that coupling.
    //
    // Break either leg — give `destroy_tag` a caller, or add a hash-contributing
    // bit outside `MASK_TAG_SPECIAL` — and this becomes a silent false negative.
    pub rules_by_set: RulesBySet,
    pub rules_by_tag: RulesByTag,
    pub sets_by_tag: SetsByTag,

    /// C++ `uint32IntervalVector* rules_any` — cached `rules_by_tag[tag_any]`.
    /// Snapshot of a `tag_any` entry, which only `reindex` writes, so runtime
    /// interning cannot stale it. Read by nothing in the matcher — the stats
    /// lines in `cg-comp` / `vislcg3` are its only consumers, as in the C++.
    pub rules_any: Option<Uint32IntervalVector>,
    /// C++ `boost::dynamic_bitset<>* sets_any` — cached `sets_by_tag[tag_any]`.
    /// Same snapshot argument as `rules_any`; additionally every read goes
    /// through `insert_if_exists`, which only ever GROWS the destination, so a
    /// snapshot taken at reindex cannot under-set bits.
    pub sets_any: Option<DynBitset>,

    // --- delimiter sets (nullable `Set*`) ---
    pub delimiters: Option<SetId>,
    pub soft_delimiters: Option<SetId>,
    pub text_delimiters: Option<SetId>,

    pub tag_any: u32,
    /// C++ `uint32Vector preferred_targets` (tag hashes).
    pub preferred_targets: Uint32Vector,
    /// C++ `uint32SortedVector reopen_mappings`.
    pub reopen_mappings: Uint32SortedVector,
    pub parentheses: Parentheses,
    pub parentheses_reverse: Parentheses,

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

impl Default for GrammarCore {
    /// Faithful analog of the C++ `Grammar() = default;`: every member takes its
    /// zero/empty value except `mapping_prefix`, whose C++ member initializer is
    /// `'@'`.
    fn default() -> Self {
        GrammarCore {
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
            source_names: Vec::new(),
            binary_path: None,
            single_tags_list: Arena::new(),
            tags_by_hash: FlatUnorderedMap::default(),
            sets_list: Arena::new(),
            sets_list_order: Vec::new(),
            sets_all: SortedVector::new(),
            sets_by_name: Uint32FlatHashMap::default(),
            set_name_seeds: SetNameSeeds::default(),
            sets_by_contents: BTreeMap::default(),
            set_alias: Uint32FlatHashMap::default(),
            maybe_used_sets: SortedVector::new(),
            static_sets: StaticSets::default(),
            regex_tags: RegexTags::default(),
            icase_tags: SortedVector::new(),
            contexts_arena: Arena::new(),
            templates: Contexts::default(),
            contexts: Contexts::default(),
            rules_by_set: RulesBySet::default(),
            rules_by_tag: RulesByTag::default(),
            sets_by_tag: SetsByTag::default(),
            rules_any: None,
            sets_any: None,
            delimiters: None,
            soft_delimiters: None,
            text_delimiters: None,
            tag_any: 0,
            preferred_targets: Uint32Vector::default(),
            reopen_mappings: Uint32SortedVector::new(),
            parentheses: Parentheses::default(),
            parentheses_reverse: Parentheses::default(),
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

mod overlay;
mod tag_space;

pub use overlay::{Grammar, TagHashRef, TagIndex, TagStore};
pub use tag_space::TagSpace;

impl GrammarCore {
    /// C++ `Grammar::single_tags` over a grammar still being built: its own
    /// hash index, with no run half.
    #[inline]
    pub fn single_tags(&self) -> TagIndex<'_> {
        TagIndex {
            core: &self.tags_by_hash,
            run: None,
        }
    }
}

/// A grammar being built interns into itself.
impl TagSpace for GrammarCore {
    #[inline]
    fn tag(&self, id: TagId) -> &Tag {
        &self.single_tags_list[id.0]
    }

    #[inline]
    fn tag_at_hash(&self, hash: u32) -> Option<TagId> {
        self.single_tags().find(hash).tag()
    }

    fn regex_tags(&self) -> &RegexTags {
        &self.regex_tags
    }

    fn icase_tags(&self) -> &IcaseTags {
        &self.icase_tags
    }

    fn insert_tag(&mut self, tag: Tag, hash: u32) -> TagId {
        let idx = self.single_tags_list.alloc(tag);
        self.single_tags_list.get_mut(idx).number = idx;
        let id = TagId(idx);
        self.tags_by_hash.insert((hash, id));
        id
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
impl Drop for GrammarCore {
    fn drop(&mut self) {}
}

impl GrammarCore {
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
            let mut nhash = hash_value_str(&name, 0);
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
    ///
    /// The C++ recurses into the components; the sets still being numbered are
    /// kept on a heap stack instead, so a set built from sets however deep
    /// costs no stack, and each is numbered once its components are, as in
    /// the recursion.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn add_set_to_list(&mut self, s: SetId) {
        if !self.set_unlisted(s) {
            return;
        }
        let mut open = vec![(s, 0usize)];
        while let Some((set, next)) = open.last_mut() {
            let set = *set;
            if let Some(&sit) = self.sets_list[set.0].sets.get(*next) {
                *next += 1;
                // C++ addSetToList(getSet(sit)); getSet null → deref crash.
                #[expect(
                    clippy::unwrap_used,
                    reason = "until set_adjust_sets numbers them, a set's members are the hashes of sets add_set registered in sets_by_contents, which only reindex's step (16) empties, after this runs in its step (10)"
                )]
                let child = self.get_set(sit).unwrap();
                if self.set_unlisted(child) {
                    open.push((child, 0));
                }
                continue;
            }
            open.pop();
            self.sets_list_order.push(set);
            self.sets_list[set.0].number = SetNumber(ui32(self.sets_list_order.len() - 1));
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
        #[expect(
            clippy::expect_used,
            reason = "allocate_tag refuses only an empty tag, one starting with `(`, or a reserved dependency or relation number (ReservedNumber::check), and the STR_DUMMY literal is none of these"
        )]
        let t = self
            .allocate_tag(STR_DUMMY)
            .expect("the dummy set's tag is a literal and cannot fail");
        self.add_tag_to_set(t, set_c);
        #[expect(
            clippy::expect_used,
            reason = "add_set refuses a name or content that another set already has, and both callers (parse_grammar_data, conv_grammar) make the dummy first, before any other set"
        )]
        let set_c = self
            .add_set(set_c)
            .expect("the dummy set is freshly built and cannot collide");
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

    /// A grammar-construction failure at the current source line. These are
    /// failures in what the grammar SAYS, so they are parse errors even though
    /// they surface here rather than in the parser.
    ///
    /// The grammar has a line but no cursor, so it cannot place the failure in
    /// the source; the parser's directive loop fills the file and span in on the
    /// way past — `[spec:cg3:req:diagnostics.span]`.
    pub(crate) fn error(&self, kind: crate::error::ParseErrorKind) -> crate::error::ParseError {
        crate::error::ParseError {
            file: String::new(),
            line: self.lines,
            near: String::new(),
            span: None,
            kind,
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.allocate-tag-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.allocate-tag-fn]
    /// Interns a tag from raw text. Empty / leading-`(` texts are hard errors
    /// (`CG3Quit(1)`; the stderr diagnostic is deferred I/O); anything
    /// else goes to [`intern_text`](Self::intern_text), which refuses a
    /// dependency or relation number the hash tables reserve.
    pub fn allocate_tag(&mut self, txt: &str) -> Result<TagId, crate::error::ParseError> {
        let first = txt.chars().next().unwrap_or('\0');
        if first == '\0' {
            return Err(self.error(crate::error::ParseErrorKind::EmptyTag));
        }
        if first == '(' {
            return Err(
                self.error(crate::error::ParseErrorKind::TagStartsWithParen {
                    tag: txt.to_string(),
                }),
            );
        }
        self.intern_text(txt)
            .map_err(|cause| self.error(crate::error::ParseErrorKind::ReservedNumber { cause }))
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
        t.map(|t| self.intern_contextual_test(t))
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-contextual-test-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-contextual-test-fn]
    /// [`Self::add_contextual_test`] of a test, not a null one: returns `t`
    /// itself, or the equal test already in `contexts`.
    pub fn intern_contextual_test(&mut self, t: CtxId) -> CtxId {
        ContextualTest::rehash(&mut self.contexts_arena, t);

        // t->linked = addContextualTest(t->linked)
        if let Some(linked) = self.contexts_arena[t.0].linked {
            let new_linked = self.intern_contextual_test(linked);
            self.contexts_arena[t.0].linked = Some(new_linked);
        }

        // for (auto& it : t->ors) it = addContextualTest(it)
        let ors = self.contexts_arena[t.0].ors.clone();
        let mut new_ors: Vec<CtxId> = Vec::with_capacity(ors.len());
        for it in ors {
            new_ors.push(self.intern_contextual_test(it));
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
        result
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-template-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-template-fn]
    /// Registers a named template. Keyed purely by `hash_value(name)` with no
    /// seed/collision handling (quirk: a genuine name-hash collision misreports as
    /// a redefinition). Redefinition → `CG3Quit(1)` (diagnostic deferred).
    pub fn add_template(
        &mut self,
        test: CtxId,
        name: &str,
    ) -> Result<(), crate::error::ParseError> {
        let cn = hash_value_str(name, 0);
        if self.templates.contains_key(&cn) {
            return Err(self.error(crate::error::ParseErrorKind::TemplateRedefined {
                name: name.to_string(),
            }));
        }
        self.templates.insert(cn, test);
        Ok(())
    }

    // [spec:cg3:def:grammar.cg3.grammar.add-anchor-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-anchor-fn]
    /// Registers a named section anchor. `primary` re-definition of an existing
    /// anchor → `CG3Quit(1)`. `at > rule_by_number.size()` (strict `>`) clamps to
    /// the size. Stores only when the anchor did NOT already exist (non-primary
    /// re-adds silently keep the old position — quirk reproduced).
    pub fn add_anchor(
        &mut self,
        to: &str,
        mut at: u32,
        primary: bool,
    ) -> Result<(), crate::error::ParseError> {
        let ah = {
            let tid = self.allocate_tag(to)?;
            self.single_tags_list[tid.0].hash
        };
        let exists = self.anchors.contains(ah.get());
        if primary && exists {
            return Err(self.error(crate::error::ParseErrorKind::AnchorRedefined {
                name: to.to_string(),
            }));
        }
        if at > self.rule_by_number.capacity() {
            // "Warning: No corresponding rule available for anchor ...": deferred.
            at = ui32(self.rule_by_number.capacity());
        }
        if !exists {
            self.anchors.insert((ah.get(), at));
        }
        Ok(())
    }
}

impl GrammarCore {
    // [spec:cg3:def:grammar.cg3.grammar.add-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.add-set-fn]
    /// Registers a fully-built set, canonicalizing by content and by name, and
    /// returns the canonical `SetId` (the C++ `Set*& to` in/out reference — the
    /// caller reassigns). Steps: delimiter capture, SET→LIST folding, fail-fast
    /// splitting, name registration (the always-break-once loop), content
    /// registration. `getNonEmpty()` (`trie` if non-empty else `trie_special`) is
    /// inlined (not ported on `Set`).
    pub fn add_set(&mut self, mut to: SetId) -> Result<SetId, crate::error::ParseError> {
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
            let mut members: Vec<SetId> = Vec::with_capacity(to_sets.len());
            for i in 0..to_sets.len() {
                if i > 0 && to_set_ops[i - 1] != S_OR {
                    all_tags = false;
                    break;
                }
                #[expect(
                    clippy::unwrap_used,
                    reason = "until set_adjust_sets numbers them, a set's members are the hashes of sets add_set registered in sets_by_contents, which only reindex's step (16) empties"
                )]
                let s = self.get_set(to_sets[i]).unwrap();
                members.push(s);
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
                for s in members {
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
            let positive = self.add_set(positive)?;
            let negative = self.add_set(negative)?;
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
            let mut nhash = hash_value_str(&name, 0);
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
                            return Err(self.error(crate::error::ParseErrorKind::SetRedefined {
                                name: name.to_string(),
                            }));
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
        if let std::collections::btree_map::Entry::Vacant(e) = self.sets_by_contents.entry(chash) {
            e.insert(to);
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
                    return Err(self.error(crate::error::ParseErrorKind::SetContentCollision));
                }
                self.destroy_set(to);
            }
        }
        to = self.sets_by_contents[&chash];
        Ok(to)
    }

    // [spec:cg3:def:grammar.cg3.grammar.append-to-set-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.append-to-set-fn]
    /// Implements LIST `+=`: extends the already-defined set named `to->name` with
    /// the new content in `to`, returning the merged canonical `SetId`. Precondition
    /// (quirk): `undefSet` must find that set — a `None` would crash in `addSet`
    /// (`to->name` deref); reproduced via `unwrap`. Delimiter capture at the end
    /// overwrites unconditionally (unlike `addSet`).
    pub fn append_to_set(&mut self, mut to: SetId) -> Result<SetId, crate::error::ParseError> {
        let to_name = self.sets_list[to.0].name.clone();
        let to_line = self.sets_list[to.0].line;

        // (1) Pull the currently-registered set out of the name index.
        #[expect(
            clippy::unwrap_used,
            reason = "parse_list, the only caller, appends only once get_set has found a set by this name, and undef_set's last lookup is that one"
        )]
        let tset = self.undef_set(&to_name).unwrap();
        // (2) Re-register it under fresh keys.
        let tset = self.add_set(tset)?;

        if !self.sets_list[tset.0].sets.is_empty() {
            let first_hash = self.sets_list[tset.0].sets[0];
            #[expect(
                clippy::unwrap_used,
                reason = "until set_adjust_sets numbers them, a set's members are the hashes of sets add_set registered in sets_by_contents, which only reindex's step (16) empties"
            )]
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
                to = self.add_set(to)?;

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
                let ptrie = self.sets_list[fset.0].trie.clone();
                let tvs = trie_get_tags(&ptrie, self);
                for tv in &tvs {
                    trie_insert(&mut self.sets_list.get_mut(to.0).trie, tv, 0);
                }
                let ptrie_sp = self.sets_list[fset.0].trie_special.clone();
                let tvs = trie_get_tags(&ptrie_sp, self);
                for tv in &tvs {
                    trie_insert(&mut self.sets_list.get_mut(to.0).trie_special, tv, 0);
                }

                let second_hash = self.sets_list[tset.0].sets[1];
                #[expect(
                    clippy::unwrap_used,
                    reason = "until set_adjust_sets numbers them, a set's members are the hashes of sets add_set registered in sets_by_contents, which only reindex's step (16) empties"
                )]
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
        to = self.add_set(to)?;

        // (6) Delimiter capture (unconditional overwrite).
        let final_name = self.sets_list[to.0].name.clone();
        if final_name == STR_DELIMITSET {
            self.delimiters = Some(to);
        } else if final_name == STR_SOFTDELIMITSET {
            self.soft_delimiters = Some(to);
        } else if final_name == STR_TEXTDELIMITSET {
            self.text_delimiters = Some(to);
        }
        Ok(to)
    }

    // [spec:cg3:def:grammar.cg3.grammar.get-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.get-tags-fn]
    /// Collects the set's tag combinations into `rv`. Recurses over component sets
    /// (resolved by content hash via `getSet`; other operators than OR are ignored,
    /// per the source ToDo) then appends this set's own trie paths via the shared
    /// `tv` buffer (the sort-then-pop quirk lives in `trie_get_tags_into`).
    ///
    /// The C++ recurses into the component sets; the sets whose components
    /// are still being collected are kept on a heap stack instead, and each
    /// set's own paths are collected once its components' are, as in the
    /// recursion.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn get_tags(&self, set: SetId, rv: &mut TagVectorSet) {
        let mut open = vec![(set, 0usize)];
        while let Some(done) = self.next_set_done(&mut open) {
            self.get_own_tags(done, rv);
        }
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
    ///
    /// The C++ recurses into the components; the sets still to visit are kept
    /// on a heap stack instead, taken in the recursion's order.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn get_tag_list_any(&self, set: SetId, the_tags: &mut TagList) {
        let mut todo = vec![set];
        while let Some(set) = todo.pop() {
            let ty = self.sets_list[set.0].r#type;
            let members = &self.sets_list[set.0].sets;
            if ty.intersects(ST_SET_UNIFY | ST_TAG_UNIFY) {
                the_tags.clear();
                // single_tags.find(tag_any)->second — null-deref crash if absent.
                let tid = {
                    let it = self.single_tags().find(self.tag_any);
                    it.get().1
                };
                the_tags.push(tid);
            } else if !members.is_empty() {
                // getTagList_Any(*sets_list[iter]) — `iter` is a set NUMBER.
                todo.extend(
                    members
                        .iter()
                        .rev()
                        .map(|&iter| self.set_id_by_number(SetNumber(iter))),
                );
            } else {
                let set = &self.sets_list[set.0];
                trie_get_tag_list_append(&set.trie, the_tags, self);
                trie_get_tag_list_append(&set.trie_special, the_tags, self);
            }
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.remove-numeric-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn]
    /// Returns the hash of a variant of set `s` with all `T_NUMERICAL` tags
    /// removed, building a new `_G_<name>_B_` set only when something was actually
    /// removed. Composite and leaf cases per the spec; `ntags` is a
    /// `BTreeMap<TagVector, bool>` (C++ `std::map<TagVector, bool>`).
    ///
    /// The C++ recurses into the component sets. The sets built from sets
    /// whose components are still being stripped are kept on a heap stack
    /// instead, and a component's result is taken into its set as soon as it
    /// is known, before the next component is begun, as in the recursion.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn remove_numeric_tags(&mut self, s: u32) -> Result<u32, crate::error::ParseError> {
        let mut open: Vec<walks::NumericStrip> = Vec::new();
        let mut stripped = self.strip_numeric_enter(&mut open, s)?;
        loop {
            if let Some(ns) = stripped.take() {
                let Some(parent) = open.last_mut() else {
                    return Ok(ns);
                };
                if ns == 0 {
                    return Err(self.error(crate::error::ParseErrorKind::EmptyNumericBranch));
                }
                let i = parent.next - 1;
                if ns != parent.sets[i] {
                    parent.sets[i] = ns;
                    parent.did = true;
                }
            }
            let Some(top) = open.last_mut() else {
                return Ok(0);
            };
            if let Some(&member) = top.sets.get(top.next) {
                top.next += 1;
                stripped = self.strip_numeric_enter(&mut open, member)?;
            } else if let Some(done) = open.pop() {
                stripped = Some(self.strip_numeric_composite(done)?);
            }
        }
    }
}

impl GrammarCore {
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
        let size = self.sets_list_order.len();
        let bs: &mut DynBitset = self
            .sets_by_tag
            .entry(t)
            .or_insert_with(|| vec![false; size]);
        bs[r as usize] = true;
    }

    // [spec:cg3:def:grammar.cg3.grammar.index-set-to-rule-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.index-set-to-rule-fn]
    /// Records which tags trigger rule number `r` through target set `s`. Special
    /// / tag-unify sets also index `tag_any`. Descends into BOTH tries and every
    /// child (unlike `indexSets`, it does NOT stop for special sets).
    ///
    /// The C++ recurses into the child sets; the sets still to index are kept
    /// on a heap stack instead, taken in the recursion's order.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn index_set_to_rule(&mut self, r: u32, s: SetId) {
        let mut todo = vec![s];
        while let Some(s) = todo.pop() {
            let ty = self.sets_list[s.0].r#type;
            if ty.intersects(ST_SPECIAL | ST_TAG_UNIFY) {
                let ta = self.tag_any;
                self.index_tag_to_rule(ta, r);
            }
            let trie = self.sets_list[s.0].trie.clone();
            let trie_special = self.sets_list[s.0].trie_special.clone();
            trie_index_to_rule(&trie, self, r);
            trie_index_to_rule(&trie_special, self, r);
            // indexSetToRule(r, sets_list[i]) — `i` is a set NUMBER.
            todo.extend(self.members_by_number(s));
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.index-sets-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.index-sets-fn]
    /// Maps each tag hash to the bitset of set numbers containing it. Special /
    /// tag-unify sets index `tag_any` and RETURN immediately (no trie/child
    /// descent — the key difference from `indexSetToRule`).
    ///
    /// The C++ recurses into the child sets; the sets still to index are kept
    /// on a heap stack instead, taken in the recursion's order.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn index_sets(&mut self, r: u32, s: SetId) {
        let mut todo = vec![s];
        while let Some(s) = todo.pop() {
            let ty = self.sets_list[s.0].r#type;
            if ty.intersects(ST_SPECIAL | ST_TAG_UNIFY) {
                let ta = self.tag_any;
                self.index_tag_to_set(ta, r);
                continue;
            }
            let trie = self.sets_list[s.0].trie.clone();
            let trie_special = self.sets_list[s.0].trie_special.clone();
            trie_index_to_set(&trie, self, r);
            trie_index_to_set(&trie_special, self, r);
            // indexSets(r, sets_list[i]) — `i` is a set NUMBER.
            todo.extend(self.members_by_number(s));
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.set-adjust-sets-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.set-adjust-sets-fn]
    /// Rewrites `s->sets` from content hashes to set numbers, recursively, once
    /// per set (`ST_USED` is the visited marker, cleared on entry). No presence
    /// check on the content-hash lookup (C++ UB → HashMap index panic).
    ///
    /// The C++ recurses into the child sets; the sets still to visit are kept
    /// on a heap stack instead.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn set_adjust_sets(&mut self, s: SetId) {
        let mut todo = vec![s];
        while let Some(s) = todo.pop() {
            let members = self.adjust_one_set(s);
            todo.extend(members);
        }
    }

    // [spec:cg3:def:grammar.cg3.grammar.context-adjust-target-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.context-adjust-target-fn]
    /// Rewrites a test's set references from content hash to set number,
    /// recursively, once per test (`is_used` doubles as the visited marker).
    ///
    /// The C++ recurses into the tests a test refers to, which a `.cg3b` can
    /// chain to any length; the tests still to visit are kept on a heap stack
    /// instead.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn context_adjust_target(&mut self, test: CtxId) {
        let mut todo = vec![test];
        while let Some(test) = todo.pop() {
            let next = self.adjust_one_context(test);
            todo.extend(next);
        }
    }

    // [spec:cg3:def:contextual-test.cg3.contextual-test.mark-used-fn]
    // [spec:cg3:sem:contextual-test.cg3.contextual-test.mark-used-fn]
    /// C++ `void ContextualTest::markUsed(Grammar&)`. INLINED HERE (not on
    /// `ContextualTest`): the sibling deferred it because it recurses into
    /// `Grammar::getSet(..)->markUsed(..)`. Ported faithfully as a private
    /// `Grammar` method; `is_used` guards against re-processing. `getSet` null →
    /// deref crash (reproduced via `unwrap`).
    ///
    /// The C++ recurses into the tests a test refers to, which a `.cg3b` can
    /// chain to any length; the tests still to visit are kept on a heap stack
    /// instead.
    // [spec:cg3:req:robustness.depth-bounded]
    fn context_mark_used(&mut self, test: CtxId) {
        let mut todo = vec![test];
        while let Some(test) = todo.pop() {
            let next = self.mark_one_context_used(test);
            todo.extend(next);
        }
    }
}

// [spec:cg3:def:grammar.cg3.trie-index-to-rule-fn]
// [spec:cg3:sem:grammar.cg3.trie-index-to-rule-fn]
/// Free fn. Maps every tag (at every depth, incl. non-terminals) to
/// rule number `r` via `grammar.indexTagToRule(tag->hash, r)`. The `trie` must be
/// an EXTERNAL copy (callers clone the set's trie out first) so it does not alias
/// the `&mut Grammar` borrow. Walks with a [`TrieWalk`] where the C++ recurses.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_index_to_rule(trie: &TagTrie, grammar: &mut GrammarCore, r: u32) {
    crate::tag_trie::TrieWalk::new(trie).each(|k, _| {
        let h = grammar.single_tags_list[k.0].hash;
        grammar.index_tag_to_rule(h.get(), r);
    });
}

// [spec:cg3:def:grammar.cg3.trie-index-to-set-fn]
// [spec:cg3:sem:grammar.cg3.trie-index-to-set-fn]
/// Free fn. Identical shape to `trie_index_to_rule` but sets bit `r` (a set
/// number) in `sets_by_tag[tag->hash]` for every tag in the trie.
// [spec:cg3:req:robustness.depth-bounded]
pub fn trie_index_to_set(trie: &TagTrie, grammar: &mut GrammarCore, r: u32) {
    crate::tag_trie::TrieWalk::new(trie).each(|k, _| {
        let h = grammar.single_tags_list[k.0].hash;
        grammar.index_tag_to_set(h.get(), r);
    });
}

/// The smallest trie entry in a `.cg3b`: a `u32` tag index, a `u8` terminal
/// flag and a `u32` child count.
pub(crate) const TRIE_ENTRY_SIZE: usize = 9;

// [spec:cg3:def:grammar.cg3.trie-unserialize-fn+1]
// [spec:cg3:sem:grammar.cg3.trie-unserialize-fn+1]
// [spec:cg3:req:robustness.binary-grammar-validated]
/// Free fn. Deserializes a tag-trie from a binary-grammar stream (mirrors
/// `trie_serialize`). Per entry: BE `u32` tag index → key `TagId(index)` (the
/// arena index IS the tag number; C++ dereferenced `single_tags_list[index]` to
/// obtain the `Tag*` used as the flat_map key), BE `u8` terminal flag, BE `u32`
/// child count, then that many entries of the child level.
///
/// DIVERGENCE: the tag index is checked against `tag_limit` (the C++ indexes
/// past the list), a short read is an error, and a child level is read by
/// setting the parent aside on a stack rather than by recursing, so a trie as
/// deep as the file allows cannot exhaust the call stack.
pub(crate) fn trie_unserialize(
    trie: &mut TagTrie,
    input: &mut crate::binary_grammar::Cg3bCursor<'_>,
    num_tags: u32,
    tag_limit: u32,
) -> Result<(), crate::error::GrammarError> {
    *trie = read_trie_levels(std::mem::take(trie), input, num_tags, tag_limit)?;
    Ok(())
}

/// The loop behind [`trie_unserialize`]: reads `num_tags` entries into
/// `level`, setting a level aside whenever an entry opens a child level and
/// hanging the child back under its key once the child's entries are read.
fn read_trie_levels(
    mut level: TagTrie,
    input: &mut crate::binary_grammar::Cg3bCursor<'_>,
    num_tags: u32,
    tag_limit: u32,
) -> Result<TagTrie, crate::error::GrammarError> {
    // Each level set aside: its trie, the key its open child hangs from, and
    // how many of its own entries are still to read.
    let mut parents: Vec<(TagTrie, TagId, u32)> = Vec::new();
    let mut left = num_tags;
    loop {
        if left == 0 {
            let Some((parent, key, parent_left)) = parents.pop() else {
                return Ok(level);
            };
            let child = std::mem::replace(&mut level, parent);
            level.entry(key).or_default().trie = Some(Box::new(child));
            left = parent_left;
            continue;
        }
        left -= 1;
        let tag = TagId(input.index("trie tag", tag_limit)?);
        let terminal: u8 = input.be("trie terminal flag")?;
        let child_count = input.count("trie child count", TRIE_ENTRY_SIZE)?;
        let node = level.entry(tag).or_default();
        node.terminal = terminal != 0;
        if child_count != 0 {
            let child = node.trie.take().map(|b| *b).unwrap_or_default();
            parents.push((std::mem::replace(&mut level, child), tag, left));
            left = child_count;
        }
    }
}
