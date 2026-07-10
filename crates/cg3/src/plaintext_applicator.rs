//! Port of `src/PlaintextApplicator.{cpp,hpp}` — the "plaintext" input format:
//! raw text is tokenized on whitespace + leading/trailing punctuation into one
//! cohort per token, the grammar runs, and each surviving cohort's bare wordform
//! is written back (space-separated, one window per line).
//!
//! ## Composition, not inheritance
//! C++ `class PlaintextApplicator : public virtual GrammarApplicator`. The Rust
//! applicator OWNS a [`GrammarApplicator`] via `base` and forwards to it
//! (`self.base.run_grammar_on_window`, `self.base.store`, `self.base.grammar`,
//! `self.base.gWindow`). `printCohort`/`printSingleWindow`/`runGrammarOnText`
//! are reimplemented here.
//!
//! ## Reproduced quirk (DEAD-code / single-window)
//! The tokenizer sets `cCohort = null` after appending EVERY token's cohort, so
//! at the top of each subsequent line `cCohort` is null. Consequently the three
//! `cCohort`-gated break blocks (soft-delimiter break, hard break) and the
//! "empty readings → initEmptyCohort" guard NEVER fire, and `cSWindow` (once
//! set) is never nulled by them. In practice the ENTIRE plaintext stream
//! accumulates into ONE `SingleWindow` — only the `cCohort`-independent
//! soft-lookback `delimitAt` path can ever split it. This is faithfully
//! preserved (the gated blocks are ported verbatim even though they are
//! effectively dead).
//!
//! ## Engine / core mismatches (noted)
//! * ICU `u_ispunct` → `char::is_ascii_punctuation` (ASCII-only approximation —
//!   NON-ASCII punctuation like `«»¡¿` will NOT be peeled, a known parity gap
//!   vs. ICU's full Unicode punctuation classification).
//! * ICU `u_isupper` → `char::is_uppercase`; `UnicodeString::toLower()` →
//!   `str::to_lowercase` (locale-independent full Unicode lowering).
//! * `does_set_match_cohort_normal` gained a 4th `context` param (pass `None`);
//!   `add_tag` is `add_tag(&str, type)`.

use std::io::{Read, Seek, Write};

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::cohort::CT_REMOVED;
use crate::grammar::Grammar;
use crate::grammar_applicator::GrammarApplicator;
use crate::uextras::{get_line_clean, u_fflush, u_fputc, ux_strip_bom};

/// C++ `grammar->single_tags[hash]` (operator[]) — hash → `TagId`, `TagId(0)` on
/// a miss (benign; see `niceline_applicator`).
fn tag_by_hash(grammar: &Grammar, hash: u32) -> TagId {
    let it = grammar.single_tags.find(hash);
    if it != grammar.single_tags.end() {
        it.get().1
    } else {
        TagId(0)
    }
}

// [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator]
/// C++ `class PlaintextApplicator : public virtual GrammarApplicator`.
pub struct PlaintextApplicator {
    /// The composed engine base (C++ `public virtual GrammarApplicator`).
    pub base: GrammarApplicator,
    /// C++ `bool add_tags = false` — when set, magic readings get a `<cg-conv>`
    /// tag + case tags and are printed; by default readings are `noprint`.
    pub add_tags: bool,
}

impl PlaintextApplicator {
    // [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.plaintext-applicator-fn]
    // [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.plaintext-applicator-fn]
    /// C++ `PlaintextApplicator::PlaintextApplicator(std::ostream& ux_err)` —
    /// forwards `ux_err` to the base and sets `allow_magic_readings = true`.
    /// `add_tags` keeps its `false` default.
    pub fn new(mut base: GrammarApplicator) -> Self {
        base.allow_magic_readings = true;
        PlaintextApplicator { base, add_tags: false }
    }

    // [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.run-grammar-on-text-fn]
    /// C++ `void PlaintextApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`. Tokenizes raw plaintext into cohorts; overrides
    /// the base driver.
    ///
    /// PORT NOTES: same generic-handle / deferred-I/O notes as the Niceline
    /// variant. `get_line_clean` is called with `keep_tabs = false`, so TABs
    /// collapse to single spaces (matching the C++ default `keep_tabs`).
    pub fn run_grammar_on_text<R, W>(&mut self, input: &mut R, output: &mut W)
    where
        R: Read + Seek,
        W: Write,
    {
        // ux_stdin/ux_stdout, validity guards, no-delimiter warnings: deferred I/O.

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
        let mut c_reading: Option<ReadingId>;

        let mut l_swindow: Option<SwId> = None;
        let mut l_cohort: Option<CohortId> = None;

        self.base.gWindow.window_span = self.base.num_windows;

        ux_strip_bom(input);

        loop {
            lines += 1;
            let mut packoff = get_line_clean(&mut line, &mut cleaned, input, false);

            // C++ `while (!input.eof())`: a blank line (packoff == 0 but
            // `line[0]` holds the newline) is NOT end-of-stream; only a read
            // that stores nothing is. Sampled here, acted on at the bottom
            // (matches the base run_grammar_on_text driver).
            let hit_eof = packoff == 0 && line[0] == '\0';

            // Trim trailing whitespace.
            while cleaned[0] != '\0' && packoff > 0 && crate::inlines::isspace(cleaned[packoff - 1]) {
                cleaned[packoff - 1] = '\0';
                packoff -= 1;
            }

            let mut is_text = false;

            if !ignoreinput && cleaned[0] != '\0' && cleaned[0] != '<' {
                // cCohort empty-readings init (dead in practice: cCohort is null).
                if let Some(cc) = c_cohort {
                    if self.base.store.cohorts.get(cc.0).readings.is_empty() {
                        self.base.init_empty_cohort(cc);
                    }
                }

                // (a) Soft-limit lookback (the ONLY split path that can fire).
                if let Some(sw) = c_swindow {
                    let over_soft = self.base.store.single_windows.get(sw.0).cohorts.len()
                        >= self.base.soft_limit as usize;
                    if over_soft && self.base.grammar.soft_delimiters.is_some() && !did_soft_lookback
                    {
                        did_soft_lookback = true;
                        let sd = self.base.grammar.sets_list
                            [self.base.grammar.soft_delimiters.unwrap().0]
                            .number;
                        let cohorts = self.base.store.single_windows.get(sw.0).cohorts.clone();
                        for &c in cohorts.iter().rev() {
                            if self.base.does_set_match_cohort_normal(c, sd, None) {
                                did_soft_lookback = false;
                                let cohort = self.base.delimit_at(sw, c);
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

                // (b) Soft-delimiter break (DEAD: cCohort is null here).
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
                        l_cohort = Some(cc);
                        c_swindow = None;
                        c_cohort = None;
                        self.base.numCohorts += 1;
                        did_soft_lookback = false;
                    }
                }

                // (c) Hard break (DEAD: cCohort is null here).
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
                        l_cohort = Some(cc);
                        c_swindow = None;
                        c_cohort = None;
                        self.base.numCohorts += 1;
                        did_soft_lookback = false;
                    }
                }

                // New window (fires only once, on the first token line).
                if c_swindow.is_none() {
                    let sw = self
                        .base
                        .gWindow
                        .alloc_append_single_window(&mut self.base.store);
                    self.base.init_empty_single_window(sw);
                    l_swindow = Some(sw);
                    // lCohort = cSWindow->cohorts[0] (the boundary cohort).
                    l_cohort = Some(self.base.store.single_windows.get(sw.0).cohorts[0]);
                    c_swindow = Some(sw);
                    c_cohort = None;
                    self.base.numWindows += 1;
                    did_soft_lookback = false;
                }

                // Drain a window if enough queued (dead: next never grows here).
                if self.base.gWindow.next.len() > self.base.num_windows as usize {
                    self.base.gWindow.shuffle_windows_down(&mut self.base.store);
                    self.base.run_grammar_on_window(output);
                    if self.base.numWindows % reset_after == 0 {
                        self.base.reset_indexes();
                    }
                    // verbose progress: deferred.
                }

                // Raw split on spaces.
                let mut tokens_raw: Vec<String> = Vec::new();
                {
                    let mut base = 0usize;
                    let mut sp = 0usize;
                    loop {
                        while cleaned[sp] != '\0' && cleaned[sp] != ' ' {
                            sp += 1;
                        }
                        if cleaned[sp] != ' ' {
                            break;
                        }
                        cleaned[sp] = '\0';
                        if cleaned[base] != '\0' {
                            tokens_raw.push(cleaned[base..sp].iter().collect());
                        }
                        sp += 1;
                        base = sp;
                    }
                    if cleaned[base] != '\0' {
                        let mut end = base;
                        while cleaned[end] != '\0' {
                            end += 1;
                        }
                        tokens_raw.push(cleaned[base..end].iter().collect());
                    }
                }

                // Punctuation splitting.
                let mut tokens: Vec<Vec<char>> = Vec::new();
                for raw in &tokens_raw {
                    let mut p: Vec<char> = raw.chars().collect();
                    let mut start = 0usize;
                    let mut len = p.len();
                    // Peel LEADING punctuation into single-char tokens.
                    while start < p.len() && len > 0 && u_ispunct(p[start]) {
                        tokens.push(vec![p[start]]);
                        start += 1;
                        len -= 1;
                    }
                    let tkz = tokens.len();
                    // Peel TRAILING punctuation (appended in reverse order).
                    while len > 0 && u_ispunct(p[start + len - 1]) {
                        tokens.push(vec![p[start + len - 1]]);
                        p[start + len - 1] = '\0';
                        len -= 1;
                    }
                    // Insert the remaining middle token at position tkz.
                    if len > 0 && p[start] != '\0' {
                        let middle: Vec<char> = p[start..start + len].to_vec();
                        tokens.insert(tkz, middle);
                    }
                }

                // Cohort creation.
                for token in &tokens {
                    let first_upper = !token.is_empty() && u_isupper(token[0]);
                    let mut all_upper = first_upper;
                    let mut mixed_upper = false;
                    for &ch in token.iter().skip(1) {
                        if u_isupper(ch) {
                            mixed_upper = true;
                        } else {
                            all_upper = false;
                        }
                    }

                    let sw = c_swindow.unwrap();
                    let cc = crate::cohort::alloc_cohort(&mut self.base.store, Some(sw));
                    let gn = self.base.gWindow.cohort_counter;
                    self.base.gWindow.cohort_counter =
                        self.base.gWindow.cohort_counter.wrapping_add(1);
                    let token_str: String = token.iter().collect();
                    let wf_text = format!("\"<{token_str}>\"");
                    let wf = self.base.add_tag(&wf_text, crate::tag::TagType::empty());
                    {
                        let c = self.base.store.cohorts.get_mut(cc.0);
                        c.global_number = gn;
                        c.wordform = Some(wf);
                    }
                    c_cohort = Some(cc);
                    l_cohort = Some(cc);
                    self.base.numCohorts += 1;
                    let cr = self.base.init_empty_cohort(cc);
                    c_reading = Some(cr);
                    self.base.store.readings.get_mut(cr.0).noprint = !self.add_tags;
                    if self.add_tags {
                        let tag = self.base.add_tag("<cg-conv>", crate::tag::TagType::empty());
                        self.base.add_tag_to_reading(cr, tag);
                    }
                    if self.add_tags && (first_upper || all_upper || mixed_upper) {
                        let baseform = self.base.store.readings.get(cr.0).baseform;
                        self.base.del_tag_from_reading_hash(cr, baseform);
                        let lowered: String = token_str.to_lowercase();
                        let base_tag_text = format!("\"{lowered}\"");
                        let bt = self.base.add_tag(&base_tag_text, crate::tag::TagType::empty());
                        self.base.add_tag_to_reading(cr, bt);
                        if all_upper {
                            let t = self.base.add_tag("<all-upper>", crate::tag::TagType::empty());
                            self.base.add_tag_to_reading(cr, t);
                        }
                        if first_upper {
                            let t = self.base.add_tag("<first-upper>", crate::tag::TagType::empty());
                            self.base.add_tag_to_reading(cr, t);
                        }
                        if mixed_upper && !all_upper {
                            let t = self.base.add_tag("<mixed-upper>", crate::tag::TagType::empty());
                            self.base.add_tag_to_reading(cr, t);
                        }
                    }
                    crate::single_window::append_cohort(
                        &mut self.base.gWindow,
                        &mut self.base.store,
                        sw,
                        cc,
                    );
                    c_cohort = None;
                }
            } else {
                is_text = true;
            }

            if is_text {
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

            // Loop termination: the C++ `while(!input.eof())` re-check at the
            // top of the loop, using the EOF state sampled after get_line_clean.
            if hit_eof {
                break;
            }
        }

        self.base.input_eof = true;

        // Finalization (in practice cCohort is null → this is unreached).
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
            let t = Some(tmp);
            crate::single_window::free_swindow(&mut self.base.gWindow, &mut self.base.store, t);
            self.base.gWindow.previous.remove(0);
        }

        u_fflush(output);
    }

    // [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.print-cohort-fn]
    // [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-cohort-fn]
    /// C++ `void PlaintextApplicator::printCohort(Cohort* cohort,
    /// std::ostream& output, bool)`. Prints the bare wordform + trailing space;
    /// boundary cohort (local_number 0) and `CT_REMOVED` cohorts print nothing.
    pub fn print_cohort<W: Write>(&mut self, cohort: CohortId, output: &mut W, _profiling: bool) {
        let (local_number, removed, wf) = {
            let c = self.base.store.cohorts.get(cohort.0);
            (c.local_number, c.r#type.intersects(CT_REMOVED), c.wordform)
        };
        if local_number == 0 {
            return;
        }
        if removed {
            return;
        }
        // "%.*S " of wordform.data()+2 for size()-4 → strip "\"<" and ">\"", plus a space.
        let inner = {
            let tag = &self.base.grammar.single_tags_list[wf.expect("cohort wordform").0].tag;
            strip_wordform_brackets(tag)
        };
        let _ = write!(output, "{inner} ");
    }

    // [spec:cg3:def:plaintext-applicator.cg3.plaintext-applicator.print-single-window-fn]
    // [spec:cg3:sem:plaintext-applicator.cg3.plaintext-applicator.print-single-window-fn]
    /// C++ `void PlaintextApplicator::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling = false)`. One line of
    /// space-separated wordforms; `window->text`/`text_post` are NOT emitted.
    pub fn print_single_window<W: Write>(
        &mut self,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        let all_cohorts = self.base.store.single_windows.get(window.0).all_cohorts.clone();
        for cohort in all_cohorts {
            self.print_cohort(cohort, output, profiling);
        }
        u_fputc('\n', output);
        u_fflush(output);
    }
}

/// ICU `u_ispunct` approximation — ASCII punctuation only (parity gap: ICU
/// classifies the full Unicode punctuation categories; non-ASCII punctuation is
/// NOT peeled here).
fn u_ispunct(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// ICU `u_isupper` → Rust `char::is_uppercase` (full Unicode uppercase).
fn u_isupper(c: char) -> bool {
    c.is_uppercase()
}

/// `wordform.data()+2` for `size()-4` — strip the leading `"<` and trailing `>"`
/// of a wordform stored as `"<word>"`, leaving `word`.
fn strip_wordform_brackets(tag: &str) -> String {
    let chars: Vec<char> = tag.chars().collect();
    if chars.len() < 4 {
        return String::new();
    }
    chars[2..chars.len() - 2].iter().collect()
}
