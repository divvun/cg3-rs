//! `src/GrammarApplicator_context.cpp` impl of `GrammarApplicator`.
//!
//! The rule-application context accessors. `context_stack` is a stack (`Vec`) of
//! [`Rule_Context`](super::Rule_Context) frames; the "current" context is its
//! `back()` (Rust `last()`). Each frame records, for the rule currently firing,
//! its matched `target`, its explicit `attach_to`, its `mark` cohort, and the
//! per-frame unification state. These six methods read/write the top frame.
//!
//! ARENA MODEL: `Cohort*` → [`CohortId`], `Reading*` → [`ReadingId`], and the
//! returned C++ `ReadingSpec` (a `{cohort, reading, subreading}` triple) is
//! [`ReadingSpec`](super::ReadingSpec) (all three `Option<…Id>`). `set_attach_to`
//! derives the cohort from `reading->parent`, so it reads `self.store`.
//! `check_unif_tags` dereferences the frame's `unif_tags` raw pointer (which
//! aliases an entry of `unif_tags_store`), matching the C++
//! `*(context_stack.back().unif_tags)`; the pointer is a `const void*` compared
//! only by identity (never dereferenced), so it stays a raw pointer here too.

use super::{ReadingSpec, unif_tags_t};
use crate::arena::{CohortId, ReadingId};

impl super::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.get-attach-to-fn]
    // [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.get-attach-to-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-attach-to-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-attach-to-fn]
    /// C++ `ReadingSpec GrammarApplicator::get_attach_to()`. Returns the current
    /// context's explicit attach target (does NOT fall back to `target`); an
    /// empty stack yields a default-constructed (all-null) `ReadingSpec`.
    pub fn get_attach_to(&self) -> ReadingSpec {
        if self.context_stack.is_empty() {
            ReadingSpec::default()
        } else {
            self.context_stack.last().unwrap().attach_to.clone()
        }
    }

    // [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.get-mark-fn]
    // [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.get-mark-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-mark-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-mark-fn]
    /// C++ `Cohort* GrammarApplicator::get_mark()`. Returns the current context's
    /// mark cohort (`X`/MARK reference), `None` on an empty stack (the stored
    /// `mark` may itself be `None`).
    pub fn get_mark(&self) -> Option<CohortId> {
        if self.context_stack.is_empty() {
            None
        } else {
            self.context_stack.last().unwrap().mark
        }
    }

    // [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.get-apply-to-fn]
    // [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.get-apply-to-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-apply-to-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-apply-to-fn]
    /// C++ `ReadingSpec GrammarApplicator::get_apply_to()`. The reading the rule
    /// action acts on: prefers an explicit `attach_to` (when its `cohort` is
    /// non-null) over the matched `target`; an empty stack yields the default
    /// (all-null) `ReadingSpec`.
    pub fn get_apply_to(&self) -> ReadingSpec {
        if self.context_stack.is_empty() {
            ReadingSpec::default()
        } else if self.context_stack.last().unwrap().attach_to.cohort.is_some() {
            self.context_stack.last().unwrap().attach_to.clone()
        } else {
            self.context_stack.last().unwrap().target.clone()
        }
    }

    // [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.set-attach-to-fn]
    // [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.set-attach-to-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-attach-to-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-attach-to-fn]
    /// C++ `void GrammarApplicator::set_attach_to(Reading* reading, Reading*
    /// subreading)`. Records an attach target on the current context (silent
    /// no-op when the stack is empty). The cohort is derived from
    /// `reading->parent` — not passed separately; `reading` is unconditionally
    /// dereferenced in C++ so it is a bare [`ReadingId`], while `subreading`
    /// (stored as-is, may be null) is `Option<ReadingId>`.
    pub fn set_attach_to(&mut self, reading: ReadingId, subreading: Option<ReadingId>) {
        if !self.context_stack.is_empty() {
            // spec.cohort = reading->parent (read before the mutable borrow below).
            let parent = self.store.readings.get(reading.0).parent;
            let spec = &mut self.context_stack.last_mut().unwrap().attach_to;
            spec.cohort = parent;
            spec.reading = Some(reading);
            spec.subreading = subreading;
        }
    }

    // [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.set-mark-fn]
    // [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.set-mark-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.set-mark-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.set-mark-fn]
    /// C++ `void GrammarApplicator::set_mark(Cohort* cohort)`. Sets the current
    /// context's mark cohort (silent no-op when the stack is empty). `cohort` is
    /// only stored, never dereferenced, so it is nullable → `Option<CohortId>`.
    pub fn set_mark(&mut self, cohort: Option<CohortId>) {
        if !self.context_stack.is_empty() {
            self.context_stack.last_mut().unwrap().mark = cohort;
        }
    }

    // [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.check-unif-tags-fn]
    // [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.check-unif-tags-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.check-unif-tags-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.check-unif-tags-fn]
    /// C++ `bool GrammarApplicator::check_unif_tags(uint32_t set, const void*
    /// val)`. Unification helper keyed on the current context's `unif_tags` map:
    /// empty stack → false; first sight of `set` records `val` and returns true;
    /// every later attempt for `set` succeeds only if it presents the identical
    /// pointer (enforcing "same tag across all uses").
    pub fn check_unif_tags(&mut self, set: u32, val: *const ()) -> bool {
        if self.context_stack.is_empty() {
            return false;
        }
        // auto& unif_tags = *(context_stack.back().unif_tags);
        // The C++ dereferences the pointer unconditionally; a null here would be
        // UB there, so a null pointer (`None`) faithfully panics ("crash").
        let ptr = self
            .context_stack
            .last()
            .unwrap()
            .unif_tags
            .expect("check_unif_tags: active context frame has a null unif_tags pointer");
        // SAFETY: `unif_tags` aliases a live `unif_tags_store` entry for the
        // duration of the frame, exactly as the C++ `unif_tags_t*` does.
        let unif_tags: &mut unif_tags_t = unsafe { &mut *ptr };
        if let Some(&existing) = unif_tags.get(&set) {
            return existing == val;
        }
        unif_tags.insert(set, val);
        true
    }
}
