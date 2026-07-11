//! `src/GrammarApplicator_runRules.cpp` impl of GrammarApplicator.
//!
//! LITERAL bug-for-bug port of the rule-application engine. The flagged CG-3
//! quirks are reproduced deliberately:
//!   * `run_single_rule` self-reorders `rule.tests` on a failing context test
//!     (moves the failing test to the front) — a mutation of the "const" rule
//!     via C++ `mutable`; here it writes back into the grammar arena.
//!   * `update_rule_to_cohorts` performs a live-iterator-safe insert into a
//!     `CohortSet` that is currently being iterated by an active `run_single_rule`
//!     frame (the `cohortsets`/`rocits` raw-pointer bookkeeping).
//!
//! RECONCILIATION NOTES (see crate report): this file assumes the applicator
//! grows a `store: RuntimeStore` field, that `SingleWindow::rule_to_cohorts`
//! becomes `Vec<CohortSet>` (and `nested_rule_to_cohorts` `Option<Box<CohortSet>>`),
//! that `CohortSet` sort/insert can resolve the store-aware `compare_Cohort`, and
//! calls the sibling engine methods (matchSet / runContextualTest / reflow /
//! context / core) by their C++-matching signatures — none of which exist yet.

use crate::arena::{RuleId, SwId};
use crate::interval_vector::uint32IntervalVector;
use crate::reading::ReadingList;
use crate::strings::KEYWORDS::*;

// C++ anonymous `enum { RV_NOTHING = 1, RV_SOMETHING = 2, RV_DELIMITED = 4,
// RV_TRACERULE = 8 };` — the return-value bit flags of runRulesOnSingleWindow.

mod dispatch;
mod restructure;
mod schedule;
mod single_rule;
mod window;

const RV_NOTHING: u32 = 1;
const RV_SOMETHING: u32 = 2;
const RV_DELIMITED: u32 = 4;
const RV_TRACERULE: u32 = 8;

/// C++ `constexpr int GSR_ANY = 32767` — the "amalgamate all sub-readings"
/// sentinel for `get_sub_reading` / `rule.sub_reading`.
const GSR_ANY: i32 = 32767;

/// Expand a `uint32IntervalVector` to the ascending list of its member values
/// (the C++ `for (auto v : iv)`). Not a manifest symbol — iteration helper.
fn iv_to_vec(iv: &uint32IntervalVector) -> Vec<u32> {
    let mut out = Vec::new();
    let mut it = iv.begin();
    let end = iv.end();
    while it != end {
        out.push(it.value());
        it.advance();
    }
    out
}

/// The shared, mutable per-`runRulesOnSingleWindow` state that C++ captures by
/// reference into the `reading_cb`/`cohort_cb` closures and the helper lambdas.
/// The C++ closures alias `this` and these locals through the stack; the port
/// threads this struct explicitly (`&mut RRState`) into the dispatch/helper
/// methods, while the two `RuleCallback` trampolines carry raw `*mut Self` +
/// `*mut RRState` (reproducing the C++ aliasing, matching the raw-pointer design
/// the applicator struct already uses for `cohortsets`/`rocits`). Not a manifest
/// symbol.
pub(crate) struct RRState {
    pub(crate) current: SwId,
    /// The `rules` parameter (read-only working set).
    pub(crate) rules: uint32IntervalVector,
    /// `current.valid_rules.intersect(rules)` — grows as tags are added.
    pub(crate) intersects: uint32IntervalVector,
    /// The current rule (`rule`); WITH temporarily reassigns it.
    pub(crate) rule: RuleId,
    /// The re-seatable outer cursor value (`*iter_rules`).
    pub(crate) iter_val: u32,
    pub(crate) removed: ReadingList,
    pub(crate) selected: ReadingList,
    pub(crate) readings_changed: bool,
    pub(crate) should_repeat: bool,
    pub(crate) should_bail: bool,
    pub(crate) delimited: bool,
    /// `Sorter::do_sort` — re-sort every rule_to_cohorts when the rule finishes.
    pub(crate) do_sort: bool,
}

/// First member value of an interval set, or `None` when empty.
fn iv_first(iv: &uint32IntervalVector) -> Option<u32> {
    if iv.empty() { None } else { Some(iv.front()) }
}

/// First member value strictly greater than `v` (the C++ `++iter_rules`).
fn iv_next_after(iv: &uint32IntervalVector, v: u32) -> Option<u32> {
    let lb = iv.lower_bound(v.wrapping_add(1));
    if lb == iv.end() {
        None
    } else {
        Some(lb.value())
    }
}

/// C++ `u_sscanf(str, "%[0-9cd]->%[0-9pm]", &dep_self, &dep_parent) == 2`.
/// Splits on the literal `"->"` and validates each side against its scanset:
/// the left side accepts only `[0-9cd]`, the right only `[0-9pm]`. A scanset
/// match consumes the maximal leading run of accepted chars (may be empty →
/// the scanf field is empty but still "matched"); both fields must be present
/// (two conversions) for the whole match to succeed. Any char outside the
/// scanset simply terminates that field (later chars are ignored by `%[...]`),
/// but here the `->` delimiter and end-of-string bound the fields, so an
/// out-of-scanset char before `->` means the arrow won't be found at that point
/// — reproduced by requiring the ENTIRE side to be within the scanset.
fn split_dep_mapping(s: &str) -> Option<(String, String)> {
    let idx = s.find("->")?;
    let left = &s[..idx];
    let right = &s[idx + 2..];
    // `%[0-9cd]` consumes the leading run of accepted chars; the field matches
    // (possibly empty) only if the run reaches the `->` (i.e. every char before
    // the arrow is in-scanset — otherwise the arrow is past a rejected char and
    // scanf's `%[...]` would have stopped, so `->` never lines up).
    if !left
        .chars()
        .all(|c| c.is_ascii_digit() || c == 'c' || c == 'd')
    {
        return None;
    }
    // `%[0-9pm]` consumes to end-of-string (bounded by NUL); it always "matches"
    // (empty run allowed) but the second conversion only counts if scanning got
    // this far, which it did. Extra out-of-scanset chars after the run are left
    // unconsumed but the conversion still succeeded.
    let run_end = right
        .char_indices()
        .find(|&(_, c)| !(c.is_ascii_digit() || c == 'p' || c == 'm'))
        .map(|(i, _)| i)
        .unwrap_or(right.len());
    Some((left.to_string(), right[..run_end].to_string()))
}

/// C++ `u_sscanf(field, "%i", &out) == 1` for the dep-mapping numeric fields.
/// `%i` accepts an optional sign and (via C strtol base 0) `0x`/`0` prefixes;
/// the fields here only ever hold `[0-9]` runs (the scanset filtered the rest),
/// so a plain unsigned decimal parse of the leading digit run is faithful. An
/// empty/non-numeric field fails the conversion (returns `None`).
fn parse_scanf_i(field: &str) -> Option<u32> {
    let digits: String = field.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}
