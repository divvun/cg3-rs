//! Port of `src/NicelineApplicator.{cpp,hpp}` — the "Niceline" CG output format,
//! one cohort per line (`wordform<TAB>reading<TAB>reading...`), each reading a
//! space-separated tag list whose baseform is written as `[base]`.
//!
//! ## Composition, not inheritance
//! C++ `class NicelineApplicator : public virtual GrammarApplicator`. Rust has no
//! inheritance, so the applicator OWNS a [`GrammarApplicator`] via `base` and
//! forwards to its engine methods / arenas (`self.base.run_grammar_on_window`,
//! `self.base.doc.store`, `self.base.grammar`, `self.base.doc.stream`). The two virtual
//! overrides (`printReading`, `printCohort`, `printSingleWindow`,
//! `runGrammarOnText`) are reimplemented here; where the C++ base would be
//! dispatched to (it never is for Niceline — every print path is overridden) the
//! Rust just calls the local method.
//!
//! ## Engine / core mismatches (noted, faithfully worked around)
//! * `grammar->single_tags[hash]` operator[] (default-insert-null on miss) →
//!   [`tag_by_hash`] local helper (the `pub(super)` one in `core` is not
//!   reachable from this sibling module), returning `TagId(0)` on a miss (benign;
//!   these hashes are always present).
//! * `does_set_match_cohort_normal` gained a 4th `context: Option<&mut …>` param
//!   in the port; the C++ 2-arg call passes `None`.
//! * `add_tag` in the port is `add_tag(&str, type: u32)`; the C++ `addTag(base)`
//!   1-arg overload maps to `add_tag(text, 0)`.
//! * The `Reading*` deep copy `alloc_reading(*sub)` is
//!   [`crate::reading::alloc_reading_copy`] (copies the whole `->next` chain).
//! * `ux_stderr`/`ux_stdin`/`ux_stdout` are `Option<()>` placeholders in the
//!   base, so the diagnostic emissions (`u_fprintf(ux_stderr, …)`) and the
//!   `input.good()/eof()` guards are elided — but the one-shot `did_warn_*`
//!   latch STATE is reproduced verbatim (the observable quirk).

use std::io::{Read, Seek, Write};

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::cohort::{CT_RELATED, CT_REMOVED, unignore_all};
use crate::grammar::Grammar;
use crate::grammar_applicator::{Engine, GrammarApplicator};
use crate::inlines::{isnl, skipto_nospan};
use crate::tag::{T_DEPENDENCY, T_MAPPING, T_RELATION};
use crate::types::TagHash;
use crate::uextras::{get_line_clean, u_fflush, u_fputc, ux_strip_bom};

/// C++ `Strings.hpp` string constants used by the driver (UTF-16 → UTF-8 &str).
const STR_DUMMY: &str = "__CG3_DUMMY_STRINGBIT__";

/// C++ `grammar->single_tags[hash]` (operator[]) — resolve a hash to its
/// `TagId`. operator[] would default-insert a null `Tag*` on a miss (deref
/// crash); a miss here returns `TagId(0)` which cannot crash — benign for the
/// always-present hashes the call sites use.
fn tag_by_hash(grammar: &Grammar, hash: TagHash) -> TagId {
    let it = grammar.single_tags.find(hash.get());
    if it != grammar.single_tags.end() {
        it.get().1
    } else {
        TagId(0)
    }
}

// [spec:cg3:def:niceline-applicator.cg3.niceline-applicator]
/// C++ `class NicelineApplicator : public virtual GrammarApplicator`.
pub struct NicelineApplicator<'a> {
    /// The composed engine base (C++ `public virtual GrammarApplicator`;
    /// wave 4: BORROWED, matching the C++ shared virtual-base subobject).
    pub base: &'a mut GrammarApplicator,
    /// C++ `bool did_warn_statictags = false` — one-shot "cannot output static
    /// tags" warning latch.
    pub did_warn_statictags: bool,
    /// C++ `bool did_warn_subreadings = false` — one-shot "cannot output
    /// sub-readings" warning latch.
    pub did_warn_subreadings: bool,
}

impl<'a> NicelineApplicator<'a> {
    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.niceline-applicator-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.niceline-applicator-fn]
    /// C++ `NicelineApplicator::NicelineApplicator(std::ostream& ux_err)` —
    /// forwards `ux_err` to `GrammarApplicator(ux_err)`; no body of its own. The
    /// two latches keep their `false` in-class defaults.
    ///
    /// DIVERGENCE: the base ctor takes the owned `Grammar` (the port owns it by
    /// value at construction); the `ux_err` stream is an `Option<()>`
    /// placeholder, so it is not stored.
    pub fn new(base: &'a mut GrammarApplicator) -> Self {
        NicelineApplicator {
            base,
            did_warn_statictags: false,
            did_warn_subreadings: false,
        }
    }

    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.run-grammar-on-text-fn]
    /// C++ `void NicelineApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`. Niceline-specific per-line tokenizer: one cohort
    /// per line, TABs separating readings, `[base]` baseforms.
    ///
    /// PORT NOTES: `input`/`output` are generic Rust handles (C++ `std::istream&`
    /// / `std::ostream&`). Storing them into `ux_stdin`/`ux_stdout` is elided
    /// (`Option<()>` placeholders). The `input.good()/eof()/output/grammar`
    /// validity guards and every `u_fprintf(ux_stderr,…)` diagnostic (including
    /// the "looked like a cohort but wasn't" and "no valid baseform" warnings)
    /// are deferred with the I/O layer, but their control-flow effects
    /// (`goto istext`, baseform fallback) are preserved. `line`/`cleaned` are
    /// native `String`s filled by `get_line_clean`; the C++ `UChar*` pointer
    /// walks become BYTE-offset `usize` cursors read via `inlines::char_at`
    /// (which yields `'\0'` past the end, matching the NUL-terminated buffer).
    // Faithful-port mirrors: assignments kept 1:1 with the C++ text even where
    // the ported reads were elided (see the deferred-I/O / driver notes).
    pub fn run_grammar_on_text<F, R, W>(
        &mut self,
        fmt: &mut F,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: Read + Seek,
        W: Write,
    {
        crate::error::catch_fatal(|| self.run_grammar_on_text_impl(fmt, input, output))
    }

    #[allow(unused_assignments, unused_variables)]
    fn run_grammar_on_text_impl<F, R, W>(&mut self, fmt: &mut F, input: &mut R, output: &mut W)
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: Read + Seek,
        W: Write,
    {
        // ux_stdin = &input; ux_stdout = &output; (elided: Option<()> placeholders)
        // The good()/eof()/output/grammar validity checks (each CG3Quit(1) with a
        // u_fprintf diagnostic) are deferred with the I/O layer.
        // No-hard/soft-delimiter warnings: deferred I/O.

        let mut line = String::new();
        let mut cleaned = String::new();
        let ignoreinput = false;
        let mut did_soft_lookback = false;

        self.base.index();

        let reset_after: u32 = (self.base.cfg.num_windows + 4) * 2 + 1;
        let mut lines: u32 = 0;

        let mut c_swindow: Option<SwId> = None;
        let mut c_cohort: Option<CohortId> = None;
        #[allow(unused_assignments)]
        let mut c_reading: Option<ReadingId> = None;

        let mut l_swindow: Option<SwId> = None;
        let mut l_cohort: Option<CohortId> = None;

        self.base.doc.stream.window_span = self.base.cfg.num_windows;

        ux_strip_bom(input);

        // C++ `while (!input.eof())`: loop until get_line_clean stops producing.
        loop {
            lines += 1;
            let mut packoff = get_line_clean(&mut line, &mut cleaned, input, true);

            // C++ `while (!input.eof())`: a blank line (packoff == 0 but
            // `line[0]` holds the newline) is NOT end-of-stream; only a read
            // that stores nothing is. Sampled here, acted on at the bottom
            // (matches the base run_grammar_on_text driver).
            let hit_eof = packoff == 0 && line.is_empty();

            // Trim trailing whitespace.
            while let Some(c) = cleaned.chars().next_back() {
                if !crate::inlines::isspace(c) {
                    break;
                }
                cleaned.pop();
                packoff = cleaned.len();
            }

            let mut is_text = false;

            if !ignoreinput && !cleaned.is_empty() && !cleaned.starts_with('<') {
                // space = &cleaned[0]; SKIPTO_NOSPAN(space, '\t');
                let mut space = 0usize;
                skipto_nospan(&cleaned, &mut space, '\t');

                if crate::inlines::char_at(&cleaned, space) != '\0'
                    && crate::inlines::char_at(&cleaned, space) != '\t'
                {
                    // "looked like a cohort but wasn't - treated as text": deferred.
                    is_text = true;
                } else {
                    // The C++ NUL-cuts the buffer at the TAB; natively the
                    // wordform is cleaned[..space] and readings start past it.

                    // (a) Soft-limit lookback.
                    if let Some(sw) = c_swindow {
                        let over_soft = self.base.doc.store.single_windows.get(sw.0).cohorts.len()
                            >= self.base.cfg.soft_limit as usize;
                        if over_soft
                            && self.base.grammar.soft_delimiters.is_some()
                            && !did_soft_lookback
                        {
                            did_soft_lookback = true;
                            let sd = self.base.grammar.sets_list
                                [self.base.grammar.soft_delimiters.unwrap().0]
                                .number
                                .get();
                            let cohorts =
                                self.base.doc.store.single_windows.get(sw.0).cohorts.clone();
                            for &c in cohorts.iter().rev() {
                                if self.base.engine().does_set_match_cohort_normal(c, sd, None) {
                                    did_soft_lookback = false;
                                    let cohort = self.base.engine().delimit_at(sw, c);
                                    // cSWindow = cohort->parent->next;
                                    let parent =
                                        self.base.doc.store.cohorts.get(cohort.0).parent.unwrap();
                                    c_swindow =
                                        self.base.doc.store.single_windows.get(parent.0).next;
                                    if let Some(cc) = c_cohort {
                                        self.base.doc.store.cohorts.get_mut(cc.0).parent =
                                            c_swindow;
                                    }
                                    // verbose soft-limit warning: deferred.
                                    break;
                                }
                            }
                        }
                    }

                    // (b) Soft-delimiter on the current cohort.
                    if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
                        let over_soft = self.base.doc.store.single_windows.get(sw.0).cohorts.len()
                            >= self.base.cfg.soft_limit as usize;
                        let sd_hit = self.base.grammar.soft_delimiters.is_some() && {
                            let sd = self.base.grammar.sets_list
                                [self.base.grammar.soft_delimiters.unwrap().0]
                                .number
                                .get();
                            self.base
                                .engine()
                                .does_set_match_cohort_normal(cc, sd, None)
                        };
                        if over_soft && sd_hit {
                            // verbose soft-limit warning: deferred.
                            let rs = self.base.doc.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let te = self.base.cfg.endtag;
                                let tid = tag_by_hash(&self.base.grammar, te);
                                self.base.engine().add_tag_to_reading(r, tid);
                            }
                            crate::single_window::append_cohort(
                                &mut self.base.doc.store,
                                &mut self.base.doc.cohorts,
                                &mut self.base.doc.deps,
                                sw,
                                cc,
                            );
                            l_swindow = Some(sw);
                            c_swindow = None;
                            c_cohort = None;
                            self.base.doc.num_cohorts += 1;
                            did_soft_lookback = false;
                        }
                    }

                    // (c) Hard break.
                    if let Some(cc) = c_cohort {
                        let sw = c_swindow.unwrap();
                        let over_hard = self.base.doc.store.single_windows.get(sw.0).cohorts.len()
                            >= self.base.cfg.hard_limit as usize;
                        let delim_hit = self.base.cfg.dep_delimit == 0
                            && self.base.grammar.delimiters.is_some()
                            && {
                                let d = self.base.grammar.sets_list
                                    [self.base.grammar.delimiters.unwrap().0]
                                    .number
                                    .get();
                                self.base.engine().does_set_match_cohort_normal(cc, d, None)
                            };
                        if over_hard || delim_hit {
                            // (!is_conv && over_hard) "Hard limit ... forcing break": deferred.
                            let rs = self.base.doc.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let te = self.base.cfg.endtag;
                                let tid = tag_by_hash(&self.base.grammar, te);
                                self.base.engine().add_tag_to_reading(r, tid);
                            }
                            crate::single_window::append_cohort(
                                &mut self.base.doc.store,
                                &mut self.base.doc.cohorts,
                                &mut self.base.doc.deps,
                                sw,
                                cc,
                            );
                            l_swindow = Some(sw);
                            c_swindow = None;
                            c_cohort = None;
                            self.base.doc.num_cohorts += 1;
                            did_soft_lookback = false;
                        }
                    }

                    // No current window: allocate + init a fresh one.
                    if c_swindow.is_none() {
                        let sw = self
                            .base
                            .doc
                            .stream
                            .alloc_append_single_window(&mut self.base.doc.store);
                        self.base.engine().init_empty_single_window(sw);
                        c_swindow = Some(sw);
                        l_swindow = Some(sw);
                        c_cohort = None;
                        self.base.doc.num_windows += 1;
                        did_soft_lookback = false;
                    }

                    // Pending cCohort: append it.
                    if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
                        crate::single_window::append_cohort(
                            &mut self.base.doc.store,
                            &mut self.base.doc.cohorts,
                            &mut self.base.doc.deps,
                            sw,
                            cc,
                        );
                    }

                    // Drain a window if enough have queued up.
                    if self.base.doc.stream.next.len() > self.base.cfg.num_windows as usize {
                        self.base.engine().shuffle_windows_down();
                        self.base.engine().run_grammar_on_window_with(fmt, output);
                        if self.base.doc.num_windows.is_multiple_of(reset_after) {
                            self.base.reset_indexes();
                        }
                        // verbose progress: deferred.
                    }

                    // Build wordform: "\"<" + text-before-TAB + ">\"".
                    let sw = c_swindow.unwrap();
                    let inner: String = cleaned[0..space].to_string();
                    let wf_text = format!("\"<{inner}>\"");

                    let cc = crate::cohort::alloc_cohort(&mut self.base.doc.store, Some(sw));
                    let gn = self.base.doc.cohorts.next_cohort_number();
                    let wf = self.base.add_tag(&wf_text, crate::tag::TagType::empty());
                    {
                        let c = self.base.doc.store.cohorts.get_mut(cc.0);
                        c.global_number = gn;
                        c.wordform = Some(wf);
                    }
                    c_cohort = Some(cc);
                    l_cohort = Some(cc);
                    self.base.doc.num_cohorts += 1;

                    // Reading loop: advance past the TAB.
                    space += 1;
                    while crate::inlines::char_at(&cleaned, space) != '\0' {
                        let cr = crate::reading::alloc_reading(&mut self.base.doc.store, Some(cc));
                        c_reading = Some(cr);
                        crate::inlines::insert_if_exists(
                            &mut self.base.doc.store.cohorts.get_mut(cc.0).possible_sets,
                            self.base.grammar.sets_any.as_ref(),
                        );

                        // base = space; skip a leading quoted baseform / [bracket].
                        let mut base = space;
                        if crate::inlines::char_at(&cleaned, space) == '"' {
                            space += 1;
                            skipto_nospan(&cleaned, &mut space, '"');
                        }
                        if crate::inlines::char_at(&cleaned, space) == '[' {
                            skipto_nospan(&cleaned, &mut space, ']');
                        }

                        let mut mappings: crate::tag::TagList = Vec::new();

                        // tab = u_strchr(space, '\t'); the C++ NUL-cuts there —
                        // natively the reading segment is cleaned[..seg_end].
                        let tab: Option<usize> = cleaned[space..].find('\t').map(|i| space + i);
                        let seg_end = tab.unwrap_or(cleaned.len());
                        let seg = &cleaned[..seg_end];

                        // Token loop: while (space=strchr(space,' ')) != null.
                        loop {
                            // advance space to next ' ' within this reading region.
                            let mut sp = space;
                            while crate::inlines::char_at(seg, sp) != '\0'
                                && crate::inlines::char_at(seg, sp) != ' '
                            {
                                sp += 1;
                            }
                            if crate::inlines::char_at(seg, sp) != ' ' {
                                break;
                            }
                            space = sp;
                            if base < space {
                                // [x] -> "x" rewrite (applied to the extracted
                                // token; the C++ rewrote the buffer in place).
                                let mut tok: String = seg[base..space].to_string();
                                if tok.starts_with('[') && tok.ends_with(']') {
                                    tok = format!("\"{}\"", &tok[1..tok.len() - 1]);
                                }
                                let tag = self.base.add_tag(&tok, crate::tag::TagType::empty());
                                let (ttype, first) = {
                                    let t = &self.base.grammar.single_tags_list[tag.0];
                                    (t.r#type, t.tag.chars().next().unwrap_or('\0'))
                                };
                                if ttype.intersects(T_MAPPING)
                                    || first == self.base.grammar.mapping_prefix
                                {
                                    mappings.push(tag);
                                } else {
                                    self.base.engine().add_tag_to_reading(cr, tag);
                                }
                            }
                            // base = ++space; skip quoted / bracketed base again.
                            space += 1;
                            base = space;
                            if crate::inlines::char_at(seg, space) == '"' {
                                space += 1;
                                skipto_nospan(seg, &mut space, '"');
                            }
                            if crate::inlines::char_at(seg, space) == '[' {
                                skipto_nospan(seg, &mut space, ']');
                            }
                        }
                        // Trailing token `base` (runs to the segment end).
                        if base < seg_end {
                            let end = seg_end;
                            let mut tok: String = seg[base..end].to_string();
                            if tok.starts_with('[') && tok.ends_with(']') {
                                tok = format!("\"{}\"", &tok[1..tok.len() - 1]);
                            }
                            let tag = self.base.add_tag(&tok, crate::tag::TagType::empty());
                            let (ttype, first) = {
                                let t = &self.base.grammar.single_tags_list[tag.0];
                                (t.r#type, t.tag.chars().next().unwrap_or('\0'))
                            };
                            if ttype.intersects(T_MAPPING)
                                || first == self.base.grammar.mapping_prefix
                            {
                                mappings.push(tag);
                            } else {
                                self.base.engine().add_tag_to_reading(cr, tag);
                            }
                        }

                        if self.base.doc.store.readings.get(cr.0).baseform.is_none() {
                            let h = {
                                let wfid = self
                                    .base
                                    .doc
                                    .store
                                    .cohorts
                                    .get(cc.0)
                                    .wordform
                                    .expect("cohort wordform");
                                self.base.grammar.single_tags_list[wfid.0].hash
                            };
                            self.base.doc.store.readings.get_mut(cr.0).baseform = Some(h);
                            // "Line %u had no valid baseform." warning: deferred.
                        }
                        if !mappings.is_empty() {
                            self.base
                                .engine()
                                .split_mappings(&mut mappings, cc, cr, true);
                        }
                        crate::cohort::append_reading(&mut self.base.doc.store, cc, cr);
                        self.base.doc.num_readings += 1;

                        if let Some(t) = tab {
                            space = t + 1;
                        } else {
                            break;
                        }
                    }
                    if self.base.doc.store.cohorts.get(cc.0).readings.is_empty() {
                        self.base.engine().init_empty_cohort(cc);
                    }
                }
            } else {
                is_text = true;
            }

            if is_text {
                // istext:
                if !cleaned.is_empty() && !line.is_empty() {
                    let text: String = line.clone();
                    if let Some(lc) = l_cohort {
                        self.base
                            .doc
                            .store
                            .cohorts
                            .get_mut(lc.0)
                            .text
                            .push_str(&text);
                    } else if let Some(ls) = l_swindow {
                        self.base
                            .doc
                            .store
                            .single_windows
                            .get_mut(ls.0)
                            .text
                            .push_str(&text);
                    } else {
                        // C++ virtual printPlainTextLine.
                        fmt.print_plain_text_line(&mut self.base.engine(), &text, output);
                    }
                }
            }

            self.base.doc.num_lines += 1;
            line.clear();
            cleaned.clear();

            // Loop termination: the C++ `while(!input.eof())` re-check at the
            // top of the loop, using the EOF state sampled after get_line_clean.
            if hit_eof {
                break;
            }
        }

        self.base.doc.input_eof = true;

        // Finalization.
        if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
            crate::single_window::append_cohort(
                &mut self.base.doc.store,
                &mut self.base.doc.cohorts,
                &mut self.base.doc.deps,
                sw,
                cc,
            );
            if self.base.doc.store.cohorts.get(cc.0).readings.is_empty() {
                self.base.engine().init_empty_cohort(cc);
            }
            let rs = self.base.doc.store.cohorts.get(cc.0).readings.clone();
            for r in rs {
                let te = self.base.cfg.endtag;
                let tid = tag_by_hash(&self.base.grammar, te);
                self.base.engine().add_tag_to_reading(r, tid);
            }
            #[allow(unused_assignments)]
            {
                c_reading = None;
                c_cohort = None;
                c_swindow = None;
            }
        }
        while self.base.engine().rotate_next().is_some() {
            self.base.engine().run_grammar_on_window_with(fmt, output);
        }

        self.base.engine().shuffle_windows_down();
        while !self.base.doc.stream.previous.is_empty() {
            let tmp = self.base.doc.stream.previous[0];
            // C++ virtual printSingleWindow — the most-derived format decides.
            fmt.print_single_window(&mut self.base.engine(), tmp, output, false);
            let t = Some(tmp);
            crate::single_window::free_swindow(
                &mut self.base.doc.store,
                &mut self.base.doc.cohorts,
                &mut self.base.doc.deps,
                t,
            );
            self.base.doc.stream.previous.remove(0);
        }

        u_fflush(output);
    }
}

/// Niceline print-vtable strategy carrying the two one-shot warn latches.
///
/// The C++ `NicelineApplicator` print overrides (`printReading`, `printCohort`,
/// `printSingleWindow`) dispatch off the shared `GrammarApplicator` subobject;
/// wave-4 peels them onto this standalone strategy so `format_converter`'s
/// `ConvFormat` can drive them with an [`Engine`] split-borrow view while the
/// one-shot `did_warn_*` latch STATE lives on the strategy value.
#[derive(Default)]
pub struct NicelineFormat {
    /// C++ `bool did_warn_statictags = false` — one-shot "cannot output static
    /// tags" warning latch.
    pub did_warn_statictags: bool,
    /// C++ `bool did_warn_subreadings = false` — one-shot "cannot output
    /// sub-readings" warning latch.
    pub did_warn_subreadings: bool,
}

impl NicelineFormat {
    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-reading-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-reading-fn]
    /// C++ `void NicelineApplicator::printReading(const Reading* reading,
    /// std::ostream& output)`.
    pub fn print_reading_e<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        reading: ReadingId,
        output: &mut W,
    ) {
        let (noprint, deleted, baseform, parent_cid, next) = {
            let r = e.doc.store.readings.get(reading.0);
            (
                r.noprint,
                r.deleted,
                r.baseform.unwrap_or(TagHash(0)),
                r.parent,
                r.next,
            )
        };
        if noprint {
            return;
        }
        if deleted {
            return;
        }
        u_fputc('\t', output);
        if baseform != TagHash(0) {
            // "[%.*S]" of tag.data()+1 for tag.size()-2 → strip both quotes, wrap [].
            let tid = tag_by_hash(e.grammar, baseform);
            let tag = &e.grammar.single_tags_list[tid.0].tag;
            let inner = strip_surrounding_one(tag);
            let _ = write!(output, "[{inner}]");
        }

        let parent_cid = parent_cid.expect("reading has no parent cohort");
        let wordform_hash = {
            let wf = e.doc.store.cohorts.get(parent_cid.0).wordform;
            wf.map(|t| e.grammar.single_tags_list[t.0].hash)
                .unwrap_or(TagHash(0))
        };

        let tags_list: Vec<u32> = e.doc.store.readings.get(reading.0).tags_list.clone();
        let mut unique: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        for tter in tags_list {
            let tter = TagHash(tter);
            if (!e.cfg.show_end_tags && tter == e.cfg.endtag) || tter == e.cfg.begintag {
                continue;
            }
            if tter == baseform || tter == wordform_hash {
                continue;
            }
            if e.cfg.unique_tags {
                if unique.contains(&tter.get()) {
                    continue;
                }
                unique.insert(tter.get());
            }
            let tid = tag_by_hash(e.grammar, tter);
            let ttype = e.grammar.single_tags_list[tid.0].r#type;
            if ttype.intersects(T_DEPENDENCY) && e.doc.deps.has_dep && !e.cfg.dep_original {
                continue;
            }
            if ttype.intersects(T_RELATION) && e.doc.deps.has_relations {
                continue;
            }
            let _ = write!(output, " {}", e.grammar.single_tags_list[tid.0].tag);
        }

        // Dependency block.
        let parent_removed = e
            .doc
            .store
            .cohorts
            .get(parent_cid.0)
            .r#type
            .intersects(CT_REMOVED);
        if e.doc.deps.has_dep && !parent_removed {
            {
                let c = e.doc.store.cohorts.get_mut(parent_cid.0);
                if c.dep_self.is_none() {
                    c.dep_self = Some(c.global_number);
                }
            }
            let (p_global, p_local, p_dep_parent, p_dep_self, p_sw) = {
                let c = e.doc.store.cohorts.get(parent_cid.0);
                (
                    c.global_number,
                    c.local_number,
                    c.dep_parent,
                    c.dep_self.map_or(0, |g| g.get()),
                    c.parent,
                )
            };
            let mut pr = parent_cid;
            if let Some(pdp) = p_dep_parent {
                if pdp == crate::types::GlobalNumber(0) {
                    if let Some(sw) = p_sw {
                        pr = e.doc.store.single_windows.get(sw.0).cohorts[0];
                    }
                } else if let Some(&mapped) = e.doc.cohorts.cohort_map.get(&pdp) {
                    pr = mapped;
                }
            }
            let arrow = if e.cfg.unicode_tags { "\u{2192}" } else { "->" };
            if e.cfg.dep_absolute {
                let pr_global = e.doc.store.cohorts.get(pr.0).global_number;
                let _ = write!(output, " #{p_global}{arrow}{pr_global}");
            } else if !e.doc.dep_has_spanned {
                let pr_local = e.doc.store.cohorts.get(pr.0).local_number;
                let _ = write!(output, " #{p_local}{arrow}{pr_local}");
            } else if let Some(pdp) = p_dep_parent {
                let _ = write!(output, " #{p_dep_self}{arrow}{pdp}");
            } else {
                let _ = write!(output, " #{p_dep_self}{arrow}{p_dep_self}");
            }
        }

        // Relations block.
        let (p_related, p_global2, relations) = {
            let c = e.doc.store.cohorts.get(parent_cid.0);
            (
                c.r#type.intersects(CT_RELATED),
                c.global_number,
                c.relations.clone(),
            )
        };
        if p_related {
            let _ = write!(output, " ID:{p_global2}");
            for (rel_hash, targets) in relations.iter() {
                for siter in targets.iter() {
                    let tid = tag_by_hash(e.grammar, TagHash(*rel_hash));
                    let _ = write!(
                        output,
                        " R:{}:{siter}",
                        e.grammar.single_tags_list[tid.0].tag
                    );
                }
            }
        }

        // Trace block.
        if e.cfg.trace {
            let hit_by: Vec<u32> = e.doc.store.readings.get(reading.0).hit_by.clone();
            for hb in hit_by {
                u_fputc(' ', output);
                e.print_trace(output, hb);
            }
        }

        // Sub-reading warning (lossy quirk: sub-readings are NOT printed).
        if next.is_some() && !self.did_warn_subreadings {
            // "Niceline CG format cannot output sub-readings! …": deferred emission.
            self.did_warn_subreadings = true;
        }
    }

    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-cohort-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-cohort-fn]
    /// C++ `void NicelineApplicator::printCohort(Cohort* cohort,
    /// std::ostream& output, bool profiling = false)`.
    pub fn print_cohort_e<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) {
        let local_number = e.doc.store.cohorts.get(cohort.0).local_number;
        let removed = e
            .doc
            .store
            .cohorts
            .get(cohort.0)
            .r#type
            .intersects(CT_REMOVED);

        // `goto removed` from local_number == 0 or CT_REMOVED skips the body.
        if local_number != 0 && !removed {
            let wblank = e.doc.store.cohorts.get(cohort.0).wblank.clone();
            if !wblank.is_empty() {
                e.print_plain_text_line(&wblank, output);
                if !isnl(wblank.chars().next_back().unwrap_or('\0')) {
                    u_fputc('\n', output);
                }
            }

            // "%.*S" of wordform.data()+2 for size()-4 → strip "\"<" and ">\"".
            let (wf_inner, has_wread) = {
                let c = e.doc.store.cohorts.get(cohort.0);
                let wf = c.wordform.expect("cohort wordform");
                let tag = &e.grammar.single_tags_list[wf.0].tag;
                (strip_wordform_brackets(tag), c.wread.is_some())
            };
            let _ = write!(output, "{wf_inner}");
            if has_wread && !self.did_warn_statictags {
                // "Niceline CG format cannot output static tags! …": deferred.
                self.did_warn_statictags = true;
            }

            if !profiling {
                unignore_all(&mut e.doc.store, cohort);
                if !e.cfg.split_mappings {
                    e.merge_mappings(cohort);
                }
            }

            let readings: Vec<ReadingId> = e.doc.store.cohorts.get(cohort.0).readings.clone();
            if readings.is_empty() {
                u_fputc('\t', output);
            }
            for r in readings {
                self.print_reading_e(e, r, output);
            }
        }

        // removed:
        u_fputc('\n', output);
        let text = e.doc.store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() && text.chars().any(|c| !is_ws(&e.cfg.ws, c)) {
            e.print_plain_text_line(&text, output);
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }
    }

    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-single-window-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-single-window-fn]
    /// C++ `void NicelineApplicator::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling = false)`.
    pub fn print_single_window_e<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        let (all_cohorts, text, text_post) = {
            let w = e.doc.store.single_windows.get(window.0);
            (w.all_cohorts.clone(), w.text.clone(), w.text_post.clone())
        };

        if !text.is_empty() {
            e.print_plain_text_line(&text, output);
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }

        for cohort in all_cohorts {
            self.print_cohort_e(e, cohort, output, profiling);
        }

        if !text_post.is_empty() {
            e.print_plain_text_line(&text_post, output);
            if !isnl(text_post.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }

        u_fputc('\n', output);
        u_fflush(output);
    }
}

/// C++ `UString::find_first_not_of(ws)` membership: is `c` in the (NUL-
/// terminated) whitespace set `ws`?
fn is_ws(ws: &[crate::types::UChar; 4], c: char) -> bool {
    for &w in ws {
        if w == '\0' {
            break;
        }
        if w == c {
            return true;
        }
    }
    false
}

/// `tag.substr(1, size()-2)` — strip one leading and one trailing code point (the
/// two surrounding `"` of a baseform stored as `"base"`), leaving `base`.
fn strip_surrounding_one(tag: &str) -> String {
    let chars: Vec<char> = tag.chars().collect();
    if chars.len() < 2 {
        return String::new();
    }
    chars[1..chars.len() - 1].iter().collect()
}

/// `wordform.data()+2` for `size()-4` — strip the leading `"<` (2) and trailing
/// `>"` (2) of a wordform stored as `"<word>"`, leaving `word`.
fn strip_wordform_brackets(tag: &str) -> String {
    let chars: Vec<char> = tag.chars().collect();
    if chars.len() < 4 {
        return String::new();
    }
    chars[2..chars.len() - 2].iter().collect()
}
