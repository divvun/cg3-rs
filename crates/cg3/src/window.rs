//! Types ported from `src/Window.hpp` — the document-level `Window` (the whole
//! stream of single-windows: history in `previous`, the active `current`, and
//! the pending `next`) plus its `SingleWindowCont` container typedef.
//!
//! STAGE-B DISSOLUTION. The single C++ `class Window` is split into three
//! cohesive Rust views held side-by-side on
//! [`Document`](crate::grammar_applicator::Document):
//!
//! * [`WindowStream`] — the ordered stream (`previous`/`current`/`next`) plus
//!   the window numbering (`window_counter`/`window_span`) and every stream
//!   method (`alloc_*`, `back`, `shuffle_windows_down`, `rebuild_*`, `destroy`).
//! * [`CohortRegistry`] — the global cohort numbering (`cohort_counter`) and the
//!   ordered `cohort_map`, plus `next_cohort_number()`.
//! * [`DepBookkeeping`] — the dependency / relation maps (`dep_map`,
//!   `dep_window`, `relation_map`) together with the doc-bucket dependency
//!   latches (`has_dep`, `has_relations`, `dep_highest_seen`) that moved in from
//!   `GrammarApplicator`.
//!
//! The members map 1:1 onto the former `Window` fields; the semantics are
//! unchanged — the split is purely a re-homing so the god object decomposes.
//!
//! Arena model: every `SingleWindow*` becomes a [`SwId`] and every `Cohort*`
//! becomes a [`CohortId`]. The C++ `GrammarApplicator* parent` back-reference is
//! not ported: the engine owns the views (on `Document`) and threads the store
//! into the methods that need it.

use std::collections::BTreeMap;

use crate::arena::{CohortId, SwId};
use crate::flat_unordered_map::Uint32FlatHashMap;
use crate::single_window::{alloc_swindow, free_swindow};
use crate::store::RuntimeStore;
use crate::types::GlobalNumber;

// [spec:cg3:def:window.cg3.single-window-cont]
/// C++ `typedef std::vector<SingleWindow*> SingleWindowCont`. Each
/// `SingleWindow*` becomes a [`SwId`] in the arena model.
pub type SingleWindowCont = Vec<SwId>;

// [spec:cg3:def:window.cg3.window]
/// C++ `class Window` — DISSOLVED (Stage-B) into three cohesive views. The
/// former single struct owned the ordered stream of single-windows for one
/// document plus the cohort/dependency/relation bookkeeping maps; those members
/// are now split 1:1 across [`WindowStream`] (the stream + numbering),
/// [`CohortRegistry`] (`cohort_counter` + `cohort_map`), and [`DepBookkeeping`]
/// (`dep_map`/`dep_window`/`relation_map` + the doc dependency latches). The
/// three views live side-by-side on
/// [`Document`](crate::grammar_applicator::Document); semantics are unchanged.
///
/// The stream half — C++ `SingleWindowCont previous`, `SingleWindow* current`,
/// `SingleWindowCont next`, plus `window_counter`/`window_span`.
#[derive(Default)]
pub struct WindowStream {
    // C++ `GrammarApplicator* parent` is not ported (dead back-pointer: the
    // engine owns the views and passes the store/registry/deps where needed).
    pub window_counter: u32,
    pub window_span: u32,

    /// C++ `SingleWindowCont previous` — the history stream.
    pub previous: SingleWindowCont,
    /// C++ `SingleWindow* current` — the active single-window.
    pub current: Option<SwId>,
    /// C++ `SingleWindowCont next` — the pending stream.
    pub next: SingleWindowCont,
}

// [spec:cg3:def:window.cg3.window]
/// The cohort-registry half of the dissolved C++ `Window` — the global cohort
/// numbering plus the ordered `cohort_map`.
#[derive(Default)]
pub struct CohortRegistry {
    pub cohort_counter: GlobalNumber,

    /// C++ `std::map<uint32_t, Cohort*> cohort_map` — ordered (global cohort
    /// number → cohort). `std::map` (a red-black tree) → `BTreeMap`.
    pub cohort_map: BTreeMap<GlobalNumber, CohortId>,
}

// [spec:cg3:def:window.cg3.window]
/// The dependency / relation half of the dissolved C++ `Window` — the dep/
/// relation maps plus the doc-bucket dependency latches that moved in from
/// `GrammarApplicator` (`has_dep`, `has_relations`, `dep_highest_seen`).
#[derive(Default)]
pub struct DepBookkeeping {
    /// C++ `uint32FlatHashMap dep_map` (u32 → u32).
    pub dep_map: Uint32FlatHashMap,
    /// C++ `std::map<uint32_t, Cohort*> dep_window` — ordered (global cohort
    /// number → cohort). `std::map` → `BTreeMap`.
    pub dep_window: BTreeMap<GlobalNumber, CohortId>,
    /// C++ `uint32FlatHashMap relation_map` (u32 → u32).
    pub relation_map: Uint32FlatHashMap,

    /// Doc latch (moved from `GrammarApplicator`): the document acquired
    /// dependency structure.
    pub has_dep: bool,
    /// Doc latch (moved from `GrammarApplicator`): the document acquired relation
    /// structure.
    pub has_relations: bool,
    /// Doc latch (moved from `GrammarApplicator`): highest dependency number seen
    /// so far (per-window reset).
    pub dep_highest_seen: GlobalNumber,
}

impl CohortRegistry {
    /// C++ `gWindow->cohort_counter++` — the post-increment idiom every stream
    /// applicator uses to number a fresh cohort: returns the current number and
    /// advances the counter (unsigned wrap, as in C++).
    pub fn next_cohort_number(&mut self) -> GlobalNumber {
        let n = self.cohort_counter;
        self.cohort_counter = self.cohort_counter.wrapping_add(1);
        n
    }
}

// ---------------------------------------------------------------------------
// Ported method bodies (Window.cpp).
//
// ARENA-MODEL NOTES
// * The dissolved `Window` views are per-applicator singletons (NOT in the
//   store), so the stream methods are `&self`/`&mut self` on [`WindowStream`]
//   and take `store: &mut RuntimeStore` to reach the pooled `SingleWindow`/
//   `Cohort` arenas. `SingleWindow*` → [`SwId`], `Cohort*` → [`CohortId`]. The
//   `free_swindow`-flavoured methods (`destroy`) additionally thread the
//   `&mut CohortRegistry` + `&mut DepBookkeeping` views that `free_swindow`
//   prunes.
// * `++window_counter` is reproduced with `wrapping_add` (unsigned overflow
//   wraps in C++). `next.front()`/`next.back()` → `next[0]`/`next.last()`;
//   `next.insert(next.begin(), …)` → `next.insert(0, …)`;
//   `next.erase(next.begin())` → `next.remove(0)`.
// * `parent->variables` (in `shuffleWindowsDown`) is a `GrammarApplicator`
//   member — the variable snapshot is not threaded (see task brief).

impl WindowStream {
    // C++ `Window::Window(GrammarApplicator* p) : parent(p)` — the back-pointer
    // is not ported, so construction is just `WindowStream::default()`.

    // [spec:cg3:def:window.cg3.window.window-fn]
    // [spec:cg3:sem:window.cg3.window.window-fn]
    /// C++ destructor `Window::~Window()`. Recycles every single-window:
    /// `free_swindow` on each of `previous`, then `current`, then `next`. The
    /// loops iterate by value, so the (now-stale) ids inside `previous`/`next`
    /// are left in place (mirroring C++, where the vectors die right after);
    /// `current` is passed by reference and nulled. STORE + REGISTRY + DEPS free
    /// fn — Rust `Drop` cannot take those, so this is invoked explicitly.
    ///
    /// V-NOTE (Stage-B): the receiver is now [`WindowStream`], not `Window`; the
    /// `free_swindow` map-pruning targets (`cohort_map`/`dep_window`/
    /// `relation_map`) are threaded as the `cohorts`/`deps` view params. Behavior
    /// unchanged.
    pub fn destroy(
        &mut self,
        store: &mut RuntimeStore,
        cohorts: &mut CohortRegistry,
        deps: &mut DepBookkeeping,
    ) {
        // Index loops: `free_swindow` prunes the maps (via `cohorts`/`deps`) but
        // never touches the `previous`/`next` lists themselves.
        for i in 0..self.previous.len() {
            let iter = self.previous[i];
            free_swindow(store, cohorts, deps, Some(iter));
        }
        let current = self.current;
        free_swindow(store, cohorts, deps, current);
        self.current = None; // C++ passes `current` by reference: nulled.
        for i in 0..self.next.len() {
            let iter = self.next[i];
            free_swindow(store, cohorts, deps, Some(iter));
        }
    }

    // [spec:cg3:def:window.cg3.window.alloc-single-window-fn]
    // [spec:cg3:sem:window.cg3.window.alloc-single-window-fn]
    /// C++ `SingleWindow* Window::allocSingleWindow()`. A bare allocation:
    /// `alloc_swindow(this)`, `++window_counter`, `swindow->number = counter`;
    /// no stream insertion or sibling links.
    ///
    /// V-NOTE (Stage-B): receiver is [`WindowStream`], not `Window`.
    pub fn alloc_single_window(&mut self, store: &mut RuntimeStore) -> SwId {
        let swindow = alloc_swindow(store, Some(0));
        self.window_counter = self.window_counter.wrapping_add(1);
        store.single_windows.get_mut(swindow.0).number = self.window_counter;
        swindow
    }

    // [spec:cg3:def:window.cg3.window.alloc-push-single-window-fn]
    // [spec:cg3:sem:window.cg3.window.alloc-push-single-window-fn]
    /// C++ `SingleWindow* Window::allocPushSingleWindow()`. Allocates, bumps the
    /// counter, then links to the FRONT of `next`: if `next` is non-empty, splice
    /// before its front; if `current` is set, link after it. Inserts at the
    /// front of `next`.
    ///
    /// V-NOTE (Stage-B): receiver is [`WindowStream`], not `Window`.
    pub fn alloc_push_single_window(&mut self, store: &mut RuntimeStore) -> SwId {
        let swindow = alloc_swindow(store, Some(0));
        self.window_counter = self.window_counter.wrapping_add(1);
        store.single_windows.get_mut(swindow.0).number = self.window_counter;
        if !self.next.is_empty() {
            let front = self.next[0];
            store.single_windows.get_mut(swindow.0).next = Some(front);
            store.single_windows.get_mut(front.0).previous = Some(swindow);
        }
        if let Some(current) = self.current {
            store.single_windows.get_mut(swindow.0).previous = Some(current);
            store.single_windows.get_mut(current.0).next = Some(swindow);
        }
        self.next.insert(0, swindow);
        swindow
    }

    // [spec:cg3:def:window.cg3.window.alloc-append-single-window-fn]
    // [spec:cg3:sem:window.cg3.window.alloc-append-single-window-fn]
    /// C++ `SingleWindow* Window::allocAppendSingleWindow()`. Allocates, bumps
    /// the counter, then links to the BACK of `next`. QUIRK (faithful): if `next`
    /// is empty, NO sibling links are set at all — it is not linked to `current`,
    /// so its `previous` stays null (unlike `allocPushSingleWindow`).
    ///
    /// V-NOTE (Stage-B): receiver is [`WindowStream`], not `Window`.
    pub fn alloc_append_single_window(&mut self, store: &mut RuntimeStore) -> SwId {
        let swindow = alloc_swindow(store, Some(0));
        self.window_counter = self.window_counter.wrapping_add(1);
        store.single_windows.get_mut(swindow.0).number = self.window_counter;
        if !self.next.is_empty() {
            let back = *self.next.last().unwrap();
            store.single_windows.get_mut(swindow.0).previous = Some(back);
            store.single_windows.get_mut(back.0).next = Some(swindow);
        }
        self.next.push(swindow);
        swindow
    }

    // [spec:cg3:def:window.cg3.window.back-fn]
    // [spec:cg3:sem:window.cg3.window.back-fn]
    /// C++ `SingleWindow* Window::back()`. The last single-window of the
    /// document: `next.back()`, else `current`, else `previous.back()`, else
    /// null. Touches only `self` → `&self`.
    ///
    /// V-NOTE (Stage-B): receiver is [`WindowStream`], not `Window`.
    pub fn back(&self) -> Option<SwId> {
        if !self.next.is_empty() {
            Some(*self.next.last().unwrap())
        } else if self.current.is_some() {
            self.current
        } else if !self.previous.is_empty() {
            Some(*self.previous.last().unwrap())
        } else {
            None
        }
    }

    // [spec:cg3:def:window.cg3.window.shuffle-windows-down-fn]
    // [spec:cg3:sem:window.cg3.window.shuffle-windows-down-fn]
    /// C++ `void Window::shuffleWindowsDown()`. Advances the active window: if
    /// `current` is set, snapshot the applicator's variable state into
    /// `current->variables_set = parent->variables` (a `GrammarApplicator`
    /// placeholder — not threaded), clear `current->variables_rem`, push
    /// `current` onto `previous`, and null `current`; then pop the front of
    /// `next` into `current` if any.
    ///
    /// NOTE: engine code must go through
    /// `GrammarApplicator::shuffle_windows_down` / `rotate_next`, which perform
    /// the C++ `current->variables_set = parent->variables` snapshot first —
    /// `WindowStream` has no back-pointer to the applicator, so calling this raw
    /// method directly skips that snapshot.
    ///
    /// V-NOTE (Stage-B): receiver is [`WindowStream`], not `Window`.
    pub fn shuffle_windows_down(&mut self, store: &mut RuntimeStore) {
        if let Some(current) = self.current {
            // current->variables_set = parent->variables; — GrammarApplicator
            // placeholder, not threaded.
            store
                .single_windows
                .get_mut(current.0)
                .variables_rem
                .clear(0);
            self.previous.push(current);
            self.current = None;
        }
        if !self.next.is_empty() {
            self.current = Some(self.next[0]);
            self.next.remove(0);
        }
    }

    // [spec:cg3:def:window.cg3.window.rebuild-single-window-links-fn]
    // [spec:cg3:sem:window.cg3.window.rebuild-single-window-links-fn]
    /// C++ `void Window::rebuildSingleWindowLinks()`. Rebuilds the
    /// `previous`/`next` sibling chain across all single-windows in document
    /// order (`previous`, then `current`, then `next`), keeping a running
    /// predecessor `s_window`, and nulls the last window's `next`.
    ///
    /// V-NOTE (Stage-B): receiver is [`WindowStream`], not `Window`.
    pub fn rebuild_single_window_links(&mut self, store: &mut RuntimeStore) {
        let mut s_window: Option<SwId> = None;
        for &iter in &self.previous {
            store.single_windows.get_mut(iter.0).previous = s_window;
            if let Some(sw) = s_window {
                store.single_windows.get_mut(sw.0).next = Some(iter);
            }
            s_window = Some(iter);
        }
        if let Some(current) = self.current {
            store.single_windows.get_mut(current.0).previous = s_window;
            if let Some(sw) = s_window {
                store.single_windows.get_mut(sw.0).next = Some(current);
            }
            s_window = Some(current);
        }
        for &iter in &self.next {
            store.single_windows.get_mut(iter.0).previous = s_window;
            if let Some(sw) = s_window {
                store.single_windows.get_mut(sw.0).next = Some(iter);
            }
            s_window = Some(iter);
        }
        if let Some(sw) = s_window {
            store.single_windows.get_mut(sw.0).next = None;
        }
    }

    // [spec:cg3:def:window.cg3.window.rebuild-cohort-links-fn]
    // [spec:cg3:sem:window.cg3.window.rebuild-cohort-links-fn]
    /// C++ `void Window::rebuildCohortLinks()`. Rebuilds the global cohort
    /// `prev`/`next` chain across the whole document. Picks the first window
    /// (`previous.front()`, else `current`, else `next.front()`), then walks
    /// windows via their `->next`, linking each window's `cohorts` in order with
    /// a running `prev` cohort — spanning window boundaries. Relies on the
    /// single-window `->next` chain already being correct.
    ///
    /// V-NOTE (Stage-B): receiver is [`WindowStream`], not `Window`.
    pub fn rebuild_cohort_links(&self, store: &mut RuntimeStore) {
        let mut s_window: Option<SwId> = if !self.previous.is_empty() {
            Some(self.previous[0])
        } else if self.current.is_some() {
            self.current
        } else if !self.next.is_empty() {
            Some(self.next[0])
        } else {
            None
        };

        let mut prev: Option<CohortId> = None;
        while let Some(sw) = s_window {
            let cohorts = store.single_windows.get(sw.0).cohorts.clone();
            for citer in cohorts {
                {
                    let c = store.cohorts.get_mut(citer.0);
                    c.prev = prev;
                    c.next = None;
                }
                if let Some(p) = prev {
                    store.cohorts.get_mut(p.0).next = Some(citer);
                }
                prev = Some(citer);
            }
            s_window = store.single_windows.get(sw.0).next;
        }
    }
}
