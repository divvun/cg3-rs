//! Types ported from `src/SingleWindow.hpp` — the `SingleWindow` (one sentence
//! worth of cohorts inside a [`Window`](crate::window::Window)) plus the
//! `compare_Cohort` ordering functor.
//!
//! Arena model: `Cohort*` → [`CohortId`], `SingleWindow*` → [`SwId`]. The
//! `Window* parent` back-reference has no arena id (a `Window` is a singleton
//! owned by the engine), so it is kept as a raw `Option<u32>` handle
//! placeholder. `CohortVector` (`std::vector<Cohort*>`) → `Vec<CohortId>`, and
//! `CohortSet` (`sorted_vector<Cohort*, compare_Cohort>`) → `Vec<CohortId>`
//! (the compare_Cohort ordering needs the runtime store and is applied by the
//! engine layer).

use crate::arena::{CohortId, SwId};
use crate::flat_unordered_map::Uint32FlatHashMap;
use crate::flat_unordered_set::Uint32FlatHashSet;
use crate::inlines::ui32;
use crate::interval_vector::uint32IntervalVector;
use crate::sorted_vector::uint32SortedVector;
use crate::store::RuntimeStore;
use crate::types::UString;
use crate::window::Window;

// [spec:cg3:def:single-window.cg3.single-window]
/// C++ `class SingleWindow`: an ordered run of cohorts (one "window"/sentence)
/// belonging to a parent [`Window`](crate::window::Window).
#[derive(Default)]
pub struct SingleWindow {
    pub number: u32,
    pub has_enclosures: bool,
    pub flush_after: bool,
    /// C++ `SingleWindow* next`.
    pub next: Option<SwId>,
    /// C++ `SingleWindow* previous`.
    pub previous: Option<SwId>,
    /// C++ `Window* parent` — back-reference to the owning `Window`, which is
    /// not an arena type (no id exists). Raw handle placeholder wired later.
    pub parent: Option<u32>,
    pub text: UString,
    pub text_post: UString,
    /// C++ `CohortVector all_cohorts` (`std::vector<Cohort*>`).
    pub all_cohorts: Vec<CohortId>,
    /// C++ `CohortVector cohorts` (`std::vector<Cohort*>`).
    pub cohorts: Vec<CohortId>,
    pub valid_rules: uint32IntervalVector,
    pub hit_external: uint32SortedVector,
    /// C++ `std::vector<CohortSet> rule_to_cohorts`; each `CohortSet`
    /// (`sorted_vector<Cohort*, compare_Cohort>`).
    pub rule_to_cohorts: Vec<crate::cohort::CohortSet>,
    /// C++ `std::unique_ptr<CohortSet> nested_rule_to_cohorts` — a nullable,
    /// heap-owned `CohortSet`.
    pub nested_rule_to_cohorts: Option<Box<crate::cohort::CohortSet>>,
    /// C++ `uint32FlatHashMap variables_set` (u32 → u32).
    pub variables_set: Uint32FlatHashMap,
    /// C++ `uint32FlatHashSet variables_rem`.
    pub variables_rem: Uint32FlatHashSet,
    pub variables_output: uint32SortedVector,
    /// C++ `Reading bag_of_tags` — an embedded (by-value) `Reading`.
    /// Cross-concern: resolves once the `reading` module lands.
    pub bag_of_tags: crate::reading::Reading,
}

// [spec:cg3:def:single-window.cg3.compare-cohort]
/// C++ `struct compare_Cohort` — the strict-weak `Cohort*` ordering functor
/// (by `local_number`, tie-broken by owning single-window `number`). Its
/// `operator()` needs the runtime store to resolve a `CohortId`; see the
/// `call` method below.
#[derive(Default)]
pub struct compare_Cohort;

// ---------------------------------------------------------------------------
// Ported free functions + method bodies (SingleWindow.cpp).
//
// ARENA-MODEL NOTES
// * `SingleWindow*` values live in the runtime `pool<SingleWindow>`
//   (`pool_swindows`); here they live in `RuntimeStore.single_windows` (an
//   `Arena<SingleWindow>`). The pool's "get a pre-cleared object / else
//   allocate new" split maps onto `Arena::alloc` (reuse a freed slot / else
//   push a new slot); its `put` (which `clear()`s then stores) maps onto
//   `single_window_clear` + `Arena::free_slot`.
// * `SingleWindow::parent` is the owning `Window`, a per-applicator singleton
//   that is NOT in the store and has no arena id. Every C++ `parent->…` access
//   (the `relation_map` / `cohort_map` / `dep_window` bookkeeping) is therefore
//   threaded through an explicit `window: &mut Window` parameter rather than
//   resolved from the (placeholder) `parent` field. `parent->parent` is the
//   `GrammarApplicator` (`dep_highest_seen`), a placeholder — not threaded.
// * `less_Cohort` / `compare_Cohort::operator()` dereference two `Cohort*` and
//   their owning `SingleWindow*`, so they become store-taking free functions
//   (`&RuntimeStore` is enough — read-only).

// [spec:cg3:def:single-window.cg3.alloc-swindow-fn]
// [spec:cg3:sem:single-window.cg3.alloc-swindow-fn]
/// C++ free fn `SingleWindow* alloc_swindow(Window* p)`.
///
/// `pool_swindows.get()` returns either a pre-`clear()`-ed pooled object (whose
/// only non-blank field is set here: `parent = p`) or null, in which case a
/// `new SingleWindow(p)` (a blank object with `parent = p`) is made. Both
/// branches yield an otherwise-blank `SingleWindow` parented to `p`, so there is
/// NO pooled-vs-new divergence: writing a fresh `SingleWindow { parent: p, .. }`
/// into a reused-or-new arena slot is exact. `p` is the `Window` placeholder
/// handle (`Option<u32>`); the singleton `Window` has no arena id.
pub fn alloc_swindow(store: &mut RuntimeStore, p: Option<u32>) -> SwId {
    let sw = SingleWindow { parent: p, ..SingleWindow::default() };
    SwId(store.single_windows.alloc(sw))
}

// [spec:cg3:def:single-window.cg3.free-swindow-fn]
// [spec:cg3:sem:single-window.cg3.free-swindow-fn]
/// C++ free fn `void free_swindow(SingleWindow*& s)`.
///
/// If `s` is null, returns immediately. Otherwise mirrors `pool_swindows.put(s)`
/// — which calls `s->clear()` (the teardown + field reset in
/// [`single_window_clear`]) and stores the object for reuse — by clearing then
/// returning the slot to the arena free-list; freeing an already-freed slot is a
/// no-op, matching the pool's silently-ignored duplicate insert. Finally nulls
/// the caller's handle (`s = 0`). Needs the owning `window` because `clear`
/// prunes `window.relation_map`.
pub fn free_swindow(window: &mut Window, store: &mut RuntimeStore, s: &mut Option<SwId>) {
    let id = match *s {
        Some(id) => id,
        None => return,
    };
    single_window_clear(window, store, id);
    store.single_windows.free_slot(id.0);
    *s = None;
}

/// Shared teardown prologue — the identical body of `~SingleWindow()` and the
/// first half of `SingleWindow::clear()` (the C++ duplicates it verbatim). NOT a
/// manifest symbol — port infra factoring the duplication.
///
/// (1) If `cohorts.size() > 1`, prune `window.relation_map`: erase every entry
/// whose value (a global cohort number) is `<= cohorts.back()->global_number`.
/// The C++ iterate-and-`erase(iterator)` becomes collect-the-matching-keys then
/// `erase(key)` (the map port cannot both hold a const iterator and mutate); the
/// resulting live-entry set is identical. (2) Return every cohort in
/// `all_cohorts` to the pool. `free_cohort` is NOT yet ported (Cohort.cpp is
/// skeleton-only), so this stands in with `cohorts.free_slot`, which recycles
/// the cohort's arena slot but does not yet clear the cohort or free its
/// readings — see the crate report. (3) Splice this window out of the sibling
/// doubly-linked list.
fn single_window_teardown(window: &mut Window, store: &mut RuntimeStore, sw_id: SwId) {
    // (1) relation_map prune.
    if store.single_windows.get(sw_id.0).cohorts.len() > 1 {
        let back = *store.single_windows.get(sw_id.0).cohorts.last().unwrap();
        let threshold = store.cohorts.get(back.0).global_number;
        let mut to_erase: Vec<u32> = Vec::new();
        {
            let mut it = window.relation_map.begin();
            while it != window.relation_map.end() {
                let pair = *it.get();
                if pair.1 <= threshold {
                    to_erase.push(pair.0);
                }
                it.pre_increment();
            }
        }
        for k in to_erase {
            window.relation_map.erase(k);
        }
    }

    // (2) free_cohort(iter) for every cohort in all_cohorts. Must go through
    // free_cohort → cohort_clear so the cohort is erased from the Window's
    // cohort_map/dep_window — a bare free_slot leaves stale map entries that
    // later resolve dep links to freed slots (C++ Cohort::clear() erases them).
    let all = store.single_windows.get(sw_id.0).all_cohorts.clone();
    for iter in all {
        let mut h = Some(iter);
        crate::cohort::free_cohort(store, Some(&mut *window), &mut h);
    }

    // (3) Splice out of the sibling doubly-linked list.
    let next = store.single_windows.get(sw_id.0).next;
    let previous = store.single_windows.get(sw_id.0).previous;
    match (next, previous) {
        (Some(n), Some(p)) => {
            store.single_windows.get_mut(n.0).previous = Some(p);
            store.single_windows.get_mut(p.0).next = Some(n);
        }
        _ => {
            if let Some(n) = next {
                store.single_windows.get_mut(n.0).previous = None;
            }
            if let Some(p) = previous {
                store.single_windows.get_mut(p.0).next = None;
            }
        }
    }
}

// [spec:cg3:def:single-window.cg3.single-window.single-window-fn]
// [spec:cg3:sem:single-window.cg3.single-window.single-window-fn]
/// C++ destructor `SingleWindow::~SingleWindow()`.
///
/// Runs exactly the shared teardown ([`single_window_teardown`]): prune the
/// owning window's `relation_map`, recycle every cohort in `all_cohorts`, then
/// splice this window out of the sibling chain. (`CG_TRACE_OBJECTS` diagnostics
/// are compile-time only.) STORE + WINDOW taking free fn — a `Window` is not in
/// the store and Rust `Drop` cannot take those, so the destructor body is a
/// manual function (invoked explicitly by the engine layer).
pub fn single_window_destroy(window: &mut Window, store: &mut RuntimeStore, sw_id: SwId) {
    single_window_teardown(window, store, sw_id);
}

// [spec:cg3:def:single-window.cg3.single-window.clear-fn]
// [spec:cg3:sem:single-window.cg3.single-window.clear-fn]
/// C++ `void SingleWindow::clear()` — resets the single-window at `sw_id` to a
/// pristine, reusable state (used by the pool).
///
/// Performs the same teardown as the destructor ([`single_window_teardown`]),
/// then zeroes/clears every field. QUIRK (faithful): `nested_rule_to_cohorts`
/// (the `unique_ptr<CohortSet>`) is NOT reset here, so a stale nested set can
/// survive a `clear()` and be reused via the pool. The outer `rule_to_cohorts`
/// vector keeps its size/capacity — only the per-index sets are emptied.
pub fn single_window_clear(window: &mut Window, store: &mut RuntimeStore, sw_id: SwId) {
    single_window_teardown(window, store, sw_id);

    let sw = store.single_windows.get_mut(sw_id.0);
    sw.number = 0;
    sw.has_enclosures = false;
    sw.flush_after = false;
    sw.next = None;
    sw.previous = None;
    sw.parent = None;
    sw.text.clear();
    sw.text_post.clear();
    sw.cohorts.clear();
    sw.all_cohorts.clear();
    sw.valid_rules.clear();
    sw.hit_external.clear();
    for cs in &mut sw.rule_to_cohorts {
        cs.clear();
    }
    sw.variables_set.clear(0);
    sw.variables_rem.clear(0);
    sw.variables_output.clear();
    // bag_of_tags is an embedded Reading value (not an arena object); its C++
    // `clear()` resets it to blank. `reading_clear` needs an arena id, so the
    // value is reset to Default here (a bag-of-tags never holds a `next` chain).
    sw.bag_of_tags = crate::reading::Reading::default();
    // QUIRK: nested_rule_to_cohorts intentionally NOT reset.
}

// [spec:cg3:def:single-window.cg3.single-window.append-cohort-fn]
// [spec:cg3:sem:single-window.cg3.single-window.append-cohort-fn]
/// C++ `void SingleWindow::appendCohort(Cohort* cohort)` — appends `cohort_id`
/// as the new last cohort of the window `sw_id` and wires up every link.
///
/// STORE + WINDOW taking free fn: it touches `store.cohorts` (the cohort and its
/// siblings), `store.single_windows` (the previous/next windows), and the owning
/// `window`'s `cohort_map`/`dep_window`. The `if (cohort->dep_self)` branch sets
/// `parent->parent->dep_highest_seen` on the `GrammarApplicator` (a placeholder)
/// — not threaded.
pub fn append_cohort(
    window: &mut Window,
    store: &mut RuntimeStore,
    sw_id: SwId,
    cohort_id: CohortId,
) {
    let RuntimeStore { cohorts, single_windows, .. } = store;

    // cohort->local_number = UI32(cohorts.size()); cohort->parent = this;
    let local_number = ui32(single_windows.get(sw_id.0).cohorts.len());
    let global_number = {
        let c = cohorts.get_mut(cohort_id.0);
        c.local_number = local_number;
        c.parent = Some(sw_id);
        c.global_number
    };
    // if (cohort->dep_self) { parent->parent->dep_highest_seen = cohort->dep_self; }
    // parent->parent is the GrammarApplicator placeholder — not threaded.

    // Backward link.
    if single_windows.get(sw_id.0).cohorts.is_empty() {
        // if (previous && !previous->cohorts.empty())
        if let Some(prev_id) = single_windows.get(sw_id.0).previous {
            if let Some(pb) = single_windows.get(prev_id.0).cohorts.last().copied() {
                cohorts.get_mut(pb.0).next = Some(cohort_id);
                cohorts.get_mut(cohort_id.0).prev = Some(pb);
            }
        }
    } else {
        // cohort->prev = cohorts.back(); cohorts.back()->next = cohort;
        let back = *single_windows.get(sw_id.0).cohorts.last().unwrap();
        cohorts.get_mut(cohort_id.0).prev = Some(back);
        cohorts.get_mut(back.0).next = Some(cohort_id);
    }

    // Forward link: if (next && !next->cohorts.empty())
    if let Some(next_id) = single_windows.get(sw_id.0).next {
        if let Some(nf) = single_windows.get(next_id.0).cohorts.first().copied() {
            cohorts.get_mut(nf.0).prev = Some(cohort_id);
            cohorts.get_mut(cohort_id.0).next = Some(nf);
        }
    }

    // cohorts.push_back(cohort); all_cohorts.push_back(cohort);
    {
        let sw = single_windows.get_mut(sw_id.0);
        sw.cohorts.push(cohort_id);
        sw.all_cohorts.push(cohort_id);
    }

    // parent->cohort_map[global] = cohort; parent->dep_window[global] = cohort;
    window.cohort_map.insert(global_number, cohort_id);
    window.dep_window.insert(global_number, cohort_id);
    // if (cohort->local_number == 0) parent->cohort_map[0] = cohort;
    if local_number == 0 {
        window.cohort_map.insert(0, cohort_id);
    }
}

// [spec:cg3:def:single-window.cg3.less-cohort-fn]
// [spec:cg3:sem:single-window.cg3.less-cohort-fn]
/// C++ `inline bool less_Cohort(const Cohort* a, const Cohort* b)` — strict-less
/// comparator: if `local_number`s tie, order by the owning single-window's
/// `number`; otherwise order by `local_number`. STORE-TAKING free fn (read-only)
/// because it dereferences both cohorts and their parent single-windows.
pub fn less_cohort(store: &RuntimeStore, a: CohortId, b: CohortId) -> bool {
    let ca = store.cohorts.get(a.0);
    let cb = store.cohorts.get(b.0);
    if ca.local_number == cb.local_number {
        // a->parent->number < b->parent->number
        let an = store.single_windows.get(ca.parent.unwrap().0).number;
        let bn = store.single_windows.get(cb.parent.unwrap().0).number;
        return an < bn;
    }
    ca.local_number < cb.local_number
}

impl compare_Cohort {
    // [spec:cg3:def:single-window.cg3.compare-cohort.operator-fn]
    // [spec:cg3:sem:single-window.cg3.compare-cohort.operator-fn]
    /// C++ `bool operator()(const Cohort* a, const Cohort* b) const` — returns
    /// `less_Cohort(a, b)`. Takes the store to resolve the ids (the C++ functor
    /// is stateless).
    pub fn call(&self, store: &RuntimeStore, a: CohortId, b: CohortId) -> bool {
        less_cohort(store, a, b)
    }
}
