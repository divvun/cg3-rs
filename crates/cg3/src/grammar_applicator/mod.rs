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

use std::collections::BTreeMap;

use crate::arena::{CohortId, CtxId, ReadingId, RuleId, TagId};
use crate::cohort::CohortSet;
use crate::cohort_iterator::{
    CohortIterator, DepAncestorIter, DepDescendentIter, DepParentIter, TopologyLeftIter,
    TopologyRightIter,
};
use crate::flat_unordered_map::Uint32FlatHashMap;
use crate::flat_unordered_set::{Uint32FlatHashSet, Uint64FlatHashSet};
use crate::interval_vector::uint32IntervalVector;
use crate::process::Process;
use crate::scoped_stack::ScopedStack;
use crate::sorted_vector::{sorted_vector, uint32SortedVector};
use crate::tag::TagList;
use crate::types::{UChar, UString, Uint32Vector};

pub mod core;
pub mod stream_format;
pub mod run_rules;
pub mod run_grammar;
pub mod run_contextual_test;
pub mod match_set;
pub mod reflow;
pub mod context;

// C++ `cg3.h` `cg3_sformat` — the stream serialisation format tag used by
// `fmt_input` / `fmt_output`. The public C-API header (`cg3.h`) is not yet
// ported to Rust, so the enum is defined here (in the engine skeleton) where it
// is first needed; a later pass may relocate it to a `cg3` C-API module.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u32)]
pub enum cg3_sformat {
    CG3SF_INVALID = 0,
    #[default]
    CG3SF_CG = 1,
    CG3SF_NICELINE = 2,
    CG3SF_APERTIUM = 3,
    CG3SF_MATXIN = 4,
    CG3SF_FST = 5,
    CG3SF_PLAIN = 6,
    CG3SF_JSONL = 7,
    CG3SF_BINARY = 8,
}

// [spec:cg3:def:grammar-applicator.cg3.regexgrps-t]
/// C++ `typedef std::vector<UnicodeString> regexgrps_t` — the captured regex
/// groups for one context frame (`UnicodeString` → UTF-8 [`UString`]).
pub type regexgrps_t = Vec<UString>;

// Value can be either tag or trie, but we only ever compare pointers and never
// dereference, so just make it a raw address (`const void*` → `*const ()`).
// [spec:cg3:def:grammar-applicator.cg3.unif-tags-t]
/// C++ `typedef bc::flat_map<uint32_t, const void*> unif_tags_t`.
pub type unif_tags_t = BTreeMap<u32, *const ()>;
// [spec:cg3:def:grammar-applicator.cg3.unif-sets-t]
/// C++ `typedef bc::flat_map<uint32_t, uint32SortedVector> unif_sets_t`.
pub type unif_sets_t = BTreeMap<u32, uint32SortedVector>;

// [spec:cg3:def:grammar-applicator.cg3.tmpl-context-t]
/// C++ `struct tmpl_context_t` — the active template-test window (`min`/`max`
/// bounds), the stack of `linked` tests, and the `in_template` flag. The
/// `clear()` member (`tmpl-context-t.clear-fn`) is a method left for the impl
/// pass.
#[derive(Default, Clone)]
pub struct tmpl_context_t {
    pub min: Option<CohortId>,
    pub max: Option<CohortId>,
    /// C++ `std::vector<const ContextualTest*> linked`.
    pub linked: Vec<CtxId>,
    pub in_template: bool,
}

// [spec:cg3:def:grammar-applicator.cg3.d-smc-context]
/// C++ `struct dSMC_Context` — the mutable context threaded through the
/// `doesSetMatchCohort*` matchers (the pending test, the `deep`/`origin`
/// out-targets, the option bitmask, and the match/barrier flags). `Cohort**
/// deep` (a pointer to the caller's `Cohort*` slot) → `*mut Option<CohortId>`,
/// itself nullable.
#[derive(Default, Clone)]
pub struct dSMC_Context {
    /// C++ `const ContextualTest* test`.
    pub test: Option<CtxId>,
    /// C++ `Cohort** deep`.
    pub deep: Option<*mut Option<CohortId>>,
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
pub struct Rule_Context {
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

// [spec:cg3:def:grammar-applicator.cg3.rule-callback]
/// C++ `typedef std::function<void(void)> RuleCallback` — the reading/cohort
/// callbacks handed to `runSingleRule`.
pub type RuleCallback = Box<dyn FnMut()>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.all-mappings-t]
/// C++ `typedef std::map<Reading*, TagList> all_mappings_t`.
pub type all_mappings_t = BTreeMap<ReadingId, TagList>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.rs-type]
/// C++ `typedef std::map<int32_t, uint32IntervalVector> RSType` — the
/// per-section rule schedule (negative keys are the before/after/null sections).
pub type RSType = BTreeMap<i32, uint32IntervalVector>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.externals-t]
/// C++ `typedef std::map<uint32_t, Process> externals_t` — the running
/// EXTERNAL child processes, keyed by tag hash.
pub type externals_t = BTreeMap<u32, Process>;

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.readings-plain-t]
/// C++ `typedef bc::flat_map<uint32_t, Reading*> readings_plain_t`.
pub type readings_plain_t = BTreeMap<u32, ReadingId>;

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
impl crate::pool::Poolable for unif_tags_t {
    fn clear(&mut self) {
        BTreeMap::clear(self);
    }
}
impl crate::pool::Poolable for unif_sets_t {
    fn clear(&mut self) {
        BTreeMap::clear(self);
    }
}
impl crate::pool::Poolable for uint32SortedVector {
    fn clear(&mut self) {
        uint32SortedVector::clear(self);
    }
}

// [spec:cg3:def:grammar-applicator.cg3.grammar-applicator]
/// C++ `class GrammarApplicator` — the constraint-grammar application engine.
pub struct GrammarApplicator {
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
    pub dry_run: bool,
    pub owns_grammar: bool,
    pub input_eof: bool,
    pub seen_barrier: bool,
    pub is_conv: bool,
    pub split_mappings: bool,
    pub pipe_deleted: bool,
    pub add_spacing: bool,
    pub print_ids: bool,

    pub fmt_input: cg3_sformat,
    pub fmt_output: cg3_sformat,

    pub dep_has_spanned: bool,
    pub dep_delimit: u32,
    pub dep_absolute: bool,
    pub dep_original: bool,
    pub dep_block_loops: bool,
    pub dep_block_crossing: bool,

    pub num_windows: u32,
    pub soft_limit: u32,
    pub hard_limit: u32,
    pub sections: Uint32Vector,
    pub valid_rules: uint32IntervalVector,
    pub trace_rules: uint32IntervalVector,
    pub debug_rules: uint32IntervalVector,
    pub variables: Uint32FlatHashMap,
    pub verbosity_level: u32,
    pub debug_level: u32,
    pub section_max_count: u32,

    pub has_dep: bool,
    pub parse_dep: bool,
    pub dep_highest_seen: u32,
    /// C++ `std::unique_ptr<Window> gWindow` — the owned document window.
    pub gWindow: crate::window::Window,
    pub has_relations: bool,

    /// C++ `const Grammar* grammar` — the applicator OWNS the loaded grammar.
    pub grammar: crate::grammar::Grammar,
    /// The runtime object arenas (pooled `Cohort`/`Reading`/`SingleWindow`).
    ///
    /// REQUIRED FIELD ADDED BY THE `reflow` METHOD PASS. The mod.rs scaffold's
    /// doc header already states the applicator "OWNS the runtime object arenas
    /// via `store`", but the struct itself was missing the field, so no arena id
    /// (`CohortId`/`ReadingId`/`SwId`) could be resolved. Every ported engine
    /// method resolves runtime objects through here (`self.store.cohorts` /
    /// `self.store.readings` / `self.store.single_windows`), and `Window`/
    /// `SingleWindow`/`Cohort` free fns are threaded `&mut self.store`.
    pub store: crate::store::RuntimeStore,
    /// C++ `Profiler* profiler` — the raw pointer to main's Profiler becomes
    /// OWNED `Option<Profiler>`: the driver (vislcg3) moves the profiler in
    /// before the run and takes it back out afterwards to write the database.
    pub profiler: Option<crate::profiler::Profiler>,

    /// C++ `UChar* filebase` — nullable input-file basename buffer.
    pub filebase: Option<UString>,

    pub span_pattern_latin: UString,
    pub span_pattern_utf: UString,
    /// C++ `UChar ws[4]{ ' ', '\t', 0, 0 }` — the whitespace set.
    pub ws: [UChar; 4],

    pub numLines: u32,
    pub numWindows: u32,
    pub numCohorts: u32,
    pub numReadings: u32,

    pub did_index: bool,
    /// C++ `sorted_vector<std::pair<uint32_t, uint32_t>> dep_deep_seen`.
    pub dep_deep_seen: sorted_vector<(u32, u32)>,

    pub numsections: u32,
    pub runsections: RSType,
    pub externals: externals_t,

    pub ci_depths: Uint32Vector,
    pub cohortIterators: BTreeMap<u32, CohortIterator>,
    pub topologyLeftIters: BTreeMap<u32, TopologyLeftIter>,
    pub topologyRightIters: BTreeMap<u32, TopologyRightIter>,
    pub depParentIters: BTreeMap<u32, DepParentIter>,
    pub depDescendentIters: BTreeMap<u32, DepDescendentIter>,
    pub depAncestorIters: BTreeMap<u32, DepAncestorIter>,

    pub match_single: u32,
    pub match_comp: u32,
    pub match_sub: u32,
    pub begintag: u32,
    pub endtag: u32,
    pub substtag: u32,
    pub tag_begin: Option<TagId>,
    pub tag_end: Option<TagId>,
    pub tag_subst: Option<TagId>,
    pub par_left_tag: u32,
    pub par_right_tag: u32,
    pub par_left_pos: u32,
    pub par_right_pos: u32,
    pub did_final_enclosure: bool,
    pub mprefix_key: u32,
    pub mprefix_value: u32,

    pub tmpl_cntx: tmpl_context_t,

    pub regexgrps_store: Vec<regexgrps_t>,
    /// C++ `bc::flat_map<uint32_t, uint8_t> regexgrps_z`.
    pub regexgrps_z: BTreeMap<u32, u8>,
    /// C++ `bc::flat_map<uint32_t, regexgrps_t*> regexgrps_c` — values are
    /// indices into `regexgrps_store`.
    pub regexgrps_c: BTreeMap<u32, usize>,
    pub same_basic: u32,
    pub rule_target: Option<CohortId>,
    pub context_target: Option<CohortId>,
    pub merge_with: Option<CohortId>,
    pub current_rule: Option<RuleId>,
    pub context_stack: Vec<Rule_Context>,
    /// C++ `std::vector<CohortSet*> cohortsets` — aliases per-window cohort sets.
    pub cohortsets: Vec<*mut CohortSet>,
    /// C++ `std::vector<size_t*> rocits` — aliases per-window cursor indices.
    pub rocits: Vec<*mut usize>,

    pub readings_plain: readings_plain_t,
    /// C++ `std::vector<URegularExpression*> text_delimiters` — owned compiled
    /// regexes (ICU `URegularExpression*` → `regex::Regex`).
    pub text_delimiters: Vec<regex::Regex>,

    /// C++ `bc::flat_map<uint32_t, unif_tags_t*> unif_tags_rs` — values are
    /// indices into `unif_tags_store`.
    pub unif_tags_rs: BTreeMap<u32, usize>,
    pub unif_tags_store: Vec<unif_tags_t>,
    /// C++ `bc::flat_map<uint32_t, unif_sets_t*> unif_sets_rs` — values are
    /// indices into `unif_sets_store`.
    pub unif_sets_rs: BTreeMap<u32, usize>,
    pub unif_sets_store: Vec<unif_sets_t>,
    pub unif_last_wordform: u32,
    pub unif_last_baseform: u32,
    pub unif_last_textual: u32,
    /// C++ `bc::flat_map<uint32_t, uint32_t> rule_hits`.
    pub rule_hits: BTreeMap<u32, u32>,

    pub ss_taglist: ScopedStack<TagList>,
    pub ss_utags: ScopedStack<unif_tags_t>,
    pub ss_usets: ScopedStack<unif_sets_t>,
    pub ss_u32sv: ScopedStack<uint32SortedVector>,

    pub index_regexp_yes: Uint64FlatHashSet,
    pub index_regexp_no: Uint64FlatHashSet,
    pub index_icase_yes: Uint64FlatHashSet,
    pub index_icase_no: Uint64FlatHashSet,
    pub index_readingSet_yes: Vec<Uint32FlatHashSet>,
    pub index_readingSet_no: Vec<Uint32FlatHashSet>,
    pub index_ruleCohort_no: Uint32FlatHashSet,

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

impl GrammarApplicator {
    /// Constructs an applicator that owns `grammar`, with every field at its C++
    /// default-member-initialiser value. This is NOT the real
    /// `grammar-applicator-fn` constructor (which wires streams, options, and
    /// the begin/end/subst tags); that semantic lands in the impl pass.
    pub fn new(grammar: crate::grammar::Grammar) -> Self {
        GrammarApplicator {
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
            dry_run: false,
            owns_grammar: false,
            input_eof: false,
            seen_barrier: false,
            is_conv: false,
            split_mappings: false,
            pipe_deleted: false,
            add_spacing: true,
            print_ids: false,

            fmt_input: cg3_sformat::CG3SF_CG,
            fmt_output: cg3_sformat::CG3SF_CG,

            dep_has_spanned: false,
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
            variables: Default::default(),
            verbosity_level: 0,
            debug_level: 0,
            section_max_count: 0,

            has_dep: false,
            parse_dep: false,
            dep_highest_seen: 0,
            gWindow: crate::window::Window::new(None),
            has_relations: false,

            grammar,
            store: crate::store::RuntimeStore::new(),
            profiler: None,

            filebase: None,

            span_pattern_latin: Default::default(),
            span_pattern_utf: Default::default(),
            ws: [' ', '\t', '\0', '\0'],

            numLines: 0,
            numWindows: 0,
            numCohorts: 0,
            numReadings: 0,

            did_index: false,
            dep_deep_seen: Default::default(),

            numsections: 0,
            runsections: Default::default(),
            externals: Default::default(),

            ci_depths: vec![0u32; 6],
            cohortIterators: Default::default(),
            topologyLeftIters: Default::default(),
            topologyRightIters: Default::default(),
            depParentIters: Default::default(),
            depDescendentIters: Default::default(),
            depAncestorIters: Default::default(),

            match_single: 0,
            match_comp: 0,
            match_sub: 0,
            begintag: 0,
            endtag: 0,
            substtag: 0,
            tag_begin: None,
            tag_end: None,
            tag_subst: None,
            par_left_tag: 0,
            par_right_tag: 0,
            par_left_pos: 0,
            par_right_pos: 0,
            did_final_enclosure: false,
            mprefix_key: 0,
            mprefix_value: 0,

            tmpl_cntx: Default::default(),

            regexgrps_store: Default::default(),
            regexgrps_z: Default::default(),
            regexgrps_c: Default::default(),
            same_basic: 0,
            rule_target: None,
            context_target: None,
            merge_with: None,
            current_rule: None,
            context_stack: Default::default(),
            cohortsets: Default::default(),
            rocits: Default::default(),

            readings_plain: Default::default(),
            text_delimiters: Default::default(),

            unif_tags_rs: Default::default(),
            unif_tags_store: Default::default(),
            unif_sets_rs: Default::default(),
            unif_sets_store: Default::default(),
            unif_last_wordform: 0,
            unif_last_baseform: 0,
            unif_last_textual: 0,
            rule_hits: Default::default(),

            ss_taglist: ScopedStack::new(),
            ss_utags: ScopedStack::new(),
            ss_usets: ScopedStack::new(),
            ss_u32sv: ScopedStack::new(),

            index_regexp_yes: Default::default(),
            index_regexp_no: Default::default(),
            index_icase_yes: Default::default(),
            index_icase_no: Default::default(),
            index_readingSet_yes: Default::default(),
            index_readingSet_no: Default::default(),
            index_ruleCohort_no: Default::default(),

            reset_cohorts_for_loop: false,
            finish_reading_loop: true,
            finish_cohort_loop: true,
            in_nested: false,
            used_regex: 0,

            subs_any: Vec::new(),
        }
    }
}
