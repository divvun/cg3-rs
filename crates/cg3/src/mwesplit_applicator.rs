//! Port of `src/MweSplitApplicator.{cpp,hpp}` — the multi-word-expression
//! splitter. It runs the (trivial dummy) grammar via the base driver, then at
//! PRINT time splits every cohort whose readings carry component-word wordform
//! tags into one cohort per component word.
//!
//! ## Composition, not inheritance
//! C++ `class MweSplitApplicator : public virtual GrammarApplicator`. The Rust
//! applicator OWNS a [`GrammarApplicator`] via `base`. `runGrammarOnText`
//! delegates to the base driver.
//!
//! ## Virtual dispatch
//! In C++ the base driver reaches this class's overridden `printSingleWindow`
//! through virtual dispatch — that is where the MWE splitting happens. The Rust
//! base is an owned field, so the virtual link is modelled with a flag: the
//! ctor sets `base.mwe_split_at_print = true`, and the base
//! `GrammarApplicator::print_single_window` (core.rs) forwards to
//! [`GrammarApplicator::mwe_print_single_window`] (defined here, an inherent
//! method on the base type) when that flag is set. Every C++ virtual call site
//! (`runGrammarOnText` FLUSH/EOF drains, the retire loop at the head of
//! `runGrammarOnWindow`) thereby dispatches exactly as the C++ does.
//!
//! ## Engine / core mismatches (noted)
//! * `grammar->single_tags[hash]` operator[] → [`tag_by_hash`] (`TagId(0)` on a
//!   miss; benign for the always-present hashes).
//! * `reindex()` (C++ default args) → `finish()` on the conv grammar's draft.
//! * `alloc_reading(*sub)` deep-copy → [`crate::reading::alloc_reading_copy`]
//!   (copies the whole `->next` sub-reading chain).
//! * `add_tag` is `add_tag(&str, type)`.
//!
//! ## Reproduced quirks
//! * `printSingleWindow` captures `cs = window->cohorts.size()` BEFORE the loop;
//!   `splitMwe` appends its new cohorts to `window->cohorts` mid-iteration, but
//!   the captured `cs` bound means the freshly-appended split cohorts are NOT
//!   re-iterated.
//! * `splitMwe`'s fragile `prev`/`pos` invariants: a head reading without a
//!   wordform tag would dereference a null `prev` — preserved (the eligibility
//!   check guarantees every head reading HAS one before the chain walk, so
//!   `prev` is non-null before any `prev = prev->next` step).

use std::io::Write;

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::grammar::Grammar;
use crate::grammar_applicator::{Engine, GrammarApplicator};
use crate::inlines::{isnl, ui32};
use crate::tag::T_WORDFORM;
use crate::types::TagHash;
use crate::uextras::write_char;

/// C++ `Strings.hpp` constants (UTF-16 → UTF-8 &str).
const STR_CMD_SETVAR: &str = "<STREAMCMD:SETVAR:";
const STR_CMD_REMVAR: &str = "<STREAMCMD:REMVAR:";
const STR_CMD_FLUSH: &str = "<STREAMCMD:FLUSH>";

/// C++ `grammar->single_tags[hash]` (operator[]) — hash → `TagId`, `TagId(0)` on
/// a miss (benign; see `niceline_applicator`).
fn tag_by_hash(grammar: &Grammar, hash: TagHash) -> TagId {
    let it = grammar.single_tags().find(hash.get());
    if it != grammar.single_tags().end() {
        it.get().1
    } else {
        TagId(0)
    }
}

// [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator]
/// C++ `class MweSplitApplicator : public virtual GrammarApplicator`.
pub struct MweSplitApplicator {
    /// The composed engine base (C++ `public virtual GrammarApplicator`).
    pub base: GrammarApplicator,
}

impl MweSplitApplicator {
    // [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.mwe-split-applicator-fn]
    // [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.mwe-split-applicator-fn]
    /// C++ `MweSplitApplicator::MweSplitApplicator`. Builds
    /// and installs a minimal dummy grammar (a delimiters set holding the
    /// never-matching `STR_DUMMY` sentinel tag), then `setGrammar`, sets
    /// `is_conv = true`. (The C++ `owns_grammar = true` is dropped — Rust owns
    /// the grammar by value, so the flag has no observable effect.)
    ///
    /// The grammar is the converters' shared minimal one
    /// (`format_converter::conv_grammar`), installed as `base.grammar` in place of the C++ `setGrammar(res)`.
    pub fn new(mut base: GrammarApplicator) -> Result<Self, crate::error::Cg3Error> {
        let grammar = crate::format_converter::conv_grammar()?;
        base.grammar = Grammar::from_core(std::sync::Arc::new(grammar));
        base.set_grammar()?;
        base.cfg.is_conv = true;
        Ok(MweSplitApplicator { base })
    }

    // [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.run-grammar-on-text-fn]
    /// C++ `void MweSplitApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)` — a one-line delegation to the base driver. The MWE
    /// splitting happens only at print time (the base output path dispatches to
    /// this class's overridden `printSingleWindow` via [`MweSplitFormat`]).
    ///
    /// `input`/`output` are threaded as method params (`R: Read + Seek` /
    /// `W: Write`), matching the base
    /// [`GrammarApplicator::run_grammar_on_text`](GrammarApplicator::run_grammar_on_text)
    /// signature.
    pub fn run_grammar_on_text<R, W>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        self.base
            .run_grammar_on_text_with(&mut MweSplitFormat, input, output)
    }
}

/// The MweSplit print vtable (wave 4): C++ `MweSplitApplicator` overrides
/// `printSingleWindow` only; the other slots fall through to the base.
pub struct MweSplitFormat;

impl crate::grammar_applicator::stream_format::StreamFormat for MweSplitFormat {
    fn print_cohort<W: std::io::Write>(
        &mut self,
        e: &mut Engine<'_>,
        cohort: crate::arena::CohortId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), crate::error::RunError> {
        let trace = e.cfg.trace;
        e.print_cohort(cohort, output, profiling, trace);
        Ok(())
    }

    fn print_single_window<W: std::io::Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: crate::arena::SwId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), crate::error::RunError> {
        e.mwe_print_single_window(window, output, profiling)
    }

    fn print_stream_command<W: std::io::Write>(
        &mut self,
        e: &mut Engine<'_>,
        cmd: &str,
        output: &mut W,
    ) {
        e.print_stream_command(cmd, output);
    }

    fn print_plain_text_line<W: std::io::Write>(
        &mut self,
        e: &mut Engine<'_>,
        line: &str,
        output: &mut W,
    ) {
        e.print_plain_text_line(line, output);
    }
}

// ===========================================================================
// The overridden virtuals, as inherent methods on the split-borrow `Engine<'_>`
// view so the base driver's print path can dispatch to them (Rust stand-in for
// the C++ vtable; see module docs). They live in this file because they ARE
// `MweSplitApplicator::{maybeWfTag, splitMwe, printSingleWindow}`.
// ===========================================================================
impl Engine<'_> {
    // [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.maybe-wf-tag-fn]
    // [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.maybe-wf-tag-fn]
    /// C++ `const Tag* MweSplitApplicator::maybeWfTag(const Reading* r)`. Returns
    /// the first "extra" wordform-type (`T_WORDFORM`) tag on `r` that is neither
    /// the reading's own baseform nor its cohort's wordform, or `None`.
    pub fn mwe_maybe_wf_tag(&self, r: ReadingId) -> Option<TagId> {
        let rr = self.doc.store.readings.get(r.0);
        let baseform = rr.baseform.unwrap_or(TagHash(0));
        let wordform_hash = {
            #[expect(
                clippy::expect_used,
                reason = "the readings split here come from a cohort's reading list, which the stream readers fill with alloc_reading(Some(cohort))"
            )]
            let p = rr.parent.expect("reading parent");
            self.doc
                .store
                .cohorts
                .get(p.0)
                .wordform
                .map(|t| self.grammar.single_tags_list[t.0].hash)
                .unwrap_or(TagHash(0))
        };
        for &tter in &rr.tags_list {
            let tter = TagHash(tter);
            if (!self.cfg.show_end_tags && tter == self.cfg.endtag) || tter == self.cfg.begintag {
                continue;
            }
            if tter == baseform || tter == wordform_hash {
                continue;
            }
            let tid = tag_by_hash(self.grammar, tter);
            if self.grammar.tag_type(tid).intersects(T_WORDFORM) {
                return Some(tid);
            }
        }
        None
    }

    /// The C++ eligibility check, and more: whether the head readings of an
    /// MWE cohort can be split — every one carries a wordform tag, and no
    /// wordform tag on a reading with a sub-reading is blank inside.
    fn mwe_splittable(&self, heads: &[ReadingId]) -> bool {
        let n_wftags = heads
            .iter()
            .filter(|&&r| self.mwe_maybe_wf_tag(r).is_some())
            .count();
        if n_wftags < heads.len() {
            // "Some but not all main-readings ... not splitting." warning
            // (when n_wftags > 0): deferred.
            return false;
        }
        !self.mwe_inner_word_blank(heads)
    }

    // [spec:cg3:req:robustness.stream-text]
    /// Whether a reading that has a sub-reading carries a wordform tag whose
    /// text between `"<` and `>"` is all blank.
    ///
    /// DIVERGENCE: the C++ trims such a tag to a span that starts past its
    /// end, and the substring it takes then runs on into the closing `>"`,
    /// building a garbage wordform. There is no word to split out, so the
    /// cohort is left whole, as the C++ leaves one whose readings disagree.
    fn mwe_inner_word_blank(&self, heads: &[ReadingId]) -> bool {
        const RTRIMBLANK: &[char] = &[' ', '\n', '\r', '\t'];
        for &r in heads {
            let mut sub = Some(r);
            while let Some(s) = sub {
                let next = self.doc.store.readings.get(s.0).next;
                if next.is_some()
                    && let Some(wf) = self.mwe_maybe_wf_tag(s)
                {
                    let chars: Vec<char> =
                        self.grammar.single_tags_list[wf.0].tag.chars().collect();
                    let inner = chars.get(2..chars.len().saturating_sub(2)).unwrap_or(&[]);
                    if !inner.is_empty() && inner.iter().all(|c| RTRIMBLANK.contains(c)) {
                        return true;
                    }
                }
                sub = next;
            }
        }
        false
    }

    // [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.split-mwe-fn+1]
    // [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.split-mwe-fn+1]
    /// C++ `std::vector<Cohort*> MweSplitApplicator::splitMwe(Cohort* cohort)`.
    /// Splits one MWE cohort into a vector of new cohorts (one per component
    /// word), or returns the original cohort unchanged if it cannot/should not be
    /// split.
    pub fn mwe_split_mwe(
        &mut self,
        cohort: CohortId,
    ) -> Result<Vec<CohortId>, crate::error::RunError> {
        // rtrimblank = { ' ', '\n', '\r', '\t' }; textprefix = ":".
        const RTRIMBLANK: &[char] = &[' ', '\n', '\r', '\t'];
        const TEXTPREFIX: &str = ":";

        let mut cos: Vec<CohortId> = Vec::new();

        // Eligibility check.
        let head_readings = self.doc.store.cohorts.get(cohort.0).readings.clone();
        if !self.mwe_splittable(&head_readings) {
            cos.push(cohort);
            return Ok(cos);
        }

        #[expect(
            clippy::expect_used,
            reason = "the cohort is split while its window is printed, and a cohort in a window has a parent: alloc_cohort(Some(sw)) and append_cohort set it"
        )]
        let parent = self
            .doc
            .store
            .cohorts
            .get(cohort.0)
            .parent
            .expect("cohort parent");
        let mut pretext: String = String::new();

        for r in head_readings {
            // pos = SIZE_MAX; prev = null. (++pos wraps SIZE_MAX to 0 on the first tag.)
            let mut pos: usize = usize::MAX;
            let mut prev: Option<ReadingId> = None;

            // Walk the sub-reading chain (for sub = r; sub; sub = sub->next).
            let mut sub_opt: Option<ReadingId> = Some(r);
            while let Some(sub) = sub_opt {
                let wf_tag = self.mwe_maybe_wf_tag(sub);
                if let Some(wf_tag) = wf_tag {
                    pos = pos.wrapping_add(1);

                    // Ensure a cohort exists at index pos.
                    while cos.len() < pos + 1 {
                        let c = crate::cohort::alloc_cohort(&mut self.doc.store, Some(parent));
                        let gn = self.doc.cohorts.next_cohort_number();
                        self.doc.store.cohorts.get_mut(c.0).global_number = gn;
                        crate::single_window::append_cohort(
                            &mut self.doc.store,
                            &mut self.doc.cohorts,
                            &mut self.doc.deps,
                            parent,
                            c,
                        );
                        if !pretext.is_empty() {
                            self.doc.store.cohorts.get_mut(c.0).text = pretext.clone();
                            pretext.clear();
                        }
                        cos.push(c);
                    }
                    let c = cos[pos];

                    // Reconstruct the trimmed wordform from wfTag->tag ("<...>").
                    let has_next = self.doc.store.readings.get(sub.0).next.is_some();
                    let wf_chars: Vec<char> = self.grammar.single_tags_list[wf_tag.0]
                        .tag
                        .chars()
                        .collect();
                    let wf_beg = 2usize; // index just after "\"<"
                    let sp_beg0 = find_first_not_of(&wf_chars, RTRIMBLANK, wf_beg);
                    let sp_beg = if has_next { sp_beg0 } else { wf_beg };
                    let wf_end = wf_chars.len() - 3; // index of last content char (before ">\"")
                    let sp_end = 1 + find_last_not_of(&wf_chars, RTRIMBLANK, wf_end);
                    // wf = substr(0,wfBeg) + substr(spBeg, spEnd-spBeg) + substr(wfEnd+1).
                    let mut wf = String::new();
                    wf.extend(&wf_chars[0..wf_beg]);
                    wf.extend(&wf_chars[sp_beg..sp_end]);
                    wf.extend(&wf_chars[wf_end + 1..]);

                    // Ambiguity guard.
                    let existing_wf = self.doc.store.cohorts.get(c.0).wordform;
                    if let Some(ewf) = existing_wf {
                        let existing_text = &self.grammar.single_tags_list[ewf.0].tag;
                        if *wf != **existing_text {
                            // "Ambiguous wordform-tags for same cohort ... not splitting." deferred.
                            cos.clear();
                            cos.push(cohort);
                            return Ok(cos);
                        }
                    }
                    let wf_id = self.add_tag(&wf, crate::tag::TagType::empty())?;
                    self.doc.store.cohorts.get_mut(c.0).wordform = Some(wf_id);

                    // Blank/text handling.
                    if sp_beg > wf_beg {
                        let mid: String = wf_chars[wf_beg..sp_beg].iter().collect();
                        pretext = format!("{TEXTPREFIX}{mid}");
                    }
                    if sp_end < wf_end + 1 {
                        let mid: String = wf_chars[sp_end..wf_end + 1].iter().collect();
                        self.doc.store.cohorts.get_mut(c.0).text = format!("{TEXTPREFIX}{mid}");
                    }

                    // Reading migration: rNew = alloc_reading(*sub) (deep copy of chain).
                    let r_new = {
                        let src = clone_reading(self.doc.store.readings.get(sub.0));
                        crate::reading::alloc_reading_copy(&mut self.doc.store, &src)
                    };
                    // Erase every tag equal to wfTag->hash or rNew->parent->wordform->hash.
                    let wf_hash = self.grammar.single_tags_list[wf_tag.0].hash;
                    let new_parent_wf_hash = {
                        // rNew->parent is still `sub`'s parent (the original cohort)
                        // until reparented below; the C++ reads it before reparenting.
                        #[expect(
                            clippy::expect_used,
                            reason = "alloc_reading_copy keeps sub's parent, and sub is on the chain of a reading of this cohort (see mwe_maybe_wf_tag)"
                        )]
                        let p = self
                            .doc
                            .store
                            .readings
                            .get(r_new.0)
                            .parent
                            .expect("rNew parent");
                        self.doc
                            .store
                            .cohorts
                            .get(p.0)
                            .wordform
                            .map(|t| self.grammar.single_tags_list[t.0].hash)
                            .unwrap_or(TagHash(0))
                    };
                    {
                        let rr = self.doc.store.readings.get_mut(r_new.0);
                        let mut i = 0usize;
                        while i < rr.tags_list.len() {
                            let tter = rr.tags_list[i];
                            if tter == wf_hash.get() || tter == new_parent_wf_hash.get() {
                                rr.tags_list.remove(i);
                                rr.tags.erase(tter);
                            } else {
                                i += 1;
                            }
                        }
                    }
                    crate::cohort::append_reading(&mut self.doc.store, c, r_new);
                    self.doc.store.readings.get_mut(r_new.0).parent = Some(c);

                    // Free the leftover sub-reading chain hanging off `prev`.
                    if let Some(prev_id) = prev {
                        let leftover = self.doc.store.readings.get(prev_id.0).next;
                        crate::reading::free_reading(&mut self.doc.store, leftover);
                        self.doc.store.readings.get_mut(prev_id.0).next = None;
                    }
                    prev = Some(r_new);
                } else {
                    // prev = prev->next (prev is guaranteed non-null by eligibility).
                    #[expect(
                        clippy::expect_used,
                        reason = "mwe_splittable, checked at the top of this fn, found a wordform tag on every head reading, so the chain's first pass set prev; later passes keep it on the copy of the current sub-reading"
                    )]
                    let prev_id = prev.expect("splitMwe: null prev on wf-less sub-reading");
                    prev = self.doc.store.readings.get(prev_id.0).next;
                }

                sub_opt = self.doc.store.readings.get(sub.0).next;
            }
        }

        if cos.is_empty() {
            // "Tried splitting ..., but got no new cohorts; shouldn't happen." deferred.
            cos.push(cohort);
        }
        // cos[0] = the head reading = the LAST word: move the original text onto it.
        let orig_text = self.doc.store.cohorts.get(cohort.0).text.clone();
        self.doc.store.cohorts.get_mut(cos[0].0).text = orig_text;
        cos.reverse();
        Ok(cos)
    }

    // [spec:cg3:def:mwe-split-applicator.cg3.mwe-split-applicator.print-single-window-fn]
    // [spec:cg3:sem:mwe-split-applicator.cg3.mwe-split-applicator.print-single-window-fn]
    /// C++ `void MweSplitApplicator::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling = false)`. Prints a window,
    /// MWE-splitting every cohort on the way out via the INHERITED `printCohort`.
    ///
    /// Reached from the base `print_single_window` when `mwe_split_at_print` is
    /// set (the C++ virtual dispatch).
    pub fn mwe_print_single_window<W: Write>(
        &mut self,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), crate::error::RunError> {
        // Variables block.
        let vars_output: Vec<u32> = self
            .doc
            .store
            .single_windows
            .get(window.0)
            .variables_output
            .iter()
            .copied()
            .collect();
        for var in vars_output {
            let key_tag = {
                let tid = tag_by_hash(self.grammar, TagHash(var));
                self.grammar.single_tags_list[tid.0].tag.clone()
            };
            let value_hash: Option<u32> = {
                let w = self.doc.store.single_windows.get(window.0);
                let it = w.variables_set.find(var);
                if it != w.variables_set.end() {
                    Some(it.get().1)
                } else {
                    None
                }
            };
            match value_hash {
                Some(vh) => {
                    if vh != self.grammar.tag_any {
                        let vtid = tag_by_hash(self.grammar, TagHash(vh));
                        let _ = writeln!(
                            output,
                            "{STR_CMD_SETVAR}{}={}>",
                            key_tag, self.grammar.single_tags_list[vtid.0].tag
                        );
                    } else {
                        let _ = writeln!(output, "{STR_CMD_SETVAR}{key_tag}>");
                    }
                }
                None => {
                    let _ = writeln!(output, "{STR_CMD_REMVAR}{key_tag}>");
                }
            }
        }

        let (text, text_post, flush_after) = {
            let w = self.doc.store.single_windows.get(window.0);
            (w.text.clone(), w.text_post.clone(), w.flush_after)
        };

        if !text.is_empty() {
            self.print_plain_text_line(&text, output);
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                write_char('\n', output);
            }
        }

        // Cohorts: cs is captured BEFORE the loop (splitMwe appends mid-iteration,
        // but those new cohorts are NOT re-iterated).
        let cs = ui32(self.doc.store.single_windows.get(window.0).cohorts.len());
        for c in 0..cs {
            let cohort = self.doc.store.single_windows.get(window.0).cohorts[c as usize];
            let split = self.mwe_split_mwe(cohort)?;
            for iter in split {
                // Inherited GrammarApplicator::printCohort.
                let trace = self.cfg.trace;
                self.print_cohort(iter, output, profiling, trace);
            }
        }

        if !text_post.is_empty() {
            self.print_plain_text_line(&text_post, output);
            if !isnl(text_post.chars().next_back().unwrap_or('\0')) {
                write_char('\n', output);
            }
        }

        write_char('\n', output);
        if flush_after {
            let _ = writeln!(output, "{STR_CMD_FLUSH}");
        }
        let _ = output.flush();
        Ok(())
    }
}

/// C++ `find_first_not_of(set, pos)` over a `&[char]` — first index
/// `>= pos` whose char is NOT in `set` (else the length, mirroring the fact that
/// these tags always have non-blank content within the brackets).
fn find_first_not_of(s: &[char], set: &[char], pos: usize) -> usize {
    let mut i = pos;
    while i < s.len() && set.contains(&s[i]) {
        i += 1;
    }
    i
}

/// C++ `find_last_not_of(set, pos)` over a `&[char]` — last index
/// `<= pos` whose char is NOT in `set`.
fn find_last_not_of(s: &[char], set: &[char], pos: usize) -> usize {
    let mut i = pos as isize;
    while i >= 0 && set.contains(&s[i as usize]) {
        i -= 1;
    }
    // For the always-content-bearing MWE tags this never underflows past 0.
    i.max(0) as usize
}

// Wave 4 (w4-file-split-fmt): the verbatim Reading field-copy is
// consolidated in `crate::reading::clone_verbatim`.
use crate::reading::clone_verbatim as clone_reading;
