//! Port of `src/ApertiumApplicator.{cpp,hpp}` — the Apertium stream I/O
//! applicator (`^wordform/reading.../reading$` cohorts with superblanks).
//!
//! COMPOSITION-OVER-INHERITANCE. C++ `class ApertiumApplicator : public virtual
//! GrammarApplicator`. In Rust the wrapper either owns or borrows the base
//! engine, and every engine/core call goes through `self.base.<method>` /
//! `self.base.store` / `self.base.grammar` / `self.base.window`. Apertium-
//! specific member flags live alongside.
//!
//! ARENA MODEL. C++ `Cohort*`/`Reading*`/`SingleWindow*`/`Tag*` become the arena
//! ids `CohortId`/`ReadingId`/`SwId`/`TagId` resolved through
//! `self.base.store` (`cohorts`/`readings`/`single_windows` arenas) and
//! `self.base.grammar.single_tags_list` (the `Tag` arena). Nullable pointers →
//! `Option<…Id>`. The char-by-char C++ state machines walk `UChar` (UTF-16 code
//! units); here `UString = String` and the walks are over `Vec<char>` scratch
//! buffers (matching the already-ported engine `run_grammar.rs` convention).
//!
//! OUTPUT SINK. C++ `std::ostream& output` → generic `output: &mut W`
//! (`W: std::io::Write`); the `uextras::{u_fputc, u_fflush}`
//! primitives write UTF-8. `u_fprintf_u` (UChar pattern) collapses to
//! `format_args!` with the literal Unicode chars.
//!
//! REPRODUCED BUGS (bug-for-bug):
//! * `esc_lt` sentinel `'\1'` substitution for escaped `\<` in reading baseforms
//!   (see `run_grammar_on_text` reading loop + `process_reading` rewrite).
//! * `parseStreamVar`: in the comma/`=` case (b), the live member `variables`
//!   map is NEVER updated — only the single bare-identifier case (a) touches it,
//!   and only when `c_swindow` is null.

use std::io::Write;
use std::ops::DerefMut;

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::cohort::{CT_AP_UNKNOWN, CT_REMOVED, alloc_cohort, append_reading, unignore_all};
use crate::grammar_applicator::GrammarApplicator;
use crate::inlines::{hash_value, insert_if_exists};
use crate::reading::{Reading, ReadingList, alloc_reading, free_reading};
use crate::single_window::{SingleWindow, append_cohort};
use crate::tag::{T_BASEFORM, T_DEPENDENCY, T_MAPPING, T_WORDFORM, TagList};
use crate::types::{TagHash, UString, flags_t};
use crate::uextras::{U_EOF, u_fflush, u_fgetc, u_fputc, ux_strip_bom};

// C++ `constexpr UChar esc_lt = '\1';` — the sentinel the reading scanner
// substitutes for an escaped `\<` so it becomes literal baseform text rather
// than a tag opener.
const ESC_LT: char = '\u{1}';

// C++ `Strings.hpp` string constants (UTF-16 → UTF-8 &str). Duplicated from the
// engine's private copies (`grammar_applicator/core.rs`) so this module compiles
// without depending on those private consts.
const STR_BEGINTAG: &str = ">>>";
const STR_ENDTAG: &str = "<<<";
const STR_CMD_SETVAR: &str = "<STREAMCMD:SETVAR:";
const STR_CMD_REMVAR: &str = "<STREAMCMD:REMVAR:";

// C++ `Strings.hpp` `constexpr UChar not_sign = u'¬';`.
const NOT_SIGN: char = '\u{AC}';

// [spec:cg3:def:apertium-applicator.cg3.apertium-casing]
/// C++ `enum ApertiumCasing { Nochange; Title; Upper; }`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ApertiumCasing {
    Nochange,
    Title,
    Upper,
}

// [spec:cg3:def:apertium-applicator.cg3.apertium-applicator]
/// C++ `class ApertiumApplicator : public virtual GrammarApplicator`.
pub struct ApertiumApplicator<B = Box<GrammarApplicator>> {
    /// The base engine (C++ inheritance → composition).
    pub base: B,
    pub wordform_case: bool,
    pub print_word_forms: bool,
    pub print_only_first: bool,
    pub delimit_lexical_units: bool,
    pub surface_readings: bool,
}

// ---------------------------------------------------------------------------
// Port-infra helpers (un-annotated).
// ---------------------------------------------------------------------------

/// Resolve a tag hash to its `TagId` via `grammar->single_tags[hash]`. Mirrors
/// the engine's private `tag_by_hash` (not visible from this sibling module):
/// a miss returns `TagId(0)` (benign — call sites pass always-present hashes).
fn tag_by_hash(grammar: &crate::grammar::Grammar, hash: TagHash) -> TagId {
    let it = grammar.single_tags.find(hash.get());
    if it != grammar.single_tags.end() {
        it.get().1
    } else {
        TagId(0)
    }
}

/// C++ `reverse(Reading* head)` (the `inlines.hpp` `->next`-chain reversal),
/// specialised to the arena `ReadingId` chain: reverses the singly-linked
/// sub-reading `next` chain in place and returns the new head.
use crate::reading::reverse as reverse_reading;

/// C++ `substr(tag->tag, 2)` — drop the first 2 chars of the tag text.
fn substr_from(s: &str, start: usize) -> String {
    s.chars().skip(start).collect()
}

impl ApertiumApplicator<Box<GrammarApplicator>> {
    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.apertium-applicator-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.apertium-applicator-fn]
    /// C++ `ApertiumApplicator::ApertiumApplicator(std::ostream& ux_err)` — forwards
    /// `ux_err` to the base `GrammarApplicator(ux_err)` ctor (body empty); all
    /// Apertium flags keep their in-class defaults.
    pub fn new(base: GrammarApplicator) -> Self {
        Self::with_base(Box::new(base))
    }
}

impl<'a> ApertiumApplicator<&'a mut GrammarApplicator> {
    /// Borrow the shared virtual-base analogue used by [`FormatConverter`](crate::format_converter::FormatConverter).
    pub fn borrowing(base: &'a mut GrammarApplicator) -> Self {
        Self::with_base(base)
    }
}

impl<B> ApertiumApplicator<B>
where
    B: DerefMut<Target = GrammarApplicator>,
{
    fn with_base(base: B) -> Self {
        ApertiumApplicator {
            base,
            wordform_case: false,
            print_word_forms: true,
            print_only_first: false,
            delimit_lexical_units: true,
            surface_readings: false,
        }
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.merge-mappings-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.merge-mappings-fn]
    /// C++ `void ApertiumApplicator::mergeMappings(Cohort& cohort)`. Collapses
    /// byte-for-byte-identical readings (incl. mapping tags), keeping the FIRST of
    /// each hash group and `free_reading`-ing the rest, then re-sorting by
    /// `cmp_number`. Iteration over the group map is by ascending key (BTreeMap).
    pub fn merge_mappings(&mut self, cohort: CohortId) {
        use std::collections::BTreeMap;
        let trace = self.base.trace;
        let store = &mut self.base.store;

        let readings: ReadingList = store.cohorts.get(cohort.0).readings.clone();
        let mut mlist: BTreeMap<u32, ReadingList> = BTreeMap::new();
        for &r in &readings {
            let mut hp = store.readings.get(r.0).hash;
            if trace {
                let hb = store.readings.get(r.0).hit_by.clone();
                for iter_hb in hb {
                    hp = hash_value(iter_hb, hp);
                }
            }
            let mut sub = store.readings.get(r.0).next;
            while let Some(s) = sub {
                hp = hash_value(store.readings.get(s.0).hash, hp);
                if trace {
                    let hb = store.readings.get(s.0).hit_by.clone();
                    for iter_hb in hb {
                        hp = hash_value(iter_hb, hp);
                    }
                }
                sub = store.readings.get(s.0).next;
            }
            mlist.entry(hp).or_default().push(r);
        }

        if mlist.len() == readings.len() {
            return;
        }

        store.cohorts.get_mut(cohort.0).readings.clear();
        let mut order: Vec<ReadingId> = Vec::new();

        for (_k, clist) in mlist.into_iter() {
            // Keep the first reading of the group, free the rest.
            order.push(clist[0]);
            for &cit in &clist[1..] {
                let opt = Some(cit);
                free_reading(store, opt);
            }
        }

        order.sort_by(|&a, &b| {
            let ra = store.readings.get(a.0);
            let rb = store.readings.get(b.0);
            if Reading::cmp_number(ra, rb) {
                std::cmp::Ordering::Less
            } else if Reading::cmp_number(rb, ra) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let c = store.cohorts.get_mut(cohort.0);
        for (i, r) in order.into_iter().enumerate() {
            c.readings.insert(i, r);
        }
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.parse-stream-var-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.parse-stream-var-fn]
    /// C++ `void ApertiumApplicator::parseStreamVar(const SingleWindow* cSWindow,
    /// UString& cleaned, uint32FlatHashMap& variables_set, uint32FlatHashSet&
    /// variables_rem, uint32SortedVector& variables_output)`.
    ///
    /// `cleaned` is a `Vec<char>` (the C++ mutates it in place with NUL
    /// terminators; here the `u_strchr`/prefix walks operate on `usize` indices
    /// over that buffer and slices are re-interned with `add_tag`). BUG-FOR-BUG:
    /// the live member `variables` map is only updated in case (a) (single bare
    /// identifier) and only when `c_swindow` is null.
    pub fn parse_stream_var(
        &mut self,
        c_swindow: Option<SwId>,
        cleaned: &[char],
        variables_set: &mut crate::flat_unordered_map::Uint32FlatHashMap,
        variables_rem: &mut crate::flat_unordered_set::Uint32FlatHashSet,
        variables_output: &mut crate::sorted_vector::uint32SortedVector,
    ) {
        let tag_any = self.base.grammar.tag_any;

        // Helper: intern a slice [start, end) of `cleaned` as a tag, returning
        // its hash. Empty slice yields `None` (caller checks `s[0]`).
        let slice_str =
            |start: usize, end: usize| -> String { cleaned[start..end].iter().collect() };

        let setvar: Vec<char> = STR_CMD_SETVAR.chars().collect();
        let remvar: Vec<char> = STR_CMD_REMVAR.chars().collect();

        // u_strncmp(&cleaned[0], STR_CMD_SETVAR, size) == 0
        if cleaned.len() >= setvar.len() && cleaned[..setvar.len()] == setvar[..] {
            let base = setvar.len();
            let len = cleaned.len();
            // s points just past the prefix.
            let s0 = base;
            // c = u_strchr(s, ','), d = u_strchr(s, '=')
            let find_from = |from: usize, ch: char| -> Option<usize> {
                (from..len).find(|&i| cleaned[i] == ch)
            };
            let c = find_from(s0, ',');
            let d = find_from(s0, '=');

            if c.is_none() && d.is_none() {
                // Case (a): single bare identifier.
                let ident = slice_str(s0, len);
                let tag = self.base.add_tag(&ident, crate::tag::TagType::empty());
                let hash = self.base.grammar.single_tags_list.get(tag.0).hash.get();
                variables_set.insert((hash, tag_any));
                variables_rem.erase(hash);
                variables_output.insert(hash);
                if c_swindow.is_none() {
                    self.base.variables.insert((hash, tag_any));
                }
            } else {
                // Case (b): comma/`=` list. Walk `s` re-computing `c`/`d`.
                let mut s = Some(s0);
                let mut c = c;
                let mut d = d;
                let mut a: u32;
                let mut b: u32;
                while c.is_some() || d.is_some() {
                    if let Some(dd) = d {
                        if d.is_some() && (c.is_none() || dd < c.unwrap()) {
                            // `=` before the next `,`. identifier before `=`.
                            let ss = s.unwrap();
                            if ss >= dd {
                                // empty identifier before `=`
                                tracing::warn!(
                                    "Warning: SETVAR on line {} had no identifier before the =! Defaulting to identifier *.",
                                    self.base.numLines
                                );
                                a = tag_any;
                            } else {
                                let ident = slice_str(ss, dd);
                                let t = self.base.add_tag(&ident, crate::tag::TagType::empty());
                                a = self.base.grammar.single_tags_list.get(t.0).hash.get();
                            }
                            // if (c) { *c = 0; s = c + 1; }
                            let mut new_s = s;
                            if let Some(cc) = c {
                                new_s = Some(cc + 1);
                            }
                            // value after `=`: d[1] .. (c or len)
                            let val_end = c.unwrap_or(len);
                            if dd + 1 >= val_end {
                                tracing::warn!(
                                    "Warning: SETVAR on line {} had no value after the =! Defaulting to value *.",
                                    self.base.numLines
                                );
                                b = tag_any;
                            } else {
                                let val = slice_str(dd + 1, val_end);
                                let t = self.base.add_tag(&val, crate::tag::TagType::empty());
                                b = self.base.grammar.single_tags_list.get(t.0).hash.get();
                            }
                            if c.is_none() {
                                d = None;
                                new_s = None;
                            }
                            s = new_s;
                            variables_set.insert((a, b));
                            variables_rem.erase(a);
                            variables_output.insert(a);
                        } else if let Some(cc) = c {
                            // comma-separated bare identifier.
                            let ss = s.unwrap();
                            if ss >= cc {
                                tracing::warn!(
                                    "Warning: SETVAR on line {} had no identifier after the ,! Defaulting to identifier *.",
                                    self.base.numLines
                                );
                                a = tag_any;
                            } else {
                                let ident = slice_str(ss, cc);
                                let t = self.base.add_tag(&ident, crate::tag::TagType::empty());
                                a = self.base.grammar.single_tags_list.get(t.0).hash.get();
                            }
                            s = Some(cc + 1);
                            variables_set.insert((a, tag_any));
                            variables_rem.erase(a);
                            variables_output.insert(a);
                        }
                    } else if let Some(cc) = c {
                        // d is None but c exists — comma-separated bare identifier.
                        let ss = s.unwrap();
                        if ss >= cc {
                            tracing::warn!(
                                "Warning: SETVAR on line {} had no identifier after the ,! Defaulting to identifier *.",
                                self.base.numLines
                            );
                            a = tag_any;
                        } else {
                            let ident = slice_str(ss, cc);
                            let t = self.base.add_tag(&ident, crate::tag::TagType::empty());
                            a = self.base.grammar.single_tags_list.get(t.0).hash.get();
                        }
                        s = Some(cc + 1);
                        variables_set.insert((a, tag_any));
                        variables_rem.erase(a);
                        variables_output.insert(a);
                    }

                    if let Some(ss) = s {
                        c = find_from(ss, ',');
                        d = find_from(ss, '=');
                        if c.is_none() && d.is_none() {
                            // final bare identifier.
                            let ident = slice_str(ss, len);
                            let t = self.base.add_tag(&ident, crate::tag::TagType::empty());
                            a = self.base.grammar.single_tags_list.get(t.0).hash.get();
                            variables_set.insert((a, tag_any));
                            variables_rem.erase(a);
                            variables_output.insert(a);
                            s = None;
                        }
                    }
                }
            }
        } else if cleaned.len() >= remvar.len() && cleaned[..remvar.len()] == remvar[..] {
            let base = remvar.len();
            let len = cleaned.len();
            let find_from = |from: usize, ch: char| -> Option<usize> {
                (from..len).find(|&i| cleaned[i] == ch)
            };
            let mut s = base;
            let mut c = find_from(s, ',');
            while let Some(cc) = c {
                // while (c && *c) — *c is ',' which is non-NUL, so continue.
                // if (s[0]) — s must not be at the terminator (i.e. s < len and
                // slice non-empty).
                if s < cc {
                    let ident = slice_str(s, cc);
                    let t = self.base.add_tag(&ident, crate::tag::TagType::empty());
                    let a = self.base.grammar.single_tags_list.get(t.0).hash.get();
                    variables_set.erase(a);
                    variables_rem.insert(a);
                    variables_output.insert(a);
                }
                s = cc + 1;
                c = find_from(s, ',');
            }
            // if (s && s[0]) — trailing identifier.
            if s < len {
                let ident = slice_str(s, len);
                let t = self.base.add_tag(&ident, crate::tag::TagType::empty());
                let a = self.base.grammar.single_tags_list.get(t.0).hash.get();
                variables_set.erase(a);
                variables_rem.insert(a);
                variables_output.insert(a);
            }
        }
        // Neither prefix matched → do nothing.
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.process-reading-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.process-reading-fn]
    /// C++ `void ApertiumApplicator::processReading(Reading* cReading, UChar* p,
    /// Tag* wform)`. Parses one Apertium analysis string (already extracted
    /// between `/` and the next `/`/`$`) into `c_reading`, incl. sub-readings.
    /// `p` is the `Vec<char>` reading buffer (mutable: `esc_lt` → `<` rewrites).
    pub fn process_reading(&mut self, c_reading: ReadingId, mut p: Vec<char>, wform: TagId) {
        self.base.add_tag_to_reading(c_reading, wform);

        let mut taglist: TagList = Vec::new();
        let mut bf: UString = String::from("\"");
        let mut tags: TagList = Vec::new();
        let mut prefix_tags: TagList = Vec::new();

        let len = p.len();
        let mut i = 0usize; // cursor == C++ `p`
        while i < len {
            let mut n = i;
            // advance n while *n not in { # + < }, rewriting esc_lt -> '<'
            while n < len && p[n] != '#' && p[n] != '+' && p[n] != '<' {
                if p[n] == ESC_LT {
                    p[n] = '<';
                }
                n += 1;
            }
            // baseform text [i, n)
            if n != i {
                let seg: String = p[i..n].iter().collect();
                bf.push_str(&seg);
                i = n;
            }
            // tag start `<`
            if n < len && p[n] == '<' {
                i = n + 1;
                // advance n to closing '>'
                while n < len && p[n] != '>' {
                    n += 1;
                }
                if n >= len || p[n] != '>' {
                    tracing::warn!(
                        "Warning: Did not find matching > to close the tag on line {}.",
                        self.base.numLines
                    );
                    continue;
                }
                let tagtext: String = p[i..n].iter().collect();
                let t = self.base.add_tag(&tagtext, crate::tag::TagType::empty());
                // bf.size() == 1 means only the opening quote so far.
                if bf.chars().count() == 1 {
                    prefix_tags.push(t);
                } else {
                    tags.push(t);
                }
                i = n + 1;
            }
            // multiword marker `#`
            if n < len && p[n] == '#' {
                i = n;
                while n < len && p[n] != '<' && p[n] != '+' {
                    n += 1;
                }
                let seg: String = p[i..n].iter().collect();
                bf.push_str(&seg);
                i = n;
            }
            // sub-reading delimiter `+`
            if n < len && p[n] == '+' {
                bf.push('"');
                let base_tag = self.base.add_tag(&bf, crate::tag::TagType::empty());
                taglist.push(base_tag);
                taglist.extend(tags.iter().copied());
                taglist.extend(prefix_tags.iter().copied());
                bf.truncate(1); // resize(1) — keep the leading quote
                tags.clear();
                prefix_tags.clear();
                i = n + 1;
            }
        }

        // Final segment (bf always holds the quote → non-empty).
        bf.push('"');
        let base_tag = self.base.add_tag(&bf, crate::tag::TagType::empty());
        taglist.push(base_tag);
        taglist.extend(tags.iter().copied());
        taglist.extend(prefix_tags.iter().copied());

        // Assign tags to reading(s), scanning from the BACK for baseforms.
        while !taglist.is_empty() {
            let mut reading = c_reading;
            // reverse_foreach over taglist
            let mut ri = taglist.len();
            while ri > 0 {
                ri -= 1;
                let riter = taglist[ri];
                if self
                    .base
                    .grammar
                    .single_tags_list
                    .get(riter.0)
                    .r#type
                    .intersects(T_BASEFORM)
                {
                    // sub-reading if the current reading already has a baseform.
                    if self.base.store.readings.get(reading.0).baseform.is_some() {
                        let parent = self.base.store.readings.get(reading.0).parent;
                        let nr = Reading::allocate_reading(&mut self.base.store, parent);
                        self.base.store.readings.get_mut(reading.0).next = Some(nr);
                        reading = nr;
                        self.base.add_tag_to_reading(reading, wform);
                    }
                    // Add tags from ri forward to end.
                    let mut mappings: TagList = Vec::new();
                    let mprefix = self.base.grammar.mapping_prefix;
                    // faithful port: C++ index walk over [ri, taglist.size()), not 0..len
                    #[allow(clippy::needless_range_loop)]
                    for k in ri..taglist.len() {
                        let iter = taglist[k];
                        let t = self.base.grammar.single_tags_list.get(iter.0);
                        let is_mapping =
                            t.r#type.intersects(T_MAPPING) || t.tag.starts_with(mprefix);
                        if is_mapping {
                            mappings.push(iter);
                        } else {
                            self.base.add_tag_to_reading(reading, iter);
                        }
                    }
                    if !mappings.is_empty() {
                        let parent = self.base.store.readings.get(reading.0).parent.unwrap();
                        self.base
                            .split_mappings(&mut mappings, parent, reading, true);
                    }
                    // Pop trailing non-baseform tags, then the baseform.
                    while let Some(&last) = taglist.last() {
                        if self
                            .base
                            .grammar
                            .single_tags_list
                            .get(last.0)
                            .r#type
                            .intersects(T_BASEFORM)
                        {
                            break;
                        }
                        taglist.pop();
                    }
                    taglist.pop();
                    // C++ reverse_foreach keeps iterating: `reading` stays on the
                    // just-created sub-reading so earlier groups chain off it.
                    // After the pops, taglist.len() == ri, so the `ri -= 1` at the
                    // top of the loop continues the reverse scan correctly.
                }
            }
        }
    }

    /// C++ overload `processReading(Reading*, UString&, Tag*)` → forwards.
    pub fn process_reading_str(
        &mut self,
        c_reading: ReadingId,
        reading_string: &str,
        wform: TagId,
    ) {
        let p: Vec<char> = reading_string.chars().collect();
        self.process_reading(c_reading, p, wform);
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.test-pr-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.test-pr-fn]
    /// C++ `void ApertiumApplicator::testPR(std::ostream& output)`. Round-trips six
    /// hard-coded analysis strings through `processReading`/`printReading`.
    pub fn test_pr<W: Write>(&mut self, output: &mut W) {
        let texts = [
            "venir<vblex><imp><p2><sg>",
            "venir<vblex><inf>+lo<prn><enc><p3><nt><sg>",
            "be<vblex><inf># happy",
            "sellout<vblex><imp><p2><sg># ouzh+indirect<prn><obj><p3><m><sg>",
            "be# happy<vblex><inf>",
            "aux3<tag>+aux2<tag>+aux1<tag>+main<tag>",
        ];
        for text in texts {
            let reading = alloc_reading(&mut self.base.store, None);
            let wform = tag_by_hash(&self.base.grammar, TagHash(self.base.grammar.tag_any));
            self.process_reading_str(reading, text, wform);
            let mut reading = reading;
            if self.base.grammar.sub_readings_ltr
                && self.base.store.readings.get(reading.0).next.is_some()
            {
                reading = reverse_reading(&mut self.base.store, reading);
            }
            self.print_reading_2(reading, output);
            let _ = writeln!(output);
            let opt = Some(reading);
            free_reading(&mut self.base.store, opt);
        }
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.print-reading-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-reading-fn]
    /// C++ `void ApertiumApplicator::printReading(const Reading* reading,
    /// std::ostream& output, ApertiumCasing casing, int32_t firstlower)`. The
    /// 4-arg core: prints one reading (and its `next` sub-reading chain).
    pub fn print_reading<W: Write>(
        &self,
        reading: ReadingId,
        output: &mut W,
        casing: ApertiumCasing,
        firstlower: i32,
    ) {
        let store = &self.base.store;
        let grammar = &self.base.grammar;
        let r = store.readings.get(reading.0);

        if let Some(next) = r.next {
            self.print_reading(next, output, casing, firstlower);
            u_fputc('+', output);
        }

        let baseform = r.baseform.unwrap_or(TagHash(0));
        let parent = r.parent;

        if baseform != TagHash(0) {
            // Lop off the surrounding '"' quotes.
            let tid = tag_by_hash(grammar, baseform);
            let tagtext = &grammar.single_tags_list.get(tid.0).tag;
            let inner: Vec<char> = tagtext.chars().collect();
            // data()+1, length size-2
            let mut bf: Vec<char> = if inner.len() >= 2 {
                inner[1..inner.len() - 1].to_vec()
            } else {
                Vec::new()
            };

            let parent_type = parent
                .map(|c| store.cohorts.get(c.0).r#type)
                .unwrap_or_default();

            if self.wordform_case {
                if casing == ApertiumCasing::Upper {
                    bf = bf.iter().flat_map(|c| c.to_uppercase()).collect();
                } else if casing == ApertiumCasing::Title && r.next.is_none() {
                    let fl = firstlower as usize;
                    if fl < bf.len() {
                        let up: Vec<char> = bf[fl].to_uppercase().collect();
                        if let Some(&first_up) = up.first() {
                            bf[fl] = first_up;
                        }
                    }
                }
            }

            let mut bf_escaped: Vec<char> = Vec::new();
            for &ch in &bf {
                if matches!(
                    ch,
                    '^' | '\\' | '/' | '$' | '[' | ']' | '{' | '}' | '<' | '>'
                ) {
                    bf_escaped.push('\\');
                }
                if (parent_type.intersects(CT_AP_UNKNOWN)) && ch == '@' {
                    bf_escaped.push('\\');
                }
                bf_escaped.push(ch);
            }
            if self.surface_readings && !bf.is_empty() && bf_escaped.first() == Some(&'@') {
                bf_escaped[0] = '#';
            }
            let bf_str: String = bf_escaped.iter().collect();
            let _ = write!(output, "{bf_str}");
        }

        if self.surface_readings && !self.base.trace {
            return;
        }

        // Reorder: MAPPING tags before the multiword join.
        let mut tags_list: Vec<u32> = Vec::new();
        let mut multitags_list: Vec<u32> = Vec::new();
        let mut multi = false;
        for &tter in r.tags_list.iter() {
            let tag = grammar
                .single_tags_list
                .get(tag_by_hash(grammar, TagHash(tter)).0);
            if tag.tag.starts_with('+') {
                multi = true;
            } else if tag.r#type.intersects(T_MAPPING) {
                multi = false;
            }
            if tag.r#type.intersects(T_DEPENDENCY) && self.base.has_dep && !self.base.dep_original {
                continue;
            }
            if multi {
                multitags_list.push(tter);
            } else {
                tags_list.push(tter);
            }
        }
        tags_list.extend(multitags_list);

        let mut used_tags = crate::sorted_vector::uint32SortedVector::new();
        let escape = if self.surface_readings { "\\" } else { "" };
        for tter in tags_list {
            if self.base.unique_tags {
                if used_tags.find(tter) != used_tags.end() {
                    continue;
                }
                used_tags.insert(tter);
            }
            if tter == self.base.endtag.get() || tter == self.base.begintag.get() {
                continue;
            }
            let tag = grammar
                .single_tags_list
                .get(tag_by_hash(grammar, TagHash(tter)).0);
            if !tag.r#type.intersects(T_BASEFORM) && !tag.r#type.intersects(T_WORDFORM) {
                let first = tag.tag.chars().next();
                if first == Some('+') {
                    let _ = write!(output, "{}", tag.tag);
                } else if first == Some('&') {
                    let inner = substr_from(&tag.tag, 2);
                    let _ = write!(output, "{escape}<{inner}{escape}>");
                } else {
                    let _ = write!(output, "{escape}<{}{escape}>", tag.tag);
                }
            }
        }

        // Dependency output.
        if self.base.has_dep
            && r.next.is_none()
            && let Some(pcid) = parent
        {
            let parent_removed = store.cohorts.get(pcid.0).r#type.intersects(CT_REMOVED);
            if !parent_removed {
                let (local_number, dep_self, dep_parent, sw_parent, global_number) = {
                    let pc = store.cohorts.get(pcid.0);
                    (
                        pc.local_number,
                        pc.dep_self,
                        pc.dep_parent,
                        pc.parent,
                        pc.global_number,
                    )
                };
                let _ = dep_self; // C++ sets it below (read-only path here).
                // Determine parent cohort `pr`.
                let mut pr = pcid;
                if let Some(dp) = dep_parent {
                    if dp == crate::types::GlobalNumber(0) {
                        if let Some(sw) = sw_parent
                            && let Some(&first) = store.single_windows.get(sw.0).cohorts.first()
                        {
                            pr = first;
                        }
                    } else if let Some(&cid) = self.base.window.cohort_map.get(&dp) {
                        pr = cid;
                    }
                }
                let pr_local = store.cohorts.get(pr.0).local_number;
                let _ = global_number;
                let _ = write!(output, "<#{}\u{2192}{}>", local_number, pr_local);
            }
        }

        if self.base.trace {
            for &iter_hb in r.hit_by.iter() {
                u_fputc('<', output);
                self.base.print_trace(output, iter_hb);
                u_fputc('>', output);
            }
        }
    }

    /// C++ 2-arg overload `printReading(const Reading*, std::ostream&)` — derives
    /// `casing`/`firstlower` and calls the 4-arg form.
    pub fn print_reading_2<W: Write>(&self, reading: ReadingId, output: &mut W) {
        let mut casing = ApertiumCasing::Nochange;
        let store = &self.base.store;
        let grammar = &self.base.grammar;

        if self.wordform_case {
            // Walk to the last sub-reading that has a baseform.
            let mut last = reading;
            loop {
                let r = store.readings.get(last.0);
                match r.next {
                    Some(next) if store.readings.get(next.0).baseform.is_some() => last = next,
                    _ => break,
                }
            }
            if store.readings.get(last.0).baseform.is_some()
                && let Some(pcid) = store.readings.get(reading.0).parent
                && let Some(wf) = store.cohorts.get(pcid.0).wordform
            {
                let wftag: Vec<char> = grammar.single_tags_list.get(wf.0).tag.chars().collect();
                // wf_length = size - 4; walk indices [0, wf_length),
                // reading char at index i+2 (skip the leading `"<`).
                let wf_length = wftag.len().saturating_sub(4);
                let mut uppercaseseen = 0i32;
                let mut alphabeticsseen = 0i32;
                for i in 0..wf_length {
                    let c = wftag[i + 2];
                    if c.is_alphabetic() {
                        alphabeticsseen += 1;
                        if c.is_uppercase() {
                            uppercaseseen += 1;
                        }
                    }
                }
                if uppercaseseen == alphabeticsseen && uppercaseseen >= 2 {
                    casing = ApertiumCasing::Upper;
                } else if wftag.len() > 2 && wftag[2].is_uppercase() && uppercaseseen == 1 {
                    casing = ApertiumCasing::Title;
                }
            }
        }
        self.print_reading(reading, output, casing, 0);
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.print-cohort-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-cohort-fn]
    /// C++ `void ApertiumApplicator::printCohort(Cohort* cohort, std::ostream&
    /// output, bool profiling)`.
    pub fn print_cohort<W: Write>(&mut self, cohort: CohortId, output: &mut W, profiling: bool) {
        let (local_number, ctype) = {
            let c = self.base.store.cohorts.get(cohort.0);
            (c.local_number, c.r#type)
        };
        if local_number == 0 || (ctype.intersects(CT_REMOVED)) {
            let text = self.base.store.cohorts.get(cohort.0).text.clone();
            if !text.is_empty() {
                let _ = write!(output, "{text}");
            }
            return;
        }

        if !profiling {
            unignore_all(&mut self.base.store, cohort);
            if !self.base.split_mappings {
                self.merge_mappings(cohort);
            }
        }

        let wblank = self.base.store.cohorts.get(cohort.0).wblank.clone();
        if !wblank.is_empty() {
            let _ = write!(output, "{wblank}");
        }

        if self.delimit_lexical_units {
            let _ = write!(output, "^");
        }

        if self.print_word_forms {
            let (wf_tid, wread) = {
                let c = self.base.store.cohorts.get(cohort.0);
                (c.wordform, c.wread)
            };
            let wf_tid = wf_tid.expect("printCohort: cohort has no wordform");
            let wf_chars: Vec<char> = self
                .base
                .grammar
                .single_tags_list
                .get(wf_tid.0)
                .tag
                .chars()
                .collect();
            // data()+2, length size-4 (drop the wrapping `"<` and `>"`).
            let wf: Vec<char> = if wf_chars.len() >= 4 {
                wf_chars[2..wf_chars.len() - 2].to_vec()
            } else {
                Vec::new()
            };
            let mut wf_escaped: Vec<char> = Vec::new();
            for &ch in &wf {
                if matches!(
                    ch,
                    '^' | '\\' | '/' | '$' | '[' | ']' | '{' | '}' | '<' | '>'
                ) {
                    wf_escaped.push('\\');
                }
                if (ctype.intersects(CT_AP_UNKNOWN)) && ch == '@' {
                    wf_escaped.push('\\');
                }
                wf_escaped.push(ch);
            }
            let wf_str: String = wf_escaped.iter().collect();
            let _ = write!(output, "{wf_str}");

            // Static reading tags.
            if let Some(wread) = wread {
                let wf_hash = self.base.grammar.single_tags_list.get(wf_tid.0).hash;
                let tags_list = self.base.store.readings.get(wread.0).tags_list.clone();
                for tter in tags_list {
                    let tter = TagHash(tter);
                    if tter == wf_hash {
                        continue;
                    }
                    let tid = tag_by_hash(&self.base.grammar, tter);
                    let tagtext = &self.base.grammar.single_tags_list.get(tid.0).tag;
                    let _ = write!(output, "<{tagtext}>");
                }
            }
        }

        let mut need_slash = self.print_word_forms;

        // Sort readings by cmp_number.
        self.sort_readings_field(cohort, |c| &mut c.readings);
        let readings = self.base.store.cohorts.get(cohort.0).readings.clone();
        for reading in readings {
            let mut reading = reading;
            if self.base.store.readings.get(reading.0).noprint {
                continue;
            }
            if need_slash {
                let _ = write!(output, "/");
            }
            need_slash = true;
            if self.base.grammar.sub_readings_ltr
                && self.base.store.readings.get(reading.0).next.is_some()
            {
                reading = reverse_reading(&mut self.base.store, reading);
            }
            self.print_reading_2(reading, output);
            if self.print_only_first {
                break;
            }
        }

        if self.base.trace {
            self.sort_readings_field(cohort, |c| &mut c.delayed);
            let delayed = self.base.store.cohorts.get(cohort.0).delayed.clone();
            for reading in delayed {
                let mut reading = reading;
                if self.base.store.readings.get(reading.0).noprint {
                    continue;
                }
                if need_slash {
                    let _ = write!(output, "/{NOT_SIGN}");
                }
                need_slash = true;
                if self.base.grammar.sub_readings_ltr
                    && self.base.store.readings.get(reading.0).next.is_some()
                {
                    reading = reverse_reading(&mut self.base.store, reading);
                }
                self.print_reading_2(reading, output);
            }
            self.sort_readings_field(cohort, |c| &mut c.deleted);
            let deleted = self.base.store.cohorts.get(cohort.0).deleted.clone();
            for reading in deleted {
                let mut reading = reading;
                if self.base.store.readings.get(reading.0).noprint {
                    continue;
                }
                if need_slash {
                    let _ = write!(output, "/{NOT_SIGN}");
                }
                need_slash = true;
                if self.base.grammar.sub_readings_ltr
                    && self.base.store.readings.get(reading.0).next.is_some()
                {
                    reading = reverse_reading(&mut self.base.store, reading);
                }
                self.print_reading_2(reading, output);
            }
        }

        if self.delimit_lexical_units {
            let _ = write!(output, "$");
        }

        let text = self.base.store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() {
            let _ = write!(output, "{text}");
        }
    }

    /// Sort a cohort's chosen reading list (`readings`/`delayed`/`deleted`) by
    /// `Reading::cmp_number` — factors the repeated `std::sort(..., cmp_number)`.
    fn sort_readings_field(
        &mut self,
        cohort: CohortId,
        pick: impl Fn(&mut crate::cohort::Cohort) -> &mut ReadingList,
    ) {
        let mut list = pick(self.base.store.cohorts.get_mut(cohort.0)).clone();
        let store = &self.base.store;
        list.sort_by(|&a, &b| {
            let ra = store.readings.get(a.0);
            let rb = store.readings.get(b.0);
            if Reading::cmp_number(ra, rb) {
                std::cmp::Ordering::Less
            } else if Reading::cmp_number(rb, ra) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        *pick(self.base.store.cohorts.get_mut(cohort.0)) = list;
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.print-single-window-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.print-single-window-fn]
    /// C++ `void ApertiumApplicator::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling)`.
    pub fn print_single_window<W: Write>(&mut self, window: SwId, output: &mut W, profiling: bool) {
        let text = self.base.store.single_windows.get(window.0).text.clone();
        if !text.is_empty() {
            let _ = write!(output, "{text}");
        }

        let all_cohorts = self
            .base
            .store
            .single_windows
            .get(window.0)
            .all_cohorts
            .clone();
        for cohort in all_cohorts {
            self.print_cohort(cohort, output, profiling);
            u_fflush(output);
        }

        let text_post = self
            .base
            .store
            .single_windows
            .get(window.0)
            .text_post
            .clone();
        if !text_post.is_empty() {
            let _ = write!(output, "{text_post}");
            u_fflush(output);
        }

        if self.base.store.single_windows.get(window.0).flush_after {
            u_fputc('\0', output);
        }
    }

    // [spec:cg3:def:apertium-applicator.cg3.apertium-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:apertium-applicator.cg3.apertium-applicator.run-grammar-on-text-fn]
    /// C++ `void ApertiumApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`. The Apertium stream driver (char-by-char state
    /// machine). Validation `CG3Quit(1)` diagnostics + no-delimiter warnings are
    /// emitted faithfully (to the error sink); the grammar is assumed present.
    // Faithful-port mirrors: assignments kept 1:1 with the C++ text even where
    // the ported reads were elided (see the deferred-I/O / driver notes).
    pub fn run_grammar_on_text<R, W>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        let mut fmt = ApertiumFormat::from_app(self);
        crate::error::catch_fatal(|| self.run_grammar_on_text_impl(&mut fmt, input, output))
    }

    /// Run the Apertium parser while routing output through a most-derived
    /// stream format, matching C++ virtual dispatch in `FormatConverter`.
    pub fn run_grammar_on_text_with<F, R, W>(
        &mut self,
        fmt: &mut F,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        crate::error::catch_fatal(|| self.run_grammar_on_text_impl(fmt, input, output))
    }

    #[allow(unused_assignments, unused_variables)]
    fn run_grammar_on_text_impl<F, R, W>(&mut self, fmt: &mut F, input: &mut R, output: &mut W)
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        // ux_stdin/ux_stdout are Option<()> placeholders — assignment elided.

        // No-hard/soft-delimiter warnings.
        let no_hard = self.base.grammar.delimiters.is_none();
        let no_soft = self.base.grammar.soft_delimiters.is_none();
        if no_hard {
            if no_soft {
                tracing::warn!(
                    "Warning: No soft or hard delimiters defined in grammar. Hard limit of {} cohorts may break windows in unintended places.",
                    self.base.hard_limit
                );
            } else {
                tracing::warn!(
                    "Warning: No hard delimiters defined in grammar. Soft limit of {} cohorts may break windows in unintended places.",
                    self.base.soft_limit
                );
            }
        }

        let mut c: char = '\0';
        let mut in_blank = false;
        let mut in_wblank = false;
        let mut in_cohort = false;
        let mut blank: UString = String::new();
        let mut wblank: UString = String::new();
        let mut token: UString = String::new();

        self.base.index();

        let reset_after: u32 = (self.base.num_windows + 4) * 2 + 1;

        self.base.begintag = {
            let t = self
                .base
                .add_tag(STR_BEGINTAG, crate::tag::TagType::empty());
            self.base.grammar.single_tags_list.get(t.0).hash
        };
        self.base.endtag = {
            let t = self.base.add_tag(STR_ENDTAG, crate::tag::TagType::empty());
            self.base.grammar.single_tags_list.get(t.0).hash
        };

        let mut c_swindow: Option<SwId> = None;
        let mut c_cohort: Option<CohortId> = None;
        let mut l_swindow: Option<SwId> = None;
        let mut l_cohort: Option<CohortId> = None;

        self.base.window.window_span = self.base.num_windows;

        let mut variables_set = crate::flat_unordered_map::Uint32FlatHashMap::default();
        let mut variables_rem = crate::flat_unordered_set::Uint32FlatHashSet::default();
        let mut variables_output = crate::sorted_vector::uint32SortedVector::new();

        ux_strip_bom(input);

        // Main character loop: while ((c = u_fgetc(input)) != U_EOF).
        loop {
            c = u_fgetc(input);
            if c == U_EOF {
                break;
            }

            if c == '\n' {
                self.base.numLines = self.base.numLines.wrapping_add(1);
            }

            if c == '\\' {
                let n = u_fgetc(input);
                if !in_cohort {
                    blank.push(c);
                    blank.push(n);
                } else {
                    token.push(c);
                    token.push(n);
                }
                continue;
            }

            if c == '\0' {
                self.flush(
                    fmt,
                    true,
                    &mut in_blank,
                    &mut in_wblank,
                    &mut in_cohort,
                    &mut blank,
                    &mut token,
                    &mut l_swindow,
                    &mut l_cohort,
                    &mut c_swindow,
                    &mut c_cohort,
                    &mut variables_set,
                    &mut variables_rem,
                    &mut variables_output,
                    c,
                    output,
                );
                continue;
            }

            if !in_cohort && c == '[' {
                if in_blank {
                    in_wblank = true;
                }
                in_blank = true;
            } else if !in_blank && c == '^' {
                in_cohort = true;
            }

            if !in_cohort {
                blank.push(c);
            } else {
                token.push(c);
            }

            if in_wblank && c == ']' {
                in_wblank = false;
            } else if in_blank && c == ']' {
                in_blank = false;
                let bchars: Vec<char> = blank.chars().collect();
                if bchars.len() > 14
                    && bchars.get(1) == Some(&'<')
                    && bchars.get(bchars.len() - 2) == Some(&'>')
                {
                    // cleaned = blank.substr(1, size-3): drop leading '[' and
                    // trailing '>]' (no trailing '>').
                    let cleaned: Vec<char> = bchars[1..bchars.len() - 2].to_vec();
                    self.parse_stream_var(
                        c_swindow,
                        &cleaned,
                        &mut variables_set,
                        &mut variables_rem,
                        &mut variables_output,
                    );
                }
            } else if !in_blank && c == '$' {
                if !in_cohort {
                    tracing::error!(
                        "Error: $ found without prior ^ on line {}.",
                        self.base.numLines
                    );
                    // CG3Quit(1) — abort in C++; keep going in the port.
                    return;
                }
                in_cohort = false;

                // Word-bound-blank extraction.
                if !blank.is_empty() {
                    wblank.clear();
                    let bchars: Vec<char> = blank.chars().collect();
                    let b = find_sub(&bchars, &['[', '[']);
                    let e = b.and_then(|bi| find_sub_from(&bchars, &[']', ']'], bi));
                    if let (Some(bi), Some(ei)) = (b, e) {
                        // NOT the bare closing `[[/]]` blank.
                        if !(ei == bi + 3 && bchars.get(bi + 2) == Some(&'/')) {
                            wblank = bchars[bi..].iter().collect();
                            blank = bchars[..bi].iter().collect();
                        }
                    }
                }
                if !wblank.is_empty() {
                    let wchars: Vec<char> = wblank.chars().collect();
                    let n = wchars.len();
                    if wchars[n - 1] != ']' || (n < 2 || wchars[n - 2] != ']') {
                        tracing::error!(
                            "Error: Word-bound blank was not immediately prior to token on line {}",
                            self.base.numLines
                        );
                        return;
                    }
                }

                // Attach leftover blank to the nearest text sink.
                if let Some(cc) = c_cohort {
                    let t = &mut self.base.store.cohorts.get_mut(cc.0).text;
                    t.push_str(&blank);
                    blank.clear();
                } else if let Some(lc) = l_cohort {
                    self.base.store.cohorts.get_mut(lc.0).text.push_str(&blank);
                    blank.clear();
                } else if let Some(ls) = l_swindow {
                    self.base
                        .store
                        .single_windows
                        .get_mut(ls.0)
                        .text
                        .push_str(&blank);
                    blank.clear();
                }

                // Create a window if none.
                if c_swindow.is_none() {
                    self.ensure_endtag(l_swindow);
                    let sw = {
                        let base = &mut *self.base;
                        base.window.alloc_append_single_window(&mut base.store)
                    };
                    self.base.init_empty_single_window(sw);
                    // Move the variable collections into the window (C++
                    // `cSWindow->variables_set = variables_set; ...clear()`).
                    let set_pairs = collect_map(&variables_set);
                    let rem_items = collect_set(&variables_rem);
                    let out_items = variables_output.as_slice().to_vec();
                    {
                        let sww = self.base.store.single_windows.get_mut(sw.0);
                        sww.variables_set.insert_range(set_pairs);
                        sww.variables_rem.insert_range(rem_items);
                        sww.variables_output.insert_range(&out_items);
                    }
                    variables_set.clear(0);
                    variables_rem.clear(0);
                    variables_output.clear();
                    c_swindow = Some(sw);
                    l_swindow = Some(sw);
                    self.base.store.single_windows.get_mut(sw.0).text = blank.clone();
                    blank.clear();
                    self.base.numWindows = self.base.numWindows.wrapping_add(1);
                }
                let cs = c_swindow.unwrap();

                // Allocate the cohort.
                let cc = alloc_cohort(&mut self.base.store, Some(cs));
                l_cohort = Some(cc);
                c_cohort = Some(cc);
                let gn = self.base.window.next_cohort_number();
                self.base.store.cohorts.get_mut(cc.0).global_number = gn;
                self.base.numCohorts = self.base.numCohorts.wrapping_add(1);
                self.base.store.cohorts.get_mut(cc.0).text = blank.clone();
                blank.clear();
                self.base.store.cohorts.get_mut(cc.0).wblank = wblank.clone();
                wblank.clear();

                // Parse the wordform.
                let tchars: Vec<char> = token.chars().collect();
                let mut p = 1usize; // skip '^'
                let mut wf = String::from("\"<");
                while p < tchars.len() && tchars[p] != '/' && tchars[p] != '<' && tchars[p] != '$' {
                    if tchars[p] == '\\' {
                        p += 1;
                    }
                    if p < tchars.len() {
                        wf.push(tchars[p]);
                    }
                    p += 1;
                }
                wf.push_str(">\"");
                let wf_tid = self.base.add_tag(&wf, crate::tag::TagType::empty());
                self.base.store.cohorts.get_mut(cc.0).wordform = Some(wf_tid);

                // Static reading.
                if p < tchars.len() && tchars[p] == '<' {
                    p += 1;
                    let wread = alloc_reading(&mut self.base.store, Some(cc));
                    self.base.store.cohorts.get_mut(cc.0).wread = Some(wread);
                    let mut tagbuf = String::new();
                    while p < tchars.len() && tchars[p] != '/' && tchars[p] != '$' {
                        if tchars[p] == '\\' {
                            p += 1;
                            if p < tchars.len() {
                                tagbuf.push(tchars[p]);
                            }
                            p += 1;
                            continue;
                        }
                        if tchars[p] == '<' {
                            p += 1;
                            continue;
                        }
                        if tchars[p] == '>' {
                            let t = self.base.add_tag(&tagbuf, crate::tag::TagType::empty());
                            self.base.add_tag_to_reading(wread, t);
                            tagbuf.clear();
                            p += 1;
                            continue;
                        }
                        tagbuf.push(tchars[p]);
                        p += 1;
                    }
                }

                // Readings.
                if p < tchars.len() && tchars[p] == '/' {
                    p += 1;
                    let mut rbuf: Vec<char> = Vec::new();
                    while p < tchars.len() {
                        if tchars[p] == '\\' {
                            p += 1;
                            if p < tchars.len() {
                                if tchars[p] == '<' {
                                    rbuf.push(ESC_LT);
                                } else {
                                    rbuf.push(tchars[p]);
                                }
                            }
                            p += 1;
                            continue;
                        }
                        if tchars[p] == '/' || tchars[p] == '$' {
                            let c_reading = alloc_reading(&mut self.base.store, Some(cc));
                            let wf_tid2 = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                            self.process_reading(c_reading, rbuf.clone(), wf_tid2);
                            let mut c_reading = c_reading;
                            if self.base.grammar.sub_readings_ltr
                                && self.base.store.readings.get(c_reading.0).next.is_some()
                            {
                                c_reading = reverse_reading(&mut self.base.store, c_reading);
                            }
                            if self.base.store.readings.get(c_reading.0).deleted {
                                self.base
                                    .store
                                    .cohorts
                                    .get_mut(cc.0)
                                    .deleted
                                    .push(c_reading);
                            } else {
                                append_reading(&mut self.base.store, cc, c_reading);
                            }
                            self.base.numReadings = self.base.numReadings.wrapping_add(1);
                            if self.base.store.readings.get(c_reading.0).baseform.is_none() {
                                tracing::warn!(
                                    "Warning: Cohort {} on line {} had no valid baseform.",
                                    self.base.numCohorts,
                                    self.base.numLines
                                );
                            }
                            rbuf.clear();
                            p += 1;
                            continue;
                        }
                        rbuf.push(tchars[p]);
                        p += 1;
                    }
                }

                // Magic reading.
                if self.base.store.cohorts.get(cc.0).readings.is_empty() {
                    self.base.init_empty_cohort(cc);
                }
                {
                    let base = &mut *self.base;
                    insert_if_exists(
                        &mut base.store.cohorts.get_mut(cc.0).possible_sets,
                        base.grammar.sets_any.as_ref(),
                    );
                    append_cohort(&mut base.window, &mut base.store, cs, cc);
                }
                // if (cCohort->wordform->tag[2] == '@')
                {
                    let wf_tid = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                    let wftag: Vec<char> = self
                        .base
                        .grammar
                        .single_tags_list
                        .get(wf_tid.0)
                        .tag
                        .chars()
                        .collect();
                    if wftag.get(2) == Some(&'@') {
                        self.base.store.cohorts.get_mut(cc.0).r#type |= CT_AP_UNKNOWN;
                    }
                }

                // Delimiter handling.
                let mut did_delim = false;
                let cohorts_size = self.base.store.single_windows.get(cs.0).cohorts.len() as u32;
                if cohorts_size >= self.base.soft_limit
                    && self.base.grammar.soft_delimiters.is_some()
                {
                    let sd = self.base.grammar.sets_list
                        [self.base.grammar.soft_delimiters.unwrap().0]
                        .number
                        .get();
                    if self.base.does_set_match_cohort_normal(cc, sd, None) {
                        let readings = self.base.store.cohorts.get(cc.0).readings.clone();
                        for r in readings {
                            let et = tag_by_hash(&self.base.grammar, self.base.endtag);
                            self.base.add_tag_to_reading(r, et);
                        }
                        l_swindow = Some(cs);
                        c_swindow = None;
                        c_cohort = None;
                        did_delim = true;
                    }
                }
                if c_cohort.is_some() {
                    let cohorts_size =
                        self.base.store.single_windows.get(cs.0).cohorts.len() as u32;
                    let hard = cohorts_size >= self.base.hard_limit;
                    let delim_match = self.base.grammar.delimiters.is_some() && {
                        let d = self.base.grammar.sets_list
                            [self.base.grammar.delimiters.unwrap().0]
                            .number
                            .get();
                        self.base.does_set_match_cohort_normal(cc, d, None)
                    };
                    if hard || delim_match {
                        if !self.base.is_conv && cohorts_size >= self.base.hard_limit {
                            let wf_tid = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                            let wftag =
                                self.base.grammar.single_tags_list.get(wf_tid.0).tag.clone();
                            tracing::warn!(
                                "Warning: Hard limit of {} cohorts reached at cohort {} (#{}) on line {} - forcing break.",
                                self.base.hard_limit,
                                wftag,
                                self.base.numCohorts,
                                self.base.numLines
                            );
                        }
                        let readings = self.base.store.cohorts.get(cc.0).readings.clone();
                        for r in readings {
                            let et = tag_by_hash(&self.base.grammar, self.base.endtag);
                            self.base.add_tag_to_reading(r, et);
                        }
                        l_swindow = Some(cs);
                        c_swindow = None;
                        c_cohort = None;
                        did_delim = true;
                    }
                }

                if did_delim && self.base.window.next.len() as u32 > self.base.num_windows {
                    self.base.shuffle_windows_down();
                    self.base.run_grammar_on_window_with(fmt, output);
                    if reset_after != 0 && self.base.numWindows.is_multiple_of(reset_after) {
                        self.base.reset_indexes();
                    }
                }
                token.clear();
            }
        }

        self.flush(
            fmt,
            false,
            &mut in_blank,
            &mut in_wblank,
            &mut in_cohort,
            &mut blank,
            &mut token,
            &mut l_swindow,
            &mut l_cohort,
            &mut c_swindow,
            &mut c_cohort,
            &mut variables_set,
            &mut variables_rem,
            &mut variables_output,
            c,
            output,
        );
    }

    /// C++ `ensure_endtag` lambda: if `lSWindow` exists, has cohorts, and the last
    /// cohort's front reading lacks `endtag`, add `endtag` to every reading of that
    /// last cohort.
    fn ensure_endtag(&mut self, l_swindow: Option<SwId>) {
        let Some(ls) = l_swindow else { return };
        let cohorts = self.base.store.single_windows.get(ls.0).cohorts.clone();
        let Some(&back) = cohorts.last() else { return };
        let readings = self.base.store.cohorts.get(back.0).readings.clone();
        // readings.front()->tags.count(endtag) == 0
        let front_lacks = match readings.first() {
            Some(&front) => {
                self.base
                    .store
                    .readings
                    .get(front.0)
                    .tags
                    .find(self.base.endtag.get())
                    == self.base.store.readings.get(front.0).tags.end()
            }
            None => false,
        };
        if front_lacks {
            for r in readings {
                let et = tag_by_hash(&self.base.grammar, self.base.endtag);
                self.base.add_tag_to_reading(r, et);
            }
        }
    }

    /// C++ `flush(bool n)` lambda from `runGrammarOnText`. Drains all pending
    /// windows, prints them, and resets the driver state.
    #[allow(clippy::too_many_arguments)]
    fn flush<F, W>(
        &mut self,
        fmt: &mut F,
        n: bool,
        in_blank: &mut bool,
        in_wblank: &mut bool,
        in_cohort: &mut bool,
        blank: &mut UString,
        token: &mut UString,
        l_swindow: &mut Option<SwId>,
        l_cohort: &mut Option<CohortId>,
        c_swindow: &mut Option<SwId>,
        c_cohort: &mut Option<CohortId>,
        variables_set: &mut crate::flat_unordered_map::Uint32FlatHashMap,
        variables_rem: &mut crate::flat_unordered_set::Uint32FlatHashSet,
        variables_output: &mut crate::sorted_vector::uint32SortedVector,
        c: char,
        output: &mut W,
    ) where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        W: Write,
    {
        self.ensure_endtag(*l_swindow);

        let back_swindow = if n { self.base.window.back() } else { None };
        if let Some(bs) = back_swindow {
            self.base.store.single_windows.get_mut(bs.0).flush_after = true;
        }

        if !blank.is_empty() {
            if let Some(lc) = *l_cohort {
                self.base.store.cohorts.get_mut(lc.0).text.push_str(blank);
            } else if let Some(ls) = *l_swindow {
                let last = self
                    .base
                    .store
                    .single_windows
                    .get(ls.0)
                    .cohorts
                    .last()
                    .copied();
                if let Some(back) = last {
                    self.base.store.cohorts.get_mut(back.0).text.push_str(blank);
                } else {
                    self.base
                        .store
                        .single_windows
                        .get_mut(ls.0)
                        .text
                        .push_str(blank);
                }
            } else {
                fmt.print_plain_text_line(&mut self.base, blank, output);
            }
            blank.clear();
        }

        // Run the grammar & print results.
        while self.base.rotate_next().is_some() {
            self.base.run_grammar_on_window_with(fmt, output);
        }
        self.base.shuffle_windows_down();
        while !self.base.window.previous.is_empty() {
            let tmp = self.base.window.previous[0];
            fmt.print_single_window(&mut self.base, tmp, output, false);
            let opt = Some(tmp);
            {
                let base = &mut *self.base;
                crate::single_window::free_swindow(&mut base.window, &mut base.store, opt);
            }
            self.base.window.previous.remove(0);
        }

        if c != '\0' && c != '\u{FFFF}' {
            fmt.print_plain_text_line(&mut self.base, &c.to_string(), output);
        }

        if n && back_swindow.is_none() {
            u_fputc('\0', output);
        }
        u_fflush(output);

        *in_blank = false;
        *in_wblank = false;
        *in_cohort = false;
        *l_swindow = None;
        *l_cohort = None;
        *c_swindow = None;
        *c_cohort = None;
        token.clear();
        variables_rem.clear(0);
        variables_set.clear(0);
        variables_output.clear();
        self.base.variables.clear(0);
    }
}

/// Apertium's print-vtable state. This lets an Apertium input driver borrow the
/// shared engine while a different most-derived format handles output.
#[derive(Clone)]
pub struct ApertiumFormat {
    pub wordform_case: bool,
    pub print_word_forms: bool,
    pub print_only_first: bool,
    pub delimit_lexical_units: bool,
    pub surface_readings: bool,
}

impl Default for ApertiumFormat {
    fn default() -> Self {
        Self {
            wordform_case: false,
            print_word_forms: true,
            print_only_first: false,
            delimit_lexical_units: true,
            surface_readings: false,
        }
    }
}

impl ApertiumFormat {
    fn from_app<B>(app: &ApertiumApplicator<B>) -> Self
    where
        B: DerefMut<Target = GrammarApplicator>,
    {
        Self {
            wordform_case: app.wordform_case,
            print_word_forms: app.print_word_forms,
            print_only_first: app.print_only_first,
            delimit_lexical_units: app.delimit_lexical_units,
            surface_readings: app.surface_readings,
        }
    }

    fn with_app<T>(
        &mut self,
        app: &mut GrammarApplicator,
        f: impl FnOnce(&mut ApertiumApplicator<&mut GrammarApplicator>) -> T,
    ) -> T {
        let mut apertium = ApertiumApplicator::borrowing(app);
        apertium.wordform_case = self.wordform_case;
        apertium.print_word_forms = self.print_word_forms;
        apertium.print_only_first = self.print_only_first;
        apertium.delimit_lexical_units = self.delimit_lexical_units;
        apertium.surface_readings = self.surface_readings;
        f(&mut apertium)
    }
}

impl crate::grammar_applicator::stream_format::StreamFormat for ApertiumFormat {
    fn print_cohort<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) {
        self.with_app(app, |a| a.print_cohort(cohort, output, profiling));
    }

    fn print_single_window<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        self.with_app(app, |a| a.print_single_window(window, output, profiling));
    }

    fn print_stream_command<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        cmd: &str,
        output: &mut W,
    ) {
        app.print_stream_command(cmd, output);
    }

    fn print_plain_text_line<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        line: &str,
        output: &mut W,
    ) {
        app.print_plain_text_line(line, output);
    }
}

// ---------------------------------------------------------------------------
// Port-infra: substring search over `Vec<char>`.
// ---------------------------------------------------------------------------

/// `blank.find(needle)` from the start over `Vec<char>`.
fn find_sub(hay: &[char], needle: &[char]) -> Option<usize> {
    find_sub_from(hay, needle, 0)
}

/// `blank.find(needle, from)` over `Vec<char>`.
fn find_sub_from(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() || from > hay.len() {
        return None;
    }
    (from..=hay.len().saturating_sub(needle.len())).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// Collect the live `(key, value)` pairs of a `Uint32FlatHashMap` (the map does
/// not derive `Clone`; this reproduces the C++ copy-assign `= variables_set`).
fn collect_map(m: &crate::flat_unordered_map::Uint32FlatHashMap) -> Vec<(u32, u32)> {
    m.iter().copied().collect()
}

/// Collect the live items of a `Uint32FlatHashSet` (same rationale as
/// [`collect_map`]).
fn collect_set(s: &crate::flat_unordered_set::Uint32FlatHashSet) -> Vec<u32> {
    s.iter().collect()
}

// Ensure the flags_t/SingleWindow/Reading imports are considered used even if a
// branch is elided; these are load-bearing types in the signatures above.
const _: fn() = || {
    let _: Option<flags_t> = None;
    let _: Option<SingleWindow> = None;
    let _: Option<Reading> = None;
};
