//! Port of `src/NicelineApplicator.{cpp,hpp}` — the "Niceline" CG output format,
//! one cohort per line (`wordform<TAB>reading<TAB>reading...`), each reading a
//! space-separated tag list whose baseform is written as `[base]`.
//!
//! ## Composition, not inheritance
//! C++ `class NicelineApplicator : public virtual GrammarApplicator`. Rust has no
//! inheritance, so the applicator OWNS a [`GrammarApplicator`] via `base` and
//! forwards to its engine methods / arenas (`self.base.run_grammar_on_window`,
//! `self.base.store`, `self.base.grammar`, `self.base.gWindow`). The two virtual
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
use crate::cohort::{CT_RELATED, CT_REMOVED, DEP_NO_PARENT, unignore_all};
use crate::grammar::Grammar;
use crate::grammar_applicator::GrammarApplicator;
use crate::inlines::{isnl, skipto_nospan};
use crate::tag::{T_DEPENDENCY, T_MAPPING, T_RELATION};
use crate::uextras::{get_line_clean, u_fflush, u_fprintf, u_fputc, ux_strip_bom};

/// C++ `Strings.hpp` string constants used by the driver (UTF-16 → UTF-8 &str).
const STR_DUMMY: &str = "__CG3_DUMMY_STRINGBIT__";

/// C++ `grammar->single_tags[hash]` (operator[]) — resolve a hash to its
/// `TagId`. operator[] would default-insert a null `Tag*` on a miss (deref
/// crash); a miss here returns `TagId(0)` which cannot crash — benign for the
/// always-present hashes the call sites use.
fn tag_by_hash(grammar: &Grammar, hash: u32) -> TagId {
    let it = grammar.single_tags.find(hash);
    if it != grammar.single_tags.end() {
        it.get().1
    } else {
        TagId(0)
    }
}

// [spec:cg3:def:niceline-applicator.cg3.niceline-applicator]
/// C++ `class NicelineApplicator : public virtual GrammarApplicator`.
pub struct NicelineApplicator {
    /// The composed engine base (C++ `public virtual GrammarApplicator`).
    pub base: GrammarApplicator,
    /// C++ `bool did_warn_statictags = false` — one-shot "cannot output static
    /// tags" warning latch.
    pub did_warn_statictags: bool,
    /// C++ `bool did_warn_subreadings = false` — one-shot "cannot output
    /// sub-readings" warning latch.
    pub did_warn_subreadings: bool,
}

impl NicelineApplicator {
    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.niceline-applicator-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.niceline-applicator-fn]
    /// C++ `NicelineApplicator::NicelineApplicator(std::ostream& ux_err)` —
    /// forwards `ux_err` to `GrammarApplicator(ux_err)`; no body of its own. The
    /// two latches keep their `false` in-class defaults.
    ///
    /// DIVERGENCE: the base ctor takes the owned `Grammar` (the port owns it by
    /// value at construction); the `ux_err` stream is an `Option<()>`
    /// placeholder, so it is not stored.
    pub fn new(base: GrammarApplicator) -> Self {
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
    /// `Vec<char>` scratch buffers (what `get_line_clean` expects); the C++
    /// `UChar*` pointer walks become `usize` indices over that buffer.
    pub fn run_grammar_on_text<R, W>(&mut self, input: &mut R, output: &mut W)
    where
        R: Read + Seek,
        W: Write,
    {
        // ux_stdin = &input; ux_stdout = &output; (elided: Option<()> placeholders)
        // The good()/eof()/output/grammar validity checks (each CG3Quit(1) with a
        // u_fprintf diagnostic) are deferred with the I/O layer.
        // No-hard/soft-delimiter warnings: deferred I/O.

        let mut line: Vec<char> = vec!['\0'; 1024];
        let mut cleaned: Vec<char> = vec!['\0'; line.len() + 1];
        let ignoreinput = false;
        let mut did_soft_lookback = false;

        self.base.index();

        let reset_after: u32 = (self.base.num_windows + 4) * 2 + 1;
        let mut lines: u32 = 0;

        let mut c_swindow: Option<SwId> = None;
        let mut c_cohort: Option<CohortId> = None;
        #[allow(unused_assignments)]
        let mut c_reading: Option<ReadingId> = None;

        let mut l_swindow: Option<SwId> = None;
        let mut l_cohort: Option<CohortId> = None;

        self.base.gWindow.window_span = self.base.num_windows;

        ux_strip_bom(input);

        // C++ `while (!input.eof())`: loop until get_line_clean stops producing.
        loop {
            lines += 1;
            let mut packoff = get_line_clean(&mut line, &mut cleaned, input, true);

            // Trim trailing whitespace.
            while cleaned[0] != '\0' && packoff > 0 && crate::inlines::isspace(cleaned[packoff - 1]) {
                cleaned[packoff - 1] = '\0';
                packoff -= 1;
            }

            let mut is_text = false;

            if !ignoreinput && cleaned[0] != '\0' && cleaned[0] != '<' {
                // space = &cleaned[0]; SKIPTO_NOSPAN(space, '\t');
                let mut space = 0usize;
                skipto_nospan(&cleaned, &mut space, '\t');

                if cleaned[space] != '\0' && cleaned[space] != '\t' {
                    // "looked like a cohort but wasn't - treated as text": deferred.
                    is_text = true;
                } else {
                    if cleaned[space] == '\0' {
                        cleaned[space + 1] = '\0';
                    }
                    cleaned[space] = '\0';

                    // (a) Soft-limit lookback.
                    if let Some(sw) = c_swindow {
                        let over_soft = self.base.store.single_windows.get(sw.0).cohorts.len()
                            >= self.base.soft_limit as usize;
                        if over_soft
                            && self.base.grammar.soft_delimiters.is_some()
                            && !did_soft_lookback
                        {
                            did_soft_lookback = true;
                            let sd = self.base.grammar.sets_list
                                [self.base.grammar.soft_delimiters.unwrap().0]
                                .number;
                            let cohorts =
                                self.base.store.single_windows.get(sw.0).cohorts.clone();
                            for &c in cohorts.iter().rev() {
                                if self.base.does_set_match_cohort_normal(c, sd, None) {
                                    did_soft_lookback = false;
                                    let cohort = self.base.delimit_at(sw, c);
                                    // cSWindow = cohort->parent->next;
                                    let parent =
                                        self.base.store.cohorts.get(cohort.0).parent.unwrap();
                                    c_swindow = self.base.store.single_windows.get(parent.0).next;
                                    if let Some(cc) = c_cohort {
                                        self.base.store.cohorts.get_mut(cc.0).parent = c_swindow;
                                    }
                                    // verbose soft-limit warning: deferred.
                                    break;
                                }
                            }
                        }
                    }

                    // (b) Soft-delimiter on the current cohort.
                    if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
                        let over_soft = self.base.store.single_windows.get(sw.0).cohorts.len()
                            >= self.base.soft_limit as usize;
                        let sd_hit = self.base.grammar.soft_delimiters.is_some() && {
                            let sd = self.base.grammar.sets_list
                                [self.base.grammar.soft_delimiters.unwrap().0]
                                .number;
                            self.base.does_set_match_cohort_normal(cc, sd, None)
                        };
                        if over_soft && sd_hit {
                            // verbose soft-limit warning: deferred.
                            let rs = self.base.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let te = self.base.endtag;
                                let tid = tag_by_hash(&self.base.grammar, te);
                                self.base.add_tag_to_reading(r, tid);
                            }
                            crate::single_window::append_cohort(
                                &mut self.base.gWindow,
                                &mut self.base.store,
                                sw,
                                cc,
                            );
                            l_swindow = Some(sw);
                            c_swindow = None;
                            c_cohort = None;
                            self.base.numCohorts += 1;
                            did_soft_lookback = false;
                        }
                    }

                    // (c) Hard break.
                    if let Some(cc) = c_cohort {
                        let sw = c_swindow.unwrap();
                        let over_hard = self.base.store.single_windows.get(sw.0).cohorts.len()
                            >= self.base.hard_limit as usize;
                        let delim_hit = self.base.dep_delimit == 0
                            && self.base.grammar.delimiters.is_some()
                            && {
                                let d = self.base.grammar.sets_list
                                    [self.base.grammar.delimiters.unwrap().0]
                                    .number;
                                self.base.does_set_match_cohort_normal(cc, d, None)
                            };
                        if over_hard || delim_hit {
                            // (!is_conv && over_hard) "Hard limit ... forcing break": deferred.
                            let rs = self.base.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let te = self.base.endtag;
                                let tid = tag_by_hash(&self.base.grammar, te);
                                self.base.add_tag_to_reading(r, tid);
                            }
                            crate::single_window::append_cohort(
                                &mut self.base.gWindow,
                                &mut self.base.store,
                                sw,
                                cc,
                            );
                            l_swindow = Some(sw);
                            c_swindow = None;
                            c_cohort = None;
                            self.base.numCohorts += 1;
                            did_soft_lookback = false;
                        }
                    }

                    // No current window: allocate + init a fresh one.
                    if c_swindow.is_none() {
                        let sw = self
                            .base
                            .gWindow
                            .alloc_append_single_window(&mut self.base.store);
                        self.base.init_empty_single_window(sw);
                        c_swindow = Some(sw);
                        l_swindow = Some(sw);
                        c_cohort = None;
                        self.base.numWindows += 1;
                        did_soft_lookback = false;
                    }

                    // Pending cCohort: append it.
                    if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
                        crate::single_window::append_cohort(
                            &mut self.base.gWindow,
                            &mut self.base.store,
                            sw,
                            cc,
                        );
                    }

                    // Drain a window if enough have queued up.
                    if self.base.gWindow.next.len() > self.base.num_windows as usize {
                        self.base.gWindow.shuffle_windows_down(&mut self.base.store);
                        self.base.run_grammar_on_window(output);
                        if self.base.numWindows % reset_after == 0 {
                            self.base.reset_indexes();
                        }
                        // verbose progress: deferred.
                    }

                    // Build wordform: "\"<" + text-before-TAB + ">\"".
                    let sw = c_swindow.unwrap();
                    let inner: String = cleaned[0..space].iter().collect();
                    let wf_text = format!("\"<{inner}>\"");

                    let cc = crate::cohort::alloc_cohort(&mut self.base.store, Some(sw));
                    let gn = self.base.gWindow.cohort_counter;
                    self.base.gWindow.cohort_counter =
                        self.base.gWindow.cohort_counter.wrapping_add(1);
                    let wf = self.base.add_tag(&wf_text, 0);
                    {
                        let c = self.base.store.cohorts.get_mut(cc.0);
                        c.global_number = gn;
                        c.wordform = Some(wf);
                    }
                    c_cohort = Some(cc);
                    l_cohort = Some(cc);
                    self.base.numCohorts += 1;

                    // Reading loop: advance past the (nulled) TAB.
                    space += 1;
                    while cleaned[space] != '\0' {
                        let cr =
                            crate::reading::alloc_reading(&mut self.base.store, Some(cc));
                        c_reading = Some(cr);
                        crate::inlines::insert_if_exists(
                            &mut self.base.store.cohorts.get_mut(cc.0).possible_sets,
                            self.base.grammar.sets_any.as_ref(),
                        );

                        // base = space; skip a leading quoted baseform / [bracket].
                        let mut base = space;
                        if cleaned[space] == '"' {
                            space += 1;
                            skipto_nospan(&cleaned, &mut space, '"');
                        }
                        if cleaned[space] == '[' {
                            skipto_nospan(&cleaned, &mut space, ']');
                        }

                        let mut mappings: crate::tag::TagList = Vec::new();

                        // tab = u_strchr(space, '\t'); if found tab[0]=0.
                        let mut tab: Option<usize> = None;
                        {
                            let mut t = space;
                            while cleaned[t] != '\0' && cleaned[t] != '\t' {
                                t += 1;
                            }
                            if cleaned[t] == '\t' {
                                cleaned[t] = '\0';
                                tab = Some(t);
                            }
                        }

                        // Token loop: while (space=strchr(space,' ')) != null.
                        loop {
                            // advance space to next ' ' within this reading region.
                            let mut sp = space;
                            while cleaned[sp] != '\0' && cleaned[sp] != ' ' {
                                sp += 1;
                            }
                            if cleaned[sp] != ' ' {
                                break;
                            }
                            space = sp;
                            cleaned[space] = '\0';
                            if cleaned[base] != '\0' {
                                // [x] -> "x" rewrite.
                                if cleaned[base] == '[' && space > 0 && cleaned[space - 1] == ']' {
                                    cleaned[base] = '"';
                                    cleaned[space - 1] = '"';
                                }
                                let tok: String = cleaned[base..space].iter().collect();
                                let tag = self.base.add_tag(&tok, 0);
                                let (ttype, first) = {
                                    let t = &self.base.grammar.single_tags_list[tag.0];
                                    (t.r#type, t.tag.chars().next().unwrap_or('\0'))
                                };
                                if ttype & T_MAPPING != 0
                                    || first == self.base.grammar.mapping_prefix
                                {
                                    mappings.push(tag);
                                } else {
                                    self.base.add_tag_to_reading(cr, tag);
                                }
                            }
                            // base = ++space; skip quoted / bracketed base again.
                            space += 1;
                            base = space;
                            if cleaned[space] == '"' {
                                space += 1;
                                skipto_nospan(&cleaned, &mut space, '"');
                            }
                            if cleaned[space] == '[' {
                                skipto_nospan(&cleaned, &mut space, ']');
                            }
                        }
                        // Trailing token `base`.
                        if cleaned[base] != '\0' {
                            // find end of this trailing token (up to NUL).
                            let mut end = base;
                            while cleaned[end] != '\0' {
                                end += 1;
                            }
                            if cleaned[base] == '[' && end > 0 && cleaned[end - 1] == ']' {
                                cleaned[base] = '"';
                                cleaned[end - 1] = '"';
                            }
                            let tok: String = cleaned[base..end].iter().collect();
                            let tag = self.base.add_tag(&tok, 0);
                            let (ttype, first) = {
                                let t = &self.base.grammar.single_tags_list[tag.0];
                                (t.r#type, t.tag.chars().next().unwrap_or('\0'))
                            };
                            if ttype & T_MAPPING != 0 || first == self.base.grammar.mapping_prefix
                            {
                                mappings.push(tag);
                            } else {
                                self.base.add_tag_to_reading(cr, tag);
                            }
                        }

                        if self.base.store.readings.get(cr.0).baseform == 0 {
                            let h = {
                                let wfid = self
                                    .base
                                    .store
                                    .cohorts
                                    .get(cc.0)
                                    .wordform
                                    .expect("cohort wordform");
                                self.base.grammar.single_tags_list[wfid.0].hash
                            };
                            self.base.store.readings.get_mut(cr.0).baseform = h;
                            // "Line %u had no valid baseform." warning: deferred.
                        }
                        if !mappings.is_empty() {
                            self.base.split_mappings(&mut mappings, cc, cr, true);
                        }
                        crate::cohort::append_reading(&mut self.base.store, cc, cr);
                        self.base.numReadings += 1;

                        if let Some(t) = tab {
                            space = t + 1;
                        } else {
                            break;
                        }
                    }
                    if self.base.store.cohorts.get(cc.0).readings.is_empty() {
                        self.base.init_empty_cohort(cc);
                    }
                }
            } else {
                is_text = true;
            }

            if is_text {
                // istext:
                if cleaned[0] != '\0' && line[0] != '\0' {
                    let text: String = line.iter().take_while(|&&c| c != '\0').collect();
                    if let Some(lc) = l_cohort {
                        self.base.store.cohorts.get_mut(lc.0).text.push_str(&text);
                    } else if let Some(ls) = l_swindow {
                        self.base.store.single_windows.get_mut(ls.0).text.push_str(&text);
                    } else {
                        self.base.print_plain_text_line(&text, output);
                    }
                }
            }

            self.base.numLines += 1;
            line[0] = '\0';
            cleaned[0] = '\0';

            // Loop termination: get_line_clean at EOF yields nothing; the C++
            // `while(!input.eof())` is reproduced by breaking when a read makes no
            // progress (matches the base run_grammar_on_text driver).
            if packoff == 0 && line[0] == '\0' {
                break;
            }
        }

        self.base.input_eof = true;

        // Finalization.
        if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
            crate::single_window::append_cohort(
                &mut self.base.gWindow,
                &mut self.base.store,
                sw,
                cc,
            );
            if self.base.store.cohorts.get(cc.0).readings.is_empty() {
                self.base.init_empty_cohort(cc);
            }
            let rs = self.base.store.cohorts.get(cc.0).readings.clone();
            for r in rs {
                let te = self.base.endtag;
                let tid = tag_by_hash(&self.base.grammar, te);
                self.base.add_tag_to_reading(r, tid);
            }
            #[allow(unused_assignments)]
            {
                c_reading = None;
                c_cohort = None;
                c_swindow = None;
            }
        }
        while !self.base.gWindow.next.is_empty() {
            self.base.gWindow.shuffle_windows_down(&mut self.base.store);
            self.base.run_grammar_on_window(output);
        }

        self.base.gWindow.shuffle_windows_down(&mut self.base.store);
        while !self.base.gWindow.previous.is_empty() {
            let tmp = self.base.gWindow.previous[0];
            self.print_single_window(tmp, output, false);
            let mut t = Some(tmp);
            crate::single_window::free_swindow(&mut self.base.gWindow, &mut self.base.store, &mut t);
            self.base.gWindow.previous.remove(0);
        }

        u_fflush(output);
    }

    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-reading-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-reading-fn]
    /// C++ `void NicelineApplicator::printReading(const Reading* reading,
    /// std::ostream& output)`.
    pub fn print_reading<W: Write>(&mut self, reading: ReadingId, output: &mut W) {
        let (noprint, deleted, baseform, parent_cid, next) = {
            let r = self.base.store.readings.get(reading.0);
            (r.noprint, r.deleted, r.baseform, r.parent, r.next)
        };
        if noprint {
            return;
        }
        if deleted {
            return;
        }
        u_fputc('\t', output);
        if baseform != 0 {
            // "[%.*S]" of tag.data()+1 for tag.size()-2 → strip both quotes, wrap [].
            let tid = tag_by_hash(&self.base.grammar, baseform);
            let tag = &self.base.grammar.single_tags_list[tid.0].tag;
            let inner = strip_surrounding_one(tag);
            u_fprintf(output, format_args!("[{inner}]"));
        }

        let parent_cid = parent_cid.expect("reading has no parent cohort");
        let wordform_hash = {
            let wf = self.base.store.cohorts.get(parent_cid.0).wordform;
            wf.map(|t| self.base.grammar.single_tags_list[t.0].hash).unwrap_or(0)
        };

        let tags_list: Vec<u32> = self.base.store.readings.get(reading.0).tags_list.clone();
        let mut unique: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        for tter in tags_list {
            if (!self.base.show_end_tags && tter == self.base.endtag) || tter == self.base.begintag
            {
                continue;
            }
            if tter == baseform || tter == wordform_hash {
                continue;
            }
            if self.base.unique_tags {
                if unique.contains(&tter) {
                    continue;
                }
                unique.insert(tter);
            }
            let tid = tag_by_hash(&self.base.grammar, tter);
            let ttype = self.base.grammar.single_tags_list[tid.0].r#type;
            if ttype & T_DEPENDENCY != 0 && self.base.has_dep && !self.base.dep_original {
                continue;
            }
            if ttype & T_RELATION != 0 && self.base.has_relations {
                continue;
            }
            u_fprintf(
                output,
                format_args!(" {}", self.base.grammar.single_tags_list[tid.0].tag),
            );
        }

        // Dependency block.
        let parent_removed =
            self.base.store.cohorts.get(parent_cid.0).r#type & CT_REMOVED != 0;
        if self.base.has_dep && !parent_removed {
            {
                let c = self.base.store.cohorts.get_mut(parent_cid.0);
                if c.dep_self == 0 {
                    c.dep_self = c.global_number;
                }
            }
            let (p_global, p_local, p_dep_parent, p_dep_self, p_sw) = {
                let c = self.base.store.cohorts.get(parent_cid.0);
                (c.global_number, c.local_number, c.dep_parent, c.dep_self, c.parent)
            };
            let mut pr = parent_cid;
            if p_dep_parent != DEP_NO_PARENT {
                if p_dep_parent == 0 {
                    if let Some(sw) = p_sw {
                        pr = self.base.store.single_windows.get(sw.0).cohorts[0];
                    }
                } else if let Some(&mapped) = self.base.gWindow.cohort_map.get(&p_dep_parent) {
                    pr = mapped;
                }
            }
            let arrow = if self.base.unicode_tags { "\u{2192}" } else { "->" };
            if self.base.dep_absolute {
                let pr_global = self.base.store.cohorts.get(pr.0).global_number;
                u_fprintf(output, format_args!(" #{p_global}{arrow}{pr_global}"));
            } else if !self.base.dep_has_spanned {
                let pr_local = self.base.store.cohorts.get(pr.0).local_number;
                u_fprintf(output, format_args!(" #{p_local}{arrow}{pr_local}"));
            } else if p_dep_parent == DEP_NO_PARENT {
                u_fprintf(output, format_args!(" #{p_dep_self}{arrow}{p_dep_self}"));
            } else {
                u_fprintf(output, format_args!(" #{p_dep_self}{arrow}{p_dep_parent}"));
            }
        }

        // Relations block.
        let (p_related, p_global2, relations) = {
            let c = self.base.store.cohorts.get(parent_cid.0);
            (c.r#type & CT_RELATED != 0, c.global_number, c.relations.clone())
        };
        if p_related {
            u_fprintf(output, format_args!(" ID:{p_global2}"));
            for (rel_hash, targets) in relations.iter() {
                for siter in targets.iter().copied() {
                    let tid = tag_by_hash(&self.base.grammar, *rel_hash);
                    u_fprintf(
                        output,
                        format_args!(
                            " R:{}:{siter}",
                            self.base.grammar.single_tags_list[tid.0].tag
                        ),
                    );
                }
            }
        }

        // Trace block.
        if self.base.trace {
            let hit_by: Vec<u32> = self.base.store.readings.get(reading.0).hit_by.clone();
            for hb in hit_by {
                u_fputc(' ', output);
                self.base.print_trace(output, hb);
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
    pub fn print_cohort<W: Write>(&mut self, cohort: CohortId, output: &mut W, profiling: bool) {
        let local_number = self.base.store.cohorts.get(cohort.0).local_number;
        let removed = self.base.store.cohorts.get(cohort.0).r#type & CT_REMOVED != 0;

        // `goto removed` from local_number == 0 or CT_REMOVED skips the body.
        if local_number != 0 && !removed {
            let wblank = self.base.store.cohorts.get(cohort.0).wblank.clone();
            if !wblank.is_empty() {
                self.base.print_plain_text_line(&wblank, output);
                if !isnl(wblank.chars().next_back().unwrap_or('\0')) {
                    u_fputc('\n', output);
                }
            }

            // "%.*S" of wordform.data()+2 for size()-4 → strip "\"<" and ">\"".
            let (wf_inner, has_wread) = {
                let c = self.base.store.cohorts.get(cohort.0);
                let wf = c.wordform.expect("cohort wordform");
                let tag = &self.base.grammar.single_tags_list[wf.0].tag;
                (strip_wordform_brackets(tag), c.wread.is_some())
            };
            u_fprintf(output, format_args!("{wf_inner}"));
            if has_wread && !self.did_warn_statictags {
                // "Niceline CG format cannot output static tags! …": deferred.
                self.did_warn_statictags = true;
            }

            if !profiling {
                unignore_all(&mut self.base.store, cohort);
                if !self.base.split_mappings {
                    self.base.merge_mappings(cohort);
                }
            }

            let readings: Vec<ReadingId> =
                self.base.store.cohorts.get(cohort.0).readings.clone();
            if readings.is_empty() {
                u_fputc('\t', output);
            }
            for r in readings {
                self.print_reading(r, output);
            }
        }

        // removed:
        u_fputc('\n', output);
        let text = self.base.store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() && text.chars().any(|c| !is_ws(&self.base.ws, c)) {
            self.base.print_plain_text_line(&text, output);
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }
    }

    // [spec:cg3:def:niceline-applicator.cg3.niceline-applicator.print-single-window-fn]
    // [spec:cg3:sem:niceline-applicator.cg3.niceline-applicator.print-single-window-fn]
    /// C++ `void NicelineApplicator::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling = false)`.
    pub fn print_single_window<W: Write>(
        &mut self,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        let (all_cohorts, text, text_post) = {
            let w = self.base.store.single_windows.get(window.0);
            (w.all_cohorts.clone(), w.text.clone(), w.text_post.clone())
        };

        if !text.is_empty() {
            self.base.print_plain_text_line(&text, output);
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }

        for cohort in all_cohorts {
            self.print_cohort(cohort, output, profiling);
        }

        if !text_post.is_empty() {
            self.base.print_plain_text_line(&text_post, output);
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
