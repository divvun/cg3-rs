//! `src/GrammarApplicator_matchSet.cpp` (the regex/set/tag/cohort matchers) —
//! implemented on the [`Matcher`] split-borrow sub-view (plan node
//! `matcher-doc-split.matcher-view`): the runtime arenas resolve through the
//! view's fields (`self.readings` / `self.cohorts` / `self.single_windows`),
//! which is the type-level proof that matching reads the document (the two
//! narrow write capabilities are documented on [`Matcher`]). Literal,
//! bug-for-bug port.
//!
//! SIBLING methods called here but DEFINED in other partials (all on
//! `impl Matcher` in their C++ translation unit's module):
//!     - reflow:   `generate_varstring_tag`
//!     - run_rules: `get_sub_reading`
//!     - context:  `get_mark`, `get_attach_to`, `check_unif_tags`
//!     - runCtx:   `run_contextual_test`
//!
//! EXPOSED here (runCtx calls them directly; run_rules / run_grammar / the
//! stream applicators go through the `Engine` forwarders in mod.rs):
//!     does_set_match_reading, does_set_match_reading_tags,
//!     does_set_match_reading_trie, does_tag_match_reading, does_tag_match_regexp,
//!     does_tag_match_icase, does_regexp_match_reading, does_regexp_match_line,
//!     does_set_match_cohort_normal, does_set_match_cohort_careful,
//!     does_set_match_cohort_helper, does_set_match_cohort_test_linked,
//!     get_tags_matching — all take `reading`/`cohort` as arena ids (ReadingId /
//!     CohortId), never `&Reading`/`&Cohort` (the arenas live inside `self`, so a
//!     `&Reading` borrowed from `self.readings` cannot coexist with `&mut self`).
//!
//! REGEX MAPPING (the C++ find == UNANCHORED search): a tag's `regexp` was
//! compiled at parse time (`/.../` unanchored, `"..."r`/`<...>r` anchored `^…$`,
//! case-insensitive when T_CASE_INSENSITIVE). Matching is `re.is_match(subject)`
//! (unanchored, Unicode-by-default — the ICU regex semantics). The C++ group
//! count == `re.captures_len() - 1` (excludes group 0).
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
//!   - readingSet: `index_reading_set_{no,yes}[set]` keyed by `reading.hash`; only
//!     consulted/written when `!bypass_index && !unif_mode`; the negative cache is
//!     additionally skipped when the set is ST_TAG_UNIFY or `unif_mode`.
//!
//! CAVEATS the lead must reconcile (NOTED, not fixed):
//!   - trie node identity (address-free): the C++ `check_unif_tags(theset.number,
//!     &kv)` records a `trie_t` ENTRY ADDRESS (the terminal node reached at some
//!     depth); run_rules' `get_tag_list` walks the same tries by that address to
//!     rebuild the root-to-node path. The port carries that identity as the
//!     address-free [`UnifKey`] (`(special, root-to-node TagId path)`) — a
//!     bijection with the entry address within a set (see `UnifKey` docs), so
//!     `does_set_match_reading_tags`/`_trie` navigate the tries FRESH from
//!     `self.grammar` at each step (short borrows) rather than laundering a
//!     detached `&Set::{ff_tags,trie,trie_special}` reference across the `&mut
//!     self` recursion. No `unsafe`; `get_tag_list` resolves the key by appending
//!     `path`.
//!   - `TagSet_SubsetOf_TSet` / `Set::ff_tags` order by the placeholder
//!     `compare_Tag` (TagId order, not Tag::hash) — the merge assumes hash order;
//!     correct once `compare_Tag` is arena-hash-aware.
//!   - trie iteration order: `trie_t` is a `BTreeMap<TagId,_>` (TagId order); the
//!     C++ flat_map iterates by `Tag::hash`. Re-derived here by hash-sorting the
//!     entries (stable), matching `tag_trie::ordered_entries`.

use crate::arena::GenArena;
use crate::arena::{CohortId, ReadingId, TagId};
use crate::cohort;
use crate::contextual_test::{
    MASK_POS_DEPREL, POS_ACTIVE, POS_ATTACH_TO, POS_CAREFUL, POS_INACTIVE, POS_LOOK_DELAYED,
    POS_LOOK_DELETED, POS_LOOK_IGNORED, POS_NO_PASS_ORIGIN, POS_NOT,
};
use crate::error::{RuleInapplicable, RunError};
use crate::grammar::Grammar;
use crate::inlines::{NUMERIC_MAX, NUMERIC_MIN, hash_value_str, make_64};
use crate::math_parser::MathParser;
use crate::rule::RF_CAPTURE_UNIF;
use crate::set::{ST_CHILD_UNIFY, ST_SPECIAL};
use crate::sorted_vector::Uint32SortedVector;
use crate::tag::{
    COps, T_ATTACHTO, T_BASEFORM, T_CASE_INSENSITIVE, T_CONTEXT, T_ENCL, T_FAILFAST,
    T_LOCAL_VARIABLE, T_MARK, T_META, T_NUMERIC_MATH, T_NUMERICAL, T_PAR_LEFT, T_PAR_RIGHT,
    T_REGEXP, T_REGEXP_ANY, T_REGEXP_LINE, T_SAME_BASIC, T_SET, T_SPECIAL, T_TARGET, T_TEXTUAL,
    T_VARIABLE, T_VARSTRING, T_WORDFORM, Tag, TagList, TagSortedVector,
};
use crate::tag_trie::{TagTrie, TrieNode};
use crate::types::{SetNumber, TagHash};
use crate::uextras::eq_ignore_case;

use super::set_ops::SetStep;
use super::{CohortMatchContext, Matcher, RegexGroups, UnifKey};

// C++ Strings.hpp set-operator enum values (`S_IGNORE, S_OR=3, S_PLUS, S_MINUS,
// ... S_FAILFAST=8`). Only the four `doesSetMatchReading` uses are reproduced.
pub(super) const S_OR: u32 = 3;
pub(super) const S_PLUS: u32 = 4;
pub(super) const S_MINUS: u32 = 5;
pub(super) const S_FAILFAST: u32 = 8;

/// The trie a [`Matcher::does_set_match_reading_trie`] walk is in, and how it
/// matches.
#[derive(Clone, Copy)]
struct TrieStep {
    set_number: u32,
    set: u32,
    special: bool,
    unif_mode: bool,
}

/// What one trie entry means for a [`Matcher::does_set_match_reading_trie`]
/// walk.
enum TrieEntry {
    /// The reading lacks the tag, or it is fail-fast: go on to the next entry.
    Skip,
    /// A whole path ends here and matches.
    Matched,
    /// Walk the entry's sub-trie next, its tag pushed onto the path.
    Descend,
}

// ===========================================================================
// Free helpers (this file's namespace, matching the C++ translation unit).
// ===========================================================================

// [spec:cg3:def:grammar-applicator-match-set.cg3.capture-regex-fn]
// [spec:cg3:sem:grammar-applicator-match-set.cg3.capture-regex-fn]
/// C++ template `captureRegex(int32_t gc, uint8_t& regexgrp_ct, RXGS*
/// regexgrps, Tag& tag)`. Harvests capture groups 1..=gc (group 0, the whole
/// match, is deliberately NOT captured) of the last successful match into
/// `regexgrps`, starting at `regexgrp_ct` and advancing it by `gc`. The C++
/// read them from the regex object's stateful last match; the port's regex has
/// no such state, so `regexp` + the matched `input` are threaded in and
/// `regexp.captures(input)` is re-run (identical leftmost captures). A group
/// that did not participate yields an empty string (the C++ got length 0).
/// Never shrinks `regexgrps` (`resize(max(regexgrp_ct+1, size))`).
fn capture_regex(
    gc: i32,
    regexgrp_ct: &mut u8,
    regexgrps: &mut RegexGroups,
    regexp: &crate::tag_regex::TagRegex,
    input: &str,
) {
    let caps = regexp.captures(input);
    let mut i = 1i32;
    while i <= gc {
        let text: String = match &caps {
            Some(c) => c
                .get(i as usize)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            None => String::new(),
        };
        let need = (*regexgrp_ct as usize) + 1;
        if regexgrps.len() < need {
            regexgrps.resize(need, String::new());
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
    b: &Uint32SortedVector,
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
// [spec:cg3:req:robustness.accepted-grammars-run]
/// C++ free fn `uint32_t test_tag_numerical(const Reading&, const Tag& tag,
/// const Tag& itag)`. Kept a free fn (arena model): `reading.parent->getMin/getMax`
/// read the cohorts + readings + grammar arenas (min/max computed on demand —
/// the C++ memo is deleted, see `cohort::min_max_for_key`), so exactly those
/// are threaded in (`Reading&` → `ReadingId`). Compares the query numeric tag
/// against a reading's numeric tag, returning `itag.hash` on a match else 0.
/// `compval` derives from the query `tag`; the threshold `V` and operator `B`
/// from the reading's `itag`. `tag_id` names `tag` in the arena, so its type
/// flags come from the run (`Grammar::tag_type`). `None` when `compval` needs
/// the reading's cohort and the reading belongs to none (see `numeric_compval`).
pub fn test_tag_numerical(
    cohorts: &GenArena<crate::cohort::Cohort>,
    readings: &GenArena<crate::reading::Reading>,
    grammar: &Grammar,
    reading: ReadingId,
    tag_id: TagId,
    tag: &Tag,
    itag: &Tag,
) -> Option<TagHash> {
    use COps::*;
    let mut m = TagHash(0);
    if tag.comparison_hash != itag.comparison_hash {
        return Some(TagHash(0));
    }
    let compval = numeric_compval(cohorts, readings, grammar, reading, tag_id, tag)?;

    let a = tag.comparison_op;
    let b = itag.comparison_op;
    let v = itag.comparison_val;
    // C++ if/else-if operator table: match on the (A, B) operator pair, with the
    // value comparison as an arm guard where the C++ arm has one.
    match (a, b) {
        (OpEquals, OpEquals) if compval == v => m = itag.hash,
        (OpNotequals, OpEquals) if compval != v => m = itag.hash,
        (OpEquals, OpNotequals) if compval != v => m = itag.hash,
        (OpNotequals, OpNotequals) if compval == v => m = itag.hash,
        (OpEquals, OpLessthan) if compval < v => m = itag.hash,
        (OpEquals, OpLessequals) if compval <= v => m = itag.hash,
        (OpEquals, OpGreaterthan) if compval > v => m = itag.hash,
        (OpEquals, OpGreaterequals) if compval >= v => m = itag.hash,
        (OpNotequals, OpLessthan) => m = itag.hash,
        (OpNotequals, OpLessequals) => m = itag.hash,
        (OpNotequals, OpGreaterthan) => m = itag.hash,
        (OpNotequals, OpGreaterequals) => m = itag.hash,
        (OpLessthan, OpNotequals) => m = itag.hash,
        (OpLessequals, OpNotequals) => m = itag.hash,
        (OpGreaterthan, OpNotequals) => m = itag.hash,
        (OpGreaterequals, OpNotequals) => m = itag.hash,
        (OpLessthan, OpEquals) if compval > v => m = itag.hash,
        (OpLessequals, OpEquals) if compval >= v => m = itag.hash,
        (OpLessthan, OpLessthan) => m = itag.hash,
        (OpLessequals, OpLessequals) => m = itag.hash,
        (OpLessequals, OpLessthan) => m = itag.hash,
        (OpLessthan, OpLessequals) => m = itag.hash,
        (OpLessthan, OpGreaterthan) if compval > v => m = itag.hash,
        (OpLessthan, OpGreaterequals) if compval > v => m = itag.hash,
        (OpLessequals, OpGreaterthan) if compval > v => m = itag.hash,
        (OpLessequals, OpGreaterequals) if compval >= v => m = itag.hash,
        (OpGreaterthan, OpEquals) if compval < v => m = itag.hash,
        (OpGreaterequals, OpEquals) if compval <= v => m = itag.hash,
        (OpGreaterthan, OpGreaterthan) => m = itag.hash,
        (OpGreaterequals, OpGreaterequals) => m = itag.hash,
        (OpGreaterequals, OpGreaterthan) => m = itag.hash,
        (OpGreaterthan, OpGreaterequals) => m = itag.hash,
        (OpGreaterthan, OpLessthan) if compval < v => m = itag.hash,
        (OpGreaterthan, OpLessequals) if compval < v => m = itag.hash,
        (OpGreaterequals, OpLessthan) if compval < v => m = itag.hash,
        (OpGreaterequals, OpLessequals) if compval <= v => m = itag.hash,
        _ => {}
    }
    Some(m)
}

// [spec:cg3:req:robustness.accepted-grammars-run]
/// The value [`test_tag_numerical`] compares with: the query tag's own, the
/// least or greatest value of its key on the reading's cohort for `MIN` and
/// `MAX`, or a math expression over those two.
///
/// DIVERGENCE: `None` when that needs the cohort and the reading belongs to
/// none — the bag of tags a `B` test reads — where the C++ followed a null
/// cohort.
fn numeric_compval(
    cohorts: &GenArena<crate::cohort::Cohort>,
    readings: &GenArena<crate::reading::Reading>,
    grammar: &Grammar,
    reading: ReadingId,
    tag_id: TagId,
    tag: &Tag,
) -> Option<f64> {
    let parent = readings.get(reading.0).parent;
    let compval = tag.comparison_val;
    // `tag.comparison_offset` shares the tag's union with `variable_hash`, so
    // it is read only under T_NUMERIC_MATH, as the C++ `&&` does: a
    // `VAR:<x=5>` tag is numerical too, and holds its variable value there.
    let comparison_offset = if grammar.tag_type(tag_id).intersects(T_NUMERIC_MATH) {
        tag.comparison_offset() as usize
    } else {
        0
    };
    if comparison_offset != 0 {
        let parent = parent?;
        let mn = cohort::get_min(cohorts, readings, grammar, parent, tag.comparison_hash);
        let mx = cohort::get_max(cohorts, readings, grammar, parent, tag.comparison_hash);
        let mut mp = MathParser::new(mn, mx);
        // exp = view(tag.tag).remove_prefix(comparison_offset).remove_suffix(1)
        let chars: Vec<char> = tag.tag.chars().collect();
        if comparison_offset >= chars.len() {
            return Some(compval);
        }
        let exp: String = chars[comparison_offset..chars.len() - 1].iter().collect();
        // C++ `mp.eval(exp)` threw here and nothing caught it, so the process
        // terminated. Leaving `compval` at the query value is the safe analog;
        // the expression and offset are reported rather than discarded.
        return match mp.eval(&exp) {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("Warning: numeric comparison failed: {e}");
                Some(compval)
            }
        };
    }
    if compval <= NUMERIC_MIN {
        return Some(cohort::get_min(
            cohorts,
            readings,
            grammar,
            parent?,
            tag.comparison_hash,
        ));
    }
    if compval >= NUMERIC_MAX {
        return Some(cohort::get_max(
            cohorts,
            readings,
            grammar,
            parent?,
            tag.comparison_hash,
        ));
    }
    Some(compval)
}

/// Collect a `Uint32FlatHashMap`'s live `(key, value)` entries in physical slot
/// order (the C++ flat_unordered_map iteration order, which `find_if` walks).
/// Not a manifest symbol — port infra so the variable branch can iterate while
/// mutating `self`.
fn collect_fum(m: &crate::flat_unordered_map::Uint32FlatHashMap) -> Vec<(u32, u32)> {
    m.iter().copied().collect()
}

/// The C++ group count of `tag.regexp` — the number of capture groups EXCLUDING
/// the whole-match group 0. `captures_len()` includes group 0, so subtract one.
/// 0 when the tag has no compiled regex.
fn group_count(tag: &Tag) -> i32 {
    tag.regexp
        .as_ref()
        .map(|re| re.captures_len() as i32 - 1)
        .unwrap_or(0)
}

// ===========================================================================
// The match-set matcher cluster, on the `Matcher<'_>` sub-view: the
// contextual-test knot's does_set_match_* family plus the tag/regexp leaves.
// Sibling calls stay method-like; the action layer enters through the `Engine`
// forwarders in mod.rs.
// ===========================================================================
impl Matcher<'_> {
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// The central single-tag dispatcher. Mutually-exclusive branches on
    /// `tag.type` (first match wins). `reading` is an id (arena model); `tag` is an
    /// owned/borrowed pattern tag NOT aliasing `self.grammar` (callers clone it out
    /// of the arena before calling), and `tag_id` names that same tag in the arena.
    ///
    /// The type flags are the RUN's, read once up front. The C++ re-reads
    /// `tag->type` at every arm, but the arms are a single `else if` chain whose
    /// conditions are pure — only a taken arm can reach code that interns a tag
    /// (and so change flags), and by then the chain is over.
    pub fn does_tag_match_reading(
        &mut self,
        reading: ReadingId,
        tag_id: TagId,
        tag: &Tag,
        unif_mode: bool,
        bypass_index: bool,
    ) -> Result<u32, crate::error::RunError> {
        let mut retval: u32 = 0;
        let mut m: u32 = 0;
        let ttype = self.grammar.tag_type(tag_id);

        if !ttype.intersects(T_SPECIAL) || ttype.intersects(T_FAILFAST) {
            // (1) plain / fail-fast tag
            let r = self.readings.get(reading.0);
            let mut raw_in = r.tags_plain_bloom.matches(tag.hash.get());
            if ttype.intersects(T_FAILFAST) {
                raw_in = r.tags_plain.find(tag.plain_hash.get()) != r.tags_plain.end();
            } else if raw_in {
                raw_in = r.tags_plain.find(tag.hash.get()) != r.tags_plain.end();
            }
            if raw_in {
                m = tag.hash.get();
            }
        } else if ttype.intersects(T_SET) {
            // (2) inline set reference
            m = self.does_set_tag_match(reading, tag, bypass_index, unif_mode)?;
        } else if ttype.intersects(T_VARSTRING) {
            // (3) varstring: generate the concrete tag, recurse
            let nt = self.expand_matched_varstring(tag_id, tag)?;
            let nt_tag = self.grammar.single_tags_list[nt.0].clone();
            m = self.does_tag_match_reading(reading, nt, &nt_tag, unif_mode, bypass_index)?;
        } else if ttype.intersects(T_META) {
            // (4) META regex against the cohort's parenthetical text
            if let Some(re) = tag.regexp.as_ref() {
                let text = {
                    let pc = self.readings.get(reading.0).parent;
                    match pc {
                        Some(cid) => self.cohorts.get(cid.0).text.clone(),
                        None => String::new(),
                    }
                };
                if !text.is_empty() {
                    if re.is_match(&text) {
                        m = tag.hash.get();
                    }
                    if m != 0 {
                        self.capture_groups(group_count(tag), tag, &text);
                    }
                }
            }
        } else if tag.regexp.is_some() {
            // (5) regular regexp tag
            m = self.does_regexp_match_reading(reading, tag_id, tag, bypass_index);
        } else if ttype.intersects(T_CASE_INSENSITIVE) {
            // (6) case-insensitive
            let textual: Vec<u32> = self
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
        } else if ttype.intersects(T_REGEXP_ANY) {
            // (7) <.*>/".*" any-forms
            if ttype.intersects(T_BASEFORM) {
                let bf = self.readings.get(reading.0).baseform.unwrap_or(TagHash(0));
                m = bf.get();
                if unif_mode {
                    if self.scratch.unif_last_baseform != TagHash(0) {
                        if self.scratch.unif_last_baseform != bf {
                            m = 0;
                        }
                    } else {
                        self.scratch.unif_last_baseform = bf;
                    }
                }
            } else if ttype.intersects(T_WORDFORM) {
                m = self.match_any_wordform(reading, tag, unif_mode)?;
            } else {
                let textual: Vec<u32> = self
                    .readings
                    .get(reading.0)
                    .tags_textual
                    .as_slice()
                    .to_vec();
                for mter in textual {
                    let (itype, ihash) = {
                        let it = self.grammar.single_tags().find(mter);
                        let tid = it.get().1;
                        (
                            self.grammar.tag_type(tid),
                            self.grammar.single_tags_list[tid.0].hash,
                        )
                    };
                    if !itype.intersects(T_BASEFORM | T_WORDFORM) {
                        m = ihash.get();
                        if unif_mode {
                            if self.scratch.unif_last_textual != TagHash(0) {
                                if self.scratch.unif_last_textual != TagHash(mter) {
                                    m = 0;
                                }
                            } else {
                                self.scratch.unif_last_textual = TagHash(mter);
                            }
                        }
                    }
                    if m != 0 {
                        break;
                    }
                }
            }
        } else if ttype.intersects(T_NUMERICAL) {
            // (8) numerical — LAST matching numerical tag wins (no break)
            m = self.match_numerical(reading, tag_id, tag)?;
        } else if ttype.intersects(T_VARIABLE | T_LOCAL_VARIABLE) {
            // (9) variable existence / value comparison
            m = 0;
            let var_entries = self.tag_variables(reading, ttype, tag)?;

            let key_info = {
                let it = self.grammar.single_tags().find(tag.comparison_hash);
                if it != self.grammar.single_tags().end() {
                    let tid = it.get().1;
                    Some((tid, self.grammar.tag_type(tid)))
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
                if let Some(itval) = found_value
                    && self.variable_value_matches(tag, itval, bypass_index)
                {
                    m = tag.hash.get();
                }
            }
        } else if ttype.intersects(T_PAR_LEFT) {
            // (10)
            let edge = (self.scratch.par_left_tag, self.scratch.par_left_pos);
            m = self.match_par_edge(reading, tag, edge)?;
        } else if ttype.intersects(T_PAR_RIGHT) {
            // (11)
            let edge = (self.scratch.par_right_tag, self.scratch.par_right_pos);
            m = self.match_par_edge(reading, tag, edge)?;
        } else if ttype.intersects(T_ENCL) {
            // (12) enclosure: the cohort right after reading.parent is enclosed
            m = self.match_enclosure(reading, tag)?;
        } else if ttype.intersects(T_TARGET) {
            // (13)
            let pc = self.readings.get(reading.0).parent;
            if self.scratch.rule_target.is_some() && pc == self.scratch.rule_target {
                m = self.grammar.tag_any;
            }
        } else if ttype.intersects(T_MARK) {
            // (14)
            let pc = self.readings.get(reading.0).parent;
            if pc == self.get_mark() {
                m = self.grammar.tag_any;
            }
        } else if ttype.intersects(T_ATTACHTO) {
            // (15)
            let pc = self.readings.get(reading.0).parent;
            if pc == self.get_attach_to().cohort {
                m = self.grammar.tag_any;
            }
        } else if ttype.intersects(T_SAME_BASIC) {
            // (16)
            let hp = self.readings.get(reading.0).hash_plain;
            if hp == self.scratch.same_basic {
                m = self.grammar.tag_any;
            }
        } else if ttype.intersects(T_CONTEXT) {
            // (17) previous context frame's position list
            if self.scratch.context_stack.len() > 1 {
                let idx = self.scratch.context_stack.len() - 2;
                let crp = tag.context_ref_pos() as usize;
                let pc = self.readings.get(reading.0).parent;
                let list = &self.scratch.context_stack[idx].context;
                // `_C1_`..`_C9_` count from 1; a 0 names no context.
                if list.get(crp.wrapping_sub(1)) == Some(&pc) {
                    m = self.grammar.tag_any;
                }
            }
        }

        if m != 0 {
            retval = m;
        }
        Ok(retval)
    }

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// The cohort `reading` belongs to, for a `tag` that asks about it.
    ///
    /// DIVERGENCE: the bag of tags a `B` test reads belongs to no cohort, and
    /// such a tag is a run error naming the rule; the C++ followed the bag's
    /// null cohort.
    fn reading_cohort(&mut self, reading: ReadingId, tag: &Tag) -> Result<CohortId, RunError> {
        match self.readings.get(reading.0).parent {
            Some(cohort) => Ok(cohort),
            None => Err(self.rule_inapplicable(RuleInapplicable::BagOfTagsCohort {
                tag: tag.tag.clone(),
            })),
        }
    }

    /// Capture `tag`'s `gc` groups in `text` into the current context frame,
    /// when there are groups and the frame collects them. Returns whether the
    /// frame collects them: a match that captures is not memoised.
    fn capture_groups(&mut self, gc: i32, tag: &Tag, text: &str) -> bool {
        let frame = self
            .scratch
            .context_stack
            .last_mut()
            .and_then(|f| f.regexgrps.map(|idx| (idx, &mut f.regexgrp_ct)))
            .filter(|_| gc > 0);
        let Some((idx, regexgrp_ct)) = frame else {
            return false;
        };
        if let Some(re) = &tag.regexp {
            let rg = &mut self.scratch.regexgrps_store[idx];
            capture_regex(gc, regexgrp_ct, rg, re, text);
        }
        true
    }

    /// Case (7) of [`Self::does_tag_match_reading`] for `"<.*>"`: the wordform
    /// of the reading's cohort, the same one throughout a unification.
    fn match_any_wordform(
        &mut self,
        reading: ReadingId,
        tag: &Tag,
        unif_mode: bool,
    ) -> Result<u32, RunError> {
        let cid = self.reading_cohort(reading, tag)?;
        #[expect(
            clippy::unwrap_used,
            reason = "every cohort gets a wordform where it is made (each stream reader, the >>> cohort in run_grammar, ADDCOHORT and the splitting rules in restructure); only cohort_clear resets it"
        )]
        let wf = self.cohorts.get(cid.0).wordform.unwrap();
        let wf_hash = self.grammar.single_tags_list[wf.0].hash;
        let mut m = wf_hash.get();
        if unif_mode {
            if self.scratch.unif_last_wordform != TagHash(0) {
                if self.scratch.unif_last_wordform != wf_hash {
                    m = 0;
                }
            } else {
                self.scratch.unif_last_wordform = wf_hash;
            }
        }
        Ok(m)
    }

    /// Case (8) of [`Self::does_tag_match_reading`], a numerical tag: the
    /// last of the reading's numerical tags it matches wins.
    fn match_numerical(
        &mut self,
        reading: ReadingId,
        tag_id: TagId,
        tag: &Tag,
    ) -> Result<u32, RunError> {
        let nums: Vec<TagId> = self
            .readings
            .get(reading.0)
            .tags_numerical
            .values()
            .copied()
            .collect();
        let mut m = 0;
        for tid in nums {
            let itag = self.grammar.single_tags_list[tid.0].clone();
            let rv = test_tag_numerical(
                self.cohorts,
                self.readings,
                self.grammar,
                reading,
                tag_id,
                tag,
                &itag,
            );
            let Some(rv) = rv else {
                let why = RuleInapplicable::BagOfTagsCohort {
                    tag: tag.tag.clone(),
                };
                return Err(self.rule_inapplicable(why));
            };
            if rv != TagHash(0) {
                m = rv.get();
            }
        }
        Ok(m)
    }

    /// The variables a `VAR:` or `LVAR:` tag reads for `reading`: the global
    /// ones, or for `LVAR:` outside the current window, its own window's.
    fn tag_variables(
        &mut self,
        reading: ReadingId,
        ttype: crate::tag::TagType,
        tag: &Tag,
    ) -> Result<Vec<(u32, u32)>, RunError> {
        let cid = self.reading_cohort(reading, tag)?;
        let sw_opt = self.cohorts.get(cid.0).parent;
        if sw_opt == self.stream.current || !ttype.intersects(T_LOCAL_VARIABLE) {
            return Ok(collect_fum(self.variables));
        }
        #[expect(
            clippy::unwrap_used,
            reason = "a cohort in a window has a parent: alloc_cohort(Some(sw)) and append_cohort set it, and only cohort_clear, on free, resets it"
        )]
        let sw = sw_opt.unwrap();
        Ok(collect_fum(&self.single_windows.get(sw.0).variables_set))
    }

    /// Cases (10) and (11) of [`Self::does_tag_match_reading`], `_LEFT_` and
    /// `_RIGHT_`: the reading is the enclosure's `edge` — its tag, at its
    /// position — while one is being run.
    fn match_par_edge(
        &mut self,
        reading: ReadingId,
        tag: &Tag,
        edge: (TagHash, u32),
    ) -> Result<u32, RunError> {
        let (edge_tag, edge_pos) = edge;
        if edge_tag == TagHash(0) {
            return Ok(0);
        }
        let cid = self.reading_cohort(reading, tag)?;
        let r = self.readings.get(reading.0);
        let has = r.tags.find(edge_tag.get()) != r.tags.end();
        let at = self.cohorts.get(cid.0).local_number == edge_pos;
        Ok(if at && has { self.grammar.tag_any } else { 0 })
    }

    /// Case (12) of [`Self::does_tag_match_reading`], `_ENCL_`: the cohort
    /// after the reading's own in its window is enclosed.
    fn match_enclosure(&mut self, reading: ReadingId, tag: &Tag) -> Result<u32, RunError> {
        let cid = self.reading_cohort(reading, tag)?;
        let (sw_id, local_number) = {
            let c = self.cohorts.get(cid.0);
            #[expect(
                clippy::unwrap_used,
                reason = "a cohort in a window has a parent: alloc_cohort(Some(sw)) and append_cohort set it, and only cohort_clear, on free, resets it"
            )]
            let sw = c.parent.unwrap();
            (sw, c.local_number as usize)
        };
        let all = &self.single_windows.get(sw_id.0).all_cohorts;
        // std::find(begin + local_number, end, reading.parent), then ++c.
        let mut idx = local_number;
        while idx < all.len() && all[idx] != cid {
            idx += 1;
        }
        let cpos = idx + 1;
        let enclosed = cpos < all.len() && self.cohorts.get(all[cpos].0).enclosed != 0;
        Ok(u32::from(enclosed))
    }

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// Whether variable value `itval` is the one a `VAR:name=value` tag asks
    /// for; a bare `VAR:name` asks only that the variable be set.
    ///
    /// The value's hash shares the tag's union with the other roles, and a
    /// `VAR:<…>` tag is parsed as numerical too, which can overwrite it: a failed
    /// math offset leaves 0 there (`VAR:<x=5+>`), which the C++ reads as a bare
    /// test and so does this. A slot holding any other role is a bare test as
    /// well, and a value hash no interned tag answers to matches nothing.
    fn variable_value_matches(&mut self, tag: &Tag, itval: u32, bypass_index: bool) -> bool {
        let want = match tag.extra {
            crate::tag::TagUnion::VariableHash(h) => h,
            _ => 0,
        };
        if want == 0 {
            return true;
        }
        let it = self.grammar.single_tags().find(want);
        if it == self.grammar.single_tags().end() {
            return false;
        }
        let comp_tid = it.get().1;
        let comp_tag = self.grammar.single_tags_list[comp_tid.0].clone();
        let comp_type = self.grammar.tag_type(comp_tid);
        if comp_type.intersects(T_REGEXP) {
            self.does_tag_match_regexp(itval, &comp_tag, bypass_index) != 0
        } else if comp_type.intersects(T_CASE_INSENSITIVE) {
            self.does_tag_match_icase(itval, &comp_tag, bypass_index) != 0
        } else {
            comp_tag.hash.get() == itval
        }
    }

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// A `SET:name` tag: matches when the reading matches the set so named.
    /// Only the `STATIC-SETS` keep their names past `reindex`, so a name that is
    /// not one of them — written in the grammar, or built by a varstring — is a
    /// run error naming the rule.
    ///
    /// DIVERGENCE: the C++ read past the end of its name table instead.
    fn does_set_tag_match(
        &mut self,
        reading: ReadingId,
        tag: &Tag,
        bypass_index: bool,
        unif_mode: bool,
    ) -> Result<u32, crate::error::RunError> {
        let it = self.grammar.sets_by_name.find(hash_value_str(&tag.tag, 0));
        if it == self.grammar.sets_by_name.end() {
            let why = crate::error::RuleInapplicable::SetNotStatic {
                name: tag.tag.clone(),
            };
            return Err(self.rule_inapplicable(why));
        }
        let set = it.get().1;
        Ok(self.does_named_set_match(reading, set, bypass_index, unif_mode)? as u32)
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-trie-fn]
    /// Recursive trie walk: does the reading contain a complete tag path in the
    /// set's trie (or `trie_special` when `special`)? The sub-trie to walk is named
    /// by `set`/`special`/`path` (the `TagId` prefix already descended), re-borrowed
    /// fresh from `self.grammar` at each step so no trie borrow is held across a
    /// `&mut self` re-entry — `path` is threaded (push on descend, pop on backtrack),
    /// giving each terminal its root-to-node path for the address-free [`UnifKey`].
    /// Entries are visited in ascending-`Tag::hash` order (the C++ flat_map order).
    ///
    /// The C++ recurses per trie level. Here each level being walked is kept
    /// on a heap stack instead: descending pushes the child level, and a level
    /// that runs out without a match pops back to its parent, dropping the tag
    /// the parent pushed onto `path` — where the recursion would return false.
    /// A match at any depth leaves `path` as the caller passed it.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn does_set_match_reading_trie(
        &mut self,
        reading: ReadingId,
        set_number: u32,
        set: u32,
        special: bool,
        path: &mut Vec<TagId>,
        unif_mode: bool,
    ) -> Result<bool, crate::error::RunError> {
        let base = path.len();
        let mut levels = vec![self.trie_level_entries(set, special, path)];
        loop {
            let Some(level) = levels.last_mut() else {
                return Ok(false);
            };
            let Some(tid) = level.next() else {
                levels.pop();
                if !levels.is_empty() {
                    path.pop();
                }
                continue;
            };
            let step = TrieStep {
                set_number,
                set,
                special,
                unif_mode,
            };
            match self.trie_entry_step(reading, step, path, tid)? {
                TrieEntry::Skip => {}
                TrieEntry::Matched => {
                    path.truncate(base);
                    return Ok(true);
                }
                TrieEntry::Descend => levels.push(self.trie_level_entries(set, special, path)),
            }
        }
    }

    /// The entries of the trie level [`Self::does_set_match_reading_trie`]
    /// walks at `path`, in ascending `Tag::hash` order, snapshot with a short
    /// borrow; none when `path` names no level.
    fn trie_level_entries(
        &self,
        set: u32,
        special: bool,
        path: &[TagId],
    ) -> std::vec::IntoIter<TagId> {
        let mut entries: Vec<(TagId, u32)> = match self.trie_level_at(set, special, path) {
            Some(t) => t
                .keys()
                .map(|k| (*k, self.grammar.single_tags_list[k.0].hash.get()))
                .collect(),
            None => Vec::new(),
        };
        entries.sort_by_key(|e| e.1);
        let tids: Vec<TagId> = entries.into_iter().map(|(tid, _)| tid).collect();
        tids.into_iter()
    }

    /// One entry of a trie level: test the reading for the tag, and say
    /// whether the walk skips it, has matched a whole path through it, or
    /// goes on into its sub-trie (with the tag left pushed onto `path`).
    fn trie_entry_step(
        &mut self,
        reading: ReadingId,
        step: TrieStep,
        path: &mut Vec<TagId>,
        tid: TagId,
    ) -> Result<TrieEntry, crate::error::RunError> {
        let tagv = self.grammar.single_tags_list[tid.0].clone();
        let matched = self.does_tag_match_reading(reading, tid, &tagv, step.unif_mode, false)? != 0;
        if !matched || self.grammar.tag_type(tid).intersects(T_FAILFAST) {
            return Ok(TrieEntry::Skip);
        }
        path.push(tid);
        // Re-borrow the node fresh to read its flags (short borrow).
        let (terminal, has_child) = match self.trie_node_at(step.set, step.special, path) {
            Some(n) => (n.terminal, n.trie.is_some()),
            None => (false, false),
        };
        if terminal {
            let unified = !step.unif_mode || {
                let key = UnifKey {
                    special: step.special,
                    path: path.clone(),
                };
                self.check_unif_tags(step.set_number, key)
            };
            if !unified {
                path.pop();
                return Ok(TrieEntry::Skip);
            }
            return Ok(TrieEntry::Matched);
        }
        if has_child {
            return Ok(TrieEntry::Descend);
        }
        path.pop();
        Ok(TrieEntry::Skip)
    }

    /// Resolve `set`'s `trie`/`trie_special` (per `special`) down `path` (a
    /// `TagId` sequence) to the named node, `None` when the path is absent. A
    /// SHORT borrow: the returned ref lives only until the caller's snapshot
    /// completes, so it is never held across a `&mut self` re-entry. `path` is the
    /// root-to-node key of the address-free [`UnifKey`]; navigating it fresh at
    /// each matcher step replaces holding a laundered sub-trie borrow. Requires a
    /// non-empty `path` (a node is named by at least one key).
    fn trie_node_at(&self, set: u32, special: bool, path: &[TagId]) -> Option<&TrieNode> {
        let s = self.grammar.set_by_number(SetNumber(set));
        let root = if special { &s.trie_special } else { &s.trie };
        let mut node = root.get(path.first()?)?;
        for tid in &path[1..] {
            node = node.trie.as_deref()?.get(tid)?;
        }
        Some(node)
    }

    /// The trie LEVEL walked when at `path`: the root trie for an empty `path`
    /// (the C++ top-level `doesSetMatchReading_trie(theset.trie_special)` walk),
    /// else the child-trie of the node named by `path`. Short borrow, like
    /// [`Self::trie_node_at`].
    fn trie_level_at(&self, set: u32, special: bool, path: &[TagId]) -> Option<&TagTrie> {
        let s = self.grammar.set_by_number(SetNumber(set));
        let root = if special { &s.trie_special } else { &s.trie };
        if path.is_empty() {
            return Some(root);
        }
        self.trie_node_at(set, special, path)?.trie.as_deref()
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-tags-fn]
    /// Tests whether a reading matches a LIST set. Takes the set's `number` (the
    /// resolved `theset.number`) and navigates its `ff_tags`/`trie`/`trie_special`
    /// fresh from `self.grammar` at each step (short borrows), so no grammar borrow
    /// aliases `&mut self` — the former laundered-reference `unsafe` is gone.
    pub fn does_set_match_reading_tags(
        &mut self,
        reading: ReadingId,
        set_number: u32,
        unif_mode: bool,
    ) -> Result<bool, crate::error::RunError> {
        let mut retval = false;

        // Fail-fast pre-check. Snapshot the ff_tags ids with a short borrow.
        let ff: Vec<TagId> = {
            let s = self.grammar.set_by_number(SetNumber(set_number));
            if s.ff_tags.empty() {
                Vec::new()
            } else {
                s.ff_tags.iter().copied().collect()
            }
        };
        for tid in ff {
            let tagv = self.grammar.single_tags_list[tid.0].clone();
            if self.does_tag_match_reading(reading, tid, &tagv, unif_mode, false)? != 0 {
                return Ok(false);
            }
        }

        // Main fast path: merge-intersect the reading's plain tags with the trie's
        // first-level keys (both ascending by hash). `entries` is snapshotted with
        // a short borrow, each key with its node's flags (never held across a
        // `&mut self` re-entry). `path` is the root-to-node key of the [`UnifKey`].
        let plain: Vec<u32> = self.readings.get(reading.0).tags_plain.as_slice().to_vec();
        let entries: Vec<(TagId, u32, bool, bool)> = {
            match self.trie_level_at(set_number, false, &[]) {
                Some(t) if !plain.is_empty() => {
                    let mut e: Vec<(TagId, u32, bool, bool)> = t
                        .iter()
                        .map(|(k, n)| {
                            let hash = self.grammar.single_tags_list[k.0].hash.get();
                            (*k, hash, n.terminal, n.trie.is_some())
                        })
                        .collect();
                    e.sort_by_key(|x| x.1);
                    e
                }
                _ => Vec::new(),
            }
        };
        if !entries.is_empty() {
            let front_hash = plain[0]; // tags_plain.front() (smallest)
            let smallest_trie_hash = entries[0].1; // trie.begin()->first->hash
            let mut oi = plain.partition_point(|&x| x < smallest_trie_hash);
            let mut ii = entries.partition_point(|e| e.1 < front_hash);
            let mut path: Vec<TagId> = Vec::new();
            while oi < plain.len() && ii < entries.len() {
                if plain[oi] == entries[ii].1 {
                    let (tid, _, terminal, has_child) = entries[ii];
                    path.clear();
                    path.push(tid);
                    if terminal {
                        if unif_mode {
                            let key = UnifKey {
                                special: false,
                                path: path.clone(),
                            };
                            if !self.check_unif_tags(set_number, key) {
                                ii += 1;
                                continue;
                            }
                        }
                        retval = true;
                        break;
                    }
                    if has_child
                        && self.does_set_match_reading_trie(
                            reading, set_number, set_number, false, &mut path, unif_mode,
                        )?
                    {
                        retval = true;
                        break;
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

        if !retval {
            let has_special = self
                .grammar
                .set_by_number(SetNumber(set_number))
                .trie_special
                .is_empty();
            if !has_special {
                let mut path: Vec<TagId> = Vec::new();
                retval = self.does_set_match_reading_trie(
                    reading, set_number, set_number, true, &mut path, unif_mode,
                )?;
            }
        }
        Ok(retval)
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// Tests whether a reading matches a LIST or SET set, evaluating operators
    /// recursively with a yes/no memo cache.
    ///
    /// The C++ recurses into the member sets of a set built from sets. Here
    /// each such set part way through its members is a frame on a heap stack
    /// (see `set_ops`), stepping through them in the C++ order, so a set built
    /// from sets however deep costs no stack.
    // [spec:cg3:req:robustness.depth-bounded]
    pub fn does_set_match_reading(
        &mut self,
        reading: ReadingId,
        set: u32,
        bypass_index: bool,
        unif_mode: bool,
    ) -> Result<bool, crate::error::RunError> {
        let mut open: Vec<super::set_ops::SetFrame> = Vec::new();
        let mut decided = self.set_match_begin(&mut open, reading, set, bypass_index, unif_mode)?;
        loop {
            // A set that is decided hands its result to the set it is a member
            // of, where the recursion would return it.
            if let Some(matched) = decided.take() {
                let Some(frame) = open.last_mut() else {
                    return Ok(matched);
                };
                self.set_frame_take(frame, matched);
            }
            let Some(frame) = open.last_mut() else {
                return Ok(false);
            };
            match frame.step() {
                SetStep::Test(member, unif) => {
                    decided =
                        self.set_match_begin(&mut open, reading, member, bypass_index, unif)?;
                }
                SetStep::Done(retval) => {
                    let Some(frame) = open.pop() else {
                        return Ok(retval);
                    };
                    decided = Some(self.set_match_finish(reading, frame, retval));
                }
            }
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    /// Runs the LINK-chain test that follows the current one, if any, returning
    /// whether it matched (true when there is no linked test).
    pub fn does_set_match_cohort_test_linked(
        &mut self,
        cohort: CohortId,
        set: u32,
        context: &mut CohortMatchContext,
    ) -> Result<bool, crate::error::RunError> {
        let mut retval = true;
        // The template link taken off `tmpl_cntx.linked`, to put back.
        let mut reset: Option<crate::arena::CtxId> = None;
        let mut linked: Option<crate::arena::CtxId> = None;
        let mut min: Option<CohortId> = None;
        let mut max: Option<CohortId> = None;

        let ctx_test_linked = context
            .test
            .and_then(|cid| self.grammar.contexts_arena[cid.0].linked);
        if let Some(l) = ctx_test_linked {
            linked = Some(l);
        } else if !self.scratch.tmpl_cntx.linked.is_empty() {
            min = self.scratch.tmpl_cntx.min;
            max = self.scratch.tmpl_cntx.max;
            linked = self.scratch.tmpl_cntx.linked.last().copied();
            self.scratch.tmpl_cntx.linked.pop();
            reset = linked;
        }
        if let Some(l) = linked {
            if !context.did_test {
                let lpos = self.grammar.contexts_arena[l.0].pos;
                // A LINK target is its own test object; the POS_TMPL_OVERRIDE
                // write never reached it, so it runs with no override.
                let lref = crate::contextual_test::TestRef::new(l);
                let (cparent, clocal) = {
                    let c = self.cohorts.get(cohort.0);
                    (c.parent, c.local_number)
                };
                let res = if lpos.intersects(POS_NO_PASS_ORIGIN) {
                    self.run_linked_test(
                        cparent,
                        clocal,
                        lref,
                        context.deep.as_deref_mut(),
                        Some(cohort),
                    )?
                } else {
                    self.run_linked_test(
                        cparent,
                        clocal,
                        lref,
                        context.deep.as_deref_mut(),
                        context.origin,
                    )?
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
        if let Some(l) = reset {
            self.scratch.tmpl_cntx.linked.push(l);
        }
        if !retval {
            self.scratch.tmpl_cntx.min = min;
            self.scratch.tmpl_cntx.max = max;
        }
        Ok(retval)
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
        mut context: Option<&mut CohortMatchContext>,
    ) -> Result<bool, crate::error::RunError> {
        let mut retval = false;
        let mut utags = self.scratch.ss_utags.get();
        let mut usets = self.scratch.ss_usets.get();
        let orz = self
            .scratch
            .context_stack
            .last()
            .map_or(0, |f| f.regexgrp_ct);

        let (stype, snumber) = {
            let s = self.grammar.set_by_number(SetNumber(set)); // grammar->sets_list[set]
            (s.r#type, s.number.get())
        };
        let cur_flags = self
            .scratch
            .current_rule
            .map(|rid| self.grammar.rule_by_number[rid.0].flags)
            .unwrap_or_default();
        let child_unify = stype.intersects(ST_CHILD_UNIFY);
        let cap_unif = cur_flags.intersects(RF_CAPTURE_UNIF);

        if context.is_some()
            && !cap_unif
            && child_unify
            && let Some(f) = self.scratch.context_stack.last()
        {
            #[expect(
                clippy::unwrap_used,
                reason = "a set is matched under a context frame only while run_single_rule_body matches a reading, after giving the frame its unif_tags and unif_sets indices (fresh, or from the plain-signature cache), or while an action runs under a saved copy of such a frame"
            )]
            let (ut_idx, us_idx) = (f.unif_tags.unwrap(), f.unif_sets.unwrap());
            utags = self.scratch.unif_tags_store[ut_idx].clone();
            usets = self.scratch.unif_sets_store[us_idx].clone();
        }

        let bypass = stype.intersects(ST_CHILD_UNIFY | ST_SPECIAL);
        if self.does_set_match_reading(reading, snumber, bypass, false)? {
            retval = true;
            if let Some(ctx) = context.as_deref_mut() {
                if ctx.options.intersects(POS_ATTACH_TO) {
                    // reading.matched_target = true (scratch-resident flag).
                    self.scratch.matched_target.insert(reading);
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
        if retval
            && let Some(ctx) = context.as_deref_mut()
            && !ctx.in_barrier
        {
            let attach = ctx.options.intersects(POS_ATTACH_TO);
            retval = self.does_set_match_cohort_test_linked(cohort, set, ctx)?;
            if attach {
                // reading.matched_tests = retval — retval can be FALSE, so
                // this is a real bool store (insert-or-remove), not a mark.
                self.scratch.set_matched_tests(reading, retval);
                if retval && let Some(f) = self.scratch.context_stack.last_mut() {
                    f.attach_to.cohort = Some(cohort);
                    f.attach_to.reading = None; // set by doesSetMatchCohortNormal
                    f.attach_to.subreading = Some(reading);
                }
            }
        }

        // Rollback on failure.
        if !retval
            && context.is_some()
            && !cap_unif
            && child_unify
            && let Some(f) = self.scratch.context_stack.last()
        {
            #[expect(
                clippy::unwrap_used,
                reason = "a set is matched under a context frame only while run_single_rule_body matches a reading, after giving the frame its unif_tags and unif_sets indices (fresh, or from the plain-signature cache), or while an action runs under a saved copy of such a frame"
            )]
            let (ut_idx, us_idx) = (f.unif_tags.unwrap(), f.unif_sets.unwrap());
            let entry = &mut self.scratch.unif_tags_store[ut_idx];
            let differs = utags.len() != entry.len() || utags != *entry;
            if differs {
                std::mem::swap(entry, &mut utags);
            }
            let entry = &mut self.scratch.unif_sets_store[us_idx];
            let differs = usets.len() != entry.len();
            if differs {
                std::mem::swap(entry, &mut usets);
            }
        }
        if !retval && let Some(f) = self.scratch.context_stack.last_mut() {
            f.regexgrp_ct = orz;
        }
        Ok(retval)
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-normal-fn]
    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// Normal cohort matching: the set matches if ANY eligible reading matches.
    pub fn does_set_match_cohort_normal(
        &mut self,
        cohort: CohortId,
        set: u32,
        mut context: Option<&mut CohortMatchContext>,
    ) -> Result<bool, crate::error::RunError> {
        let mut retval = false;

        let opts = context.as_deref().map(|c| c.options).unwrap_or_default();
        let guard = !(context.is_none()
            || (opts.intersects(POS_LOOK_DELETED | POS_LOOK_DELAYED | POS_LOOK_IGNORED | POS_NOT)));
        if guard {
            let ps = &self.cohorts.get(cohort.0).possible_sets;
            if set as usize >= ps.len() || !ps[set as usize] {
                return Ok(retval);
            }
        }

        // wread pre-check.
        let wread = self.cohorts.get(cohort.0).wread;
        if let Some(wr) = wread {
            let in_barrier = context.as_deref().map(|c| c.in_barrier).unwrap_or(false);
            if context.is_none() || !in_barrier {
                retval =
                    self.does_set_match_cohort_helper(cohort, wr, set, context.as_deref_mut())?;
            }
        }
        if retval {
            let done = match context.as_deref() {
                None => true,
                Some(c) => c.did_test,
            };
            if done {
                return Ok(retval);
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
                let active = self.readings.get(reading.0).active;
                if let Some(ctx) = context.as_deref() {
                    if !active && ctx.options.intersects(POS_ACTIVE) {
                        continue;
                    }
                    if active && ctx.options.intersects(POS_INACTIVE) {
                        continue;
                    }
                }
                if self.does_set_match_cohort_helper(
                    cohort,
                    reading,
                    set,
                    context.as_deref_mut(),
                )? {
                    retval = true;
                    self.backfill_attach_reading(cohort, reading, reading_head);
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
                    return Ok(retval);
                }
            }
        }

        // POS_NOT: run the linked test even though nothing matched.
        let do_tl = context.filter(|c| !c.matched_target && c.options.intersects(POS_NOT));
        if let Some(ctx) = do_tl {
            retval = self.does_set_match_cohort_test_linked(cohort, set, ctx)?;
        }

        // DIVERGENCE (operator decision, plan node
        // `matcher-doc-split.possible-sets-prune`): the C++ possible_sets
        // pruning that sat here (`cohort.possible_sets.reset(set)` on a failed
        // non-ACTIVE/INACTIVE match, matchSet.cpp:1048) is deleted, not ported —
        // the same assumed-non-matching pattern as the dropped
        // index_ruleCohort_no visited-set, and interleaved A/B put its benefit
        // at noise level. `possible_sets` remains the conservative may-match
        // index maintained by reflow and the stream parsers; the matcher only
        // READS it.

        Ok(retval)
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-careful-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-careful-fn+1]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-careful-fn+1]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-careful-fn+1]
    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// Careful ("C") cohort matching: the set must match EVERY eligible reading.
    pub fn does_set_match_cohort_careful(
        &mut self,
        cohort: CohortId,
        set: u32,
        mut context: Option<&mut CohortMatchContext>,
    ) -> Result<bool, crate::error::RunError> {
        let mut retval = false;

        let opts = context.as_deref().map(|c| c.options).unwrap_or_default();
        let guard = !(context.is_none()
            || (opts.intersects(POS_LOOK_DELETED | POS_LOOK_DELAYED | POS_LOOK_IGNORED | POS_NOT)));
        if guard {
            let ps = &self.cohorts.get(cohort.0).possible_sets;
            if set as usize >= ps.len() || !ps[set as usize] {
                return Ok(retval);
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
                let active = self.readings.get(reading.0).active;
                if let Some(ctx) = context.as_deref() {
                    if !active && ctx.options.intersects(POS_ACTIVE) {
                        continue;
                    }
                    if active && ctx.options.intersects(POS_INACTIVE) {
                        continue;
                    }
                }
                retval = self.does_set_match_cohort_helper(
                    cohort,
                    reading,
                    set,
                    context.as_deref_mut(),
                )?;
                if !retval {
                    break;
                }
                // DIVERGENCE: the C++ leaves attach_to.reading null here, and
                // SELECT/REMOVE/COPY through a careful attaching context crash.
                self.backfill_attach_reading(cohort, reading, reading0);
            }
            if !retval {
                break 'outer;
            }
        }

        let do_tl = context.filter(|c| !c.matched_target && c.options.intersects(POS_NOT));
        if let Some(ctx) = do_tl {
            retval = self.does_set_match_cohort_test_linked(cohort, set, ctx)?;
        }

        Ok(retval)
    }

    /// Builds the C++ `ReadingList* lists[4]` array: slot 0 = `cohort.readings`;
    /// slots 1..3 = `deleted`/`delayed`/`ignored` only when the corresponding
    /// POS_LOOK_* option is set (and a context is present). The id lists are cloned
    /// so the `cohorts` arena is not borrowed across the matcher recursion. Not a
    /// manifest symbol — shared setup for the two cohort matchers.
    fn gather_lists(
        &self,
        cohort: CohortId,
        context: Option<&CohortMatchContext>,
    ) -> [Option<Vec<ReadingId>>; 4] {
        let c = self.cohorts.get(cohort.0);
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

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// Back-fill the attach-to reading head once `reading` (possibly a
    /// sub-reading of `head`) matched an attaching context:
    /// `does_set_match_cohort_helper` knows only the sub-reading. Shared by the
    /// two cohort matchers.
    fn backfill_attach_reading(&mut self, cohort: CohortId, reading: ReadingId, head: ReadingId) {
        if let Some(f) = self.scratch.context_stack.last_mut()
            && f.attach_to.cohort == Some(cohort)
            && f.attach_to.subreading == Some(reading)
        {
            f.attach_to.reading = Some(head);
        }
    }

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
        if !bypass_index && self.scratch.index_regexp_no.contains(ih) {
            m = 0;
        } else if !bypass_index && gc == 0 && self.scratch.index_regexp_yes.contains(ih) {
            m = test;
        } else {
            // itag = *(grammar->single_tags.find(test)->second)
            let (itag_hash, itag_text) = {
                let it = self.grammar.single_tags().find(test);
                let tid = it.get().1;
                let t = &self.grammar.single_tags_list[tid.0];
                (t.hash.get(), t.tag.clone())
            };
            // The C++ unanchored find == unanchored `is_match`.
            if let Some(re) = &tag.regexp
                && re.is_match(&itag_text)
            {
                m = itag_hash;
            }
            if m != 0 {
                if !self.capture_groups(gc, tag, &itag_text) {
                    self.scratch.index_regexp_yes.insert(ih);
                }
            } else {
                self.scratch.index_regexp_no.insert(ih);
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
        if !bypass_index && self.scratch.index_icase_no.contains(ih) {
            m = 0;
        } else if !bypass_index && self.scratch.index_icase_yes.contains(ih) {
            m = test;
        } else {
            let (itag_hash, itag_text) = {
                let it = self.grammar.single_tags().find(test);
                let tid = it.get().1;
                let t = &self.grammar.single_tags_list[tid.0];
                (t.hash.get(), t.tag.clone())
            };
            if eq_ignore_case(&tag.tag, &itag_text) {
                m = itag_hash;
            }
            if m != 0 {
                self.scratch.index_icase_yes.insert(ih);
            } else {
                self.scratch.index_icase_no.insert(ih);
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
            let r = self.readings.get(reading.0);
            (r.tags_string_hash, r.tags_string.clone())
        };
        let ih = make_64(tsh, tag.hash.get());
        if !bypass_index && self.scratch.index_regexp_no.contains(ih) {
            m = 0;
        } else if !bypass_index && gc == 0 && self.scratch.index_regexp_yes.contains(ih) {
            m = tsh;
        } else {
            if let Some(re) = &tag.regexp
                && re.is_match(&ts)
            {
                m = tsh;
            }
            if m != 0 {
                if !self.capture_groups(gc, tag, &ts) {
                    self.scratch.index_regexp_yes.insert(ih);
                }
            } else {
                self.scratch.index_regexp_no.insert(ih);
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
    /// `tags_textual` entry wins. `tag_id` names `tag` in the arena, so the
    /// `T_REGEXP_LINE` test reads the run's flags.
    pub fn does_regexp_match_reading(
        &mut self,
        reading: ReadingId,
        tag_id: TagId,
        tag: &Tag,
        bypass_index: bool,
    ) -> u32 {
        if self.grammar.tag_type(tag_id).intersects(T_REGEXP_LINE) {
            return self.does_regexp_match_line(reading, tag, bypass_index);
        }
        let textual: Vec<u32> = self
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
        let tags_list: Vec<u32> = self.readings.get(reading.0).tags_list.clone();
        for &tid in the_tags {
            let tag = self.grammar.single_tags_list[tid.0].clone();
            let ttype = self.grammar.tag_type(tid);
            for &tt in &tags_list {
                let mut m: u32 = 0;
                let itag_id = {
                    let it = self.grammar.single_tags().find(tt);
                    it.get().1
                };
                let itype = self.grammar.tag_type(itag_id);
                let (ihash, itag0) = {
                    let t = &self.grammar.single_tags_list[itag_id.0];
                    (t.hash, t.tag.chars().next().unwrap_or('\0'))
                };
                if tag.regexp.is_some() {
                    m = self.does_tag_match_regexp(tt, &tag, false);
                } else if ttype.intersects(T_CASE_INSENSITIVE) {
                    m = self.does_tag_match_icase(tt, &tag, false);
                } else if (ttype.intersects(T_REGEXP_ANY)) && (itype.intersects(T_TEXTUAL)) {
                    if ttype.intersects(T_BASEFORM) {
                        if itype.intersects(T_BASEFORM) {
                            m = self.readings.get(reading.0).baseform.map_or(0, |h| h.get());
                        }
                    } else if ttype.intersects(T_WORDFORM) {
                        if itype.intersects(T_WORDFORM) {
                            #[expect(
                                clippy::unwrap_used,
                                reason = "a reading or sub-reading belongs to a cohort: readers and rules allocate one with alloc_reading(Some(cohort)) or copy one that was, and a rule's EXCEPT asks this of the reading it acts on"
                            )]
                            let cid = self.readings.get(reading.0).parent.unwrap();
                            #[expect(
                                clippy::unwrap_used,
                                reason = "every cohort gets a wordform where it is made (each stream reader, the >>> cohort in run_grammar, ADDCOHORT and the splitting rules in restructure); only cohort_clear resets it"
                            )]
                            let wf = self.cohorts.get(cid.0).wordform.unwrap();
                            m = self.grammar.single_tags_list[wf.0].hash.get();
                        }
                    } else if !itype.intersects(T_BASEFORM | T_WORDFORM) {
                        let tag0 = tag.tag.chars().next().unwrap_or('\0');
                        if (tag0 == '"' && itag0 == '"') || (tag0 == '<' && itag0 == '<') {
                            m = ihash.get();
                        }
                    }
                } else if (ttype.intersects(T_NUMERICAL)) && (itype.intersects(T_NUMERICAL)) {
                    let itag = self.grammar.single_tags_list[itag_id.0].clone();
                    let rv = test_tag_numerical(
                        self.cohorts,
                        self.readings,
                        self.grammar,
                        reading,
                        tid,
                        &tag,
                        &itag,
                    );
                    #[expect(
                        clippy::unwrap_used,
                        reason = "test_tag_numerical is None only for a reading with no cohort, and a rule's EXCEPT asks this of the reading it acts on, which belongs to one: readers and rules allocate one with alloc_reading(Some(cohort)) or copy one that was"
                    )]
                    let rv = rv.unwrap();
                    m = rv.get();
                } else if tag.hash == ihash {
                    m = ihash.get();
                }
                if m != 0 {
                    rv_tags.push(itag_id);
                }
            }
        }
    }
}
