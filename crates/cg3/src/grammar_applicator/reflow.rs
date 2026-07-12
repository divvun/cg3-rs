//! `src/GrammarApplicator_reflow.cpp` impl of GrammarApplicator — the
//! reading/mapping/dependency/relation reflow surgery. LITERAL, bug-for-bug
//! Wave-2 port.
//!
//! ============================================================================
//! CROSS-PARTIAL / SIGNATURE RECONCILIATION (see task report)
//! ============================================================================
//! EXPOSED signatures (the sibling partials — matchSet / runRules / runGrammar /
//! core — call these):
//!   - `add_tag_to_reading(&mut self, reading: ReadingId, tag: TagId) -> u32`
//!     (2-arg form == C++ `addTagToReading(Reading&, Tag*)` with the default
//!     `rehash = true`; the private `add_tag_to_reading_rehash` carries the
//!     explicit-`rehash` C++ signature for `reflow_reading`/`split_mappings`).
//!   - `del_tag_from_reading(&mut self, reading: ReadingId, tag: TagId)`
//!     (C++ `delTagFromReading(Reading&, Tag*)` / `(Reading&, uint32_t)`).
//!   - `reflow_reading(&mut self, reading: ReadingId)`.
//!   - `generate_varstring_tag(&mut self, tag: &Tag) -> TagId`
//!     (C++ `generateVarstringTag(const Tag*) -> const Tag*`, matching the
//!     matchSet header's declared form).
//!   - `reflow_dependency_window(&mut self, max: u32)`,
//!     `reflow_relation_window(&mut self)` (C++ `max` default 0 → no arg, per the
//!     run_grammar call site),
//!     `make_base_from_word(&mut self, tag: TagId) -> TagId`,
//!     `is_child_of(&mut self, child: CohortId, parent: CohortId) -> bool`,
//!     `attach_parent_child(...)`, `would_parent_child_loop/cross(...)`,
//!     `split_all_mappings(&mut self, &mut all_mappings_t, CohortId, bool)`,
//!     `merge_mappings(&mut self, CohortId)`, `delimit_at(&mut self, SwId,
//!      CohortId) -> CohortId`, `reflow_textuals*`.
//!
//! KNOWN sibling MISMATCHES (NOT fixable from reflow.rs — noted for the leads):
//!   * `run_rules.rs` calls `add_tag_to_reading(r, nt)` / `del_tag_from_reading(
//!     r, nt)` / `generate_varstring_tag(tag)` with a `TagId` `nt`/`tag`.
//!     `add_tag_to_reading`/`del_tag_from_reading` HERE take `TagId` (so those
//!     match), but `generate_varstring_tag` HERE takes `&Tag` (the matchSet
//!     header's declared form). run_rules must pass `&self.grammar.single_tags_list
//!     [tag.0]` (clone first) — a run_rules-side edit.
//!   * `run_grammar.rs` calls `add_tag_to_reading(c_reading, self.begintag)` with
//!     `self.begintag` (a `u32` hash — the C++ `uint32_t` overload). Since Rust
//!     cannot overload and `TagId` was chosen (majority of call sites pass a
//!     `TagId`), those sites must pass `self.tag_begin.unwrap()` / `self.tag_end
//!     .unwrap()` (both already on the struct) — a run_grammar-side edit.
//!
//! REPRODUCED BUGS (see the C++ + the reflow spec):
//!   * `del_tag_from_reading` does NOT update the bloom filters, `tags_string`/
//!     `tags_string_hash`, or the window bag-of-tags (blooms are additive-only).
//!   * `add_tag_to_reading`'s bag-of-tags baseform test uses `!reading.baseform`
//!     AFTER `reading.baseform` was just set, so `bot.baseform` is written only
//!     when the reading ALREADY had a baseform (the likely-bug quirk).
//!   * `would_parent_child_cross`'s loop body does not depend on the loop
//!     variable `i` (recomputes the same `parent.dep_parent` lookup every
//!     iteration).

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::cohort::{CT_DEP_DONE, CT_ENCLOSED, CT_IGNORED, CT_NUM_CURRENT, CT_REMOVED};
use crate::types::{GlobalNumber, TagHash};
use crate::inlines::{erase, hash_value, insert_if_exists, ui32};
use crate::reading::{Reading, ReadingList, alloc_reading_copy, free_reading, reading_rehash};
use crate::tag::{
    T_BASEFORM, T_CASE_INSENSITIVE, T_DEPENDENCY, T_MAPPING, T_NUMERICAL, T_REGEXP, T_RELATION,
    T_SPECIAL, T_TEXTUAL, T_VARSTRING, T_WORDFORM, Tag,
};

// ---------------------------------------------------------------------------
// Local Strings.hpp stand-ins (only KEYWORDS is ported in `crate::strings`, so
// the varstring marker constants are reproduced verbatim here — same precedent
// as the local string stand-ins in `grammar.rs`/`tag.rs`). The `_raw` forms are
// the literal markers; the sentinel forms use the U+0001 control code so a
// combined `%$1` cannot accidentally match `%L`/`$1` during substitution.
const STR_VSU_RAW: &str = "%u";
const STR_VSUU_RAW: &str = "%U";
const STR_VSL_RAW: &str = "%l";
const STR_VSLL_RAW: &str = "%L";
const STR_VS_RAW: [&str; 9] = ["$1", "$2", "$3", "$4", "$5", "$6", "$7", "$8", "$9"];

const STR_VSU: &str = "\u{1}u";
const STR_VSUU: &str = "\u{1}U";
const STR_VSL: &str = "\u{1}l";
const STR_VSLL: &str = "\u{1}L";
const STR_VS: [&str; 9] = [
    "\u{1}1", "\u{1}2", "\u{1}3", "\u{1}4", "\u{1}5", "\u{1}6", "\u{1}7", "\u{1}8", "\u{1}9",
];

// ---------------------------------------------------------------------------
// Free helpers (this file's namespace, matching the C++ translation unit).
// ---------------------------------------------------------------------------

/// `uextras.cpp` `findAndReplace(UnicodeString& str, from, to)`: replaces every
/// occurrence of `from` with `to`, advancing PAST each replacement (so text
/// introduced by `to` is not re-scanned), and returns the number of
/// replacements. Ported over `Vec<char>` for index-precise splicing (UTF-8
/// `char` == the "one code unit" the C++ UChar ops assume). NOT a manifest
/// symbol — port infra.
fn find_and_replace(str: &mut Vec<char>, from: &str, to: &str) -> usize {
    let from_v: Vec<char> = from.chars().collect();
    let to_v: Vec<char> = to.chars().collect();
    if from_v.is_empty() {
        return 0;
    }
    let mut rv = 0usize;
    let mut offset = 0usize;
    while offset + from_v.len() <= str.len() {
        if str[offset..offset + from_v.len()] == from_v[..] {
            str.splice(offset..offset + from_v.len(), to_v.iter().copied());
            offset += to_v.len();
            rv += 1;
        } else {
            offset += 1;
        }
    }
    rv
}

/// `UnicodeString::lastIndexOf(needle, len, start)`: the greatest index `>=
/// start` at which `needle` begins in `hay`, or `-1`. Reproduces the ICU
/// three-arg overload used by the case-marker loop. NOT a manifest symbol.
fn last_index_of(hay: &[char], needle: &[char], start: usize) -> i32 {
    if needle.is_empty() || needle.len() > hay.len() {
        return -1;
    }
    let mut i = hay.len() - needle.len();
    loop {
        if i >= start && hay[i..i + needle.len()] == needle[..] {
            return i as i32;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    -1
}

impl super::GrammarApplicator {
    // =======================================================================
    // makeBaseFromWord
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.make-base-from-word-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.make-base-from-word-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.make-base-from-word-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.make-base-from-word-fn]
    /// C++ `Tag* makeBaseFromWord(Tag* tag)` — strips the inner `<`/`>` of a
    /// wordform tag (`"<foo>"` → `"foo"`), interns and returns the result. The
    /// `uint32_t` overload resolved the hash to a `Tag*`; here the sole caller
    /// (run_grammar) already passes a `TagId`, so this is the single `TagId`
    /// form. Ported over `char`s (the "one code unit" analog of the C++ UChar
    /// splice: keep everything except the chars at index 1 and `len-2`).
    pub fn make_base_from_word(&mut self, tag: TagId) -> TagId {
        let chars: Vec<char> = self.grammar.single_tags_list[tag.0].tag.chars().collect();
        let len = chars.len();
        if len < 4 {
            return tag;
        }
        // n.resize(len-2); n[0] = n[len-3] = '"'; copy len-4 units from tag[2].
        let mut n: Vec<char> = vec!['\0'; len - 2];
        n[0] = '"';
        n[len - 3] = '"';
        n[1..((len - 4) + 1)].copy_from_slice(&chars[2..((len - 4) + 2)]);
        let s: String = n.into_iter().collect();
        // addTag(n) — the type-less overload interns the raw text (type 0).
        self.add_tag(&s, crate::tag::TagType::empty())
    }

    // =======================================================================
    // Dependency tree predicates
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.is-child-of-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.is-child-of-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.is-child-of-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.is-child-of-fn]
    /// C++ `bool isChildOf(const Cohort* child, const Cohort* parent)` — is
    /// `child` a descendant of (or the same as) `parent`, climbing `dep_parent`
    /// via `gWindow->cohort_map` (capped at 1000). `&mut self` only because the
    /// verbosity warning reads/writes nothing but keeps parity with the sibling
    /// signatures; the body is read-only over the arenas.
    pub fn is_child_of(&mut self, child: CohortId, parent: CohortId) -> bool {
        let mut retval = false;
        let parent_gn = self.store.cohorts.get(parent.0).global_number;
        let child_gn = self.store.cohorts.get(child.0).global_number;
        let child_dp = self.store.cohorts.get(child.0).dep_parent;

        if parent_gn == child_gn || Some(parent_gn) == child_dp {
            retval = true;
        } else {
            let mut i = 0usize;
            let mut inner = child;
            while i < 1000 {
                let inner_dp = self.store.cohorts.get(inner.0).dep_parent;
                if inner_dp == Some(GlobalNumber(0)) || inner_dp.is_none() {
                    retval = false;
                    break;
                }
                match self.gWindow.cohort_map.get(&inner_dp.unwrap()).copied() {
                    Some(next) => inner = next,
                    None => break,
                }
                if self.store.cohorts.get(inner.0).dep_parent == Some(parent_gn) {
                    retval = true;
                    break;
                }
                i += 1;
            }
            if i == 1000 && self.verbosity_level > 0 {
                // "Warning: While testing whether %u is a child of %u the counter
                // exceeded 1000 ..." — I/O deferred (ux_stderr placeholder).
            }
        }
        retval
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-loop-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-loop-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.would-parent-child-loop-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.would-parent-child-loop-fn]
    /// C++ `bool wouldParentChildLoop(const Cohort* parent, const Cohort*
    /// child)` — would attaching `child` under `parent` form a cycle. Mirror of
    /// `isChildOf` but climbing from `parent`, looking for `child`.
    // faithful port: mirrors the C++ wouldParentChildLoop condition dispatch
    // (distinct predicates that happen to share `retval` bodies).
    #[allow(clippy::if_same_then_else)]
    pub fn would_parent_child_loop(&mut self, parent: CohortId, child: CohortId) -> bool {
        let mut retval = false;
        let parent_gn = self.store.cohorts.get(parent.0).global_number;
        let parent_dp = self.store.cohorts.get(parent.0).dep_parent;
        let child_gn = self.store.cohorts.get(child.0).global_number;
        let child_dp = self.store.cohorts.get(child.0).dep_parent;

        if parent_gn == child_gn {
            retval = true;
        } else if Some(parent_gn) == child_dp {
            retval = false;
        } else if Some(parent_gn) == parent_dp {
            retval = false;
        } else if parent_dp == Some(child_gn) {
            retval = true;
        } else {
            let mut i = 0usize;
            let mut inner = parent;
            while i < 1000 {
                let inner_dp = self.store.cohorts.get(inner.0).dep_parent;
                if inner_dp == Some(GlobalNumber(0)) || inner_dp.is_none() {
                    retval = false;
                    break;
                }
                match self.gWindow.cohort_map.get(&inner_dp.unwrap()).copied() {
                    Some(next) => inner = next,
                    None => break,
                }
                if self.store.cohorts.get(inner.0).dep_parent == Some(child_gn) {
                    retval = true;
                    break;
                }
                i += 1;
            }
            if i == 1000 && self.verbosity_level > 0 {
                // Counter-exceeded warning — I/O deferred.
            }
        }
        retval
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-cross-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.would-parent-child-cross-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.would-parent-child-cross-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.would-parent-child-cross-fn]
    /// C++ `bool wouldParentChildCross(const Cohort* parent, const Cohort*
    /// child)` — would the edge cross an existing branch. QUIRK (reproduced, NOT
    /// fixed): the loop body ignores the loop variable `i`, recomputing the same
    /// `parent->dep_parent` lookup `mx - mn - 1` times.
    pub fn would_parent_child_cross(&mut self, parent: CohortId, child: CohortId) -> bool {
        let parent_gn = self.store.cohorts.get(parent.0).global_number;
        let child_gn = self.store.cohorts.get(child.0).global_number;
        let parent_dp = self.store.cohorts.get(parent.0).dep_parent;
        let mn = parent_gn.min(child_gn);
        let mx = parent_gn.max(child_gn);

        let mut i = mn.wrapping_add(1);
        while i < mx {
            if let Some(pdp) = parent_dp
                && let Some(&mid) = self.gWindow.cohort_map.get(&pdp) {
                    let mid_dp = self.store.cohorts.get(mid.0).dep_parent;
                    if let Some(mdp) = mid_dp
                        && (mdp < mn || mdp > mx) {
                            return true;
                        }
                }
            i = i.wrapping_add(1);
        }
        false
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.attach-parent-child-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.attach-parent-child-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.attach-parent-child-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.attach-parent-child-fn]
    /// C++ `bool attachParentChild(Cohort& parent, Cohort& child, bool
    /// allowloop, bool allowcrossing)` — wires a dependency edge subject to the
    /// loop/crossing guards; returns true on success.
    pub fn attach_parent_child(
        &mut self,
        parent: CohortId,
        child: CohortId,
        allowloop: bool,
        allowcrossing: bool,
    ) -> bool {
        {
            let pgn = self.store.cohorts.get(parent.0).global_number;
            self.store.cohorts.get_mut(parent.0).dep_self = Some(pgn);
            let cgn = self.store.cohorts.get(child.0).global_number;
            self.store.cohorts.get_mut(child.0).dep_self = Some(cgn);
        }

        if !allowloop && self.dep_block_loops && self.would_parent_child_loop(parent, child) {
            // Loop warning — I/O deferred.
            return false;
        }

        if !allowcrossing && self.dep_block_crossing && self.would_parent_child_cross(parent, child)
        {
            // Crossing warning — I/O deferred.
            return false;
        }

        // Detach child from its old parent.
        {
            let (cds, cdp) = {
                let c = self.store.cohorts.get(child.0);
                (c.dep_self, c.dep_parent)
            };
            let cdp = match cdp {
                None => {
                    self.store.cohorts.get_mut(child.0).dep_parent = cds;
                    cds
                }
                Some(v) => Some(v),
            };
            if let Some(dp) = cdp
                && let Some(&old) = self.gWindow.cohort_map.get(&dp) {
                    self.store
                        .cohorts
                        .get_mut(old.0)
                        .rem_child(cds.map_or(0, |g| g.get()));
                }
        }

        let parent_gn = self.store.cohorts.get(parent.0).global_number;
        let child_gn = self.store.cohorts.get(child.0).global_number;
        self.store.cohorts.get_mut(child.0).dep_parent = Some(parent_gn);
        self.store.cohorts.get_mut(parent.0).add_child(child_gn.get());

        self.store.cohorts.get_mut(parent.0).r#type |= CT_DEP_DONE;
        self.store.cohorts.get_mut(child.0).r#type |= CT_DEP_DONE;

        if !self.dep_has_spanned {
            let cp = self.store.cohorts.get(child.0).parent;
            let pp = self.store.cohorts.get(parent.0).parent;
            if cp != pp {
                // "Info: Dependency ... spans the window boundaries ..." — I/O deferred.
                self.dep_has_spanned = true;
            }
        }
        true
    }

    // =======================================================================
    // reflowDependencyWindow
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-dependency-window-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-dependency-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-dependency-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-dependency-window-fn]
    /// C++ `void reflowDependencyWindow(uint32_t max)` — resolves raw
    /// `dep_self`/`dep_parent` ids into global cohort numbers and wires
    /// parent/child links over `gWindow->dep_window`.
    pub fn reflow_dependency_window(&mut self, mut max: u32) {
        if self.dep_delimit != 0 && max == 0 && !self.input_eof && !self.gWindow.next.is_empty() {
            let back = *self.gWindow.next.last().unwrap();
            if self.store.single_windows.get(back.0).cohorts.len() > 1 {
                let c1 = self.store.single_windows.get(back.0).cohorts[1];
                max = self.store.cohorts.get(c1.0).global_number.get();
            }
        }

        // Ensure a root entry at dep_window[0]. C++ dereferences
        // `gWindow->current` lazily inside the branches that need it (it is
        // null while the input stream is still being parsed, when those
        // branches are not taken), so the unwrap must stay lazy too.
        if self.gWindow.dep_window.is_empty() || {
            let first = *self.gWindow.dep_window.values().next().unwrap();
            self.store.cohorts.get(first.0).parent.is_none()
        } {
            let cur = self.gWindow.current.unwrap();
            let c0 = self.store.single_windows.get(cur.0).cohorts[0];
            self.gWindow.dep_window.insert(GlobalNumber(0), c0);
        } else if !self.gWindow.dep_window.contains_key(&GlobalNumber(0)) {
            let first = *self.gWindow.dep_window.values().next().unwrap();
            let tmp = {
                let sw = self.store.cohorts.get(first.0).parent.unwrap();
                self.store.single_windows.get(sw.0).cohorts[0]
            };
            self.gWindow.dep_window.insert(GlobalNumber(0), tmp);
        }
        // Ensure cohort_map[0].
        if self.gWindow.cohort_map.is_empty() {
            let cur = self.gWindow.current.unwrap();
            let c0 = self.store.single_windows.get(cur.0).cohorts[0];
            self.gWindow.cohort_map.insert(GlobalNumber(0), c0);
        } else if !self.gWindow.cohort_map.contains_key(&GlobalNumber(0)) {
            let cur = self.gWindow.current.unwrap();
            let mut tmp = self.store.single_windows.get(cur.0).cohorts[0];
            let first = *self.gWindow.cohort_map.values().next().unwrap();
            if let Some(sw) = self.store.cohorts.get(first.0).parent {
                tmp = self.store.single_windows.get(sw.0).cohorts[0];
            }
            self.gWindow.cohort_map.insert(GlobalNumber(0), tmp);
        }

        // Snapshot dep_window in id order (BTreeMap iteration == C++ std::map).
        let dw: Vec<(GlobalNumber, CohortId)> = self
            .gWindow
            .dep_window
            .iter()
            .map(|(&k, &v)| (k, v))
            .collect();

        let mut begin = 0usize;
        loop {
            while begin < dw.len() {
                let c = dw[begin].1;
                let (ty, ds) = {
                    let co = self.store.cohorts.get(c.0);
                    (co.r#type, co.dep_self)
                };
                if ty.intersects(CT_DEP_DONE) || ds.is_none() {
                    begin += 1;
                } else {
                    break;
                }
            }
            self.gWindow.dep_map.clear(0);

            // Build the batch [begin, end).
            let mut end = begin;
            while end < dw.len() {
                let cohort = dw[end].1;
                let (ty, ds, gn) = {
                    let co = self.store.cohorts.get(cohort.0);
                    (co.r#type, co.dep_self, co.global_number)
                };
                if ty.intersects(CT_DEP_DONE) {
                    end += 1;
                    continue;
                }
                if ds.is_none() {
                    end += 1;
                    continue;
                }
                let ds_raw = ds.map_or(0, |g| g.get());
                if max != 0 && gn.get() >= max {
                    break;
                }
                if self.gWindow.dep_map.contains(ds_raw) {
                    break;
                }
                self.gWindow.dep_map.insert((ds_raw, gn.get()));
                self.store.cohorts.get_mut(cohort.0).dep_self = Some(gn);
                end += 1;
            }

            if self.gWindow.dep_map.empty() {
                break;
            }

            self.gWindow.dep_map.insert((0, 0));
            let mut b = begin;
            while b != end {
                let cohort = dw[b].1;
                let (ty, ds, dp, gn, ln) = {
                    let co = self.store.cohorts.get(cohort.0);
                    (
                        co.r#type,
                        co.dep_self,
                        co.dep_parent,
                        co.global_number,
                        co.local_number,
                    )
                };
                if max != 0 && gn.get() >= max {
                    break;
                }
                if dp.is_none() {
                    b += 1;
                    continue;
                }
                if ds == Some(gn) {
                    let dpv = dp.unwrap().get();
                    let dp_present = self.gWindow.dep_map.find(dpv) != self.gWindow.dep_map.end();
                    if !ty.intersects(CT_DEP_DONE) && !dp_present {
                        if self.verbosity_level > 0 {
                            let _ = ln; // "Warning: Parent %u of dep %u ..." — I/O deferred.
                        }
                        self.store.cohorts.get_mut(cohort.0).dep_parent = None;
                    } else {
                        if !ty.intersects(CT_DEP_DONE) {
                            let dep_real = self.gWindow.dep_map.find(dpv).get().1;
                            self.store.cohorts.get_mut(cohort.0).dep_parent =
                                Some(GlobalNumber(dep_real));
                        }
                        let par = self.store.cohorts.get(cohort.0).parent.unwrap();
                        let c0 = self.store.single_windows.get(par.0).cohorts[0];
                        self.gWindow.cohort_map.insert(GlobalNumber(0), c0);
                        let real_dp = self.store.cohorts.get(cohort.0).dep_parent;
                        if let Some(rdp) = real_dp
                            && let Some(&pc) = self.gWindow.cohort_map.get(&rdp) {
                                let dep_self = self
                                    .store
                                    .cohorts
                                    .get(cohort.0)
                                    .dep_self
                                    .map_or(0, |g| g.get());
                                self.store.cohorts.get_mut(pc.0).add_child(dep_self);
                            }
                        self.store.cohorts.get_mut(cohort.0).r#type |= CT_DEP_DONE;
                    }
                }
                b += 1;
            }
            // The outer loop's `begin` IS the second-pass loop variable in C++,
            // so it continues from wherever the pass stopped (== `end` normally,
            // or the early-`max`-break position).
            begin = b;
            if begin >= dw.len() {
                break;
            }
        }

        self.gWindow.dep_map.clear(0);
        self.gWindow.dep_window.clear();
    }

    // =======================================================================
    // reflowRelationWindow
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-relation-window-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-relation-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-relation-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-relation-window-fn]
    /// C++ `void reflowRelationWindow(uint32_t max)` — resolves deferred named
    /// relations (`relations_input`) into concrete `relations` via
    /// `gWindow->relation_map`. `max` defaults 0 (the run_grammar call site
    /// passes no argument), so this takes none.
    pub fn reflow_relation_window(&mut self) {
        let mut max = 0u32;
        if !self.input_eof && !self.gWindow.next.is_empty() {
            let back = *self.gWindow.next.last().unwrap();
            if self.store.single_windows.get(back.0).cohorts.len() > 1 {
                let c0 = self.store.single_windows.get(back.0).cohorts[0];
                max = self.store.cohorts.get(c0.0).global_number.get();
            }
        }

        // Walk to the leftmost cohort from current.cohorts[1] via ->prev.
        let cur = self.gWindow.current.unwrap();
        let mut cohort = Some(self.store.single_windows.get(cur.0).cohorts[1]);
        while let Some(c) = cohort {
            match self.store.cohorts.get(c.0).prev {
                Some(p) => cohort = Some(p),
                None => break,
            }
        }

        while let Some(c) = cohort {
            let gn = self.store.cohorts.get(c.0).global_number.get();
            if max != 0 && gn >= max {
                break;
            }

            // Snapshot the relations_input entries (name-hash → id set).
            let ri: Vec<(u32, Vec<u32>)> = self
                .store
                .cohorts
                .get(c.0)
                .relations_input
                .iter()
                .map(|(&k, v)| (k, v.as_slice().to_vec()))
                .collect();

            for (name, targets) in ri {
                let mut newrel = self.ss_u32sv.get();
                for target in targets {
                    if let Some(&mapped) = {
                        let it = self.gWindow.relation_map.find(target);
                        if it != self.gWindow.relation_map.end() {
                            Some(&it.get().1)
                        } else {
                            None
                        }
                    } {
                        self.store
                            .cohorts
                            .get_mut(c.0)
                            .relations
                            .entry(name)
                            .or_default()
                            .insert(mapped);
                    } else {
                        newrel.insert(target);
                    }
                }
                if newrel.empty() {
                    self.store
                        .cohorts
                        .get_mut(c.0)
                        .relations_input
                        .remove(&name);
                } else {
                    let v = newrel.as_slice().to_vec();
                    let dst = self
                        .store
                        .cohorts
                        .get_mut(c.0)
                        .relations_input
                        .entry(name)
                        .or_default();
                    dst.clear();
                    for x in v {
                        dst.insert(x);
                    }
                }
            }

            cohort = self.store.cohorts.get(c.0).next;
        }
    }

    // =======================================================================
    // reflowReading
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-reading-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-reading-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-reading-fn]
    /// C++ `void reflowReading(Reading& reading)` — rebuilds every derived tag
    /// index of the reading from its `tags_list`.
    pub fn reflow_reading(&mut self, reading: ReadingId) {
        {
            let r = self.store.readings.get_mut(reading.0);
            r.tags.clear();
            r.tags_plain.clear();
            r.tags_textual.clear();
            r.tags_numerical.clear();
            r.tags_bloom.clear();
            r.tags_textual_bloom.clear();
            r.tags_plain_bloom.clear();
            r.mapping = None;
            r.tags_string.clear();
        }

        // insert_if_exists(reading.parent->possible_sets, grammar->sets_any)
        let parent = self.store.readings.get(reading.0).parent.unwrap();
        insert_if_exists(
            &mut self.store.cohorts.get_mut(parent.0).possible_sets,
            self.grammar.sets_any.as_ref(),
        );

        // tlist.swap(reading.tags_list) — take the list, leaving it empty.
        let tlist: Vec<u32> = std::mem::take(&mut self.store.readings.get_mut(reading.0).tags_list);
        for tter in tlist {
            // addTagToReading(reading, tter, false) — the uint32_t/rehash form.
            let tid = self.grammar.single_tags.find(tter).get().1;
            self.add_tag_to_reading_rehash(reading, tid, false);
        }

        reading_rehash(&mut self.store, &self.grammar, reading);
    }

    // =======================================================================
    // generateVarstringTag
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.generate-varstring-tag-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.generate-varstring-tag-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.generate-varstring-tag-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.generate-varstring-tag-fn]
    /// C++ `Tag* generateVarstringTag(const Tag* tag)` — expands a VARSTRING
    /// template: unified-set substitution, `$1..$9` capture-group substitution,
    /// and `%u/%U/%l/%L` case markers, then interns the result. `tag` is a
    /// borrowed pattern tag NOT aliasing `self.grammar` (matchSet clones it out
    /// before calling), so the signature is `&Tag` per the matchSet header.
    ///
    /// ICU `UnicodeString` ops map to `Vec<char>` splicing (`findAndReplace`,
    /// `lastIndexOf`) and `char::to_uppercase`/`to_lowercase` (the ICU full
    /// case mapping analog; parity risk for locale-specific mappings, noted).
    pub fn generate_varstring_tag(&mut self, tag: &Tag) -> TagId {
        let mut tmp: Vec<char> = tag.tag.chars().collect();
        let mut did_something = false;

        // (1) Escape %[UuLl] and $1-9 markers to control-code sentinels.
        let raw: [&str; 13] = [
            STR_VSU_RAW,
            STR_VSUU_RAW,
            STR_VSL_RAW,
            STR_VSLL_RAW,
            STR_VS_RAW[0],
            STR_VS_RAW[1],
            STR_VS_RAW[2],
            STR_VS_RAW[3],
            STR_VS_RAW[4],
            STR_VS_RAW[5],
            STR_VS_RAW[6],
            STR_VS_RAW[7],
            STR_VS_RAW[8],
        ];
        let x01: [&str; 13] = [
            STR_VSU, STR_VSUU, STR_VSL, STR_VSLL, STR_VS[0], STR_VS[1], STR_VS[2], STR_VS[3],
            STR_VS[4], STR_VS[5], STR_VS[6], STR_VS[7], STR_VS[8],
        ];
        for i in 0..13 {
            find_and_replace(&mut tmp, raw[i], x01[i]);
        }

        // (2) Replace unified sets with their matching tags.
        if let Some(vs_sets) = &tag.vs_sets {
            let vs_names = tag.vs_names.as_ref();
            for i in 0..vs_sets.len() {
                let set_id = vs_sets[i];
                // getTagList(*(*tag->vs_sets)[i], tags) — 2-arg form, unif_mode=false.
                let tags = {
                    let mut tl = Vec::new();
                    let the_set = self.grammar.sets_list.get(set_id.0);
                    // get_tag_list borrows &self.grammar; clone the set out first
                    // to avoid aliasing while it appends into `tl`.
                    let set_clone = the_set;
                    self.get_tag_list(set_clone, &mut tl, false);
                    tl
                };
                // Build rpl: tag texts joined with '_' between multiple.
                let mut rpl = String::new();
                let n = tags.len();
                for (j, &tid) in tags.iter().enumerate() {
                    rpl.push_str(&self.grammar.single_tags_list[tid.0].tag);
                    if n - j > 1 {
                        rpl.push('_');
                    }
                }
                if let Some(names) = vs_names
                    && i < names.len() && find_and_replace(&mut tmp, &names[i], &rpl) > 0 {
                        did_something = true;
                    }
            }
        }

        // (3) Replace $1-$9 with the current context frame's capture groups.
        if let Some(frame) = self.context_stack.last() {
            let ct = frame.regexgrp_ct as usize;
            let grps_idx = frame.regexgrps;
            let mut i = 0usize;
            while i < ct && i < 9 {
                let text: String = match grps_idx {
                    Some(gi) => self.regexgrps_store[gi].get(i).cloned().unwrap_or_default(),
                    None => String::new(),
                };
                if find_and_replace(&mut tmp, STR_VS[i], &text) > 0 {
                    did_something = true;
                }
                i += 1;
            }
        }

        // (4) Handle %U %u %L %l markers, rightmost-first, until none remain.
        loop {
            let mut found = false;
            let mut mpos: i32 = -1;
            let mut pos;
            let su: Vec<char> = STR_VSU.chars().collect();
            let suu: Vec<char> = STR_VSUU.chars().collect();
            let sl: Vec<char> = STR_VSL.chars().collect();
            let sll: Vec<char> = STR_VSLL.chars().collect();
            pos = last_index_of(&tmp, &su, 0);
            if pos != -1 {
                found = true;
                mpos = mpos.max(pos);
            }
            pos = last_index_of(&tmp, &suu, mpos.max(0) as usize);
            if pos != -1 {
                found = true;
                mpos = mpos.max(pos);
            }
            pos = last_index_of(&tmp, &sl, mpos.max(0) as usize);
            if pos != -1 {
                found = true;
                mpos = mpos.max(pos);
            }
            pos = last_index_of(&tmp, &sll, mpos.max(0) as usize);
            if pos != -1 {
                found = true;
                mpos = mpos.max(pos);
            }
            if found && mpos != -1 {
                let m = mpos as usize;
                let mode = tmp[m + 1];
                // tmp.remove(mpos, 2)
                tmp.drain(m..m + 2);
                match mode {
                    'u' => {
                        let up: String = tmp[m].to_uppercase().collect();
                        // setCharAt(mpos, range[0]) — first mapped char.
                        if let Some(c0) = up.chars().next() {
                            tmp[m] = c0;
                        }
                    }
                    'U' => {
                        let tail: String = tmp[m..].iter().collect::<String>().to_uppercase();
                        tmp.truncate(m);
                        tmp.extend(tail.chars());
                    }
                    'l' => {
                        let lo: String = tmp[m].to_lowercase().collect();
                        if let Some(c0) = lo.chars().next() {
                            tmp[m] = c0;
                        }
                    }
                    'L' => {
                        let tail: String = tmp[m..].iter().collect::<String>().to_lowercase();
                        tmp.truncate(m);
                        tmp.extend(tail.chars());
                    }
                    _ => {}
                }
                did_something = true;
            } else {
                break;
            }
        }

        // (5) Re-append type suffixes so the regenerated string re-parses.
        if tag.r#type.intersects(T_CASE_INSENSITIVE) {
            tmp.push('i');
        }
        if tag.r#type.intersects(T_REGEXP) {
            tmp.push('r');
        }

        let nt: String = tmp.into_iter().collect();
        if !did_something && nt == tag.tag {
            // "Warning: Unable to generate from tag ..." — I/O deferred.
        }
        // addTag(nt, tag->type)
        self.add_tag(&nt, tag.r#type)
    }

    // =======================================================================
    // addTagToReading
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.add-tag-to-reading-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.add-tag-to-reading-fn]
    /// C++ `uint32_t addTagToReading(Reading& reading, Tag* tag)` (the 2-arg form
    /// with default `rehash = true`). See `add_tag_to_reading_rehash` for the
    /// full body.
    pub fn add_tag_to_reading(&mut self, reading: ReadingId, tag: TagId) -> TagHash {
        self.add_tag_to_reading_rehash(reading, tag, true)
    }

    /// C++ `uint32_t addTagToReading(Reading& reading, Tag* tag, bool rehash)`.
    /// Adds `tag`, updates every derived index of the reading, the parent
    /// cohort's flags, and (when `grammar->has_bag_of_tags`) the window
    /// bag-of-tags; returns the (possibly varstring-substituted) tag's hash.
    pub fn add_tag_to_reading_rehash(
        &mut self,
        reading: ReadingId,
        mut tag: TagId,
        rehash: bool,
    ) -> TagHash {
        if self.grammar.single_tags_list[tag.0]
            .r#type
            .intersects(T_VARSTRING)
        {
            let tval = self.grammar.single_tags_list[tag.0].clone();
            tag = self.generate_varstring_tag(&tval);
        }

        // Snapshot the tag's scalar fields (it lives in the grammar arena).
        let (thash, ttype, tplain, first_char, tds, tdp, tch) = {
            let t = &self.grammar.single_tags_list[tag.0];
            (
                t.hash,
                t.r#type,
                t.plain_hash,
                t.tag.chars().next().unwrap_or('\0'),
                t.dep_self,
                t.dep_parent(),
                t.comparison_hash,
            )
        };
        let _ = tplain;

        // C++ dereferences `reading.parent` only INSIDE the branches below;
        // a parentless reading (e.g. testPR fixtures) is fine as long as no
        // branch actually fires (unwrap only at the touch points, faithfully).
        let parent = self.store.readings.get(reading.0).parent;

        // possible_sets |= grammar->sets_by_tag[tag->hash]
        if let Some(bits) = self.grammar.sets_by_tag.get(&thash.get()) {
            let parent = parent.unwrap();
            let ps = &mut self.store.cohorts.get_mut(parent.0).possible_sets;
            if ps.len() < bits.len() {
                ps.resize(bits.len(), false);
            }
            for (i, &b) in bits.iter().enumerate() {
                if b {
                    ps[i] = true;
                }
            }
        }

        {
            let r = self.store.readings.get_mut(reading.0);
            r.tags.insert(thash.get());
            r.tags_list.push(thash.get());
            r.tags_bloom.insert(thash.get());
        }

        // ToDo: Remove for real ordered mode
        if self.ordered {
            let tag_text = self.grammar.single_tags_list[tag.0].tag.clone();
            let r = self.store.readings.get_mut(reading.0);
            if !r.tags_string.is_empty() {
                r.tags_string.push(' ');
            }
            r.tags_string.push_str(&tag_text);
            r.tags_string_hash = crate::inlines::hash_value_ustring(&r.tags_string, 0);
        }

        if self.grammar.parentheses.contains_key(&thash.get()) {
            self.store.cohorts.get_mut(parent.unwrap().0).is_pleft = thash.get();
        }
        if self.grammar.parentheses_reverse.contains_key(&thash.get()) {
            self.store.cohorts.get_mut(parent.unwrap().0).is_pright = thash.get();
        }

        if ttype.intersects(T_MAPPING) || first_char == self.grammar.mapping_prefix {
            self.grammar.single_tags_list[tag.0].r#type |= T_MAPPING;
            let existing = self.store.readings.get(reading.0).mapping;
            if let Some(m) = existing
                && m != tag {
                    // "Error: addTagToReading() cannot add a mapping tag ..." →
                    // CG3Quit(1). I/O deferred; the quit is faithful.
                    crate::inlines::cg3_quit(1, Some(file!()), self.grammar.lines);
                }
            self.store.readings.get_mut(reading.0).mapping = Some(tag);
        }
        if ttype.intersects(T_TEXTUAL | T_WORDFORM | T_BASEFORM) {
            let r = self.store.readings.get_mut(reading.0);
            r.tags_textual.insert(thash.get());
            r.tags_textual_bloom.insert(thash.get());
        }
        if ttype.intersects(T_NUMERICAL) {
            self.store
                .readings
                .get_mut(reading.0)
                .tags_numerical
                .insert(thash.get(), tag);
            self.store.cohorts.get_mut(parent.unwrap().0).r#type &= !CT_NUM_CURRENT;
        }
        if self.store.readings.get(reading.0).baseform.is_none() && (ttype.intersects(T_BASEFORM))
        {
            self.store.readings.get_mut(reading.0).baseform = Some(thash);
        }
        if self.parse_dep
            && (ttype.intersects(T_DEPENDENCY))
            && (!self
                .store
                .cohorts
                .get(parent.unwrap().0)
                .r#type
                .intersects(CT_DEP_DONE))
        {
            let c = self.store.cohorts.get_mut(parent.unwrap().0);
            c.dep_self = if tds == 0 { None } else { Some(GlobalNumber(tds)) };
            c.dep_parent = Some(GlobalNumber(tdp));
            if tdp == tds {
                c.dep_parent = None;
            }
            self.has_dep = true;
        }
        if self.grammar.has_relations && (ttype.intersects(T_RELATION)) {
            if tdp != 0 && tch != 0 {
                self.store
                    .cohorts
                    .get_mut(parent.unwrap().0)
                    .relations_input
                    .entry(tch)
                    .or_default()
                    .insert(tdp);
            }
            if tds != 0 {
                let gn = self.store.cohorts.get(parent.unwrap().0).global_number.get();
                self.gWindow.relation_map.insert((tds, gn));
            }
            self.has_relations = true;
            crate::cohort::set_related(&mut self.store, parent.unwrap());
        }
        if !ttype.intersects(T_SPECIAL) {
            let r = self.store.readings.get_mut(reading.0);
            r.tags_plain.insert(thash.get());
            r.tags_plain_bloom.insert(thash.get());
        }
        if rehash {
            reading_rehash(&mut self.store, &self.grammar, reading);
        }

        if self.grammar.has_bag_of_tags {
            // bot = reading.parent->parent->bag_of_tags
            let sw = self.store.cohorts.get(parent.unwrap().0).parent.unwrap();
            // NOTE quirk: `!reading.baseform` is tested AFTER reading.baseform was
            // set above, so bot.baseform is written only when the reading ALREADY
            // had a baseform (likely-bug, reproduced).
            let reading_baseform = self.store.readings.get(reading.0).baseform;
            let bot = &mut self.store.single_windows.get_mut(sw.0).bag_of_tags;
            bot.tags.insert(thash.get());
            bot.tags_list.push(thash.get());
            bot.tags_bloom.insert(thash.get());
            if ttype.intersects(T_TEXTUAL | T_WORDFORM | T_BASEFORM) {
                bot.tags_textual.insert(thash.get());
                bot.tags_textual_bloom.insert(thash.get());
            }
            if ttype.intersects(T_NUMERICAL) {
                bot.tags_numerical.insert(thash.get(), tag);
            }
            if reading_baseform.is_none() && (ttype.intersects(T_BASEFORM)) {
                bot.baseform = Some(thash);
            }
            if !ttype.intersects(T_SPECIAL) {
                bot.tags_plain.insert(thash.get());
                bot.tags_plain_bloom.insert(thash.get());
            }
            if rehash {
                // bot.rehash(): the bag-of-tags is an embedded `Reading` VALUE
                // (not an arena object, so it has no ReadingId `reading_rehash`
                // could take). Reproduce `Reading::rehash` inline — a bag never
                // holds a `next` chain, and its `mapping` is always null, so the
                // fold is just over `tags`.
                let bot = &mut self.store.single_windows.get_mut(sw.0).bag_of_tags;
                let mapping_hash = bot.mapping.map(|m| self.grammar.single_tags_list[m.0].hash);
                let mut h: u32 = 0;
                for &iter in bot.tags.iter() {
                    let fold = match mapping_hash {
                        None => true,
                        Some(mh) => mh.get() != iter,
                    };
                    if fold {
                        h = hash_value(iter, h);
                    }
                }
                bot.hash_plain = h;
                if let Some(mh) = mapping_hash {
                    h = hash_value(mh.get(), h);
                }
                bot.hash = h;
            }
        }

        thash
    }

    // =======================================================================
    // delTagFromReading
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.del-tag-from-reading-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.del-tag-from-reading-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.del-tag-from-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.del-tag-from-reading-fn]
    /// C++ `void delTagFromReading(Reading& reading, Tag* tag)` /
    /// `(Reading&, uint32_t utag)` — removes a tag by hash and refreshes state.
    /// BUGS reproduced: the bloom filters, `tags_string`/`tags_string_hash`, and
    /// the window bag-of-tags are NOT updated.
    pub fn del_tag_from_reading(&mut self, reading: ReadingId, tag: TagId) {
        let utag = self.grammar.single_tags_list[tag.0].hash;
        self.del_tag_from_reading_hash(reading, utag);
    }

    /// The `uint32_t utag` (hash) form.
    pub fn del_tag_from_reading_hash(&mut self, reading: ReadingId, utag: TagHash) {
        let mapping_hash = self
            .store
            .readings
            .get(reading.0)
            .mapping
            .map(|m| self.grammar.single_tags_list[m.0].hash);
        {
            let r = self.store.readings.get_mut(reading.0);
            erase(&mut r.tags_list, &utag.get());
            r.tags.erase(utag.get());
            r.tags_textual.erase(utag.get());
            r.tags_numerical.remove(&utag.get());
            r.tags_plain.erase(utag.get());
        }
        if let Some(mh) = mapping_hash
            && utag == mh {
                self.store.readings.get_mut(reading.0).mapping = None;
            }
        if self.store.readings.get(reading.0).baseform == Some(utag) {
            self.store.readings.get_mut(reading.0).baseform = None;
        }
        reading_rehash(&mut self.store, &self.grammar, reading);
        let parent = self.store.readings.get(reading.0).parent.unwrap();
        self.store.cohorts.get_mut(parent.0).r#type &= !CT_NUM_CURRENT;
    }

    // =======================================================================
    // unmapReading
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.unmap-reading-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.unmap-reading-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.unmap-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.unmap-reading-fn]
    /// C++ `bool unmapReading(Reading& reading, const uint32_t rule)` — removes a
    /// reading's mapping + mapped state, recording the responsible rule.
    pub fn unmap_reading(&mut self, reading: ReadingId, rule: u32) -> bool {
        let mut readings_changed = false;
        let mapping = self.store.readings.get(reading.0).mapping;
        if let Some(m) = mapping {
            self.store.readings.get_mut(reading.0).noprint = false;
            let mh = self.grammar.single_tags_list[m.0].hash;
            self.del_tag_from_reading_hash(reading, mh);
            readings_changed = true;
        }
        if self.store.readings.get(reading.0).mapped {
            self.store.readings.get_mut(reading.0).mapped = false;
            readings_changed = true;
        }
        if readings_changed {
            self.store.readings.get_mut(reading.0).hit_by.push(rule);
        }
        readings_changed
    }

    // =======================================================================
    // splitMappings / splitAllMappings
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.split-mappings-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.split-mappings-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.split-mappings-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.split-mappings-fn]
    /// C++ `void splitMappings(TagList& mappings, Cohort& cohort, Reading&
    /// reading, bool mapped)` — splits a reading into one per mapping tag.
    pub fn split_mappings(
        &mut self,
        mappings: &mut crate::tag::TagList,
        cohort: CohortId,
        reading: ReadingId,
        mapped: bool,
    ) {
        // First pass (mutating): expand varstrings; hoist non-mapping tags.
        let mapping_prefix = self.grammar.mapping_prefix;
        let mut idx = 0usize;
        while idx < mappings.len() {
            let mut t = mappings[idx];
            while self.grammar.single_tags_list[t.0]
                .r#type
                .intersects(T_VARSTRING)
            {
                let tval = self.grammar.single_tags_list[t.0].clone();
                t = self.generate_varstring_tag(&tval);
                mappings[idx] = t;
            }
            let (ttype, first_char) = {
                let tg = &self.grammar.single_tags_list[t.0];
                (tg.r#type, tg.tag.chars().next().unwrap_or('\0'))
            };
            if !(ttype.intersects(T_MAPPING) || first_char == mapping_prefix) {
                self.add_tag_to_reading(reading, t);
                mappings.remove(idx);
            } else {
                idx += 1;
            }
        }

        // If the reading already has a mapping, fold it into `mappings`.
        if let Some(m) = self.store.readings.get(reading.0).mapping {
            mappings.push(m);
            let mh = self.grammar.single_tags_list[m.0].hash;
            self.del_tag_from_reading_hash(reading, mh);
        }

        // Reuse the last mapping for the original reading.
        let tag = mappings.pop().unwrap();
        let mut i = mappings.len();

        let mappings_snapshot = mappings.clone();
        for ttag in mappings_snapshot {
            // Dedup against an existing cohort reading with the same hash_plain
            // and this mapping.
            let ttag_hash = self.grammar.single_tags_list[ttag.0].hash;
            let rp = self.store.readings.get(reading.0).hash_plain;
            let mut found = false;
            for &itr in &self.store.cohorts.get(cohort.0).readings.clone() {
                let (ihp, imap) = {
                    let r = self.store.readings.get(itr.0);
                    (r.hash_plain, r.mapping)
                };
                if ihp == rp
                    && let Some(im) = imap
                        && self.grammar.single_tags_list[im.0].hash == ttag_hash {
                            found = true;
                            break;
                        }
            }
            if found {
                continue;
            }
            // nr = alloc_reading(reading); nr->mapped; nr->number = number - i--.
            let rval = clone_reading_value(self.store.readings.get(reading.0));
            let nr = alloc_reading_copy(&mut self.store, &rval);
            let reading_number = self.store.readings.get(reading.0).number;
            {
                let n = self.store.readings.get_mut(nr.0);
                n.mapped = mapped;
                // reading.number - i-- in size_t (unsigned wraparound), UI32-truncated.
                n.number = ui32((reading_number as usize).wrapping_sub(i));
            }
            i -= 1;
            let mp = self.add_tag_to_reading(nr, ttag);
            if mp != ttag_hash {
                let mtid = self.grammar.single_tags.find(mp.get()).get().1;
                self.store.readings.get_mut(nr.0).mapping = Some(mtid);
            } else {
                self.store.readings.get_mut(nr.0).mapping = Some(ttag);
            }
            crate::cohort::append_reading(&mut self.store, cohort, nr);
            self.numReadings = self.numReadings.wrapping_add(1);
        }

        // The original reading takes the reserved `tag`.
        self.store.readings.get_mut(reading.0).mapped = mapped;
        let tag_hash = self.grammar.single_tags_list[tag.0].hash;
        let mp = self.add_tag_to_reading(reading, tag);
        if mp != tag_hash {
            let mtid = self.grammar.single_tags.find(mp.get()).get().1;
            self.store.readings.get_mut(reading.0).mapping = Some(mtid);
        } else {
            self.store.readings.get_mut(reading.0).mapping = Some(tag);
        }
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.split-all-mappings-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.split-all-mappings-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.split-all-mappings-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.split-all-mappings-fn]
    /// C++ `void splitAllMappings(all_mappings_t& all_mappings, Cohort& cohort,
    /// bool mapped)`.
    pub fn split_all_mappings(
        &mut self,
        all_mappings: &mut super::all_mappings_t,
        cohort: CohortId,
        mapped: bool,
    ) {
        if all_mappings.is_empty() {
            return;
        }
        let readings: ReadingList = self.store.cohorts.get(cohort.0).readings.clone();
        for reading in readings {
            let mut mlist = match all_mappings.remove(&reading) {
                Some(m) => m,
                None => continue,
            };
            self.split_mappings(&mut mlist, cohort, reading, mapped);
        }
        // std::sort(cohort.readings, Reading::cmp_number).
        self.sort_cohort_readings(cohort);
        if !self.grammar.reopen_mappings.empty() {
            let rs = self.store.cohorts.get(cohort.0).readings.clone();
            for reading in rs {
                if let Some(m) = self.store.readings.get(reading.0).mapping {
                    let mh = self.grammar.single_tags_list[m.0].hash;
                    if self.grammar.reopen_mappings.count(mh.get()) != 0 {
                        self.store.readings.get_mut(reading.0).mapped = false;
                    }
                }
            }
        }
        all_mappings.clear();
    }

    /// `std::sort(cohort.readings.begin(), .end(), Reading::cmp_number)` — a
    /// store-aware sort helper (the comparator reads two readings' scalars).
    fn sort_cohort_readings(&mut self, cohort: CohortId) {
        let mut v = self.store.cohorts.get(cohort.0).readings.clone();
        v.sort_by(|&a, &b| {
            let ra = self.store.readings.get(a.0);
            let rb = self.store.readings.get(b.0);
            if Reading::cmp_number(ra, rb) {
                std::cmp::Ordering::Less
            } else if Reading::cmp_number(rb, ra) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        self.store.cohorts.get_mut(cohort.0).readings = v;
    }

    // =======================================================================
    // mergeReadings / mergeMappings
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.merge-readings-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.merge-readings-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.merge-readings-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.merge-readings-fn]
    /// C++ `void mergeReadings(ReadingList& readings)` — collapses readings that
    /// differ only by mapping tags into one carrying all those mappings.
    pub fn merge_readings(&mut self, readings: &mut ReadingList) {
        // mapped: hplain → (nm, Reading); mlist: hkey → ReadingList (BTreeMap so
        // the rebuild iterates in key order == the C++ bc::flat_map order).
        let mut mapped: std::collections::BTreeMap<u32, (u32, ReadingId)> =
            std::collections::BTreeMap::new();
        let mut mlist: std::collections::BTreeMap<u32, ReadingList> =
            std::collections::BTreeMap::new();

        for &r in readings.iter() {
            let (mut hp, mut hplain) = {
                let rr = self.store.readings.get(r.0);
                if self.ordered {
                    (rr.tags_string_hash, rr.tags_string_hash)
                } else {
                    (rr.hash_plain, rr.hash_plain)
                }
            };
            let mut nm = 0u32;
            if self.trace {
                for &hb in &self.store.readings.get(r.0).hit_by.clone() {
                    hp = hash_value(hb, hp);
                }
            }
            if self.store.readings.get(r.0).mapping.is_some() {
                nm += 1;
            }
            let mut sub = self.store.readings.get(r.0).next;
            while let Some(s) = sub {
                if self.ordered {
                    let sh = self.store.readings.get(s.0).tags_string_hash;
                    hp = hash_value(sh, hp);
                    hplain = hash_value(sh, hplain);
                } else {
                    let sh = self.store.readings.get(s.0).hash_plain;
                    hp = hash_value(sh, hp);
                    hplain = hash_value(sh, hplain);
                }
                if self.trace {
                    for &hb in &self.store.readings.get(s.0).hit_by.clone() {
                        hp = hash_value(hb, hp);
                    }
                }
                if self.store.readings.get(s.0).mapping.is_some() {
                    nm += 1;
                }
                sub = self.store.readings.get(s.0).next;
            }

            if let Some(&(cnt, stored)) = mapped.get(&hplain) {
                if cnt != 0 && nm == 0 {
                    self.store.readings.get_mut(r.0).deleted = true;
                } else if cnt != nm && cnt == 0 {
                    self.store.readings.get_mut(stored.0).deleted = true;
                }
            }
            mapped.insert(hplain, (nm, r));
            mlist.entry(hp.wrapping_add(nm)).or_default().push(r);
        }

        if mlist.len() == readings.len() {
            return;
        }

        readings.clear();
        let mut order: Vec<ReadingId> = Vec::new();

        for (_key, clist) in mlist {
            let front = clist[0];
            let front_val = clone_reading_value(self.store.readings.get(front.0));
            let nr = alloc_reading_copy(&mut self.store, &front_val);
            if let Some(m) = self.store.readings.get(nr.0).mapping {
                let mh = self.grammar.single_tags_list[m.0].hash;
                erase(&mut self.store.readings.get_mut(nr.0).tags_list, &mh.get());
            }
            for iter1 in clist {
                let imap = self.store.readings.get(iter1.0).mapping;
                if let Some(im) = imap {
                    let imh = self.grammar.single_tags_list[im.0].hash;
                    let present = self
                        .store
                        .readings
                        .get(nr.0)
                        .tags_list
                        .contains(&imh.get());
                    if !present {
                        self.store.readings.get_mut(nr.0).tags_list.push(imh.get());
                    }
                }
                let opt = Some(iter1);
                free_reading(&mut self.store, opt);
            }
            order.push(nr);
        }

        order.sort_by(|&a, &b| {
            let ra = self.store.readings.get(a.0);
            let rb = self.store.readings.get(b.0);
            if Reading::cmp_number(ra, rb) {
                std::cmp::Ordering::Less
            } else if Reading::cmp_number(rb, ra) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        // readings.insert(begin, order.begin(), order.end())
        *readings = order;
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.merge-mappings-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.merge-mappings-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.merge-mappings-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.merge-mappings-fn]
    /// C++ `void mergeMappings(Cohort& cohort)`.
    pub fn merge_mappings(&mut self, cohort: CohortId) {
        let mut rs = self.store.cohorts.get(cohort.0).readings.clone();
        self.merge_readings(&mut rs);
        self.store.cohorts.get_mut(cohort.0).readings = rs;
        if self.trace {
            let mut del = self.store.cohorts.get(cohort.0).deleted.clone();
            self.merge_readings(&mut del);
            self.store.cohorts.get_mut(cohort.0).deleted = del;
            let mut dly = self.store.cohorts.get(cohort.0).delayed.clone();
            self.merge_readings(&mut dly);
            self.store.cohorts.get_mut(cohort.0).delayed = dly;
        }
    }

    // =======================================================================
    // delimitAt
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.delimit-at-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.delimit-at-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.delimit-at-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.delimit-at-fn]
    /// C++ `Cohort* delimitAt(SingleWindow& current, Cohort* cohort)` — splits
    /// `current` after `cohort` into a fresh following window and returns
    /// `current`'s new last cohort (which receives the END tag). `current.parent`
    /// (the owning `Window`) resolves to `self.gWindow` (the engine singleton).
    pub fn delimit_at(&mut self, current: SwId, cohort: CohortId) -> CohortId {
        let mut cohort = cohort;
        let mut nwin: Option<SwId> = None;
        if self.gWindow.current == Some(current) {
            nwin = Some(self.gWindow.alloc_push_single_window(&mut self.store));
        } else {
            // Search next for `current`, insert nwin after it.
            if let Some(pos) = self.gWindow.next.iter().position(|&w| w == current) {
                let n = self.gWindow.alloc_single_window(&mut self.store);
                self.gWindow.next.insert(pos + 1, n);
                nwin = Some(n);
            }
            if nwin.is_none()
                && let Some(pos) = self.gWindow.previous.iter().position(|&w| w == current) {
                    let n = self.gWindow.alloc_single_window(&mut self.store);
                    self.gWindow.previous.insert(pos, n);
                    nwin = Some(n);
                }
            self.gWindow.rebuild_single_window_links(&mut self.store);
        }

        let nwin = nwin.expect("delimitAt: nwin != 0");

        // Move window-trailing state onto nwin (std::swap flush_after/text_post,
        // copy has_enclosures).
        {
            let (cf, ct, ce) = {
                let c = self.store.single_windows.get(current.0);
                (c.flush_after, c.text_post.clone(), c.has_enclosures)
            };
            let (nf, nt) = {
                let n = self.store.single_windows.get(nwin.0);
                (n.flush_after, n.text_post.clone())
            };
            {
                let c = self.store.single_windows.get_mut(current.0);
                c.flush_after = nf;
                c.text_post = nt;
            }
            {
                let n = self.store.single_windows.get_mut(nwin.0);
                n.flush_after = cf;
                n.text_post = ct;
                n.has_enclosures = ce;
            }
        }

        // Build a synthetic BEGIN cohort in nwin.
        let ccohort = crate::cohort::alloc_cohort(&mut self.store, Some(nwin));
        {
            let gn = self.gWindow.cohort_counter;
            self.gWindow.cohort_counter = self.gWindow.cohort_counter.wrapping_add(1);
            let c = self.store.cohorts.get_mut(ccohort.0);
            c.global_number = gn;
            c.wordform = self.tag_begin;
        }
        let creading = crate::reading::alloc_reading(&mut self.store, Some(ccohort));
        self.store.readings.get_mut(creading.0).baseform = Some(self.begintag);
        insert_if_exists(
            &mut self.store.cohorts.get_mut(ccohort.0).possible_sets,
            self.grammar.sets_any.as_ref(),
        );
        let begintag_tid = self.grammar.single_tags.find(self.begintag.get()).get().1;
        self.add_tag_to_reading(creading, begintag_tid);
        crate::cohort::append_reading(&mut self.store, ccohort, creading);
        crate::single_window::append_cohort(&mut self.gWindow, &mut self.store, nwin, ccohort);
        // C++ SingleWindow::appendCohort: if (cohort->dep_self)
        // parent->parent->dep_highest_seen = cohort->dep_self;
        {
            if let Some(ds) = self.store.cohorts.get(ccohort.0).dep_self {
                self.dep_highest_seen = ds;
            }
        }

        // Relocate the tail: from the cohort just after `cohort` (found from lc).
        let lc = self.store.cohorts.get(cohort.0).local_number as usize;
        let all_len = self.store.single_windows.get(current.0).all_cohorts.len();
        let mut nc = lc;
        while nc < all_len {
            if self.store.single_windows.get(current.0).all_cohorts[nc] == cohort {
                break;
            }
            nc += 1;
        }
        nc += 1; // ++nc : first cohort after `cohort`
        let from = nc;
        let all_tail: Vec<CohortId> =
            self.store.single_windows.get(current.0).all_cohorts[from..].to_vec();
        for c in all_tail {
            self.store.cohorts.get_mut(c.0).parent = Some(nwin);
            let ty = self.store.cohorts.get(c.0).r#type;
            if ty.intersects(CT_ENCLOSED | CT_REMOVED | CT_IGNORED) {
                self.store
                    .single_windows
                    .get_mut(nwin.0)
                    .all_cohorts
                    .push(c);
            } else {
                crate::single_window::append_cohort(&mut self.gWindow, &mut self.store, nwin, c);
                // C++ SingleWindow::appendCohort: if (cohort->dep_self)
                // parent->parent->dep_highest_seen = cohort->dep_self;
                {
                    if let Some(ds) = self.store.cohorts.get(c.0).dep_self {
                        self.dep_highest_seen = ds;
                    }
                }
            }
        }
        // Truncate current: cohorts to [0..=lc], all_cohorts to [0..from).
        {
            let sw = self.store.single_windows.get_mut(current.0);
            sw.cohorts.truncate(lc + 1);
            sw.all_cohorts.truncate(from);
        }

        // cohort = current.cohorts.back(); addTagToReading(*reading, endtag).
        cohort = *self
            .store
            .single_windows
            .get(current.0)
            .cohorts
            .last()
            .unwrap();
        let endtag_tid = self.grammar.single_tags.find(self.endtag.get()).get().1;
        let rs = self.store.cohorts.get(cohort.0).readings.clone();
        for reading in rs {
            self.add_tag_to_reading(reading, endtag_tid);
        }
        let gw = &self.gWindow;
        gw.rebuild_cohort_links(&mut self.store);

        cohort
    }

    // =======================================================================
    // reflowTextuals*
    // =======================================================================

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-reading-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-reading-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-reading-fn]
    /// C++ `void reflowTextuals_Reading(Reading& r)` — re-derives a reading's
    /// `tags_textual` (and its bloom) by scanning `r.tags`, recursing into the
    /// `next` sub-reading chain first. ADD-only (does not clear first).
    pub fn reflow_textuals_reading(&mut self, r: ReadingId) {
        if let Some(next) = self.store.readings.get(r.0).next {
            self.reflow_textuals_reading(next);
        }
        let tags: Vec<u32> = self.store.readings.get(r.0).tags.as_slice().to_vec();
        for it in tags {
            let tid = self.grammar.single_tags.find(it).get().1;
            if self.grammar.single_tags_list[tid.0]
                .r#type
                .intersects(T_TEXTUAL)
            {
                let rr = self.store.readings.get_mut(r.0);
                rr.tags_textual.insert(it);
                rr.tags_textual_bloom.insert(it);
            }
        }
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-cohort-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-cohort-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-cohort-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-cohort-fn]
    /// C++ `void reflowTextuals_Cohort(Cohort& c)` — over `readings`, `deleted`,
    /// `ignored`, `delayed` (in that order).
    pub fn reflow_textuals_cohort(&mut self, c: CohortId) {
        for list in [
            self.store.cohorts.get(c.0).readings.clone(),
            self.store.cohorts.get(c.0).deleted.clone(),
            self.store.cohorts.get(c.0).ignored.clone(),
            self.store.cohorts.get(c.0).delayed.clone(),
        ] {
            for it in list {
                self.reflow_textuals_reading(it);
            }
        }
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-single-window-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-single-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-single-window-fn]
    /// C++ `void reflowTextuals_SingleWindow(SingleWindow& sw)` — over
    /// `sw.all_cohorts`.
    pub fn reflow_textuals_single_window(&mut self, sw: SwId) {
        let cohorts = self.store.single_windows.get(sw.0).all_cohorts.clone();
        for it in cohorts {
            self.reflow_textuals_cohort(it);
        }
    }

    // [spec:cg3:def:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-fn]
    // [spec:cg3:sem:grammar-applicator-reflow.cg3.grammar-applicator.reflow-textuals-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.reflow-textuals-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.reflow-textuals-fn]
    /// C++ `void reflowTextuals()` — `previous`, then `current`, then `next`.
    pub fn reflow_textuals(&mut self) {
        for sw in self.gWindow.previous.clone() {
            self.reflow_textuals_single_window(sw);
        }
        if let Some(cur) = self.gWindow.current {
            self.reflow_textuals_single_window(cur);
        }
        for sw in self.gWindow.next.clone() {
            self.reflow_textuals_single_window(sw);
        }
    }
}

// Wave 4 (w4-file-split-fmt): the verbatim Reading field-copy is
// consolidated in `crate::reading::clone_verbatim`.
use crate::reading::clone_verbatim as clone_reading_value;
