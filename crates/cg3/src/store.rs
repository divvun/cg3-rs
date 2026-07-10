//! Runtime object store — owns the pooled `Cohort` / `Reading` / `SingleWindow`
//! arenas.
//!
//! Replaces CG-3's global object pools (`pool<Cohort*>` etc.) and the implicit
//! runtime pointer graph. Owned by the `GrammarApplicator` at the engine layer.
//! Core methods that dereference runtime objects by id take `&RuntimeStore` /
//! `&mut RuntimeStore`; to touch two arenas at once (e.g. a cohort and its
//! readings) destructure the store into its fields to split the borrows:
//! `let RuntimeStore { cohorts, readings, .. } = store;`.
//!
//! Not a manifest symbol — port infrastructure standing in for the pools.

use crate::arena::GenArena;
use crate::cohort::Cohort;
use crate::reading::Reading;
use crate::single_window::SingleWindow;

#[derive(Default)]
pub struct RuntimeStore {
    pub cohorts: GenArena<Cohort>,
    pub readings: GenArena<Reading>,
    pub single_windows: GenArena<SingleWindow>,
}

impl RuntimeStore {
    pub fn new() -> Self {
        RuntimeStore::default()
    }
}
