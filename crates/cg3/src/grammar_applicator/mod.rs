//! Port of the C++ `GrammarApplicator` class (`src/GrammarApplicator.hpp` + its
//! six `.cpp` partials) — the engine that applies a loaded [`Grammar`] to a
//! stream of cohorts.
//!
//! This module is the STRUCT + SUBMODULE SCAFFOLD only: the [`GrammarApplicator`]
//! struct, its member typedefs / nested types, and a minimal `new()` that
//! default-initialises every field. The real method bodies land next, split
//! across the submodules mirroring the C++ partials:
//!
//! | Rust submodule          | C++ partial                             |
//! |-------------------------|-----------------------------------------|
//! | [`core`]                | `GrammarApplicator.cpp`                 |
//! | [`run_rules`]           | `GrammarApplicator_runRules.cpp`        |
//! | [`run_grammar`]         | `GrammarApplicator_runGrammar.cpp`      |
//! | [`run_contextual_test`] | `GrammarApplicator_runContextualTest.cpp` |
//! | [`match_set`]           | `GrammarApplicator_matchSet.cpp`        |
//! | [`reflow`]              | `GrammarApplicator_reflow.cpp`          |
//! | [`context`]             | `GrammarApplicator_context.cpp`         |
//!
//! ARENA MODEL. C++ raw pointers become arena ids: `Tag*`→[`TagId`],
//! `Set*`→[`SetId`], `Rule*`→[`RuleId`], `ContextualTest*`→[`CtxId`],
//! `Cohort*`→[`CohortId`], `Reading*`→[`ReadingId`], `SingleWindow*`→[`SwId`];
//! nullable pointers become `Option<…Id>`. The applicator OWNS the runtime
//! object arenas via [`store`](crate::store::RuntimeStore) (replacing CG-3's
//! global object pools) and OWNS the loaded [`Grammar`] (C++ `const Grammar*`).
//! The `gWindow` document window (C++ `std::unique_ptr<Window>`) is held
//! inline. Pointer-into-local-buffer optimisations (`bc::flat_map<…, T*>` keyed
//! into the `*_store` vectors, `std::vector<CohortSet*>`, `std::vector<size_t*>`)
//! stay raw pointers, matching the C++ 1:1 (as [`crate::scoped_stack`] already
//! does).

use std::collections::{BTreeMap, HashSet};

use crate::arena::{CohortId, CtxId, GenArena, ReadingId, RuleId, SetId, SwId, TagId};
use crate::cohort_iterator::{
    CohortIterator, DepAncestorIter, DepDescendentIter, DepParentIter, TopologyLeftIter,
    TopologyRightIter,
};
use crate::flat_unordered_map::Uint32FlatHashMap;
use crate::flat_unordered_set::{Uint32FlatHashSet, Uint64FlatHashSet};
use crate::interval_vector::Uint32IntervalVector;
use crate::process::Process;
use crate::scoped_stack::ScopedStack;
use crate::sorted_vector::{SortedVector, Uint32SortedVector};
use crate::tag::TagList;
use crate::types::{TagHash, UChar, UString, Uint32Vector};

pub mod context;
pub mod core;
pub mod match_set;
pub mod reflow;
pub mod run_contextual_test;
pub mod run_grammar;
pub mod run_rules;
pub mod stream_format;

/// C++ `cg3.h` `enum cg3_sformat` — the stream serialisation format tag used
/// by `fmt_input` / `fmt_output`; the variants camel-case the C++ `CG3SF_*`
/// enumerators (`CG3SF_INVALID` → `Invalid`, `CG3SF_CG` → `Cg`, ...). The
/// public C-API header (`cg3.h`) is not yet ported to Rust, so the enum is
/// defined here (in the engine skeleton) where it is first needed; a later
/// pass may relocate it to a `cg3` C-API module.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u32)]
pub enum StreamFormatKind {
    Invalid = 0,
    #[default]
    Cg = 1,
    Niceline = 2,
    Apertium = 3,
    Matxin = 4,
    Fst = 5,
    Plain = 6,
    Jsonl = 7,
    Binary = 8,
}

// [spec:cg3:def:grammar-applicator.cg3.regexgrps-t]
/// C++ `typedef std::vector<UnicodeString> regexgrps_t` — the captured regex
/// groups for one context frame (`UnicodeString` → UTF-8 [`UString`]).
pub type RegexGroups = Vec<UString>;

// [spec:cg3:def:grammar-applicator.cg3.unif-key]
/// Semantic identity of a unified trie node, replacing the C++ `const void*`
/// (`&kv`, the address of a `trie_t` entry).
///
/// C++ `check_unif_tags(set, &kv)` records the ADDRESS of a `(Tag*, trie_node_t)`
/// entry — of the terminal node reached by `doesSetMatchReading_{trie,tags}` at
/// some depth — and later compares those addresses for identity; run_rules'
/// `getTagList(..., node)` walks the same tries by that address to rebuild the
/// root-to-node tag path. Within one set an entry address is in bijection with
/// `(which trie, root-to-node path of TagIds)`: `Set::trie`/`Set::trie_special`
/// are `BTreeMap<TagId, trie_node_t>`, so a path of `TagId`s names exactly one
/// node, and the two tries are disjoint (`special` disambiguates). Keying off the
/// path instead of the address is therefore an exact, address-free equivalent —
/// same first-sight-wins recording, same identity comparisons, same `getTagList`
/// output order (the successful DFS branch pushes exactly this path).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnifKey {
    /// Which of the set's two tries the node lives in (`false` = `trie`,
    /// `true` = `trie_special`) — the C++ address is globally unique across both,
    /// so `getTagList` tries `trie` then `trie_special` on the same pointer.
    pub special: bool,
    /// Root-to-node `TagId` path (the sequence of `trie` keys descended to reach
    /// the recorded terminal node). Uniquely identifies the node within its trie.
    pub path: Vec<TagId>,
}

// [spec:cg3:def:grammar-applicator.cg3.unif-tags-t]
/// C++ `typedef bc::flat_map<uint32_t, const void*> unif_tags_t`. The `const void*`
/// value (a `trie_t` entry address) becomes the address-free [`UnifKey`] (see its
/// docs for the address↔key bijection); comparison is still pure identity, now
/// value equality of `(special, path)`.
pub type UnifTags = BTreeMap<u32, UnifKey>;
// [spec:cg3:def:grammar-applicator.cg3.unif-sets-t]
/// C++ `typedef bc::flat_map<uint32_t, uint32SortedVector> unif_sets_t`.
pub type UnifSets = BTreeMap<u32, Uint32SortedVector>;

// [spec:cg3:def:grammar-applicator.cg3.tmpl-context-t]
/// C++ `struct tmpl_context_t` — the active template-test window (`min`/`max`
/// bounds), the stack of `linked` tests, and the `in_template` flag. The
/// `clear()` member (`tmpl-context-t.clear-fn`) is a method left for the impl
/// pass.
#[derive(Default, Clone)]
pub struct TmplContext {
    pub min: Option<CohortId>,
    pub max: Option<CohortId>,
    /// C++ `std::vector<const ContextualTest*> linked`.
    pub linked: Vec<CtxId>,
    pub in_template: bool,
}

/// Wave-4 descriptor for a per-window cohort set (the C++ `CohortSet*` into
/// `SingleWindow::rule_to_cohorts` / `nested_rule_to_cohorts`). Resolved
/// against the store at every access via `cs_ref`/`cs_mut`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CsRef {
    /// `&current.rule_to_cohorts[rule]`.
    Window { sw: crate::arena::SwId, rule: u32 },
    /// `&*current.nested_rule_to_cohorts`.
    Nested { sw: crate::arena::SwId },
}

// [spec:cg3:def:grammar-applicator.cg3.d-smc-context]
/// C++ `struct dSMC_Context` — the mutable context threaded through the
/// `doesSetMatchCohort*` matchers (the pending test, the `deep`/`origin`
/// out-targets, the option bitmask, and the match/barrier flags). `Cohort**
/// deep` (a pointer to the caller's `Cohort*` slot) → a safe reborrowable
/// `Option<&mut Option<CohortId>>` (wave 4; was a raw `*mut`).
pub struct CohortMatchContext<'a> {
    /// C++ `const ContextualTest* test`.
    pub test: Option<CtxId>,
    /// C++ `Cohort** deep`.
    pub deep: Option<&'a mut Option<CohortId>>,
    /// C++ `Cohort* origin`.
    pub origin: Option<CohortId>,
    pub options: crate::contextual_test::PosFlags,
    pub did_test: bool,
    pub matched_target: bool,
    pub matched_tests: bool,
    pub in_barrier: bool,
}

// [spec:cg3:def:grammar-applicator.cg3.reading-spec]
/// C++ `struct ReadingSpec` — a (cohort, reading, sub-reading) triple naming a
/// concrete match location. All three pointers are nullable.
#[derive(Default, Clone)]
pub struct ReadingSpec {
    pub cohort: Option<CohortId>,
    pub reading: Option<ReadingId>,
    pub subreading: Option<ReadingId>,
}

// [spec:cg3:def:grammar-applicator.cg3.rule-context]
/// C++ `struct Rule_Context` — one frame of the applicator's `context_stack`:
/// the matched `target`, the accumulated `context`/`dep_context` cohort
/// positions, the `attach_to` target, the `mark` cohort, and the per-frame
/// unification/regex-capture state. The C++ `unif_tags`/`unif_sets`/
/// `regexgrps` pointers alias into the applicator's `*_store` vectors; wave 4
/// replaces the raw aliasing pointers with plain store INDICES (safe across
/// `Vec` reallocation).
#[derive(Default, Clone)]
pub struct RuleContext {
    pub target: ReadingSpec,
    /// C++ `std::vector<Cohort*> context` — positions may be null.
    pub context: Vec<Option<CohortId>>,
    /// C++ `std::vector<Cohort*> dep_context`.
    pub dep_context: Vec<Option<CohortId>>,
    pub attach_to: ReadingSpec,
    pub mark: Option<CohortId>,
    /// C++ `unif_tags_t* unif_tags` — an index into `unif_tags_store`.
    pub unif_tags: Option<usize>,
    /// C++ `unif_sets_t* unif_sets` — an index into `unif_sets_store`.
    pub unif_sets: Option<usize>,
    pub regexgrp_ct: u8,
    /// C++ `regexgrps_t* regexgrps` — an index into `regexgrps_store`.
    pub regexgrps: Option<usize>,
    pub is_with: bool,
}

// C++ `typedef std::function<void(void)> RuleCallback` (spec
// `grammar-applicator.cg3.rule-callback`) — the reading/cohort callbacks handed
// to `runSingleRule`. DISSOLVED in the port: the only two callbacks ever
// constructed were `reading_cb_dispatch`/`cohort_cb_dispatch` closing over
// `this` + the shared `RRState`; `run_single_rule` now takes `&mut RRState` and
// calls the dispatch methods directly, so the type-erased closures (and their
// raw-pointer trampolines) are gone. No port symbol remains, so the C++
// typedef's `[spec:cg3:def:...]` is intentionally left unmapped.

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.all-mappings-t]
/// C++ `typedef std::map<Reading*, TagList> all_mappings_t`.
pub type AllMappings = BTreeMap<ReadingId, TagList>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.rs-type]
/// C++ `typedef std::map<int32_t, uint32IntervalVector> RSType` — the
/// per-section rule schedule (negative keys are the before/after/null sections).
pub type RSType = BTreeMap<i32, Uint32IntervalVector>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.externals-t]
/// C++ `typedef std::map<uint32_t, Process> externals_t` — the running
/// EXTERNAL child processes, keyed by tag hash.
pub type Externals = BTreeMap<u32, Process>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.readings-plain-t]
/// C++ `typedef bc::flat_map<uint32_t, Reading*> readings_plain_t`.
pub type ReadingsPlain = BTreeMap<u32, ReadingId>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.st-retvals]
// C++ `enum ST_RETVALS { … }` — bit flags OR-ed into the `uint8_t& rvs`
// out-param of `runSingleTest`; an enum whose values combine is modelled as
// `u8` bit constants rather than a Rust enum.
pub const TRV_BREAK: u8 = 1 << 0;
pub const TRV_BARRIER: u8 = 1 << 1;
pub const TRV_BREAK_DEFAULT: u8 = 1 << 2;

// Port-infra: the `scoped_stack<C>` fields require `C: Poolable` to construct
// (the proxy `clear()`s its slot on release). These concrete element types are
// only ever pooled from this engine, so their `clear` impls live here.
impl crate::pool::Poolable for TagList {
    fn clear(&mut self) {
        Vec::clear(self);
    }
}
impl crate::pool::Poolable for UnifTags {
    fn clear(&mut self) {
        BTreeMap::clear(self);
    }
}
impl crate::pool::Poolable for UnifSets {
    fn clear(&mut self) {
        BTreeMap::clear(self);
    }
}
impl crate::pool::Poolable for Uint32SortedVector {
    fn clear(&mut self) {
        Uint32SortedVector::clear(self);
    }
}

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator+4]
/// The options-derived, setup-written, run-read-only configuration extracted
/// from the C++ `GrammarApplicator` members. This is a Stage-B re-homing: it has
/// no C++ analog as a type (the C++ class is a single flat god object); the
/// members map 1:1 onto the cfg-bucket fields of `GrammarApplicator`, keeping
/// their names, types, and per-field C++ reference comments. They are populated
/// during setup (`new` / `set_grammar` / `set_options` / `index` / CLI wiring /
/// the format-applicator constructors) and read-only during the run.
pub struct EngineConfig {
    pub always_span: bool,
    pub apply_mappings: bool,
    pub apply_corrections: bool,
    pub no_before_sections: bool,
    pub no_sections: bool,
    pub no_after_sections: bool,
    pub trace: bool,
    pub trace_name_only: bool,
    pub trace_no_removed: bool,
    pub trace_encl: bool,
    pub allow_magic_readings: bool,
    pub no_pass_origin: bool,
    /// C++ `bool unsafe` (`unsafe` is a Rust keyword → raw identifier).
    pub r#unsafe: bool,
    pub ordered: bool,
    pub show_end_tags: bool,
    pub unicode_tags: bool,
    pub unique_tags: bool,
    pub is_conv: bool,
    pub split_mappings: bool,
    pub pipe_deleted: bool,
    pub add_spacing: bool,
    pub print_ids: bool,

    pub fmt_input: StreamFormatKind,
    pub fmt_output: StreamFormatKind,

    pub dep_delimit: u32,
    pub dep_absolute: bool,
    pub dep_original: bool,
    pub dep_block_loops: bool,
    pub dep_block_crossing: bool,

    pub num_windows: u32,
    pub soft_limit: u32,
    pub hard_limit: u32,
    pub sections: Uint32Vector,
    pub valid_rules: Uint32IntervalVector,
    pub trace_rules: Uint32IntervalVector,
    pub debug_rules: Uint32IntervalVector,
    pub verbosity_level: u32,
    pub section_max_count: u32,

    pub parse_dep: bool,

    pub span_pattern_latin: UString,
    pub span_pattern_utf: UString,
    /// C++ `UChar ws[4]{ ' ', '\t', 0, 0 }` — the whitespace set.
    pub ws: [UChar; 4],

    pub did_index: bool,

    pub numsections: u32,
    pub runsections: RSType,

    pub begintag: TagHash,
    pub endtag: TagHash,
    pub substtag: TagHash,
    pub tag_begin: Option<TagId>,
    pub mprefix_key: TagHash,
    pub mprefix_value: TagHash,

    /// C++ `std::vector<URegularExpression*> text_delimiters` — owned compiled
    /// regexes (ICU `URegularExpression*` → `fancy_regex::Regex`, compiled
    /// through `crate::tag_regex`).
    pub text_delimiters: Vec<crate::tag_regex::TagRegex>,

    // [spec:cg3:req:diagnostics.runtime-input-named]
    /// ADDED — no C++ analog. What to call the input stream in a runtime
    /// diagnostic. The engine reads its input through a handle and never knew
    /// what was behind it, so a failure while reading could only report a line
    /// COUNT; the CLI opened the stream and is the only thing that knows.
    /// Defaults to [`STDIN_SOURCE_NAME`], which is the truth for a stream with
    /// no file behind it.
    pub input_name: String,
}

/// What a runtime diagnostic calls an input stream with no file behind it.
/// The counterpart of the parser's `<utf8-memory>`.
pub const STDIN_SOURCE_NAME: &str = "<stdin>";

impl EngineConfig {
    /// Every field at its C++ default-member-initialiser value (the initialisers
    /// moved verbatim out of the former `GrammarApplicator::new`).
    pub fn new() -> Self {
        EngineConfig {
            always_span: false,
            apply_mappings: true,
            apply_corrections: true,
            no_before_sections: false,
            no_sections: false,
            no_after_sections: false,
            trace: false,
            trace_name_only: false,
            trace_no_removed: false,
            trace_encl: false,
            allow_magic_readings: true,
            no_pass_origin: false,
            r#unsafe: false,
            ordered: false,
            show_end_tags: false,
            unicode_tags: false,
            unique_tags: false,
            is_conv: false,
            split_mappings: false,
            pipe_deleted: false,
            add_spacing: true,
            print_ids: false,

            fmt_input: StreamFormatKind::Cg,
            fmt_output: StreamFormatKind::Cg,

            dep_delimit: 0,
            dep_absolute: false,
            dep_original: false,
            dep_block_loops: true,
            dep_block_crossing: false,

            num_windows: 2,
            soft_limit: 300,
            hard_limit: 500,
            sections: Default::default(),
            valid_rules: Default::default(),
            trace_rules: Default::default(),
            debug_rules: Default::default(),
            verbosity_level: 0,
            section_max_count: 0,

            parse_dep: false,

            span_pattern_latin: Default::default(),
            span_pattern_utf: Default::default(),
            ws: [' ', '\t', '\0', '\0'],

            did_index: false,

            numsections: 0,
            runsections: Default::default(),

            begintag: TagHash(0),
            endtag: TagHash(0),
            substtag: TagHash(0),
            tag_begin: None,
            mprefix_key: TagHash(0),
            mprefix_value: TagHash(0),

            text_delimiters: Default::default(),
            input_name: STDIN_SOURCE_NAME.to_string(),
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::new()
    }
}

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator+4]
/// The run-mutable, document-lifetime state extracted from the C++
/// `GrammarApplicator` members (the `doc` bucket of the field triage). This is a
/// Stage-B re-homing: it has no C++ analog as a type; the members map 1:1 onto
/// the doc-bucket fields of `GrammarApplicator`, keeping their names, types, and
/// per-field C++ reference comments. Lifetime is the whole stream / window set:
/// the runtime arenas, the [`WindowStream`](crate::window::WindowStream)
/// (C++ `gWindow`) dissolved into stream / cohort-registry / dependency
/// bookkeeping views, the run-phase latches, and the per-run counters.
pub struct Document {
    /// The runtime object arenas (pooled `Cohort`/`Reading`/`SingleWindow`).
    ///
    /// Every ported engine method resolves runtime objects through here
    /// (`self.doc.store.cohorts` / `.readings` / `.single_windows`), and
    /// `WindowStream`/`SingleWindow`/`Cohort` free fns are threaded
    /// `&mut self.doc.store`.
    pub store: crate::store::RuntimeStore,
    /// C++ `std::unique_ptr<Window> gWindow` — the ordered document stream
    /// (history / active / pending single-windows). Stream half of the dissolved
    /// C++ `Window`.
    pub stream: crate::window::WindowStream,
    /// The global cohort numbering + `cohort_map` (cohort-registry half of the
    /// dissolved C++ `Window`).
    pub cohorts: crate::window::CohortRegistry,
    /// The dependency / relation bookkeeping (dep-map / dep-window / relation-map
    /// plus the `has_dep`/`has_relations`/`dep_highest_seen` doc latches) — the
    /// dependency half of the dissolved C++ `Window`.
    pub deps: crate::window::DepBookkeeping,

    /// C++ `uint32FlatHashMap variables` — the run-time global variables.
    pub variables: Uint32FlatHashMap,

    pub input_eof: bool,
    pub dep_has_spanned: bool,

    pub externals: Externals,

    /// Per-run counter — number of input lines consumed (C++ `numLines`). NOT the
    /// `cfg.num_windows` limit; see the `num_windows` counter below.
    pub num_lines: u32,
    /// Per-run counter — number of windows produced this run (C++ `numWindows`).
    /// The COUNTER, distinct from `cfg.num_windows` (the reset-after LIMIT).
    pub num_windows: u32,
    /// Per-run counter — number of cohorts produced this run (C++ `numCohorts`).
    pub num_cohorts: u32,
    /// Per-run counter — number of readings produced this run (C++ `numReadings`).
    pub num_readings: u32,
}

impl Document {
    pub fn new() -> Self {
        Document {
            store: crate::store::RuntimeStore::new(),
            stream: Default::default(),
            cohorts: Default::default(),
            deps: Default::default(),

            variables: Default::default(),

            input_eof: false,
            dep_has_spanned: false,

            externals: Default::default(),

            num_lines: 0,
            num_windows: 0,
            num_cohorts: 0,
            num_readings: 0,
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator+4]
/// The per-rule / per-window / per-cohort transient state extracted from the C++
/// `GrammarApplicator` members (the `scratch` bucket of the field triage). This
/// is a Stage-B re-homing: it has no C++ analog as a type; the members map 1:1
/// onto the scratch-bucket fields of `GrammarApplicator`, keeping their types,
/// defaults, and per-field C++ reference comments; names are snake_case, with
/// each camelCase C++ original on its field doc. Every field is cleared
/// or reset inside the rule-application loops (`context_stack`, the iterator
/// pools, the `index_*` caches, the unification/regex-capture stores, the
/// per-frame loop-control latches, etc.).
pub struct RuleScratch {
    pub seen_barrier: bool,

    /// C++ `sorted_vector<std::pair<uint32_t, uint32_t>> dep_deep_seen`.
    pub dep_deep_seen: SortedVector<(u32, u32)>,

    pub ci_depths: Uint32Vector,
    /// C++ `cohortIterators`.
    pub cohort_iterators: BTreeMap<u32, CohortIterator>,
    /// C++ `topologyLeftIters`.
    pub topology_left_iters: BTreeMap<u32, TopologyLeftIter>,
    /// C++ `topologyRightIters`.
    pub topology_right_iters: BTreeMap<u32, TopologyRightIter>,
    /// C++ `depParentIters`.
    pub dep_parent_iters: BTreeMap<u32, DepParentIter>,
    /// C++ `depDescendentIters`.
    pub dep_descendent_iters: BTreeMap<u32, DepDescendentIter>,
    /// C++ `depAncestorIters`.
    pub dep_ancestor_iters: BTreeMap<u32, DepAncestorIter>,

    pub par_left_tag: TagHash,
    pub par_right_tag: TagHash,
    pub par_left_pos: u32,
    pub par_right_pos: u32,
    pub did_final_enclosure: bool,

    pub tmpl_cntx: TmplContext,

    pub regexgrps_store: Vec<RegexGroups>,
    /// C++ `bc::flat_map<uint32_t, uint8_t> regexgrps_z`.
    pub regexgrps_z: BTreeMap<u32, u8>,
    /// C++ `bc::flat_map<uint32_t, regexgrps_t*> regexgrps_c` — values are
    /// indices into `regexgrps_store`.
    pub regexgrps_c: BTreeMap<u32, usize>,
    pub same_basic: u32,
    pub rule_target: Option<CohortId>,
    pub merge_with: Option<CohortId>,
    pub current_rule: Option<RuleId>,
    pub context_stack: Vec<RuleContext>,
    /// C++ `std::vector<CohortSet*> cohortsets` — wave 4: safe DESCRIPTORS of
    /// the per-window cohort sets the active `run_single_rule` frames iterate
    /// (resolved against the store on every access, so window restructuring
    /// can never leave a dangling pointer).
    pub cohortsets: Vec<CsRef>,
    /// C++ `std::vector<size_t*> rocits` — wave 4: the per-frame iteration
    /// cursors OWNED by value (the C++ parked pointers to stack locals). Inner
    /// frames and `update_rule_to_cohorts` adjust outer frames' cursors by
    /// index, exactly as the C++ wrote through the parked pointers.
    pub rocits: Vec<usize>,

    pub readings_plain: ReadingsPlain,

    /// C++ `Reading::matched_target` / `Reading::matched_tests` (`uint8_t : 1`
    /// bitfields on `Reading`), re-homed here as `ReadingId` membership sets
    /// (plan node `matcher-doc-split.matched-flags`): the flags are rule-scoped
    /// bookkeeping — written by the matchers during target/test evaluation and
    /// consumed by the same rule's SELECT/IFF finalisation — so they belong to
    /// the rule scratch, not the document model. Every C++ `reading.matched_* =
    /// v` store maps to insert (true) / remove (false) at the same site, every
    /// read to `contains`; they are membership-tested only, never iterated.
    /// A fresh `Reading` in C++ always starts with false flags; that holds here
    /// because `ReadingId`s are GENERATIONAL — a recycled arena slot's next
    /// occupant carries a different id, so it cannot inherit a freed reading's
    /// leftover membership. Leftover entries for freed ids are pruned once per
    /// window (see `run_grammar_on_single_window`) to keep the sets bounded.
    pub matched_target: HashSet<ReadingId>,
    pub matched_tests: HashSet<ReadingId>,

    /// C++ `bc::flat_map<uint32_t, unif_tags_t*> unif_tags_rs` — values are
    /// indices into `unif_tags_store`.
    pub unif_tags_rs: BTreeMap<u32, usize>,
    pub unif_tags_store: Vec<UnifTags>,
    /// C++ `bc::flat_map<uint32_t, unif_sets_t*> unif_sets_rs` — values are
    /// indices into `unif_sets_store`.
    pub unif_sets_rs: BTreeMap<u32, usize>,
    pub unif_sets_store: Vec<UnifSets>,
    pub unif_last_wordform: TagHash,
    pub unif_last_baseform: TagHash,
    pub unif_last_textual: TagHash,
    /// C++ `bc::flat_map<uint32_t, uint32_t> rule_hits`.
    pub rule_hits: BTreeMap<u32, u32>,

    pub ss_utags: ScopedStack<UnifTags>,
    pub ss_usets: ScopedStack<UnifSets>,
    pub ss_u32sv: ScopedStack<Uint32SortedVector>,

    pub index_regexp_yes: Uint64FlatHashSet,
    pub index_regexp_no: Uint64FlatHashSet,
    pub index_icase_yes: Uint64FlatHashSet,
    pub index_icase_no: Uint64FlatHashSet,
    /// C++ `index_readingSet_yes`.
    pub index_reading_set_yes: Vec<Uint32FlatHashSet>,
    /// C++ `index_readingSet_no`.
    pub index_reading_set_no: Vec<Uint32FlatHashSet>,

    pub reset_cohorts_for_loop: bool,
    pub finish_reading_loop: bool,
    pub finish_cohort_loop: bool,
    pub in_nested: bool,
    pub used_regex: usize,

    /// C++ `std::deque<Reading> subs_any` — the amalgamated sub-reading arena
    /// used by `get_sub_reading(GSR_ANY)`. RECONCILIATION: the amalgam lives in
    /// the readings arena; only the id is tracked here.
    pub subs_any: Vec<crate::arena::ReadingId>,
}

impl RuleScratch {
    /// Every field at its C++ default-member-initialiser value (the initialisers
    /// moved verbatim out of the former `GrammarApplicator::new`).
    pub fn new() -> Self {
        RuleScratch {
            seen_barrier: false,

            dep_deep_seen: Default::default(),

            ci_depths: vec![0u32; 6],
            cohort_iterators: Default::default(),
            topology_left_iters: Default::default(),
            topology_right_iters: Default::default(),
            dep_parent_iters: Default::default(),
            dep_descendent_iters: Default::default(),
            dep_ancestor_iters: Default::default(),

            par_left_tag: TagHash(0),
            par_right_tag: TagHash(0),
            par_left_pos: 0,
            par_right_pos: 0,
            did_final_enclosure: false,

            tmpl_cntx: Default::default(),

            regexgrps_store: Default::default(),
            regexgrps_z: Default::default(),
            regexgrps_c: Default::default(),
            same_basic: 0,
            rule_target: None,
            merge_with: None,
            current_rule: None,
            context_stack: Default::default(),
            cohortsets: Default::default(),
            rocits: Default::default(),

            readings_plain: Default::default(),

            matched_target: Default::default(),
            matched_tests: Default::default(),

            unif_tags_rs: Default::default(),
            unif_tags_store: Default::default(),
            unif_sets_rs: Default::default(),
            unif_sets_store: Default::default(),
            unif_last_wordform: TagHash(0),
            unif_last_baseform: TagHash(0),
            unif_last_textual: TagHash(0),
            rule_hits: Default::default(),

            ss_utags: ScopedStack::new(),
            ss_usets: ScopedStack::new(),
            ss_u32sv: ScopedStack::new(),

            index_regexp_yes: Default::default(),
            index_regexp_no: Default::default(),
            index_icase_yes: Default::default(),
            index_icase_no: Default::default(),
            index_reading_set_yes: Default::default(),
            index_reading_set_no: Default::default(),

            reset_cohorts_for_loop: false,
            finish_reading_loop: true,
            finish_cohort_loop: true,
            in_nested: false,
            used_regex: 0,

            subs_any: Vec::new(),
        }
    }

    /// The C++ `reading.matched_target = v` bitfield store: `true` inserts,
    /// `false` removes (so a stale membership is overwritten either way).
    pub(crate) fn set_matched_target(&mut self, id: ReadingId, v: bool) {
        if v {
            self.matched_target.insert(id);
        } else {
            self.matched_target.remove(&id);
        }
    }

    /// The C++ `reading.matched_tests = v` bitfield store; see
    /// [`Self::set_matched_target`].
    pub(crate) fn set_matched_tests(&mut self, id: ReadingId, v: bool) {
        if v {
            self.matched_tests.insert(id);
        } else {
            self.matched_tests.remove(&id);
        }
    }

    /// The paired C++ `reading.matched_target = false; reading.matched_tests =
    /// false;` clear that opens every rule evaluation of a reading.
    pub(crate) fn clear_matched(&mut self, id: ReadingId) {
        self.matched_target.remove(&id);
        self.matched_tests.remove(&id);
    }
}

impl Default for RuleScratch {
    fn default() -> Self {
        Self::new()
    }
}

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator+4]
/// The profiler state extracted from the C++ `GrammarApplicator` members (the
/// `diag` bucket of the field triage). This is a Stage-B re-homing: it has no
/// C++ analog as a type.
///
/// DIVERGENCE (operator decision, plan node `drop-dead-match-counters`): the
/// C++ `match_single`/`match_comp`/`match_sub` counters are deleted, not
/// ported — they are write-only in the reference too (declared, incremented,
/// never read; `match_comp` never even incremented), the same dead-upstream
/// class as `--dry-run`.
pub struct Diagnostics {
    /// C++ `Profiler* profiler` — the raw pointer to main's Profiler becomes
    /// OWNED `Option<Profiler>`: the driver (vislcg3) moves the profiler in
    /// before the run and takes it back out afterwards to write the database.
    pub profiler: Option<crate::profiler::Profiler>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Diagnostics { profiler: None }
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::new()
    }
}

/// C++ `class GrammarApplicator` — the constraint-grammar application engine.
///
/// Stage-B re-homing complete: the flat god-object members are partitioned into
/// exactly five subsystems — the options-derived [`EngineConfig`] (`cfg`), the
/// run-mutable document-lifetime [`Document`] (`doc`), the per-rule transient
/// [`RuleScratch`] (`scratch`), the profiler/stats [`Diagnostics`] (`diag`), and
/// the owned [`Grammar`](crate::grammar::Grammar) (`grammar`).
pub struct GrammarApplicator {
    /// The options-derived, setup-written configuration (Stage-B re-homing of the
    /// cfg-bucket members; see [`EngineConfig`]).
    pub cfg: EngineConfig,

    /// The run-mutable, document-lifetime state (Stage-B re-homing of the
    /// doc-bucket members; see [`Document`]).
    pub doc: Document,

    /// The per-rule / per-window / per-cohort transient state (Stage-B re-homing
    /// of the scratch-bucket members; see [`RuleScratch`]).
    pub scratch: RuleScratch,

    /// The profiler / statistics state (Stage-B re-homing of the diag-bucket
    /// members; see [`Diagnostics`]).
    pub diag: Diagnostics,

    /// C++ `const Grammar* grammar` — the applicator OWNS the loaded grammar.
    pub grammar: crate::grammar::Grammar,
}

impl GrammarApplicator {
    /// Constructs an applicator that owns `grammar`, with every field at its C++
    /// default-member-initialiser value. This is NOT the real
    /// `grammar-applicator-fn` constructor (which wires streams, options, and
    /// the begin/end/subst tags); that semantic lands in the impl pass.
    pub fn new(grammar: crate::grammar::Grammar) -> Self {
        GrammarApplicator {
            cfg: EngineConfig::new(),

            doc: Document::new(),

            scratch: RuleScratch::new(),

            diag: Diagnostics::new(),

            grammar,
        }
    }

    /// Splits `&mut self` into an [`Engine`] view over the five subsystems.
    ///
    /// One `engine()` call at a driver's top hands the peeled method tree the
    /// disjoint borrows it needs, ending the false borrow conflicts of the
    /// monolithic `&mut self` receiver. Stage-C decomposition converts method
    /// clusters onto this view leaves-first along the call graph; unpeeled
    /// `&mut self` methods split at the call site (`self.engine().foo(...)`).
    pub fn engine(&mut self) -> Engine<'_> {
        Engine {
            cfg: &self.cfg,
            doc: &mut self.doc,
            scratch: &mut self.scratch,
            diag: &mut self.diag,
            grammar: &mut self.grammar,
        }
    }
}

/// Split-borrow view over the engine's subsystems: one [`GrammarApplicator::engine`]
/// call at a driver's top splits `&mut self` into disjoint borrows the method
/// tree can thread, ending the false borrow conflicts of the monolithic
/// receiver. Peeled method clusters (Stage-C decomposition) live in `impl
/// Engine<'_>` blocks in the partial modules that own their C++ translation
/// unit; a method takes the narrowest borrow subset it needs by pattern-binding
/// the fields it touches. The predicate/test tree lives one level further down,
/// on the [`Matcher`] sub-view ([`Engine::matcher`]).
pub struct Engine<'a> {
    pub cfg: &'a EngineConfig,
    pub doc: &'a mut Document,
    pub scratch: &'a mut RuleScratch,
    pub diag: &'a mut Diagnostics,
    /// C++ `const Grammar* grammar` (read-only for the whole matcher tree)
    /// EXCEPT the runtime tag-generation path (`add_tag` interning a
    /// varstring/regex/icase tag), reached from the contextual matcher knot via
    /// `generate_varstring_tag`. Held `&mut` so that single write path can
    /// intern into the tag arenas; every other peeled method only reads it.
    pub grammar: &'a mut crate::grammar::Grammar,
}

/// Split-borrow sub-view of [`Engine`] for the predicate/test tree — the
/// `matchSet.cpp` + `runContextualTest.cpp` method knot plus the match-support
/// helpers it transitively calls (`get_sub_reading`, `doesWordformsMatch`,
/// `generateVarstringTag`/`addTag`, the context-frame accessors). The C++ runs
/// this tree on the same mutable god object as the rule actions; the port's
/// field-level borrows are the proof that matching only *reads* the document
/// model, with exactly one narrow, transient-shaped write capability:
///
/// * [`readings`](Matcher::readings) is the one `&mut` arena hole. Convention:
///   the matcher may only alloc/free TRANSIENT slots — the `get_sub_reading`
///   `GSR_ANY` amalgam (freed per cohort via the engine's `subs_any_clear`) and
///   the `match_bag_of_tags` bag clone (freed before returning) — plus the
///   `reflowTextuals` re-derivation reached through runtime `addTag` interning
///   (`tags_textual` is derived data). It never edits a document reading's
///   model state.
///
/// Everything else the tree touches on the document — the cohort arena (the
/// C++ wrote its getMin/getMax memo here; the port computes numeric min/max on
/// demand, see `cohort::min_max_for_key`), the single-window arena, the
/// [`WindowStream`](crate::window::WindowStream) spanning links, the
/// [`CohortRegistry`](crate::window::CohortRegistry) `cohort_map`, the global
/// `variables`, the `num_lines` counter — is held by shared reference.
/// Match STATE goes to [`scratch`](Matcher::scratch) (captures, unification,
/// memo indexes, matched-flag sets) and tag interning to
/// [`grammar`](Matcher::grammar) (append-only, per the [`Engine`] convention).
/// `Engine.doc.deps` and `Engine.diag` are not represented: the tree never
/// reads dependency bookkeeping directly (dep tests resolve through
/// `cohort_map` and per-cohort `dep_*` fields) and has no live profiler hook.
///
/// Action-layer code never holds a `Matcher`; it calls the thin `Engine`
/// forwarders below, each of which split-borrows a fresh view per call.
pub struct Matcher<'a> {
    pub cfg: &'a EngineConfig,
    /// `doc.store.cohorts` — read-only.
    pub cohorts: &'a GenArena<crate::cohort::Cohort>,
    /// `doc.store.single_windows` — read-only.
    pub single_windows: &'a GenArena<crate::single_window::SingleWindow>,
    /// `doc.store.readings` — the transient-slot `&mut` hole (see the
    /// type-level doc).
    pub readings: &'a mut GenArena<crate::reading::Reading>,
    /// `doc.stream` — window spanning (previous/current/next), read-only.
    pub stream: &'a crate::window::WindowStream,
    /// `doc.cohorts` — the global cohort registry (`cohort_map`), read-only.
    pub registry: &'a crate::window::CohortRegistry,
    /// `doc.variables` — the run-time global variables, read-only (the matcher
    /// only tests them; SETVARIABLE is an action).
    pub variables: &'a Uint32FlatHashMap,
    /// `doc.num_lines` — read by the runtime `addTag` error path.
    pub num_lines: &'a u32,
    /// Declared match state: captures, unification, memo indexes, the
    /// matched-flag sets, iterator pools, the context stack.
    pub scratch: &'a mut RuleScratch,
    /// Tag interning (append-only), per the [`Engine::grammar`] convention;
    /// also the `POS_TMPL_OVERRIDE` save/restore on `contexts_arena`.
    pub grammar: &'a mut crate::grammar::Grammar,
}

impl Engine<'_> {
    /// Splits this view into the [`Matcher`] sub-view for the predicate/test
    /// tree, by disjoint field borrows of `doc`/`doc.store` (see [`Matcher`]
    /// for the capability contract each field carries).
    pub fn matcher(&mut self) -> Matcher<'_> {
        let Document {
            store,
            stream,
            cohorts: registry,
            variables,
            num_lines,
            ..
        } = &mut *self.doc;
        let crate::store::RuntimeStore {
            cohorts,
            readings,
            single_windows,
        } = store;
        Matcher {
            cfg: self.cfg,
            cohorts: &*cohorts,
            single_windows,
            readings,
            stream,
            registry,
            variables,
            num_lines,
            scratch: self.scratch,
            grammar: self.grammar,
        }
    }
}

// The action layer's entry points into the [`Matcher`] tree: thin forwarders
// that split-borrow a fresh sub-view per call. Signatures are the C++ ones
// (annotated on the `impl Matcher` bodies); only what run_rules / run_grammar /
// reflow / the stream applicators actually call is forwarded.
impl Engine<'_> {
    pub fn does_set_match_reading(
        &mut self,
        reading: ReadingId,
        set: u32,
        bypass_index: bool,
        unif_mode: bool,
    ) -> Result<bool, crate::error::RunError> {
        self.matcher()
            .does_set_match_reading(reading, set, bypass_index, unif_mode)
    }

    pub fn does_set_match_cohort_normal(
        &mut self,
        cohort: CohortId,
        set: u32,
        context: Option<&mut CohortMatchContext>,
    ) -> Result<bool, crate::error::RunError> {
        self.matcher()
            .does_set_match_cohort_normal(cohort, set, context)
    }

    pub fn does_tag_match_reading(
        &mut self,
        reading: ReadingId,
        tag: &crate::tag::Tag,
        unif_mode: bool,
        bypass_index: bool,
    ) -> Result<u32, crate::error::RunError> {
        self.matcher()
            .does_tag_match_reading(reading, tag, unif_mode, bypass_index)
    }

    pub fn does_tag_match_regexp(
        &mut self,
        test: u32,
        tag: &crate::tag::Tag,
        bypass_index: bool,
    ) -> u32 {
        self.matcher()
            .does_tag_match_regexp(test, tag, bypass_index)
    }

    pub fn does_tag_match_icase(
        &mut self,
        test: u32,
        tag: &crate::tag::Tag,
        bypass_index: bool,
    ) -> u32 {
        self.matcher().does_tag_match_icase(test, tag, bypass_index)
    }

    pub fn get_tags_matching(
        &mut self,
        reading: ReadingId,
        the_tags: &TagList,
        rv_tags: &mut TagList,
    ) {
        self.matcher().get_tags_matching(reading, the_tags, rv_tags)
    }

    pub fn run_contextual_test(
        &mut self,
        sw: Option<SwId>,
        position: u32,
        test: CtxId,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<Option<CohortId>, crate::error::RunError> {
        self.matcher()
            .run_contextual_test(sw, position, test, deep, origin)
    }

    pub fn get_sub_reading(&mut self, tr: ReadingId, sub_reading: i32) -> Option<ReadingId> {
        self.matcher().get_sub_reading(tr, sub_reading)
    }

    pub fn does_wordforms_match(&mut self, cword: Option<TagId>, rword: Option<TagId>) -> bool {
        self.matcher().does_wordforms_match(cword, rword)
    }

    pub fn generate_varstring_tag(
        &mut self,
        tag: &crate::tag::Tag,
    ) -> Result<TagId, crate::error::RunError> {
        self.matcher().generate_varstring_tag(tag)
    }

    pub fn add_tag(
        &mut self,
        txt: &str,
        r#type: crate::tag::TagType,
    ) -> Result<TagId, crate::error::RunError> {
        self.matcher().add_tag(txt, r#type)
    }

    pub(crate) fn get_tag_list_of_set(&mut self, set: SetId, unif_mode: bool) -> TagList {
        self.matcher().get_tag_list_of_set(set, unif_mode)
    }

    pub(crate) fn get_tag_list_of_set_number(&mut self, number: u32, unif_mode: bool) -> TagList {
        self.matcher().get_tag_list_of_set_number(number, unif_mode)
    }

    pub fn get_attach_to(&mut self) -> ReadingSpec {
        self.matcher().get_attach_to()
    }

    pub fn set_mark(&mut self, cohort: Option<CohortId>) {
        self.matcher().set_mark(cohort)
    }
}
