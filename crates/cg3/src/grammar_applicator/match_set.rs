//! `src/GrammarApplicator_matchSet.cpp` impl of GrammarApplicator (the
//! regex/set/tag/cohort matchers). Literal, bug-for-bug Wave-2 port.
//!
//! ============================================================================
//! CROSS-PARTIAL / SCAFFOLD DEPENDENCIES (NOT edited here — see task report)
//! ============================================================================
//! * REQUIRED mod.rs FIELD (missing from the current scaffold): the applicator
//!   must own `pub store: RuntimeStore` (`crate::store::RuntimeStore`). The
//!   cohort/reading matchers resolve `ReadingId`/`CohortId` via `self.store`
//!   exactly as the task brief states (`self.store.readings[rid.0]`). Add:
//!   pub store: crate::store::RuntimeStore,
//!   (default `RuntimeStore::new()` in `GrammarApplicator::new`).
//!
//! * SIBLING GrammarApplicator methods CALLED here but DEFINED in other
//!   partials (do NOT define them here — that would duplicate). Expected
//!   signatures (arena-model adaptations of the C++ pointer forms):
//!     - reflow:   fn generate_varstring_tag(&mut self, tag: &Tag) -> TagId
//!       (C++ `generateVarstringTag(const Tag*) -> const Tag*`)
//!     - core/ctx: fn get_sub_reading(&mut self, r: ReadingId, offset_sub: i32)
//!       -> Option<ReadingId>   (C++ `get_sub_reading(Reading*, int32_t)`)
//!     - core/ctx: fn get_mark(&self) -> Option<CohortId>
//!     - core/ctx: fn get_attach_to(&self) -> &ReadingSpec   (uses `.cohort`)
//!     - runCtx:   fn run_contextual_test(&mut self, sw: Option<SwId>,
//!       local: u32, test: CtxId,
//!       deep: Option<*mut Option<CohortId>>,
//!       origin: Option<CohortId>) -> Option<CohortId>
//!     - runCtx:   fn check_unif_tags(&mut self, set_number: u32,
//!       node: *const core::ffi::c_void) -> bool
//!
//! EXPOSED for the other engine agents (they call these):
//!     does_set_match_reading, does_set_match_reading_tags,
//!     does_set_match_reading_trie, does_tag_match_reading, does_tag_match_regexp,
//!     does_tag_match_icase, does_regexp_match_reading, does_regexp_match_line,
//!     does_set_match_cohort_normal, does_set_match_cohort_careful,
//!     does_set_match_cohort_helper, does_set_match_cohort_test_linked,
//!     get_tags_matching — all take `reading`/`cohort` as arena ids (ReadingId /
//!     CohortId), never `&Reading`/`&Cohort` (the arena lives inside `self`, so a
//!     `&Reading` borrowed from `self.store` cannot coexist with `&mut self`).
//!
//! REGEX MAPPING (ICU `uregex_find(-1)` == UNANCHORED search): a tag's
//! `regexp: Option<regex::Regex>` was compiled at parse time (`/.../` unanchored,
//! `"..."r`/`<...>r` anchored `^…$`, `(?i)` when T_CASE_INSENSITIVE). Matching is
//! `re.is_match(subject)` (unanchored, Unicode-by-default — the ICU semantics).
//! `uregex_groupCount` == `re.captures_len() - 1` (excludes group 0).
//! `captureRegex` re-runs `re.captures(subject)` (the regex crate has no stateful
//! "last match"; identical input+regex ⇒ identical leftmost captures) and appends
//! groups 1..=gc into the current context frame's `regexgrps` (a non-participating
//! group yields an empty string), advancing `regexgrp_ct`.
//!
//! CACHE conditions reproduced exactly (yes/no memo):
//!   - regexp: key `ih = make_64(tag.hash, test)` (line: `make_64(tags_string_hash,
//!     tag.hash)`); read `index_regexp_no` always; read `index_regexp_yes` only
//!     when `gc == 0`; on match, write `index_regexp_yes` ONLY when NOT capturing
//!     (i.e. not (gc>0 && ctx frame present && frame.regexgrps set)); on non-match
//!     write `index_regexp_no`.
//!   - icase: key `make_64(tag.hash, test)`; read/write `index_icase_{no,yes}`.
//!   - readingSet: `index_readingSet_{no,yes}[set]` keyed by `reading.hash`; only
//!     consulted/written when `!bypass_index && !unif_mode`; the negative cache is
//!     additionally skipped when the set is ST_TAG_UNIFY or `unif_mode`.
//!
//! CAVEATS the lead must reconcile (NOTED, not fixed):
//!   - trie node identity (FIXED in Wave 3): `does_set_match_reading` no longer
//!     clones the set's tries; it launders `&Set::{ff_tags,trie,trie_special}`
//!     borrows out of `self.grammar` (safe: set tries are parse-time-immutable),
//!     so the `*const trie_node_t` handed to `check_unif_tags` is the address of
//!     the grammar-owned node — stable across calls and findable by run_rules'
//!     `get_tag_list` → `trie_get_tag_list_find` pointer-identity walk, matching
//!     the C++ `&kv` semantics.
//!   - `TagSet_SubsetOf_TSet` / `Set::ff_tags` order by the placeholder
//!     `compare_Tag` (TagId order, not Tag::hash) — the merge assumes hash order;
//!     correct once `compare_Tag` is arena-hash-aware.
//!   - trie iteration order: `trie_t` is a `BTreeMap<TagId,_>` (TagId order); the
//!     C++ flat_map iterates by `Tag::hash`. Re-derived here by hash-sorting the
//!     entries (stable), matching `tag_trie::ordered_entries`.

use regex::Regex;

use crate::arena::{CohortId, ReadingId, TagId};
use crate::cohort;
use crate::contextual_test::{
    MASK_POS_DEPREL, POS_ACTIVE, POS_ATTACH_TO, POS_CAREFUL, POS_INACTIVE, POS_LOOK_DELAYED,
    POS_LOOK_DELETED, POS_LOOK_IGNORED, POS_NO_PASS_ORIGIN, POS_NOT,
};
use crate::grammar::Grammar;
use crate::inlines::{NUMERIC_MAX, NUMERIC_MIN, hash_value_ustring, make_64};
use crate::math_parser::MathParser;
use crate::rule::RF_CAPTURE_UNIF;
use crate::set::{ST_ANY, ST_CHILD_UNIFY, ST_SET_UNIFY, ST_SPECIAL, ST_TAG_UNIFY};
use crate::sorted_vector::uint32SortedVector;
use crate::store::RuntimeStore;
use crate::tag::{
    C_OPS, T_ATTACHTO, T_BASEFORM, T_CASE_INSENSITIVE, T_CONTEXT, T_ENCL, T_FAILFAST,
    T_LOCAL_VARIABLE, T_MARK, T_META, T_NUMERIC_MATH, T_NUMERICAL, T_PAR_LEFT, T_PAR_RIGHT,
    T_REGEXP, T_REGEXP_ANY, T_REGEXP_LINE, T_SAME_BASIC, T_SET, T_SPECIAL, T_TARGET, T_TEXTUAL,
    T_VARIABLE, T_VARSTRING, T_WORDFORM, Tag, TagList, TagSortedVector,
};
use crate::tag_trie::trie_t;
use crate::types::{SetNumber, TagHash, UString};

use super::{dSMC_Context, regexgrps_t};

// C++ Strings.hpp set-operator enum values (`S_IGNORE, S_OR=3, S_PLUS, S_MINUS,
// ... S_FAILFAST=8`). Only the four `doesSetMatchReading` uses are reproduced.
const S_OR: u32 = 3;
const S_PLUS: u32 = 4;
const S_MINUS: u32 = 5;
const S_FAILFAST: u32 = 8;

// ===========================================================================
// Free helpers (this file's namespace, matching the C++ translation unit).
// ===========================================================================

// [spec:cg3:def:grammar-applicator-match-set.cg3.capture-regex-fn]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.capture-regex-fn]
/// C++ template `captureRegex(int32_t gc, uint8_t& regexgrp_ct, RXGS* regexgrps,
/// Tag& tag)`. Harvests capture groups 1..=gc (group 0, the whole match, is
/// deliberately NOT captured) of the last successful match into `regexgrps`,
/// starting at `regexgrp_ct` and advancing it by `gc`. The C++ read them from
/// the ICU regex object's stateful last match via `uregex_group`; the `regex`
/// crate has no such state, so `regexp` + the matched `input` are threaded in and
/// `regexp.captures(input)` is re-run (identical leftmost captures). A group that
/// did not participate yields an empty string (ICU returned len 0). Never shrinks
/// `regexgrps` (`resize(max(regexgrp_ct+1, size))`).
fn capture_regex(
    gc: i32,
    regexgrp_ct: &mut u8,
    regexgrps: &mut regexgrps_t,
    regexp: &Regex,
    input: &str,
) {
    let caps = regexp.captures(input);
    let mut i = 1i32;
    while i <= gc {
        let text: UString = match &caps {
            Some(c) => c
                .get(i as usize)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            None => UString::new(),
        };
        let need = (*regexgrp_ct as usize) + 1;
        if regexgrps.len() < need {
            regexgrps.resize(need, UString::new());
        }
        let slot = &mut regexgrps[*regexgrp_ct as usize];
        slot.clear(); // ucstr.remove()
        slot.push_str(&text); // ucstr.append(tmp, len)
        *regexgrp_ct = regexgrp_ct.wrapping_add(1);
        i += 1;
    }
}

// [spec:cg3:def:grammar-applicator-match-set.cg3.check-options-fn]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.check-options-fn]
/// C++ `inline bool _check_options(std::vector<Reading*>& rv, uint32_t options,
/// size_t nr)`. DEAD CODE (defined in the translation unit, never called — the
/// live careful/normal logic is in the cohort matchers). Reproduced for
/// completeness. `rv` (the matched readings) → `&[ReadingId]`.
pub fn check_options(
    rv: &[ReadingId],
    options: crate::contextual_test::PosFlags,
    nr: usize,
) -> bool {
    if options.intersects(POS_CAREFUL) && rv.len() != nr {
        return false;
    }
    if options.intersects(MASK_POS_DEPREL) {
        return true;
    }
    !rv.is_empty()
}

// [spec:cg3:def:grammar-applicator-match-set.cg3.tag-set-subset-of-t-set-fn]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.tag-set-subset-of-t-set-fn]
/// C++ template `TagSet_SubsetOf_TSet(const TagSortedVector& a, const T& b)` —
/// true iff every tag of `a` (by hash) is present in `b` (a sorted container of
/// tag hashes; the concrete `T` in-tree is a reading's `uint32SortedVector`).
/// `grammar` resolves each `TagId`'s hash. EDGE: dereferences `a.begin()`
/// unconditionally (callers only pass a non-empty `a`). NOTE: relies on `a` being
/// hash-ordered; `TagSortedVector`'s current comparator is the TagId-order
/// placeholder (see file header caveat).
pub fn tag_set_subset_of_t_set(
    grammar: &Grammar,
    a: &TagSortedVector,
    b: &uint32SortedVector,
) -> bool {
    let a_slice = a.as_slice();
    let b_slice = b.as_slice();
    let first_hash = grammar.single_tags_list[a_slice[0].0].hash;
    let mut bi = b.lower_bound(first_hash.get());
    let bend = b.end();
    for &aid in a_slice {
        let ah = grammar.single_tags_list[aid.0].hash.get();
        while bi != bend && b_slice[bi] < ah {
            bi += 1;
        }
        if bi == bend || b_slice[bi] != ah {
            return false;
        }
    }
    true
}

// [spec:cg3:def:grammar-applicator-match-set.cg3.test-tag-numerical-fn]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.test-tag-numerical-fn]
/// C++ free fn `uint32_t test_tag_numerical(const Reading&, const Tag& tag,
/// const Tag& itag)`. Kept a free fn (arena model): `reading.parent->getMin/getMax`
/// need `store: &mut RuntimeStore` + `grammar: &Grammar`, so both are threaded in
/// (`Reading&` → `ReadingId`). Compares the query numeric tag against a reading's
/// numeric tag, returning `itag.hash` on a match else 0. `compval` derives from
/// the query `tag`; the threshold `V` and operator `B` from the reading's `itag`.
pub fn test_tag_numerical(
    store: &mut RuntimeStore,
    grammar: &Grammar,
    reading: ReadingId,
    tag: &Tag,
    itag: &Tag,
) -> TagHash {
    use C_OPS::*;
    let mut m = TagHash(0);
    if tag.comparison_hash != itag.comparison_hash {
        return TagHash(0);
    }
    let parent = store.readings.get(reading.0).parent.unwrap();
    let mut compval = tag.comparison_val;
    // `tag.comparison_offset` aliases the `dep_parent` union member (tag.rs).
    let comparison_offset = tag.comparison_offset() as usize;
    if tag.r#type.intersects(T_NUMERIC_MATH) && comparison_offset != 0 {
        let mn = cohort::get_min(store, grammar, parent, tag.comparison_hash);
        let mx = cohort::get_max(store, grammar, parent, tag.comparison_hash);
        let mut mp = MathParser::new(mn, mx);
        // exp = view(tag.tag).remove_prefix(comparison_offset).remove_suffix(1)
        let chars: Vec<char> = tag.tag.chars().collect();
        if comparison_offset < chars.len() {
            let exp: String = chars[comparison_offset..chars.len() - 1].iter().collect();
            // C++ `mp.eval(exp)` throws on error (uncaught here → terminate). The
            // safe analog: leave `compval` at the query value on Err (noted).
            if let Ok(v) = mp.eval(&exp) {
                compval = v;
            }
        }
    } else if compval <= NUMERIC_MIN {
        compval = cohort::get_min(store, grammar, parent, tag.comparison_hash);
    } else if compval >= NUMERIC_MAX {
        compval = cohort::get_max(store, grammar, parent, tag.comparison_hash);
    }

    let a = tag.comparison_op;
    let b = itag.comparison_op;
    let v = itag.comparison_val;
    // C++ if/else-if operator table: match on the (A, B) operator pair, with the
    // value comparison as an arm guard where the C++ arm has one.
    match (a, b) {
        (OP_EQUALS, OP_EQUALS) if compval == v => m = itag.hash,
        (OP_NOTEQUALS, OP_EQUALS) if compval != v => m = itag.hash,
        (OP_EQUALS, OP_NOTEQUALS) if compval != v => m = itag.hash,
        (OP_NOTEQUALS, OP_NOTEQUALS) if compval == v => m = itag.hash,
        (OP_EQUALS, OP_LESSTHAN) if compval < v => m = itag.hash,
        (OP_EQUALS, OP_LESSEQUALS) if compval <= v => m = itag.hash,
        (OP_EQUALS, OP_GREATERTHAN) if compval > v => m = itag.hash,
        (OP_EQUALS, OP_GREATEREQUALS) if compval >= v => m = itag.hash,
        (OP_NOTEQUALS, OP_LESSTHAN) => m = itag.hash,
        (OP_NOTEQUALS, OP_LESSEQUALS) => m = itag.hash,
        (OP_NOTEQUALS, OP_GREATERTHAN) => m = itag.hash,
        (OP_NOTEQUALS, OP_GREATEREQUALS) => m = itag.hash,
        (OP_LESSTHAN, OP_NOTEQUALS) => m = itag.hash,
        (OP_LESSEQUALS, OP_NOTEQUALS) => m = itag.hash,
        (OP_GREATERTHAN, OP_NOTEQUALS) => m = itag.hash,
        (OP_GREATEREQUALS, OP_NOTEQUALS) => m = itag.hash,
        (OP_LESSTHAN, OP_EQUALS) if compval > v => m = itag.hash,
        (OP_LESSEQUALS, OP_EQUALS) if compval >= v => m = itag.hash,
        (OP_LESSTHAN, OP_LESSTHAN) => m = itag.hash,
        (OP_LESSEQUALS, OP_LESSEQUALS) => m = itag.hash,
        (OP_LESSEQUALS, OP_LESSTHAN) => m = itag.hash,
        (OP_LESSTHAN, OP_LESSEQUALS) => m = itag.hash,
        (OP_LESSTHAN, OP_GREATERTHAN) if compval > v => m = itag.hash,
        (OP_LESSTHAN, OP_GREATEREQUALS) if compval > v => m = itag.hash,
        (OP_LESSEQUALS, OP_GREATERTHAN) if compval > v => m = itag.hash,
        (OP_LESSEQUALS, OP_GREATEREQUALS) if compval >= v => m = itag.hash,
        (OP_GREATERTHAN, OP_EQUALS) if compval < v => m = itag.hash,
        (OP_GREATEREQUALS, OP_EQUALS) if compval <= v => m = itag.hash,
        (OP_GREATERTHAN, OP_GREATERTHAN) => m = itag.hash,
        (OP_GREATEREQUALS, OP_GREATEREQUALS) => m = itag.hash,
        (OP_GREATEREQUALS, OP_GREATERTHAN) => m = itag.hash,
        (OP_GREATERTHAN, OP_GREATEREQUALS) => m = itag.hash,
        (OP_GREATERTHAN, OP_LESSTHAN) if compval < v => m = itag.hash,
        (OP_GREATERTHAN, OP_LESSEQUALS) if compval < v => m = itag.hash,
        (OP_GREATEREQUALS, OP_LESSTHAN) if compval < v => m = itag.hash,
        (OP_GREATEREQUALS, OP_LESSEQUALS) if compval <= v => m = itag.hash,
        _ => {}
    }
    m
}

/// `uextras.hpp` `ux_strCaseCompare(a, b)` (ICU `u_strCaseCompare` with
/// `U_FOLD_CASE_DEFAULT`): true on full case-fold equality. Approximated with
/// Unicode lowercase folding (ICU-vs-Rust parity risk for non-ASCII), mirroring
/// the `tag.rs` stand-in. Deliberately un-annotated (its spec id belongs to the
/// `uextras` port).
fn ux_str_case_compare(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// Collect a `Uint32FlatHashMap`'s live `(key, value)` entries in physical slot
/// order (the C++ flat_unordered_map iteration order, which `find_if` walks).
/// Not a manifest symbol — port infra so the variable branch can iterate while
/// mutating `self`.
fn collect_fum(m: &crate::flat_unordered_map::Uint32FlatHashMap) -> Vec<(u32, u32)> {
    m.iter().copied().collect()
}

/// `uregex_groupCount(tag.regexp)` — the number of capture groups EXCLUDING the
/// whole-match group 0. `regex::Regex::captures_len()` includes group 0, so
/// subtract one. 0 when the tag has no compiled regex.
fn group_count(tag: &Tag) -> i32 {
    tag.regexp
        .as_ref()
        .map(|re| re.captures_len() as i32 - 1)
        .unwrap_or(0)
}

impl super::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-regexp-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-regexp-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-regexp-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-regexp-fn]
    /// Tests whether input tag `test` matches regexp pattern `tag`, with a yes/no
    /// memo cache and optional capture harvesting.
    pub fn does_tag_match_regexp(&mut self, test: u32, tag: &Tag, bypass_index: bool) -> u32 {
        let gc = group_count(tag);
        let mut m: u32 = 0;
        let ih = make_64(tag.hash.get(), test);
        if !bypass_index && self.index_regexp_no.contains(ih) {
            m = 0;
        } else if !bypass_index && gc == 0 && self.index_regexp_yes.contains(ih) {
            m = test;
        } else {
            // itag = *(grammar->single_tags.find(test)->second)
            let (itag_hash, itag_text) = {
                let it = self.grammar.single_tags.find(test);
                let tid = it.get().1;
                let t = &self.grammar.single_tags_list[tid.0];
                (t.hash.get(), t.tag.clone())
            };
            // uregex_setText + uregex_find(-1) == unanchored `is_match`.
            if let Some(re) = &tag.regexp
                && re.is_match(&itag_text)
            {
                m = itag_hash;
            }
            if m != 0 {
                let capture = gc > 0
                    && !self.context_stack.is_empty()
                    && self.context_stack.last().unwrap().regexgrps.is_some();
                if capture {
                    if let Some(re) = &tag.regexp {
                        let idx = self.context_stack.last().unwrap().regexgrps.unwrap();
                        let frame = self.context_stack.last_mut().unwrap();
                        let rg = &mut self.regexgrps_store[idx];
                        capture_regex(gc, &mut frame.regexgrp_ct, rg, re, &itag_text);
                    }
                } else {
                    self.index_regexp_yes.insert(ih);
                }
            } else {
                self.index_regexp_no.insert(ih);
            }
        }
        m
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-icase-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-icase-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-icase-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-icase-fn]
    /// Case-insensitive whole-string equality of input tag `test` vs pattern
    /// `tag`, with a yes/no memo cache.
    pub fn does_tag_match_icase(&mut self, test: u32, tag: &Tag, bypass_index: bool) -> u32 {
        let mut m: u32 = 0;
        let ih = make_64(tag.hash.get(), test);
        if !bypass_index && self.index_icase_no.contains(ih) {
            m = 0;
        } else if !bypass_index && self.index_icase_yes.contains(ih) {
            m = test;
        } else {
            let (itag_hash, itag_text) = {
                let it = self.grammar.single_tags.find(test);
                let tid = it.get().1;
                let t = &self.grammar.single_tags_list[tid.0];
                (t.hash.get(), t.tag.clone())
            };
            if ux_str_case_compare(&tag.tag, &itag_text) {
                m = itag_hash;
            }
            if m != 0 {
                self.index_icase_yes.insert(ih);
            } else {
                self.index_icase_no.insert(ih);
            }
        }
        m
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-regexp-match-line-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-regexp-match-line-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-line-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-line-fn]
    /// Ordered-mode helper (C++ "ToDo: Remove for real ordered mode"): tests a
    /// regexp tag against the reading's concatenated `tags_string`.
    pub fn does_regexp_match_line(
        &mut self,
        reading: ReadingId,
        tag: &Tag,
        bypass_index: bool,
    ) -> u32 {
        let gc = group_count(tag);
        let mut m: u32 = 0;
        let (tsh, ts) = {
            let r = self.store.readings.get(reading.0);
            (r.tags_string_hash, r.tags_string.clone())
        };
        let ih = make_64(tsh, tag.hash.get());
        if !bypass_index && self.index_regexp_no.contains(ih) {
            m = 0;
        } else if !bypass_index && gc == 0 && self.index_regexp_yes.contains(ih) {
            m = tsh;
        } else {
            if let Some(re) = &tag.regexp
                && re.is_match(&ts)
            {
                m = tsh;
            }
            if m != 0 {
                let capture = gc > 0
                    && !self.context_stack.is_empty()
                    && self.context_stack.last().unwrap().regexgrps.is_some();
                if capture {
                    if let Some(re) = &tag.regexp {
                        let idx = self.context_stack.last().unwrap().regexgrps.unwrap();
                        let frame = self.context_stack.last_mut().unwrap();
                        let rg = &mut self.regexgrps_store[idx];
                        capture_regex(gc, &mut frame.regexgrp_ct, rg, re, &ts);
                    }
                } else {
                    self.index_regexp_yes.insert(ih);
                }
            } else {
                self.index_regexp_no.insert(ih);
            }
        }
        m
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-regexp-match-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-regexp-match-reading-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-reading-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-regexp-match-reading-fn]
    /// Tests whether any textual tag of a reading matches regexp `tag`. `T_REGEXP_LINE`
    /// delegates to `does_regexp_match_line`; otherwise the first matching
    /// `tags_textual` entry wins.
    pub fn does_regexp_match_reading(
        &mut self,
        reading: ReadingId,
        tag: &Tag,
        bypass_index: bool,
    ) -> u32 {
        if tag.r#type.intersects(T_REGEXP_LINE) {
            return self.does_regexp_match_line(reading, tag, bypass_index);
        }
        let textual: Vec<u32> = self
            .store
            .readings
            .get(reading.0)
            .tags_textual
            .as_slice()
            .to_vec();
        let mut m: u32 = 0;
        for mter in textual {
            m = self.does_tag_match_regexp(mter, tag, bypass_index);
            if m != 0 {
                break;
            }
        }
        m
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn]
    /// The central single-tag dispatcher. Mutually-exclusive branches on
    /// `tag.type` (first match wins). `reading` is an id (arena model); `tag` is an
    /// owned/borrowed pattern tag NOT aliasing `self.grammar` (callers clone it out
    /// of the arena before calling).
    pub fn does_tag_match_reading(
        &mut self,
        reading: ReadingId,
        tag: &Tag,
        unif_mode: bool,
        bypass_index: bool,
    ) -> u32 {
        let mut retval: u32 = 0;
        let mut m: u32 = 0;

        if !tag.r#type.intersects(T_SPECIAL) || tag.r#type.intersects(T_FAILFAST) {
            // (1) plain / fail-fast tag
            let r = self.store.readings.get(reading.0);
            let mut raw_in = r.tags_plain_bloom.matches(tag.hash.get());
            if tag.r#type.intersects(T_FAILFAST) {
                raw_in = r.tags_plain.find(tag.plain_hash.get()) != r.tags_plain.end();
            } else if raw_in {
                raw_in = r.tags_plain.find(tag.hash.get()) != r.tags_plain.end();
            }
            if raw_in {
                m = tag.hash.get();
            }
        } else if tag.r#type.intersects(T_SET) {
            // (2) inline set reference
            let sh0 = hash_value_ustring(&tag.tag, 0);
            let sh = {
                let it = self.grammar.sets_by_name.find(sh0);
                it.get().1
            };
            m = self.does_set_match_reading(reading, sh, bypass_index, unif_mode) as u32;
        } else if tag.r#type.intersects(T_VARSTRING) {
            // (3) varstring: generate the concrete tag, recurse
            let nt = self.generate_varstring_tag(tag);
            let nt_tag = self.grammar.single_tags_list[nt.0].clone();
            m = self.does_tag_match_reading(reading, &nt_tag, unif_mode, bypass_index);
        } else if tag.r#type.intersects(T_META) {
            // (4) META regex against the cohort's parenthetical text
            if let Some(re) = tag.regexp.as_ref() {
                let text = {
                    let pc = self.store.readings.get(reading.0).parent;
                    match pc {
                        Some(cid) => self.store.cohorts.get(cid.0).text.clone(),
                        None => UString::new(),
                    }
                };
                if !text.is_empty() {
                    if re.is_match(&text) {
                        m = tag.hash.get();
                    }
                    if m != 0 {
                        let gc = group_count(tag);
                        if gc > 0
                            && !self.context_stack.is_empty()
                            && self.context_stack.last().unwrap().regexgrps.is_some()
                        {
                            let idx = self.context_stack.last().unwrap().regexgrps.unwrap();
                            let frame = self.context_stack.last_mut().unwrap();
                            let rg = &mut self.regexgrps_store[idx];
                            capture_regex(gc, &mut frame.regexgrp_ct, rg, re, &text);
                        }
                    }
                }
            }
        } else if tag.regexp.is_some() {
            // (5) regular regexp tag
            m = self.does_regexp_match_reading(reading, tag, bypass_index);
        } else if tag.r#type.intersects(T_CASE_INSENSITIVE) {
            // (6) case-insensitive
            let textual: Vec<u32> = self
                .store
                .readings
                .get(reading.0)
                .tags_textual
                .as_slice()
                .to_vec();
            for mter in textual {
                m = self.does_tag_match_icase(mter, tag, bypass_index);
                if m != 0 {
                    break;
                }
            }
        } else if tag.r#type.intersects(T_REGEXP_ANY) {
            // (7) <.*>/".*" any-forms
            if tag.r#type.intersects(T_BASEFORM) {
                let bf = self
                    .store
                    .readings
                    .get(reading.0)
                    .baseform
                    .unwrap_or(TagHash(0));
                m = bf.get();
                if unif_mode {
                    if self.unif_last_baseform != TagHash(0) {
                        if self.unif_last_baseform != bf {
                            m = 0;
                        }
                    } else {
                        self.unif_last_baseform = bf;
                    }
                }
            } else if tag.r#type.intersects(T_WORDFORM) {
                let wf_hash = {
                    let cid = self.store.readings.get(reading.0).parent.unwrap();
                    let wf = self.store.cohorts.get(cid.0).wordform.unwrap();
                    self.grammar.single_tags_list[wf.0].hash
                };
                m = wf_hash.get();
                if unif_mode {
                    if self.unif_last_wordform != TagHash(0) {
                        if self.unif_last_wordform != wf_hash {
                            m = 0;
                        }
                    } else {
                        self.unif_last_wordform = wf_hash;
                    }
                }
            } else {
                let textual: Vec<u32> = self
                    .store
                    .readings
                    .get(reading.0)
                    .tags_textual
                    .as_slice()
                    .to_vec();
                for mter in textual {
                    let (itype, ihash) = {
                        let it = self.grammar.single_tags.find(mter);
                        let tid = it.get().1;
                        let t = &self.grammar.single_tags_list[tid.0];
                        (t.r#type, t.hash)
                    };
                    if !itype.intersects(T_BASEFORM | T_WORDFORM) {
                        m = ihash.get();
                        if unif_mode {
                            if self.unif_last_textual != TagHash(0) {
                                if self.unif_last_textual != TagHash(mter) {
                                    m = 0;
                                }
                            } else {
                                self.unif_last_textual = TagHash(mter);
                            }
                        }
                    }
                    if m != 0 {
                        break;
                    }
                }
            }
        } else if tag.r#type.intersects(T_NUMERICAL) {
            // (8) numerical — LAST matching numerical tag wins (no break)
            let nums: Vec<TagId> = self
                .store
                .readings
                .get(reading.0)
                .tags_numerical
                .values()
                .copied()
                .collect();
            for tid in nums {
                let itag = self.grammar.single_tags_list[tid.0].clone();
                let rv = test_tag_numerical(&mut self.store, &self.grammar, reading, tag, &itag);
                if rv != TagHash(0) {
                    m = rv.get();
                }
            }
        } else if tag.r#type.intersects(T_VARIABLE | T_LOCAL_VARIABLE) {
            // (9) variable existence / value comparison
            m = 0;
            let cid = self.store.readings.get(reading.0).parent.unwrap();
            let sw_opt = self.store.cohorts.get(cid.0).parent;
            let use_global =
                sw_opt == self.window.current || (!tag.r#type.intersects(T_LOCAL_VARIABLE));
            let var_entries: Vec<(u32, u32)> = if use_global {
                collect_fum(&self.variables)
            } else {
                collect_fum(
                    &self
                        .store
                        .single_windows
                        .get(sw_opt.unwrap().0)
                        .variables_set,
                )
            };

            let key_info = {
                let it = self.grammar.single_tags.find(tag.comparison_hash);
                if it != self.grammar.single_tags.end() {
                    let tid = it.get().1;
                    Some((tid, self.grammar.single_tags_list[tid.0].r#type))
                } else {
                    None
                }
            };
            if let Some((key_tid, key_type)) = key_info {
                let key_tag = self.grammar.single_tags_list[key_tid.0].clone();
                let found_value: Option<u32> = if key_type.intersects(T_REGEXP) {
                    let mut fv = None;
                    for &(k, v) in &var_entries {
                        if self.does_tag_match_regexp(k, &key_tag, bypass_index) != 0 {
                            fv = Some(v);
                            break;
                        }
                    }
                    fv
                } else if key_type.intersects(T_CASE_INSENSITIVE) {
                    let mut fv = None;
                    for &(k, v) in &var_entries {
                        if self.does_tag_match_icase(k, &key_tag, bypass_index) != 0 {
                            fv = Some(v);
                            break;
                        }
                    }
                    fv
                } else {
                    // vars.find(tag.comparison_hash)
                    var_entries
                        .iter()
                        .find(|(k, _)| *k == tag.comparison_hash)
                        .map(|(_, v)| *v)
                };
                if let Some(itval) = found_value {
                    if tag.variable_hash() == 0 {
                        m = tag.hash.get();
                    } else {
                        let comp_tid = {
                            let it = self.grammar.single_tags.find(tag.variable_hash());
                            it.get().1
                        };
                        let comp_tag = self.grammar.single_tags_list[comp_tid.0].clone();
                        if comp_tag.r#type.intersects(T_REGEXP) {
                            if self.does_tag_match_regexp(itval, &comp_tag, bypass_index) != 0 {
                                m = tag.hash.get();
                            }
                        } else if comp_tag.r#type.intersects(T_CASE_INSENSITIVE) {
                            if self.does_tag_match_icase(itval, &comp_tag, bypass_index) != 0 {
                                m = tag.hash.get();
                            }
                        } else if comp_tag.hash.get() == itval {
                            m = tag.hash.get();
                        }
                    }
                }
            }
        } else if tag.r#type.intersects(T_PAR_LEFT) {
            // (10)
            if self.par_left_tag != TagHash(0) {
                let (ln, has) = {
                    let r = self.store.readings.get(reading.0);
                    let cid = r.parent.unwrap();
                    let has = r.tags.find(self.par_left_tag.get()) != r.tags.end();
                    (self.store.cohorts.get(cid.0).local_number, has)
                };
                if ln == self.par_left_pos && has {
                    m = self.grammar.tag_any;
                }
            }
        } else if tag.r#type.intersects(T_PAR_RIGHT) {
            // (11)
            if self.par_right_tag != TagHash(0) {
                let (ln, has) = {
                    let r = self.store.readings.get(reading.0);
                    let cid = r.parent.unwrap();
                    let has = r.tags.find(self.par_right_tag.get()) != r.tags.end();
                    (self.store.cohorts.get(cid.0).local_number, has)
                };
                if ln == self.par_right_pos && has {
                    m = self.grammar.tag_any;
                }
            }
        } else if tag.r#type.intersects(T_ENCL) {
            // (12) enclosure: the cohort right after reading.parent is enclosed
            let cid = self.store.readings.get(reading.0).parent.unwrap();
            let (sw_id, local_number) = {
                let c = self.store.cohorts.get(cid.0);
                (c.parent.unwrap(), c.local_number as usize)
            };
            let all = self.store.single_windows.get(sw_id.0).all_cohorts.clone();
            // std::find(begin + local_number, end, reading.parent), then ++c.
            let mut idx = local_number;
            while idx < all.len() && all[idx] != cid {
                idx += 1;
            }
            let cpos = idx + 1;
            if cpos < all.len() && self.store.cohorts.get(all[cpos].0).enclosed != 0 {
                m = 1;
            }
        } else if tag.r#type.intersects(T_TARGET) {
            // (13)
            let pc = self.store.readings.get(reading.0).parent;
            if self.rule_target.is_some() && pc == self.rule_target {
                m = self.grammar.tag_any;
            }
        } else if tag.r#type.intersects(T_MARK) {
            // (14)
            let pc = self.store.readings.get(reading.0).parent;
            if pc == self.get_mark() {
                m = self.grammar.tag_any;
            }
        } else if tag.r#type.intersects(T_ATTACHTO) {
            // (15)
            let pc = self.store.readings.get(reading.0).parent;
            if pc == self.get_attach_to().cohort {
                m = self.grammar.tag_any;
            }
        } else if tag.r#type.intersects(T_SAME_BASIC) {
            // (16)
            let hp = self.store.readings.get(reading.0).hash_plain;
            if hp == self.same_basic {
                m = self.grammar.tag_any;
            }
        } else if tag.r#type.intersects(T_CONTEXT) {
            // (17) previous context frame's position list
            if self.context_stack.len() > 1 {
                let idx = self.context_stack.len() - 2;
                let crp = tag.context_ref_pos();
                let pc = self.store.readings.get(reading.0).parent;
                let list = &self.context_stack[idx].context;
                if crp as usize <= list.len() && pc == list[(crp - 1) as usize] {
                    m = self.grammar.tag_any;
                }
            }
        }

        if m != 0 {
            self.match_single += 1;
            retval = m;
        }
        retval
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-tags-matching-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-tags-matching-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.get-tags-matching-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.get-tags-matching-fn]
    /// Appends onto `rv_tags` the reading's own tags matched by any pattern tag in
    /// `the_tags`. `rv_tags` accumulates (not cleared); duplicates possible.
    pub fn get_tags_matching(
        &mut self,
        reading: ReadingId,
        the_tags: &TagList,
        rv_tags: &mut TagList,
    ) {
        let tags_list: Vec<u32> = self.store.readings.get(reading.0).tags_list.clone();
        for &tid in the_tags {
            let tag = self.grammar.single_tags_list[tid.0].clone();
            for &tt in &tags_list {
                let mut m: u32 = 0;
                let itag_id = {
                    let it = self.grammar.single_tags.find(tt);
                    it.get().1
                };
                let (itype, ihash, itag0) = {
                    let t = &self.grammar.single_tags_list[itag_id.0];
                    (t.r#type, t.hash, t.tag.chars().next().unwrap_or('\0'))
                };
                if tag.regexp.is_some() {
                    m = self.does_tag_match_regexp(tt, &tag, false);
                } else if tag.r#type.intersects(T_CASE_INSENSITIVE) {
                    m = self.does_tag_match_icase(tt, &tag, false);
                } else if (tag.r#type.intersects(T_REGEXP_ANY)) && (itype.intersects(T_TEXTUAL)) {
                    if tag.r#type.intersects(T_BASEFORM) {
                        if itype.intersects(T_BASEFORM) {
                            m = self
                                .store
                                .readings
                                .get(reading.0)
                                .baseform
                                .map_or(0, |h| h.get());
                        }
                    } else if tag.r#type.intersects(T_WORDFORM) {
                        if itype.intersects(T_WORDFORM) {
                            let cid = self.store.readings.get(reading.0).parent.unwrap();
                            let wf = self.store.cohorts.get(cid.0).wordform.unwrap();
                            m = self.grammar.single_tags_list[wf.0].hash.get();
                        }
                    } else if !itype.intersects(T_BASEFORM | T_WORDFORM) {
                        let tag0 = tag.tag.chars().next().unwrap_or('\0');
                        if (tag0 == '"' && itag0 == '"') || (tag0 == '<' && itag0 == '<') {
                            m = ihash.get();
                        }
                    }
                } else if (tag.r#type.intersects(T_NUMERICAL)) && (itype.intersects(T_NUMERICAL)) {
                    let itag = self.grammar.single_tags_list[itag_id.0].clone();
                    m = test_tag_numerical(&mut self.store, &self.grammar, reading, &tag, &itag)
                        .get();
                } else if tag.hash == ihash {
                    m = ihash.get();
                }
                if m != 0 {
                    rv_tags.push(itag_id);
                }
            }
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    /// Recursive trie walk: does the reading contain a complete tag path in `trie`?
    /// `trie` is owned by the caller (a clone out of the set), so its sub-tries are
    /// passed by reference through the recursion without aliasing `self`. Entries
    /// are visited in ascending-`Tag::hash` order (the C++ flat_map order).
    pub fn does_set_match_reading_trie(
        &mut self,
        reading: ReadingId,
        set_number: u32,
        trie: &trie_t,
        unif_mode: bool,
    ) -> bool {
        let mut entries: Vec<(TagId, u32)> = trie
            .keys()
            .map(|k| (*k, self.grammar.single_tags_list[k.0].hash.get()))
            .collect();
        entries.sort_by_key(|e| e.1);
        for (tid, _h) in entries {
            let tagv = self.grammar.single_tags_list[tid.0].clone();
            let matched = self.does_tag_match_reading(reading, &tagv, unif_mode, false) != 0;
            if matched {
                if tagv.r#type.intersects(T_FAILFAST) {
                    continue;
                }
                let node = &trie[&tid];
                if node.terminal {
                    if unif_mode {
                        let np = (node as *const _) as *const ();
                        if !self.check_unif_tags(set_number, np) {
                            continue;
                        }
                    }
                    return true;
                }
                if let Some(child) = node.trie.as_deref()
                    && self.does_set_match_reading_trie(reading, set_number, child, unif_mode)
                {
                    return true;
                }
            }
        }
        false
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    /// Tests whether a reading matches a LIST set. Takes the set's `number` and its
    /// (caller-cloned) `ff_tags`/`trie`/`trie_special` so the grammar borrow does
    /// not alias `&mut self`.
    pub fn does_set_match_reading_tags(
        &mut self,
        reading: ReadingId,
        set_number: u32,
        ff_tags: &TagSortedVector,
        trie: &trie_t,
        trie_special: &trie_t,
        unif_mode: bool,
    ) -> bool {
        let mut retval = false;

        // Fail-fast pre-check.
        if !ff_tags.empty() {
            let ff: Vec<TagId> = ff_tags.iter().copied().collect();
            for tid in ff {
                let tagv = self.grammar.single_tags_list[tid.0].clone();
                if self.does_tag_match_reading(reading, &tagv, unif_mode, false) != 0 {
                    return false;
                }
            }
        }

        // Main fast path: merge-intersect the reading's plain tags with the trie's
        // first-level keys (both ascending by hash).
        let plain: Vec<u32> = self
            .store
            .readings
            .get(reading.0)
            .tags_plain
            .as_slice()
            .to_vec();
        if !trie.is_empty() && !plain.is_empty() {
            let mut entries: Vec<(TagId, u32)> = trie
                .keys()
                .map(|k| (*k, self.grammar.single_tags_list[k.0].hash.get()))
                .collect();
            entries.sort_by_key(|e| e.1);

            let front_hash = plain[0]; // tags_plain.front() (smallest)
            let smallest_trie_hash = entries[0].1; // trie.begin()->first->hash
            let mut oi = plain.partition_point(|&x| x < smallest_trie_hash);
            let mut ii = entries.partition_point(|e| e.1 < front_hash);
            while oi < plain.len() && ii < entries.len() {
                if plain[oi] == entries[ii].1 {
                    let tid = entries[ii].0;
                    let (terminal, has_child) = {
                        let n = &trie[&tid];
                        (n.terminal, n.trie.is_some())
                    };
                    if terminal {
                        if unif_mode {
                            let np = (&trie[&tid] as *const _) as *const ();
                            if !self.check_unif_tags(set_number, np) {
                                ii += 1;
                                continue;
                            }
                        }
                        retval = true;
                        break;
                    }
                    if has_child {
                        let child = trie[&tid].trie.as_deref().unwrap();
                        if self.does_set_match_reading_trie(reading, set_number, child, unif_mode) {
                            retval = true;
                            break;
                        }
                    }
                    ii += 1;
                }
                while oi < plain.len() && ii < entries.len() && plain[oi] < entries[ii].1 {
                    oi += 1;
                }
                while oi < plain.len() && ii < entries.len() && entries[ii].1 < plain[oi] {
                    ii += 1;
                }
            }
        }

        if !retval && !trie_special.is_empty() {
            retval = self.does_set_match_reading_trie(reading, set_number, trie_special, unif_mode);
        }
        retval
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn]
    /// Tests whether a reading matches a LIST or SET set, evaluating operators
    /// recursively with a yes/no memo cache.
    pub fn does_set_match_reading(
        &mut self,
        reading: ReadingId,
        set: u32,
        bypass_index: bool,
        unif_mode: bool,
    ) -> bool {
        if !bypass_index && !unif_mode {
            let rhash = self.store.readings.get(reading.0).hash;
            if self.index_readingSet_no[set as usize].contains(rhash) {
                return false;
            }
            if self.index_readingSet_yes[set as usize].contains(rhash) {
                return true;
            }
        }

        let mut retval = false;

        let (stype, snumber, ssets_empty) = {
            let s = self.grammar.set_by_number(SetNumber(set)); // grammar->sets_list[set]
            (s.r#type, s.number.get(), s.sets.is_empty())
        };
        let tagunif = stype.intersects(ST_TAG_UNIFY);

        if stype.intersects(ST_ANY) {
            // (a) the (*) set
            retval = true;
        } else if ssets_empty {
            // (b) LIST set. The tries MUST be the grammar-owned ones (not clones):
            // C++ stores `&kv` (a node address inside `Set::trie`) into the frame's
            // `unif_tags` via check_unif_tags, and BOTH the cross-call identity
            // compare and run_rules' getTagList → trie_getTagList(trie, tags, node)
            // rely on that address pointing into the grammar's own trie. The set
            // tries are never mutated during application (parse-time only), so the
            // laundered borrows below cannot dangle; `&mut self` re-entry only
            // touches `store`, caches, and `single_tags*`.
            let (ff, trie, trie_sp): (&TagSortedVector, &trie_t, &trie_t) = {
                let s = self.grammar.set_by_number(SetNumber(set)); // grammar->sets_list[set]
                unsafe {
                    (
                        &*(&s.ff_tags as *const TagSortedVector),
                        &*(&s.trie as *const trie_t),
                        &*(&s.trie_special as *const trie_t),
                    )
                }
            };
            retval = self.does_set_match_reading_tags(
                reading,
                snumber,
                ff,
                trie,
                trie_sp,
                tagunif || unif_mode,
            );
        } else if stype.intersects(ST_SET_UNIFY) {
            // (c) &&-unified set
            let usets_idx = self.context_stack.last().unwrap().unif_sets.unwrap();
            let usets_empty = self.unif_sets_store[usets_idx]
                .get(&snumber)
                .map(|v| v.empty())
                .unwrap_or(true);
            if usets_empty {
                // First evaluation: gather all matching sub-sets of sets[0].
                let uset_sets = {
                    let sets0 = self.grammar.set_by_number(SetNumber(set)).sets[0];
                    self.grammar.set_by_number(SetNumber(sets0)).sets.clone()
                };
                for tset_ref in uset_sets {
                    let tnum = self.grammar.set_by_number(SetNumber(tset_ref)).number.get();
                    if self.does_set_match_reading(
                        reading,
                        tnum,
                        bypass_index,
                        tagunif || unif_mode,
                    ) {
                        self.unif_sets_store[usets_idx]
                            .entry(snumber)
                            .or_default()
                            .insert(tnum);
                    }
                }
                retval = !self.unif_sets_store[usets_idx]
                    .get(&snumber)
                    .map(|v| v.empty())
                    .unwrap_or(true);
            } else {
                // Subsequent evaluations: test the previously-stored sets.
                let stored: Vec<u32> = self.unif_sets_store[usets_idx]
                    .get(&snumber)
                    .map(|v| v.as_slice().to_vec())
                    .unwrap_or_default();
                let mut sets = self.ss_u32sv.get();
                for usi in stored {
                    if self.does_set_match_reading(reading, usi, bypass_index, unif_mode) {
                        sets.insert(usi);
                    }
                }
                retval = !sets.empty();
            }
        } else {
            // (d) SET set: apply operators (non-OR binds tighter than OR)
            let ssets = self.grammar.set_by_number(SetNumber(set)).sets.clone();
            let sset_ops = self.grammar.set_by_number(SetNumber(set)).set_ops.clone();
            let size = ssets.len();
            let mut i = 0usize;
            while i < size {
                let mut m = self.does_set_match_reading(
                    reading,
                    ssets[i],
                    bypass_index,
                    tagunif || unif_mode,
                );
                let mut failfast = false;
                while i < size - 1 && sset_ops[i] != S_OR {
                    match sset_ops[i] {
                        x if x == S_PLUS => {
                            if m {
                                m = self.does_set_match_reading(
                                    reading,
                                    ssets[i + 1],
                                    bypass_index,
                                    tagunif || unif_mode,
                                );
                            }
                        }
                        x if x == S_FAILFAST => {
                            if self.does_set_match_reading(
                                reading,
                                ssets[i + 1],
                                bypass_index,
                                tagunif || unif_mode,
                            ) {
                                m = false;
                                failfast = true;
                            }
                        }
                        x if x == S_MINUS => {
                            if m && self.does_set_match_reading(
                                reading,
                                ssets[i + 1],
                                bypass_index,
                                tagunif || unif_mode,
                            ) {
                                m = false;
                            }
                        }
                        _ => panic!("Set operator not implemented!"),
                    }
                    i += 1;
                }
                if m {
                    self.match_sub += 1;
                    retval = true;
                    break;
                }
                if failfast {
                    self.match_sub += 1;
                    retval = false;
                    break;
                }
                i += 1;
            }
            // Propagate a unified tag across the set's members.
            if (unif_mode || tagunif) && !self.context_stack.is_empty() {
                let ut_idx = self.context_stack.last().unwrap().unif_tags.unwrap();
                let ut = &mut self.unif_tags_store[ut_idx];
                let mut tagptr: Option<*const ()> = None;
                for &s in ssets.iter().take(size) {
                    if let Some(&t) = ut.get(&s) {
                        tagptr = Some(t);
                        break;
                    }
                }
                if let Some(t) = tagptr {
                    for &s in ssets.iter().take(size) {
                        ut.insert(s, t);
                    }
                }
            }
        }

        // Cache the result.
        if retval {
            let rhash = self.store.readings.get(reading.0).hash;
            self.index_readingSet_yes[set as usize].insert(rhash);
        } else if !stype.intersects(ST_TAG_UNIFY) && !unif_mode {
            let rhash = self.store.readings.get(reading.0).hash;
            self.index_readingSet_no[set as usize].insert(rhash);
        }
        retval
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn]
    /// Runs the LINK-chain test that follows the current one, if any, returning
    /// whether it matched (true when there is no linked test).
    pub fn does_set_match_cohort_test_linked(
        &mut self,
        cohort: CohortId,
        set: u32,
        context: &mut dSMC_Context,
    ) -> bool {
        let mut retval = true;
        let mut reset = false;
        let mut linked: Option<crate::arena::CtxId> = None;
        let mut min: Option<CohortId> = None;
        let mut max: Option<CohortId> = None;

        let ctx_test_linked = context
            .test
            .and_then(|cid| self.grammar.contexts_arena[cid.0].linked);
        if let Some(l) = ctx_test_linked {
            linked = Some(l);
        } else if !self.tmpl_cntx.linked.is_empty() {
            min = self.tmpl_cntx.min;
            max = self.tmpl_cntx.max;
            linked = self.tmpl_cntx.linked.last().copied();
            self.tmpl_cntx.linked.pop();
            reset = true;
        }
        if let Some(l) = linked {
            if !context.did_test {
                let lpos = self.grammar.contexts_arena[l.0].pos;
                let (cparent, clocal) = {
                    let c = self.store.cohorts.get(cohort.0);
                    (c.parent, c.local_number)
                };
                let res = if lpos.intersects(POS_NO_PASS_ORIGIN) {
                    self.run_contextual_test(
                        cparent,
                        clocal,
                        l,
                        context.deep.as_deref_mut(),
                        Some(cohort),
                    )
                } else {
                    self.run_contextual_test(
                        cparent,
                        clocal,
                        l,
                        context.deep.as_deref_mut(),
                        context.origin,
                    )
                };
                context.matched_tests = res.is_some();
                let child_unify = self
                    .grammar
                    .set_by_number(SetNumber(set))
                    .r#type
                    .intersects(ST_CHILD_UNIFY);
                if !child_unify {
                    context.did_test = true;
                }
            }
            retval = context.matched_tests;
        }
        if reset {
            self.tmpl_cntx.linked.push(linked.unwrap());
        }
        if !retval {
            self.tmpl_cntx.min = min;
            self.tmpl_cntx.max = max;
        }
        retval
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-helper-fn]
    /// Core per-reading cohort matcher: child-unify snapshot/rollback, negation,
    /// the linked test, and attach-to bookkeeping.
    pub fn does_set_match_cohort_helper(
        &mut self,
        cohort: CohortId,
        reading: ReadingId,
        set: u32,
        mut context: Option<&mut dSMC_Context>,
    ) -> bool {
        let mut retval = false;
        let mut utags = self.ss_utags.get();
        let mut usets = self.ss_usets.get();
        let orz = if self.context_stack.is_empty() {
            0
        } else {
            self.context_stack.last().unwrap().regexgrp_ct
        };

        let (stype, snumber) = {
            let s = self.grammar.set_by_number(SetNumber(set)); // grammar->sets_list[set]
            (s.r#type, s.number.get())
        };
        let cur_flags = self
            .current_rule
            .map(|rid| self.grammar.rule_by_number[rid.0].flags)
            .unwrap_or_default();
        let child_unify = stype.intersects(ST_CHILD_UNIFY);
        let cap_unif = cur_flags.intersects(RF_CAPTURE_UNIF);

        if context.is_some() && !cap_unif && child_unify && !self.context_stack.is_empty() {
            let (ut_idx, us_idx) = {
                let f = self.context_stack.last().unwrap();
                (f.unif_tags.unwrap(), f.unif_sets.unwrap())
            };
            utags = self.unif_tags_store[ut_idx].clone();
            usets = self.unif_sets_store[us_idx].clone();
        }

        let bypass = stype.intersects(ST_CHILD_UNIFY | ST_SPECIAL);
        if self.does_set_match_reading(reading, snumber, bypass, false) {
            retval = true;
            if let Some(ctx) = context.as_deref_mut() {
                if ctx.options.intersects(POS_ATTACH_TO) {
                    self.store.readings.get_mut(reading.0).matched_target = true;
                }
                ctx.matched_target = true;
            }
        }

        // NOT negation, applied per-reading.
        if retval
            && let Some(ctx) = context.as_deref()
            && ctx.options.intersects(POS_NOT)
        {
            retval = !retval;
        }

        // Linked test + attach-to.
        if retval {
            let in_barrier = context.as_deref().map(|c| c.in_barrier).unwrap_or(false);
            if context.is_some() && !in_barrier {
                let attach = context
                    .as_deref()
                    .unwrap()
                    .options
                    .intersects(POS_ATTACH_TO);
                {
                    let ctx = context.as_deref_mut().unwrap();
                    retval = self.does_set_match_cohort_test_linked(cohort, set, ctx);
                }
                if attach {
                    self.store.readings.get_mut(reading.0).matched_tests = retval;
                    if retval && !self.context_stack.is_empty() {
                        let f = self.context_stack.last_mut().unwrap();
                        f.attach_to.cohort = Some(cohort);
                        f.attach_to.reading = None; // set by doesSetMatchCohortNormal
                        f.attach_to.subreading = Some(reading);
                    }
                }
            }
        }

        // Rollback on failure.
        if !retval
            && context.is_some()
            && !cap_unif
            && child_unify
            && !self.context_stack.is_empty()
        {
            let ut_idx = self.context_stack.last().unwrap().unif_tags.unwrap();
            let entry = &mut self.unif_tags_store[ut_idx];
            let differs = utags.len() != entry.len() || utags != *entry;
            if differs {
                std::mem::swap(entry, &mut utags);
            }
        }
        if !retval
            && context.is_some()
            && !cap_unif
            && child_unify
            && !self.context_stack.is_empty()
        {
            let us_idx = self.context_stack.last().unwrap().unif_sets.unwrap();
            let entry = &mut self.unif_sets_store[us_idx];
            let differs = usets.len() != entry.len();
            if differs {
                std::mem::swap(entry, &mut usets);
            }
        }
        if !retval && !self.context_stack.is_empty() {
            self.context_stack.last_mut().unwrap().regexgrp_ct = orz;
        }
        retval
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    /// Normal cohort matching: the set matches if ANY eligible reading matches.
    pub fn does_set_match_cohort_normal(
        &mut self,
        cohort: CohortId,
        set: u32,
        mut context: Option<&mut dSMC_Context>,
    ) -> bool {
        let mut retval = false;

        let opts = context.as_deref().map(|c| c.options).unwrap_or_default();
        let guard = !(context.is_none()
            || (opts.intersects(POS_LOOK_DELETED | POS_LOOK_DELAYED | POS_LOOK_IGNORED | POS_NOT)));
        if guard {
            let ps = &self.store.cohorts.get(cohort.0).possible_sets;
            if set as usize >= ps.len() || !ps[set as usize] {
                return retval;
            }
        }

        // wread pre-check.
        let wread = self.store.cohorts.get(cohort.0).wread;
        if let Some(wr) = wread {
            let in_barrier = context.as_deref().map(|c| c.in_barrier).unwrap_or(false);
            if context.is_none() || !in_barrier {
                retval = self.does_set_match_cohort_helper(cohort, wr, set, context.as_deref_mut());
            }
        }
        if retval {
            let done = match context.as_deref() {
                None => true,
                Some(c) => c.did_test,
            };
            if done {
                return retval;
            }
        }

        // 4-slot list array (readings; plus deleted/delayed/ignored per options).
        let lists = self.gather_lists(cohort, context.as_deref());

        for slot in lists.into_iter() {
            let list = match slot {
                Some(l) => l,
                None => continue,
            };
            for reading_head in list {
                let mut reading = reading_head;
                if let Some(ctx) = context.as_deref()
                    && let Some(test) = ctx.test
                {
                    let offs = self.grammar.contexts_arena[test.0].offset_sub;
                    match self.get_sub_reading(reading, offs) {
                        Some(r) => reading = r,
                        None => continue,
                    }
                }
                let active = self.store.readings.get(reading.0).active;
                if let Some(ctx) = context.as_deref() {
                    if !active && ctx.options.intersects(POS_ACTIVE) {
                        continue;
                    }
                    if active && ctx.options.intersects(POS_INACTIVE) {
                        continue;
                    }
                }
                if self.does_set_match_cohort_helper(cohort, reading, set, context.as_deref_mut()) {
                    retval = true;
                    // Back-fill the attach_to parent reading (helper only knew the subreading).
                    if !self.context_stack.is_empty() {
                        let f = self.context_stack.last().unwrap();
                        if f.attach_to.cohort == Some(cohort)
                            && f.attach_to.subreading == Some(reading)
                        {
                            self.context_stack.last_mut().unwrap().attach_to.reading =
                                Some(reading_head);
                        }
                    }
                }
                let has_linked = match context.as_deref() {
                    None => false,
                    Some(c) => c
                        .test
                        .and_then(|t| self.grammar.contexts_arena[t.0].linked)
                        .is_some(),
                };
                let did_test = context.as_deref().map(|c| c.did_test).unwrap_or(false);
                if retval && (context.is_none() || !has_linked || did_test) {
                    return retval;
                }
            }
        }

        // POS_NOT: run the linked test even though nothing matched.
        let do_tl = context
            .as_deref()
            .map(|c| !c.matched_target && c.options.intersects(POS_NOT))
            .unwrap_or(false);
        if do_tl {
            let ctx = context.as_deref_mut().unwrap();
            retval = self.does_set_match_cohort_test_linked(cohort, set, ctx);
        }

        // possible_sets pruning.
        if let Some(ctx) = context.as_deref()
            && !ctx.matched_target
            && !ctx.options.intersects(POS_ACTIVE | POS_INACTIVE)
        {
            let in_sets_any = match &self.grammar.sets_any {
                Some(sa) => (set as usize) < sa.len() && sa[set as usize],
                None => false,
            };
            if !in_sets_any {
                let was_sub = ctx
                    .test
                    .map(|t| self.grammar.contexts_arena[t.0].offset_sub != 0)
                    .unwrap_or(false);
                let ps_len = self.store.cohorts.get(cohort.0).possible_sets.len();
                if !was_sub && (set as usize) < ps_len {
                    self.store.cohorts.get_mut(cohort.0).possible_sets[set as usize] = false;
                }
            }
        }

        retval
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-careful-fn]
    /// Careful ("C") cohort matching: the set must match EVERY eligible reading.
    pub fn does_set_match_cohort_careful(
        &mut self,
        cohort: CohortId,
        set: u32,
        mut context: Option<&mut dSMC_Context>,
    ) -> bool {
        let mut retval = false;

        let opts = context.as_deref().map(|c| c.options).unwrap_or_default();
        let guard = !(context.is_none()
            || (opts.intersects(POS_LOOK_DELETED | POS_LOOK_DELAYED | POS_LOOK_IGNORED | POS_NOT)));
        if guard {
            let ps = &self.store.cohorts.get(cohort.0).possible_sets;
            if set as usize >= ps.len() || !ps[set as usize] {
                return retval;
            }
        }

        let lists = self.gather_lists(cohort, context.as_deref());

        'outer: for slot in lists.into_iter() {
            let list = match slot {
                Some(l) => l,
                None => continue,
            };
            for reading0 in list {
                let mut reading = reading0;
                if let Some(ctx) = context.as_deref()
                    && let Some(test) = ctx.test
                {
                    let offs = self.grammar.contexts_arena[test.0].offset_sub;
                    match self.get_sub_reading(reading, offs) {
                        Some(r) => reading = r,
                        None => continue,
                    }
                }
                let active = self.store.readings.get(reading.0).active;
                if let Some(ctx) = context.as_deref() {
                    if !active && ctx.options.intersects(POS_ACTIVE) {
                        continue;
                    }
                    if active && ctx.options.intersects(POS_INACTIVE) {
                        continue;
                    }
                }
                retval =
                    self.does_set_match_cohort_helper(cohort, reading, set, context.as_deref_mut());
                if !retval {
                    break;
                }
            }
            if !retval {
                break 'outer;
            }
        }

        let do_tl = context
            .as_deref()
            .map(|c| !c.matched_target && c.options.intersects(POS_NOT))
            .unwrap_or(false);
        if do_tl {
            let ctx = context.unwrap();
            retval = self.does_set_match_cohort_test_linked(cohort, set, ctx);
        }

        retval
    }

    /// Builds the C++ `ReadingList* lists[4]` array: slot 0 = `cohort.readings`;
    /// slots 1..3 = `deleted`/`delayed`/`ignored` only when the corresponding
    /// POS_LOOK_* option is set (and a context is present). The id lists are cloned
    /// so the `cohorts` arena is not borrowed across the matcher recursion. Not a
    /// manifest symbol — shared setup for the two cohort matchers.
    fn gather_lists(
        &self,
        cohort: CohortId,
        context: Option<&dSMC_Context>,
    ) -> [Option<Vec<ReadingId>>; 4] {
        let c = self.store.cohorts.get(cohort.0);
        let mut lists: [Option<Vec<ReadingId>>; 4] = [Some(c.readings.clone()), None, None, None];
        if let Some(ctx) = context {
            if ctx.options.intersects(POS_LOOK_DELETED) {
                lists[1] = Some(c.deleted.clone());
            }
            if ctx.options.intersects(POS_LOOK_DELAYED) {
                lists[2] = Some(c.delayed.clone());
            }
            if ctx.options.intersects(POS_LOOK_IGNORED) {
                lists[3] = Some(c.ignored.clone());
            }
        }
        lists
    }
}
