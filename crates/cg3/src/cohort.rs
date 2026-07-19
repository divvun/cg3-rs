//! Port of `src/Cohort.hpp` — the `Cohort` type and its aliases.
//!
//! CORE TYPE-SKELETON pass: type definitions only, no ported method bodies.
//!
//! Pointer/id mapping notes (each field follows the `.hpp` exactly):
//! * `wordform` is `Tag*` → [`Option<TagId>`].
//! * `parent` is `SingleWindow*` → [`Option<SwId>`]; `prev`/`next` are
//!   `Cohort*` (sibling chain) → [`Option<CohortId>`]; `wread` is `Reading*` →
//!   [`Option<ReadingId>`].
//! * `readings`/`deleted`/`delayed`/`ignored` are `ReadingList` →
//!   `Vec<ReadingId>` (see [`crate::reading::ReadingList`]).
//! * Already-id `uint32` fields (`global_number`, `local_number`, `enclosed`,
//!   `dep_self`, `dep_parent`, `is_pleft`, `is_pright`, `line_number`) and the
//!   dependency/relation keys stay `u32`.
//! * `possible_sets` is `boost::dynamic_bitset<>` → [`flags_t`].
//!
//! Container substitutions:
//! * `bc::flat_map` (boost ordered, sorted-by-key flat map) → [`BTreeMap`]
//!   (same key ordering) for `RelationCtn` and `num_t`.
//! * `std::unordered_map` → [`std::collections::HashMap`] for
//!   `uint32ToCohortsMap`.

use std::collections::{BTreeMap, HashMap};

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::grammar::Grammar;
use crate::inlines::{NUMERIC_MAX, NUMERIC_MIN, ui32};
use crate::reading::{Reading, ReadingList, alloc_reading, alloc_reading_copy, free_reading};
use crate::sorted_vector::{sorted_vector, uint32SortedVector};
use crate::store::RuntimeStore;
use crate::types::{GlobalNumber, UString, flags_t};
use crate::window::{CohortRegistry, DepBookkeeping};

// Cohort `type` bit flags (C++ anonymous enum, OR'd into the `uint8_t type`
// field — kept as `u8` constants to match that field's width). No spec def id.
bitflags::bitflags! {
    /// C++ `enum` of `CT_*` cohort-type bit flags over `uint8_t` (wave 4:
    /// a typed `bitflags` set instead of a bare `u8`).
    #[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
    pub struct CohortType: u8 {
        const ENCLOSED = 1 << 0;
        const RELATED = 1 << 1;
        const REMOVED = 1 << 2;
        const NUM_CURRENT = 1 << 3;
        const DEP_DONE = 1 << 4;
        const AP_UNKNOWN = 1 << 5;
        const IGNORED = 1 << 6;
    }
}

// The C++ constant names, kept so call sites read like the source.
pub const CT_ENCLOSED: CohortType = CohortType::ENCLOSED;
pub const CT_RELATED: CohortType = CohortType::RELATED;
pub const CT_REMOVED: CohortType = CohortType::REMOVED;
pub const CT_NUM_CURRENT: CohortType = CohortType::NUM_CURRENT;
pub const CT_DEP_DONE: CohortType = CohortType::DEP_DONE;
pub const CT_AP_UNKNOWN: CohortType = CohortType::AP_UNKNOWN;
pub const CT_IGNORED: CohortType = CohortType::IGNORED;

/// C++ `constexpr auto DEP_NO_PARENT = std::numeric_limits<uint32_t>::max()`.
/// C++ `DEP_NO_PARENT = UINT32_MAX` — the raw wire value of "no parent".
/// Wave 4: `Cohort::dep_parent` is `Option<u32>` (`None` = no parent); this
/// constant remains ONLY for the serialization boundaries that must write or
/// read the C++ sentinel byte-for-byte.
pub const DEP_NO_PARENT: u32 = u32::MAX;

// [spec:cg3:def:cohort.cg3.relation-ctn]
/// C++ `typedef bc::flat_map<uint32_t, uint32SortedVector> RelationCtn`.
/// Ordered flat map → [`BTreeMap`].
pub type RelationCtn = BTreeMap<u32, uint32SortedVector>;

// [spec:cg3:def:cohort.cg3.cohort-vector]
/// C++ `typedef std::vector<Cohort*> CohortVector` → `Vec<CohortId>`.
pub type CohortVector = Vec<CohortId>;

// [spec:cg3:def:cohort.cg3.cohort.num-t]
/// C++ `typedef bc::flat_map<uint32_t, double> num_t` (member typedef of
/// `Cohort`). Ordered flat map → [`BTreeMap`]; `double` → `f64`.
pub type num_t = BTreeMap<u32, f64>;

/// Comparator placeholder for [`CohortSet`] (C++ `struct compare_Cohort;`,
/// forward-declared, defined in `Cohort.cpp`). Body deferred — see report.
#[derive(Default, Clone, Copy)]
pub struct compare_Cohort;

// PLACEHOLDER `Comparator<CohortId>` so `CohortSet = sorted_vector<CohortId,
// compare_Cohort>` accessor methods (`size`/`at`/`find_n`/…) resolve. The REAL
// order is `less_Cohort` = (local_number, then owning-window number), which
// needs the runtime store to resolve a `CohortId` (see
// `crate::single_window::less_cohort`). This stateless placeholder orders by
// raw `CohortId`. CORRECTNESS: engine code that sorts/searches a CohortSet must
// use the store-aware helpers (single_window::less_cohort); binary-search via
// this trait is WRONG — a Wave-3 test-verified reconciliation item.
impl crate::sorted_vector::Comparator<CohortId> for compare_Cohort {
    fn comp(&self, a: &CohortId, b: &CohortId) -> bool {
        a.0 < b.0
    }
}

// [spec:cg3:def:cohort.cg3.cohort-set]
/// C++ `typedef sorted_vector<Cohort*, compare_Cohort> CohortSet`.
pub type CohortSet = sorted_vector<CohortId, compare_Cohort>;

// [spec:cg3:def:cohort.cg3.uint32-to-cohorts-map]
/// C++ `typedef std::unordered_map<uint32_t, CohortSet> uint32ToCohortsMap`.
/// `std::unordered_map` → [`std::collections::HashMap`].
pub type uint32ToCohortsMap = HashMap<u32, CohortSet>;

// [spec:cg3:def:cohort.cg3.cohort]
/// A cohort: one surface token with its competing [`Reading`](crate::reading::Reading)s.
pub struct Cohort {
    /// C++ `uint8_t type` (Rust keyword → `r#type`); holds the `CT_*` bit flags.
    pub r#type: CohortType,
    // ToDo (C++): Get rid of global_number in favour of Cohort* relations
    pub global_number: GlobalNumber,
    pub local_number: u32,
    pub enclosed: u32,
    /// C++ `Tag* wordform = nullptr`.
    pub wordform: Option<TagId>,
    /// Wave 4: the cohort's own dependency number (C++ `uint32_t dep_self = 0`).
    /// `None` = unset (the C++ `0` sentinel); the wire boundary maps `0`↔`None`.
    pub dep_self: Option<GlobalNumber>,
    /// C++ `dep_parent` (`DEP_NO_PARENT` = no parent). `None` = no parent.
    pub dep_parent: Option<GlobalNumber>,
    pub is_pleft: u32,
    pub is_pright: u32,
    /// C++ `SingleWindow* parent = nullptr`.
    pub parent: Option<SwId>,
    pub text: UString,
    pub wblank: UString,
    /// C++ `Cohort* prev = nullptr` — sibling chain.
    pub prev: Option<CohortId>,
    /// C++ `Cohort* next = nullptr` — sibling chain.
    pub next: Option<CohortId>,
    /// C++ `Reading* wread = nullptr`.
    pub wread: Option<ReadingId>,
    pub readings: ReadingList,
    pub deleted: ReadingList,
    pub delayed: ReadingList,
    pub ignored: ReadingList,
    pub num_max: num_t,
    pub num_min: num_t,
    pub dep_children: uint32SortedVector,
    /// C++ `boost::dynamic_bitset<> possible_sets`.
    pub possible_sets: flags_t,
    pub relations: RelationCtn,
    pub relations_input: RelationCtn,
    pub line_number: u32,
}

impl Default for Cohort {
    /// Mirrors the C++ in-class member initializers (notably
    /// `dep_parent = DEP_NO_PARENT`; all others zero/null/empty). The C++
    /// constructor additionally sets `parent = p`; that wiring is a later pass.
    fn default() -> Self {
        Cohort {
            r#type: CohortType::empty(),
            global_number: GlobalNumber(0),
            local_number: 0,
            enclosed: 0,
            wordform: None,
            dep_self: None,
            dep_parent: None,
            is_pleft: 0,
            is_pright: 0,
            parent: None,
            text: UString::new(),
            wblank: UString::new(),
            prev: None,
            next: None,
            wread: None,
            readings: ReadingList::new(),
            deleted: ReadingList::new(),
            delayed: ReadingList::new(),
            ignored: ReadingList::new(),
            num_max: num_t::new(),
            num_min: num_t::new(),
            dep_children: uint32SortedVector::new(),
            possible_sets: flags_t::new(),
            relations: RelationCtn::new(),
            relations_input: RelationCtn::new(),
            line_number: 0,
        }
    }
}

// ===========================================================================
// Ported method/function bodies (Cohort.cpp / Cohort.hpp).
//
// ARENA-MODEL / SIGNATURE CONVENTION
// * C++ `Cohort*` values live in the runtime `pool<Cohort>` (`pool_cohorts`);
//   here they live in `RuntimeStore.cohorts` (an `Arena<Cohort>`). A method that
//   only touches the cohort's OWN scalar/container fields (`add_child`,
//   `rem_child`, `add_relation`, `set_relation`, `rem_relation`) stays an
//   `impl Cohort { fn(&mut self) }`. A method that touches OTHER arena objects —
//   the sibling `prev`/`next` cohorts, the member reading lists (each element a
//   `Reading` in the readings arena), a `Tag` in the grammar arena, or the owning
//   `Window` — becomes a STORE-TAKING FREE FN. Those free fns take
//   `store: &mut RuntimeStore` (+ `grammar: &Grammar` when a `Tag` hash/value is
//   needed) + `this: CohortId`, and destructure the store
//   (`let RuntimeStore { cohorts, readings, .. } = store;`) to hold two arenas at
//   once with short borrows.
// * The owning document window is NOT an arena type and is not held by
//   `RuntimeStore` (it belongs to the engine layer). CG-3 reaches it as
//   `cohort->parent` (a `SingleWindow*`) `->parent` (a `Window*`) to erase the
//   cohort's `global_number` from `cohort_map`/`dep_window`. Since neither the
//   store nor the `u32` `SingleWindow::parent` handle can resolve the window, the
//   three fns that perform that erase (`cohort_clear`, `cohort_dtor`,
//   `free_cohort`) take an extra `window: Option<(&mut CohortRegistry, &mut
//   DepBookkeeping)>` supplied by the engine layer (Stage-B: the C++ `Window` was
//   dissolved into these views; `cohort_map` lives on `CohortRegistry`,
//   `dep_window` on `DepBookkeeping`) — MY CONVENTION for "the window/engine
//   layer calls these". The map erase happens only when the C++ null-checks pass
//   AND the caller supplied the views.

/// Frees every `Reading` id in `ids` back to the reading pool (each via
/// [`free_reading`]). NOT a manifest symbol — the shared inner loop of the C++
/// `for (auto iter : list) { free_reading(iter); }` blocks in `~Cohort`/`clear`.
fn free_reading_list(store: &mut RuntimeStore, ids: &[ReadingId]) {
    for &rid in ids {
        let opt = Some(rid);
        free_reading(store, opt);
    }
}

// [spec:cg3:def:cohort.cg3.alloc-cohort-fn]
// [spec:cg3:sem:cohort.cg3.alloc-cohort-fn]
/// C++ `Cohort* alloc_cohort(SingleWindow* p)` (which also subsumes the
/// constructor `Cohort(SingleWindow* p) : parent(p)`).
///
/// `pool_cohorts.get()` either returns a fresh `new Cohort(p)` (parent set, all
/// else in-class default) or a recycled cohort already reset by `clear()` whose
/// `parent` is reassigned to `p`. BOTH branches yield a cohort with `parent = p`
/// and every other field at its default, so — unlike `alloc_reading_copy` — there
/// is NO pooled-vs-new divergence: a single fresh `Cohort { parent: p, ..default }`
/// placed via `store.cohorts.alloc` is exact. (Consequence of the arena model:
/// `alloc` overwrites the reused slot with this fresh value, so the `clear()`
/// `ignored`-not-cleared quirk below cannot leak into a *reused* cohort here.)
///
/// `p` is `Option<SwId>` (not a bare `SwId`) to preserve the nullable
/// `SingleWindow*`.
pub fn alloc_cohort(store: &mut RuntimeStore, p: Option<SwId>) -> CohortId {
    let c = Cohort {
        parent: p,
        ..Default::default()
    };
    CohortId(store.cohorts.alloc(c))
}

// [spec:cg3:def:cohort.cg3.free-cohort-fn]
// [spec:cg3:sem:cohort.cg3.free-cohort-fn]
/// C++ `void free_cohort(Cohort*& c)`.
///
/// Returns the cohort to the pool. If `c` is `None`, returns immediately.
/// Otherwise mirrors `pool_cohorts.put(c)` — which invokes `c->clear()`
/// ([`cohort_clear`], resetting the cohort, freeing its readings, unlinking it
/// from the Window maps and sibling chain) — then returns the arena slot to
/// the free-list. (The C++ `Cohort*&` null-out is ownership by value here —
/// wave 4; a caller keeping a long-lived handle sets it `None` itself.)
pub fn free_cohort(
    store: &mut RuntimeStore,
    window: Option<(&mut CohortRegistry, &mut DepBookkeeping)>,
    c: Option<CohortId>,
) {
    let Some(id) = c else { return };
    cohort_clear(store, window, id);
    store.cohorts.free_slot(id.0);
}

// [spec:cg3:def:cohort.cg3.cohort.cohort-fn]
// [spec:cg3:sem:cohort.cg3.cohort.cohort-fn]
/// C++ destructor `Cohort::~Cohort()`.
///
/// Frees every owned reading in order (`readings`, `deleted`, `delayed`,
/// `ignored`, then `wread`); then, if `parent` is non-null, erases this cohort's
/// `global_number` from the owning Window's `cohort_map` and `dep_window` —
/// reached in C++ as `parent->parent` and, UNLIKE [`cohort_clear`], WITHOUT
/// null-checking `parent->parent` (the DTOR ASYMMETRY, reproduced: here that means
/// the erase is gated only on the cohort's own `parent`, not on the
/// SingleWindow's `parent` handle). Finally detaches from the sibling chain. Does
/// NOT reset the scalar fields (the object is being destroyed).
pub fn cohort_dtor(
    store: &mut RuntimeStore,
    window: Option<(&mut CohortRegistry, &mut DepBookkeeping)>,
    this: CohortId,
) {
    let (rd, del, dly, ign, wr) = {
        let c = store.cohorts.get(this.0);
        (
            c.readings.clone(),
            c.deleted.clone(),
            c.delayed.clone(),
            c.ignored.clone(),
            c.wread,
        )
    };
    free_reading_list(store, &rd);
    free_reading_list(store, &del);
    free_reading_list(store, &dly);
    free_reading_list(store, &ign);
    free_reading(store, wr);
    store.cohorts.get_mut(this.0).wread = None; // free_reading(wread) nulls the member

    if store.cohorts.get(this.0).parent.is_some()
        && let Some((registry, deps)) = window
    {
        let gn = store.cohorts.get(this.0).global_number;
        registry.cohort_map.remove(&gn);
        deps.dep_window.remove(&gn);
    }
    detach(store, this);
}

// [spec:cg3:def:cohort.cg3.cohort.clear-fn]
// [spec:cg3:sem:cohort.cg3.cohort.clear-fn]
/// C++ `void Cohort::clear()` — resets the cohort so it can be reused from the
/// pool.
///
/// If BOTH `parent` (the cohort's `SingleWindow`) AND `parent->parent` (that
/// SingleWindow's `Window` handle) are non-null, erases this cohort's
/// `global_number` from the Window's `cohort_map` and `dep_window` (only actually
/// performed when the engine supplied `window`). `global_number` is read for the
/// erase BEFORE it is zeroed — ordering preserved. Then detaches, resets the
/// scalars/containers, frees every reading (`readings`, `deleted`, `delayed`,
/// `ignored`, `wread`), and finally clears the `readings`/`deleted`/`delayed`
/// lists and nulls `wread`.
///
/// QUIRK (reproduced, NOT fixed): the `ignored` list is NOT cleared after its
/// readings are freed, so it is left holding dangling ids to freed/recycled
/// readings.
pub fn cohort_clear(
    store: &mut RuntimeStore,
    window: Option<(&mut CohortRegistry, &mut DepBookkeeping)>,
    this: CohortId,
) {
    // if (parent && parent->parent) { cohort_map.erase(gn); dep_window.erase(gn); }
    let sw = store.cohorts.get(this.0).parent;
    let sw_has_parent = match sw {
        Some(sw_id) => store.single_windows.get(sw_id.0).parent.is_some(),
        None => false,
    };
    if sw.is_some()
        && sw_has_parent
        && let Some((registry, deps)) = window
    {
        let gn = store.cohorts.get(this.0).global_number;
        registry.cohort_map.remove(&gn);
        deps.dep_window.remove(&gn);
    }
    detach(store, this);

    {
        let c = store.cohorts.get_mut(this.0);
        c.r#type = CohortType::empty();
        c.global_number = GlobalNumber(0);
        c.local_number = 0;
        c.enclosed = 0;
        c.wordform = None;
        c.dep_self = None;
        c.dep_parent = None;
        c.is_pleft = 0;
        c.is_pright = 0;
        c.parent = None;
        c.line_number = 0;

        c.text.clear();
        c.wblank.clear();
        c.num_max.clear();
        c.num_min.clear();
        c.dep_children.clear();
        c.possible_sets.clear();
        c.relations.clear();
        c.relations_input.clear();
    }

    let (rd, del, dly, ign, wr) = {
        let c = store.cohorts.get(this.0);
        (
            c.readings.clone(),
            c.deleted.clone(),
            c.delayed.clone(),
            c.ignored.clone(),
            c.wread,
        )
    };
    free_reading_list(store, &rd);
    free_reading_list(store, &del);
    free_reading_list(store, &dly);
    free_reading_list(store, &ign);
    free_reading(store, wr);

    let c = store.cohorts.get_mut(this.0);
    c.readings.clear();
    c.deleted.clear();
    c.delayed.clear();
    c.wread = None;
    // QUIRK: `ignored` is deliberately NOT cleared here (bug-for-bug).
}

// [spec:cg3:def:cohort.cg3.cohort.detach-fn]
// [spec:cg3:sem:cohort.cg3.cohort.detach-fn]
/// C++ `void Cohort::detach()` — unlinks this cohort from the doubly-linked
/// sibling chain. STORE-TAKING FREE FN (it writes the `next`/`prev` fields of the
/// sibling cohorts). Does not modify `parent` or any SingleWindow cohort vector.
pub fn detach(store: &mut RuntimeStore, this: CohortId) {
    let (prev, next) = {
        let c = store.cohorts.get(this.0);
        (c.prev, c.next)
    };
    if let Some(prev_id) = prev {
        store.cohorts.get_mut(prev_id.0).next = next;
    }
    if let Some(next_id) = next {
        store.cohorts.get_mut(next_id.0).prev = prev;
    }
    let c = store.cohorts.get_mut(this.0);
    c.prev = None;
    c.next = None;
}

// [spec:cg3:def:cohort.cg3.cohort.append-reading-fn]
// [spec:cg3:sem:cohort.cg3.cohort.append-reading-fn]
/// C++ `void Cohort::appendReading(Reading* read, ReadingList& readings)` — the
/// 2-arg overload whose `ReadingList& readings` parameter shadows the member,
/// letting it target ANY reading list. STORE-TAKING FREE FN (it reads/writes the
/// appended `Reading`'s `number`). `list` is the external target list (e.g. the
/// staging `ReadingList` at `GrammarApplicator_runGrammar.cpp:416`).
pub fn append_reading_to(
    store: &mut RuntimeStore,
    this: CohortId,
    read: ReadingId,
    list: &mut ReadingList,
) {
    list.push(read);
    let sz = list.len();
    if store.readings.get(read.0).number == 0 {
        store.readings.get_mut(read.0).number = ui32(sz.wrapping_mul(1000).wrapping_add(1000));
    }
    store.cohorts.get_mut(this.0).r#type &= !CT_NUM_CURRENT;
}

/// C++ 1-arg overload `void Cohort::appendReading(Reading* read)` — forwards to
/// the 2-arg form with the member `readings` list. Reproduced directly (rather
/// than delegating) because the member list lives inside the `cohorts` arena, so
/// the store is split to touch the cohort's `readings` field and the appended
/// `Reading` at once.
pub fn append_reading(store: &mut RuntimeStore, this: CohortId, read: ReadingId) {
    let RuntimeStore {
        cohorts, readings, ..
    } = store;
    let cohort = cohorts.get_mut(this.0);
    cohort.readings.push(read);
    let sz = cohort.readings.len();
    if readings.get(read.0).number == 0 {
        readings.get_mut(read.0).number = ui32(sz.wrapping_mul(1000).wrapping_add(1000));
    }
    cohort.r#type &= !CT_NUM_CURRENT;
}

// [spec:cg3:def:cohort.cg3.cohort.allocate-append-reading-fn]
// [spec:cg3:sem:cohort.cg3.cohort.allocate-append-reading-fn]
/// C++ `Reading* Cohort::allocateAppendReading()` — allocates a fresh reading
/// owned by this cohort (`alloc_reading(this)`), push_back's it onto the member
/// `readings`, sets `number` from the post-push size ONLY when it is still 0
/// (dead in practice: `alloc_reading(this)` already stages a non-zero `number`
/// from the pre-push count — reproduced literally), and clears `CT_NUM_CURRENT`.
pub fn allocate_append_reading(store: &mut RuntimeStore, this: CohortId) -> ReadingId {
    let read = alloc_reading(store, Some(this));
    let RuntimeStore {
        cohorts, readings, ..
    } = store;
    let cohort = cohorts.get_mut(this.0);
    cohort.readings.push(read);
    let sz = cohort.readings.len();
    if readings.get(read.0).number == 0 {
        readings.get_mut(read.0).number = ui32(sz.wrapping_mul(1000).wrapping_add(1000));
    }
    cohort.r#type &= !CT_NUM_CURRENT;
    read
}

/// C++ sibling overload `Reading* Cohort::allocateAppendReading(Reading& r)` —
/// identical to [`allocate_append_reading`] except the new reading is seeded via
/// `alloc_reading(r)` (a copy of `r`).
pub fn allocate_append_reading_copy(
    store: &mut RuntimeStore,
    this: CohortId,
    r: &Reading,
) -> ReadingId {
    let read = alloc_reading_copy(store, r);
    let RuntimeStore {
        cohorts, readings, ..
    } = store;
    let cohort = cohorts.get_mut(this.0);
    cohort.readings.push(read);
    let sz = cohort.readings.len();
    if readings.get(read.0).number == 0 {
        readings.get_mut(read.0).number = ui32(sz.wrapping_mul(1000).wrapping_add(1000));
    }
    cohort.r#type &= !CT_NUM_CURRENT;
    read
}

// [spec:cg3:def:cohort.cg3.cohort.update-min-max-fn]
// [spec:cg3:sem:cohort.cg3.cohort.update-min-max-fn]
/// C++ (private) `void Cohort::updateMinMax()` — recomputes the per-
/// comparison-hash numeric min/max cache. STORE + GRAMMAR TAKING FREE FN: it
/// reads each reading's `tags_numerical` (readings arena) whose values are `Tag`s
/// (grammar arena) for `comparison_hash`/`comparison_val`.
///
/// Returns immediately when `CT_NUM_CURRENT` is set. Otherwise clears both maps,
/// then over ONLY the `readings` list stores, per `tag->comparison_hash`, the
/// strict min into `num_min` and the strict max into `num_max` (the C++
/// `find(...) == end() || val < map[...]` short-circuit means an absent key is
/// simply inserted, never default-read). Sets `CT_NUM_CURRENT` at the end.
pub fn update_min_max(store: &mut RuntimeStore, grammar: &Grammar, this: CohortId) {
    let RuntimeStore {
        cohorts, readings, ..
    } = store;
    if cohorts.get(this.0).r#type.intersects(CT_NUM_CURRENT) {
        return;
    }
    {
        let c = cohorts.get_mut(this.0);
        c.num_min.clear();
        c.num_max.clear();
    }
    let reading_ids: Vec<ReadingId> = cohorts.get(this.0).readings.clone();
    for rid in reading_ids {
        let tags_numerical = &readings.get(rid.0).tags_numerical;
        for &tid in tags_numerical.values() {
            let tag = grammar.single_tags_list.get(tid.0);
            let ch = tag.comparison_hash;
            let cv = tag.comparison_val;
            let c = cohorts.get_mut(this.0);
            if !c.num_min.contains_key(&ch) || cv < c.num_min[&ch] {
                c.num_min.insert(ch, cv);
            }
            if !c.num_max.contains_key(&ch) || cv > c.num_max[&ch] {
                c.num_max.insert(ch, cv);
            }
        }
    }
    cohorts.get_mut(this.0).r#type |= CT_NUM_CURRENT;
}

// [spec:cg3:def:cohort.cg3.cohort.get-min-fn]
// [spec:cg3:sem:cohort.cg3.cohort.get-min-fn]
/// C++ `double Cohort::getMin(uint32_t key)` — refreshes the cache via
/// [`update_min_max`] then returns `num_min[key]` if present, else
/// [`NUMERIC_MIN`]. Free fn (it calls the store+grammar-taking `update_min_max`).
pub fn get_min(store: &mut RuntimeStore, grammar: &Grammar, this: CohortId, key: u32) -> f64 {
    update_min_max(store, grammar, this);
    match store.cohorts.get(this.0).num_min.get(&key) {
        Some(&v) => v,
        None => NUMERIC_MIN,
    }
}

// [spec:cg3:def:cohort.cg3.cohort.get-max-fn]
// [spec:cg3:sem:cohort.cg3.cohort.get-max-fn]
/// C++ `double Cohort::getMax(uint32_t key)` — refreshes the cache via
/// [`update_min_max`] then returns `num_max[key]` if present, else
/// [`NUMERIC_MAX`].
pub fn get_max(store: &mut RuntimeStore, grammar: &Grammar, this: CohortId, key: u32) -> f64 {
    update_min_max(store, grammar, this);
    match store.cohorts.get(this.0).num_max.get(&key) {
        Some(&v) => v,
        None => NUMERIC_MAX,
    }
}

// [spec:cg3:def:cohort.cg3.cohort.set-related-fn]
// [spec:cg3:sem:cohort.cg3.cohort.set-related-fn]
/// C++ `void Cohort::setRelated()` — sets `CT_RELATED` then forces `noprint =
/// false` on every reading in `readings`. STORE-TAKING FREE FN (it writes each
/// `Reading`).
pub fn set_related(store: &mut RuntimeStore, this: CohortId) {
    let RuntimeStore {
        cohorts, readings, ..
    } = store;
    let cohort = cohorts.get_mut(this.0);
    cohort.r#type |= CT_RELATED;
    for &rid in cohort.readings.iter() {
        readings.get_mut(rid.0).noprint = false;
    }
}

// [spec:cg3:def:cohort.cg3.cohort.unignore-all-fn]
// [spec:cg3:sem:cohort.cg3.cohort.unignore-all-fn]
/// C++ (header inline) `void Cohort::unignoreAll()` — when `ignored` is non-empty,
/// clears `deleted` on each ignored reading, appends the whole `ignored` list onto
/// the end of `readings`, then clears `ignored`. No-op when `ignored` is empty.
/// STORE-TAKING FREE FN (it writes each ignored `Reading`).
pub fn unignore_all(store: &mut RuntimeStore, this: CohortId) {
    let RuntimeStore {
        cohorts, readings, ..
    } = store;
    let cohort = cohorts.get_mut(this.0);
    if !cohort.ignored.is_empty() {
        for &rid in cohort.ignored.iter() {
            readings.get_mut(rid.0).deleted = false;
        }
        cohort.readings.extend(cohort.ignored.iter().copied());
        cohort.ignored.clear();
    }
}

impl Cohort {
    // [spec:cg3:def:cohort.cg3.cohort.add-child-fn]
    // [spec:cg3:sem:cohort.cg3.cohort.add-child-fn]
    /// C++ `void Cohort::addChild(uint32_t child)` — inserts `child` into the
    /// `dep_children` sorted set (dedup no-op if present; return value discarded).
    /// Own-field only → `&mut self`.
    pub fn add_child(&mut self, child: u32) {
        self.dep_children.insert(child);
    }

    // [spec:cg3:def:cohort.cg3.cohort.rem-child-fn]
    // [spec:cg3:sem:cohort.cg3.cohort.rem-child-fn]
    /// C++ `void Cohort::remChild(uint32_t child)` — erases `child` from
    /// `dep_children` (no-op if absent). Own-field only → `&mut self`.
    pub fn rem_child(&mut self, child: u32) {
        self.dep_children.erase(child);
    }

    // [spec:cg3:def:cohort.cg3.cohort.add-relation-fn]
    // [spec:cg3:sem:cohort.cg3.cohort.add-relation-fn]
    /// C++ `bool Cohort::addRelation(uint32_t rel, uint32_t cohort)` — adds target
    /// `cohort` under relation hash `rel` (default-creating the set), returning
    /// true iff it grew (newly added). Additive. Own-field only → `&mut self`.
    pub fn add_relation(&mut self, rel: u32, cohort: u32) -> bool {
        let cohorts = self.relations.entry(rel).or_default();
        let sz = cohorts.size();
        cohorts.insert(cohort);
        sz != cohorts.size()
    }

    // [spec:cg3:def:cohort.cg3.cohort.set-relation-fn]
    // [spec:cg3:sem:cohort.cg3.cohort.set-relation-fn]
    /// C++ `bool Cohort::setRelation(uint32_t rel, uint32_t cohort)` — makes `rel`
    /// a single-target relation pointing only at `cohort`. Erases `rel` from
    /// `relations_input`; if `relations[rel]` is already exactly `{cohort}` returns
    /// false; else clears it, inserts `cohort`, returns true. Own-fields only →
    /// `&mut self`.
    pub fn set_relation(&mut self, rel: u32, cohort: u32) -> bool {
        self.relations_input.remove(&rel);
        let cohorts = self.relations.entry(rel).or_default();
        if cohorts.size() == 1 && cohorts.find(cohort) != cohorts.end() {
            return false;
        }
        cohorts.clear();
        cohorts.insert(cohort);
        true
    }

    // [spec:cg3:def:cohort.cg3.cohort.rem-relation-fn]
    // [spec:cg3:sem:cohort.cg3.cohort.rem-relation-fn]
    /// C++ `bool Cohort::remRelation(uint32_t rel, uint32_t cohort)` — removes
    /// target `cohort` from relation `rel` (false if `rel` absent), also erasing it
    /// from `relations_input[rel]` when that key exists; returns true iff
    /// `relations[rel]` shrank. Own-fields only → `&mut self`.
    pub fn rem_relation(&mut self, rel: u32, cohort: u32) -> bool {
        if let Some(rels) = self.relations.get_mut(&rel) {
            let sz = rels.size();
            rels.erase(cohort);
            if let Some(rels_in) = self.relations_input.get_mut(&rel) {
                rels_in.erase(cohort);
            }
            return sz != rels.size();
        }
        false
    }
}
