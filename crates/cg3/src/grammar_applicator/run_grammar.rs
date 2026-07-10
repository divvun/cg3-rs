//! `src/GrammarApplicator_runGrammar.cpp` impl of `GrammarApplicator`.
//!
//! The top-level CG stream driver ([`run_grammar_on_text`](super::GrammarApplicator::run_grammar_on_text))
//! plus the two window-boundary builders
//! ([`init_empty_single_window`](super::GrammarApplicator::init_empty_single_window),
//! [`init_empty_cohort`](super::GrammarApplicator::init_empty_cohort)) and the free
//! helper [`test_string_against`].
//!
//! ## I/O model
//! C++ `std::istream& input` / `std::ostream& output` become generic Rust handles
//! passed as PARAMS: `input: &mut R` where `R: Read + Seek` (Seek is needed by
//! `ux_strip_bom`) and `output: &mut W` where `W: Write`. The `ux_stdin`/
//! `ux_stdout`/`ux_stderr` struct fields are `Option<()>` placeholders, so the
//! streams are NOT stored into them; the good()/eof()/output/grammar validity
//! guards (each a `CG3Quit(1)` + `ux_stderr` diagnostic) and every verbose
//! `u_fprintf(ux_stderr,…)` are deferred with the I/O layer, but their
//! control-flow effects are reproduced faithfully.
//!
//! `line`/`cleaned` are `Vec<char>` scratch buffers (what `get_line_clean`
//! expects); the C++ `UChar*` pointer walks over `cleaned`/`line` are translated
//! to `usize` indices over those buffers using the ported `skip*` helpers.

// [spec:cg3:def:grammar-applicator-run-grammar.cg3.test-string-against-fn]
// [spec:cg3:sem:grammar-applicator-run-grammar.cg3.test-string-against-fn]
/// C++ free fn `inline bool testStringAgainst(const UString& str,
/// std::vector<URegularExpression*>& rxs)`.
///
/// Tests whether `str` matches any of the pre-compiled regexes in `rxs`, with a
/// move-to-front (MRU) reordering side effect. The ICU `uregex_find(rx, -1,
/// &status)` (start index -1 = "whole region", UNANCHORED) maps to the `regex`
/// crate's [`Regex::is_match`], which is likewise unanchored. On the first hit,
/// if it was not already at index 0, the matching regex is swapped to the front
/// so it is tried first next time. The ICU `CG3Quit(1)`-on-error branches have no
/// analog (`is_match` is infallible), so they are dropped. Used by
/// `run_grammar_on_text` to detect text-delimiter lines via `text_delimiters`.
pub fn test_string_against(str: &str, rxs: &mut Vec<regex::Regex>) -> bool {
    let mut rv = false;
    for i in 0..rxs.len() {
        // uregex_setText + uregex_find(-1) — unanchored whole-string search.
        if rxs[i].is_match(str) {
            rv = true;
            if i != 0 {
                // Move the matching regex up front for faster future hits.
                rxs.swap(0, i);
            }
            break;
        }
    }
    rv
}

/// C++ `u_strchr(s, needle)` over a `Vec<char>` scratch buffer: return the index
/// of the first `needle` at or after `from`, scanning up to (not past) the NUL
/// terminator, or `None`. Used by the inline SETVAR/REMVAR pointer walks.
fn u_strchr(buf: &[char], from: usize, needle: char) -> Option<usize> {
    let mut i = from;
    while buf[i] != '\0' {
        if buf[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Collect the live `(key, value)` pairs from a `uint32FlatHashMap` slot table
/// (skipping the `EMPTY`/`DEL` sentinel slots), for the C++
/// `dst.insert(src.begin(), src.end())` range-inserts of `adopt_variables`.
fn live_map_pairs(m: &mut crate::flat_unordered_map::Uint32FlatHashMap) -> Vec<(u32, u32)> {
    m.get()
        .iter()
        .copied()
        .filter(|&(k, _)| k != u32::MAX && k != u32::MAX - 1)
        .collect()
}

/// Collect the live values from a `uint32FlatHashSet` slot table (skipping the
/// `EMPTY`/`DEL` sentinels), for the range-insert of `adopt_variables`.
fn live_set_values(s: &mut crate::flat_unordered_set::Uint32FlatHashSet) -> Vec<u32> {
    s.get()
        .iter()
        .copied()
        .filter(|&v| v != u32::MAX && v != u32::MAX - 1)
        .collect()
}

/// Control-flow result returned by [`got_reading`](super::GrammarApplicator::got_reading)
/// reproducing the C++ `got_reading:` block's non-local exits: a plain fall-through,
/// a `continue` (sub-reading-with-existing-next skip), or a `goto istext` (the
/// "looked like a reading but wasn't" fallback).
enum GotReading {
    /// Fell through the block normally.
    Normal,
    /// C++ `continue;` — abort this line, `++numLines` at the bottom of the loop.
    Continue,
    /// C++ `goto istext;` — treat the line as text.
    Istext,
}

impl super::GrammarApplicator {
    // [spec:cg3:def:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-single-window-fn]
    // [spec:cg3:sem:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-single-window-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.init-empty-single-window-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.init-empty-single-window-fn]
    /// C++ `void GrammarApplicator::initEmptySingleWindow(SingleWindow*
    /// cSWindow)`. Builds the leading `>>>` boundary cohort for a fresh
    /// SingleWindow: a cohort at `global_number = gWindow->cohort_counter++` with
    /// wordform `tag_begin`, holding one reading whose baseform is `begintag`, the
    /// ANY set flagged on the cohort, and the `begintag` tag added. Touches no
    /// numReadings/numCohorts counters.
    pub fn init_empty_single_window(&mut self, c_swindow: crate::arena::SwId) {
        let c_cohort = crate::cohort::alloc_cohort(&mut self.store, Some(c_swindow));
        // cCohort->global_number = gWindow->cohort_counter++;
        let gn = self.gWindow.cohort_counter;
        self.gWindow.cohort_counter = self.gWindow.cohort_counter.wrapping_add(1);
        {
            let c = self.store.cohorts.get_mut(c_cohort.0);
            c.global_number = gn;
            c.wordform = self.tag_begin;
        }
        let c_reading = crate::reading::alloc_reading(&mut self.store, Some(c_cohort));
        self.store.readings.get_mut(c_reading.0).baseform = self.begintag;
        // insert_if_exists(cReading->parent->possible_sets, grammar->sets_any);
        // cReading->parent == c_cohort.
        crate::inlines::insert_if_exists(
            &mut self.store.cohorts.get_mut(c_cohort.0).possible_sets,
            self.grammar.sets_any.as_ref(),
        );
        // addTagToReading(*cReading, begintag);  [uint32_t overload —
        // resolves the hash via grammar->single_tags[hash], then the Tag* form]
        let begin_tag_id = super::core::tag_by_hash(&self.grammar, self.begintag);
        self.add_tag_to_reading(c_reading, begin_tag_id);
        // cCohort->appendReading(cReading);
        crate::cohort::append_reading(&mut self.store, c_cohort, c_reading);
        // cSWindow->appendCohort(cCohort);
        crate::single_window::append_cohort(
            &mut self.gWindow,
            &mut self.store,
            c_swindow,
            c_cohort,
        );
        // C++ SingleWindow::appendCohort: if (cohort->dep_self)
        // parent->parent->dep_highest_seen = cohort->dep_self;
        {
            let ds = self.store.cohorts.get(c_cohort.0).dep_self;
            if ds != 0 {
                self.dep_highest_seen = ds;
            }
        }
    }

    // [spec:cg3:def:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-cohort-fn]
    // [spec:cg3:sem:grammar-applicator-run-grammar.cg3.grammar-applicator.init-empty-cohort-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.init-empty-cohort-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.init-empty-cohort-fn]
    /// C++ `Reading* GrammarApplicator::initEmptyCohort(Cohort& cCohort)`. Gives a
    /// reading-less cohort a single "magic" placeholder reading and returns it:
    /// baseform from `makeBaseFromWord(wordform)->hash` (when `allow_magic_readings`)
    /// else `wordform->hash`, ANY set flagged, the wordform tag added, `noprint =
    /// true`, appended, and `++numReadings`.
    pub fn init_empty_cohort(&mut self, c_cohort: crate::arena::CohortId) -> crate::arena::ReadingId {
        let c_reading = crate::reading::alloc_reading(&mut self.store, Some(c_cohort));
        // cCohort.wordform is dereferenced unconditionally (`->hash`).
        let wordform = self
            .store
            .cohorts
            .get(c_cohort.0)
            .wordform
            .expect("initEmptyCohort: cohort has no wordform");
        if self.allow_magic_readings {
            // baseform = makeBaseFromWord(cCohort.wordform)->hash
            let base = self.make_base_from_word(wordform);
            let h = self.grammar.single_tags_list.get(base.0).hash;
            self.store.readings.get_mut(c_reading.0).baseform = h;
        } else {
            // baseform = cCohort.wordform->hash
            let h = self.grammar.single_tags_list.get(wordform.0).hash;
            self.store.readings.get_mut(c_reading.0).baseform = h;
        }
        crate::inlines::insert_if_exists(
            &mut self.store.cohorts.get_mut(c_cohort.0).possible_sets,
            self.grammar.sets_any.as_ref(),
        );
        // addTagToReading(*cReading, cCohort.wordform);  [Tag* overload]
        self.add_tag_to_reading(c_reading, wordform);
        self.store.readings.get_mut(c_reading.0).noprint = true;
        // cCohort.appendReading(cReading);
        crate::cohort::append_reading(&mut self.store, c_cohort, c_reading);
        self.numReadings = self.numReadings.wrapping_add(1);
        c_reading
    }

    /// The C++ `got_reading:` GOTO LABEL body from `runGrammarOnText` (the block
    /// entered by both the ` "` reading line — falling through — and the `; "`
    /// deleted-reading line — via `goto got_reading`). Restructured into a helper
    /// called from both branches. `is_deleted` chooses the reading vs. deleted
    /// target list (C++ `readings = &cCohort->readings` vs `&cCohort->deleted`).
    /// Returns a [`GotReading`] signalling the block's non-local exits so the
    /// caller can reproduce the `continue` / `goto istext` control flow.
    #[allow(clippy::too_many_arguments)]
    fn got_reading(
        &mut self,
        cleaned: &mut Vec<char>,
        line: &mut Vec<char>,
        indents: &mut Vec<(usize, crate::arena::ReadingId)>,
        all_mappings: &mut super::all_mappings_t,
        variables_set: &mut crate::flat_unordered_map::Uint32FlatHashMap,
        variables_rem: &mut crate::flat_unordered_set::Uint32FlatHashSet,
        variables_output: &mut crate::sorted_vector::uint32SortedVector,
        c_swindow: &mut Option<crate::arena::SwId>,
        c_cohort: crate::arena::CohortId,
        l_swindow: &mut Option<crate::arena::SwId>,
        did_soft_lookback: &mut bool,
        is_deleted: bool,
    ) -> GotReading {
        // Count current indent level
        let mut indent = 0usize;
        while crate::inlines::isspace(line[indent]) {
            indent += 1;
        }
        while !indents.is_empty() && indent <= indents.last().unwrap().0 {
            indents.pop();
        }
        let c_reading: crate::arena::ReadingId;
        if !indents.is_empty() && indent > indents.last().unwrap().0 {
            let back = indents.last().unwrap().1;
            if self.store.readings.get(back.0).next.is_some() {
                // "Sub-reading … will be ignored and lost …": deferred emission.
                return GotReading::Continue;
            }
            let parent = self.store.readings.get(back.0).parent;
            let cr = crate::reading::Reading::allocate_reading(&mut self.store, parent);
            self.store.readings.get_mut(back.0).next = Some(cr);
            c_reading = cr;
        } else {
            c_reading = crate::reading::alloc_reading(&mut self.store, Some(c_cohort));
        }
        // insert_if_exists(cReading->parent->possible_sets, grammar->sets_any);
        let parent_cid = self
            .store
            .readings
            .get(c_reading.0)
            .parent
            .expect("reading has no parent cohort");
        crate::inlines::insert_if_exists(
            &mut self.store.cohorts.get_mut(parent_cid.0).possible_sets,
            self.grammar.sets_any.as_ref(),
        );
        let wordform = self.store.cohorts.get(c_cohort.0).wordform.unwrap();
        self.add_tag_to_reading(c_reading, wordform);

        // UChar* space = &cleaned[1]; UChar* base = space;
        let mut space = 1usize;
        let mut base = space;
        if cleaned[space] == '"' {
            space += 1;
            crate::inlines::skipto_nospan(cleaned, &mut space, '"');
            crate::inlines::skiptows(cleaned, &mut space, '\0', true, true);
            space -= 1;
        }

        // Retry without escaping, to catch baseforms that have \ before the last "
        if cleaned[space] != '"' {
            space = base;
            space += 1;
            crate::inlines::skipto_nospan_raw(cleaned, &mut space, '"');
            crate::inlines::skiptows(cleaned, &mut space, '\0', true, true);
            space -= 1;
        }

        // This does not consider wordforms as invalid readings since chained
        // CG-3 may produce such
        if cleaned[space] != '"' {
            // "looked like a reading but wasn't - treated as text": deferred.
            if !indents.is_empty() && self.store.readings.get(indents.last().unwrap().1 .0).next == Some(c_reading) {
                self.store.readings.get_mut(indents.last().unwrap().1 .0).next = None;
            }
            let mut cr = Some(c_reading);
            crate::reading::free_reading(&mut self.store, &mut cr);
            if is_deleted {
                cleaned.insert(0, ';');
                line.insert(0, ';');
            }
            return GotReading::Istext;
        }

        self.store.readings.get_mut(c_reading.0).deleted = is_deleted;

        // while (space && (space = u_strchr(space, ' ')) != 0) { … }
        // Loop over each space-delimited [base .. space) tag region.
        loop {
            match u_strchr(cleaned, space, ' ') {
                None => break,
                Some(sp) => {
                    space = sp;
                    cleaned[space] = '\0';
                    space += 1;
                    if base < cleaned.len() && cleaned[base] != '\0' {
                        let base_text: String =
                            cleaned[base..].iter().take_while(|&&c| c != '\0').collect();
                        let tag = self.add_tag(&base_text, 0);
                        let (ttype, first_char) = {
                            let t = &self.grammar.single_tags_list[tag.0];
                            (t.r#type, t.tag.chars().next().unwrap_or('\0'))
                        };
                        if ttype & crate::tag::T_MAPPING != 0
                            || first_char == self.grammar.mapping_prefix
                        {
                            // tag->type |= T_MAPPING;
                            self.grammar.single_tags_list[tag.0].r#type |= crate::tag::T_MAPPING;
                            all_mappings.entry(c_reading).or_default().push(tag);
                        } else {
                            self.add_tag_to_reading(c_reading, tag);
                        }
                    }
                    base = space;
                    if cleaned[space] == '"' {
                        space += 1;
                        crate::inlines::skipto_nospan(cleaned, &mut space, '"');
                    }
                }
            }
        }
        if base < cleaned.len() && cleaned[base] != '\0' {
            let base_text: String = cleaned[base..].iter().take_while(|&&c| c != '\0').collect();
            let tag = self.add_tag(&base_text, 0);
            let (ttype, first_char) = {
                let t = &self.grammar.single_tags_list[tag.0];
                (t.r#type, t.tag.chars().next().unwrap_or('\0'))
            };
            if ttype & crate::tag::T_MAPPING != 0 || first_char == self.grammar.mapping_prefix {
                self.grammar.single_tags_list[tag.0].r#type |= crate::tag::T_MAPPING;
                all_mappings.entry(c_reading).or_default().push(tag);
            } else {
                self.add_tag_to_reading(c_reading, tag);
            }
        }
        if self.store.readings.get(c_reading.0).baseform == 0 {
            // "Line %u had no valid baseform.": deferred emission.
        }
        if indents.is_empty() || indent <= indents.last().unwrap().0 {
            // cCohort->appendReading(cReading, *readings);
            if is_deleted {
                self.append_reading_deleted(c_cohort, c_reading);
            } else {
                crate::cohort::append_reading(&mut self.store, c_cohort, c_reading);
            }
        } else {
            if let Some(mlist) = all_mappings.get_mut(&c_reading) {
                while mlist.len() > 1 {
                    // "Sub-reading mapping … will be discarded.": deferred.
                    mlist.pop();
                }
                let mut ml = all_mappings.remove(&c_reading).unwrap();
                self.split_mappings(&mut ml, c_cohort, c_reading, true);
            }
            // readings->back()->rehash();
            let list_back = if is_deleted {
                self.store.cohorts.get(c_cohort.0).deleted.last().copied()
            } else {
                self.store.cohorts.get(c_cohort.0).readings.last().copied()
            };
            if let Some(b) = list_back {
                let grammar = std::mem::take(&mut self.grammar);
                crate::reading::reading_rehash(&mut self.store, &grammar, b);
                self.grammar = grammar;
            }
        }
        indents.push((indent, c_reading));
        self.numReadings += 1;

        // Check whether the cohort still belongs to the window, as per --dep-delimit
        let dep_self = self.store.cohorts.get(c_cohort.0).dep_self;
        if !is_deleted
            && self.dep_delimit != 0
            && self.dep_highest_seen != 0
            && (dep_self <= self.dep_highest_seen
                || dep_self.wrapping_sub(self.dep_highest_seen) > self.dep_delimit)
        {
            let gn = self.store.cohorts.get(c_cohort.0).global_number;
            self.reflow_dependency_window(gn);

            let cur = *c_swindow;
            if let Some(sw) = cur {
                let last_cohort = *self.store.single_windows.get(sw.0).cohorts.last().unwrap();
                let rs = self.store.cohorts.get(last_cohort.0).readings.clone();
                for r in rs {
                    let tid = super::core::tag_by_hash(&self.grammar, self.endtag);
                    self.add_tag_to_reading(r, tid);
                }
            }

            let nsw = self.gWindow.alloc_append_single_window(&mut self.store);
            self.init_empty_single_window(nsw);
            *c_swindow = Some(nsw);

            // cSWindow->variables_set = variables_set; variables_set.clear(); …
            {
                let sww = self.store.single_windows.get_mut(nsw.0);
                sww.variables_set.clear(0);
                sww.variables_rem.clear(0);
                sww.variables_output.clear();
            }
            let vs = live_map_pairs(variables_set);
            let vr = live_set_values(variables_rem);
            let vo: Vec<u32> = variables_output.as_slice().to_vec();
            {
                let sww = self.store.single_windows.get_mut(nsw.0);
                for (k, v) in vs {
                    sww.variables_set.insert((k, v));
                }
                for v in vr {
                    sww.variables_rem.insert(v);
                }
                for v in vo {
                    sww.variables_output.insert(v);
                }
            }
            variables_set.clear(0);
            variables_rem.clear(0);
            variables_output.clear();

            *l_swindow = Some(nsw);
            self.numWindows += 1;
            *did_soft_lookback = false;
            self.dep_highest_seen = 0;

            if self.grammar.has_bag_of_tags {
                // Slow and not 100% correct as it doesn't remove tags from prev.
                self.store.cohorts.get_mut(c_cohort.0).parent = *c_swindow;
                let rs = self.store.cohorts.get(c_cohort.0).readings.clone();
                for rit in rs {
                    self.reflow_reading(rit);
                }
            }
        }

        GotReading::Normal
    }

    /// C++ `cCohort->appendReading(cReading, cCohort->deleted)` — the 2-arg
    /// overload targeting the `deleted` list. Reproduces `Cohort::appendReading`
    /// directly (the member list lives inside the `cohorts` arena, so the store
    /// is split to touch the cohort's `deleted` field and the `Reading` at once).
    fn append_reading_deleted(&mut self, this: crate::arena::CohortId, read: crate::arena::ReadingId) {
        let crate::store::RuntimeStore { cohorts, readings, .. } = &mut self.store;
        let cohort = cohorts.get_mut(this.0);
        cohort.deleted.push(read);
        let sz = cohort.deleted.len();
        if readings.get(read.0).number == 0 {
            readings.get_mut(read.0).number =
                crate::inlines::ui32(sz.wrapping_mul(1000).wrapping_add(1000));
        }
        cohort.r#type &= !crate::cohort::CT_NUM_CURRENT;
    }

    // [spec:cg3:def:grammar-applicator-run-grammar.cg3.grammar-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:grammar-applicator-run-grammar.cg3.grammar-applicator.run-grammar-on-text-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-grammar-on-text-fn]
    /// C++ `void GrammarApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`. The main CG stream driver: reads the "VISL CG-3"
    /// text format line by line, builds Windows -> Cohorts -> Readings, runs the
    /// grammar window-by-window as enough windows accumulate, and writes results.
    ///
    /// See the module header for the I/O model and the deferred-diagnostic list.
    // `c_reading`/`l_reading` mirror the C++ `cReading`/`lReading` locals — kept
    // for a faithful 1:1 port though their reads are limited in this driver.
    #[allow(unused_assignments, unused_variables)]
    pub fn run_grammar_on_text<R, W>(&mut self, input: &mut R, output: &mut W)
    where
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        // ux_stdin = &input; ux_stdout = &output;  (elided: Option<()> placeholders)
        // The good()/eof()/output/grammar validity checks (each CG3Quit(1) with a
        // u_fprintf diagnostic) are deferred with the I/O layer.
        // No-hard/soft-delimiter warnings: deferred I/O (grammar->delimiters etc.).

        let mut line: Vec<char> = vec!['\0'; 1024];
        let mut cleaned: Vec<char> = vec!['\0'; line.len() + 1];
        let mut ignoreinput = false;
        let mut did_soft_lookback = false;
        let mut is_deleted;

        self.index();

        let reset_after: u32 = (self.num_windows + 4) * 2 + 1;
        let mut lines: u32 = 0;

        let mut c_swindow: Option<crate::arena::SwId> = None;
        let mut c_cohort: Option<crate::arena::CohortId> = None;
        let mut c_reading: Option<crate::arena::ReadingId> = None;

        let mut l_swindow: Option<crate::arena::SwId> = None;
        let mut l_cohort: Option<crate::arena::CohortId> = None;
        let mut l_reading: Option<crate::arena::ReadingId> = None;

        self.gWindow.window_span = self.num_windows;

        let mut variables_set = crate::flat_unordered_map::Uint32FlatHashMap::new();
        let mut variables_rem = crate::flat_unordered_set::Uint32FlatHashSet::new();
        let mut variables_output = crate::sorted_vector::uint32SortedVector::new();

        let mut indents: Vec<(usize, crate::arena::ReadingId)> = Vec::new();
        let mut all_mappings: super::all_mappings_t = super::all_mappings_t::new();

        crate::uextras::ux_strip_bom(input);

        // binary_maybe_window() [inlined; the C++ lambda captures cSWindow/lSWindow]
        if self.fmt_output == super::cg3_sformat::CG3SF_BINARY {
            let sw = self.gWindow.alloc_append_single_window(&mut self.store);
            self.init_empty_single_window(sw);
            c_swindow = Some(sw);
            l_swindow = Some(sw);
        }

        // C++ `while (!input.eof())`: the port loops until get_line_clean reports
        // no more progress (the `packoff == 0` check at the bottom).
        'mainloop: loop {
            lines += 1;
            let mut packoff =
                crate::uextras::get_line_clean(&mut line, &mut cleaned, input, false);

            // C++ `while (!input.eof())`: eofbit is set when a read attempt hits
            // end-of-stream. `u_fgets` distinguishes a blank line (packoff == 0
            // but `line[0]` holds the newline) from true EOF (nothing stored, so
            // `line[0]` keeps the '\0' it was reset to) — only the latter ends
            // the loop. Sampled here, acted on at the bottom of the iteration.
            let hit_eof = packoff == 0 && line[0] == '\0';

            // Trim trailing whitespace from `cleaned`.
            while cleaned[0] != '\0' && packoff > 0 && crate::inlines::isspace(cleaned[packoff - 1]) {
                cleaned[packoff - 1] = '\0';
                packoff -= 1;
            }

            let mut is_text = false;
            if ignoreinput {
                is_text = true;
            } else if cleaned[0] == '"' && cleaned[1] == '<' {
                // (1) Cohort line: scan `space` forward to the terminating `>"`.
                let mut space = 0usize;
                if cleaned[space] == '"' && cleaned[space + 1] == '<' {
                    space += 1;
                    crate::inlines::skipto_nospan(&cleaned, &mut space, '"');
                    while cleaned[space] != '\0' && cleaned[space - 1] != '>' {
                        space += 1;
                        crate::inlines::skipto_nospan(&cleaned, &mut space, '"');
                    }
                    crate::inlines::skiptows(&cleaned, &mut space, '\0', true, true);
                    space -= 1;
                }
                if cleaned[space] != '"' || cleaned[space - 1] != '>' {
                    // "looked like a cohort but wasn't - treated as text": deferred.
                    is_text = true;
                } else {
                    cleaned[space + 1] = '\0';

                    // If a pending cCohort has no readings, init it empty.
                    if let Some(cc) = c_cohort {
                        if self.store.cohorts.get(cc.0).readings.is_empty() {
                            self.init_empty_cohort(cc);
                        }
                    }

                    // (a) Soft-limit lookback.
                    if let Some(sw) = c_swindow {
                        let over_soft = self.store.single_windows.get(sw.0).cohorts.len()
                            >= self.soft_limit as usize;
                        if over_soft && self.grammar.soft_delimiters.is_some() && !did_soft_lookback {
                            did_soft_lookback = true;
                            let sd = self.grammar.sets_list
                                [self.grammar.soft_delimiters.unwrap().0]
                                .number;
                            let cohorts = self.store.single_windows.get(sw.0).cohorts.clone();
                            for &c in cohorts.iter().rev() {
                                if self.does_set_match_cohort_normal(c, sd, None) {
                                    did_soft_lookback = false;
                                    let cohort = self.delimit_at(sw, c);
                                    // cSWindow = cohort->parent->next;
                                    let parent = self.store.cohorts.get(cohort.0).parent.unwrap();
                                    c_swindow = self.store.single_windows.get(parent.0).next;
                                    if let Some(cc) = c_cohort {
                                        self.store.cohorts.get_mut(cc.0).parent = c_swindow;
                                    }
                                    // verbose soft-limit warning: deferred.
                                    break;
                                }
                            }
                        }
                    }

                    // (b) Soft-delimiter on the current cohort.
                    if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
                        let over_soft = self.store.single_windows.get(sw.0).cohorts.len()
                            >= self.soft_limit as usize;
                        let sd_hit = over_soft && self.grammar.soft_delimiters.is_some() && {
                            let sd = self.grammar.sets_list
                                [self.grammar.soft_delimiters.unwrap().0]
                                .number;
                            self.does_set_match_cohort_normal(cc, sd, None)
                        };
                        if sd_hit {
                            // verbose soft-limit warning: deferred.
                            let rs = self.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let tid = super::core::tag_by_hash(&self.grammar, self.endtag);
                                self.add_tag_to_reading(r, tid);
                            }
                            self.split_all_mappings(&mut all_mappings, cc, true);
                            crate::single_window::append_cohort(
                                &mut self.gWindow,
                                &mut self.store,
                                sw,
                                cc,
                            );
                            // C++ SingleWindow::appendCohort: if (cohort->dep_self)
                            // parent->parent->dep_highest_seen = cohort->dep_self;
                            {
                                let ds = self.store.cohorts.get(cc.0).dep_self;
                                if ds != 0 {
                                    self.dep_highest_seen = ds;
                                }
                            }
                            self.store.cohorts.get_mut(cc.0).line_number = self.numLines;
                            l_swindow = Some(sw);
                            c_swindow = None;
                            c_cohort = None;
                            self.numCohorts += 1;
                            did_soft_lookback = false;
                        }
                    }

                    // (c) Hard break.
                    if let Some(cc) = c_cohort {
                        let sw = c_swindow.unwrap();
                        let over_hard = self.store.single_windows.get(sw.0).cohorts.len()
                            >= self.hard_limit as usize;
                        let delim_hit = self.dep_delimit == 0
                            && self.grammar.delimiters.is_some()
                            && {
                                let d = self.grammar.sets_list
                                    [self.grammar.delimiters.unwrap().0]
                                    .number;
                                self.does_set_match_cohort_normal(cc, d, None)
                            };
                        if over_hard || delim_hit {
                            // (!is_conv && over_hard) "Hard limit ... forcing break": deferred.
                            let rs = self.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let tid = super::core::tag_by_hash(&self.grammar, self.endtag);
                                self.add_tag_to_reading(r, tid);
                            }
                            self.split_all_mappings(&mut all_mappings, cc, true);
                            crate::single_window::append_cohort(
                                &mut self.gWindow,
                                &mut self.store,
                                sw,
                                cc,
                            );
                            // C++ SingleWindow::appendCohort: if (cohort->dep_self)
                            // parent->parent->dep_highest_seen = cohort->dep_self;
                            {
                                let ds = self.store.cohorts.get(cc.0).dep_self;
                                if ds != 0 {
                                    self.dep_highest_seen = ds;
                                }
                            }
                            self.store.cohorts.get_mut(cc.0).line_number = self.numLines;
                            l_swindow = Some(sw);
                            c_swindow = None;
                            c_cohort = None;
                            self.numCohorts += 1;
                            did_soft_lookback = false;
                        }
                    }

                    // No current window: allocate + init a fresh one.
                    if c_swindow.is_none() {
                        let sw = self.gWindow.alloc_append_single_window(&mut self.store);
                        self.init_empty_single_window(sw);
                        l_swindow = Some(sw);
                        c_swindow = Some(sw);
                        c_cohort = None;
                        self.numWindows += 1;
                        did_soft_lookback = false;
                    }

                    // Pending cCohort: split mappings + append it.
                    if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
                        self.split_all_mappings(&mut all_mappings, cc, true);
                        crate::single_window::append_cohort(
                            &mut self.gWindow,
                            &mut self.store,
                            sw,
                            cc,
                        );
                        // C++ SingleWindow::appendCohort: if (cohort->dep_self)
                        // parent->parent->dep_highest_seen = cohort->dep_self;
                        {
                            let ds = self.store.cohorts.get(cc.0).dep_self;
                            if ds != 0 {
                                self.dep_highest_seen = ds;
                            }
                        }
                    }

                    // Drain a window if enough have queued up.
                    if self.gWindow.next.len() > (self.num_windows + 1) as usize {
                        self.shuffle_windows_down();
                        
                        self.run_grammar_on_window(output);
                        if self.numWindows % reset_after == 0 {
                            self.reset_indexes();
                        }
                        // verbose progress: deferred.
                    }

                    // First real cohort of this window → adopt the pending variables.
                    let sw = c_swindow.unwrap();
                    if self.store.single_windows.get(sw.0).all_cohorts.len() == 1 {
                        self.adopt_variables(
                            sw,
                            &mut variables_set,
                            &mut variables_rem,
                            &mut variables_output,
                        );
                    }

                    // Allocate the new cCohort.
                    let cc = crate::cohort::alloc_cohort(&mut self.store, Some(sw));
                    let gn = self.gWindow.cohort_counter;
                    self.gWindow.cohort_counter = self.gWindow.cohort_counter.wrapping_add(1);
                    // wordform = addTag(&cleaned[0]) (up to the NUL at space+1).
                    let wf_text: String =
                        cleaned.iter().take_while(|&&c| c != '\0').collect();
                    let wf = self.add_tag(&wf_text, 0);
                    {
                        let c = self.store.cohorts.get_mut(cc.0);
                        c.global_number = gn;
                        c.wordform = Some(wf);
                    }
                    c_cohort = Some(cc);
                    l_cohort = Some(cc);
                    l_reading = None;
                    indents.clear();
                    self.numCohorts += 1;
                    self.store.cohorts.get_mut(cc.0).line_number = self.numLines;

                    // Trailing word-level tags after the wordform → build `wread`.
                    space += 2;
                    if cleaned[space] != '\0' {
                        let wread = crate::reading::alloc_reading(&mut self.store, Some(cc));
                        self.store.cohorts.get_mut(cc.0).wread = Some(wread);
                        self.add_tag_to_reading(wread, wf);
                        while cleaned[space] != '\0' {
                            crate::inlines::skipws(&cleaned, &mut space, '\0', '\0', true);
                            let mut n = space;
                            if cleaned[n] == '"' {
                                n += 1;
                                crate::inlines::skipto_nospan(&cleaned, &mut n, '"');
                            }
                            crate::inlines::skiptows(&cleaned, &mut n, '\0', true, true);
                            cleaned[n] = '\0';
                            let tag_text: String =
                                cleaned[space..].iter().take_while(|&&c| c != '\0').collect();
                            let tag = self.add_tag(&tag_text, 0);
                            self.add_tag_to_reading(wread, tag);
                            space = n + 1;
                        }
                    }
                }
            } else if cleaned[0] == ' ' && cleaned[1] == '"' && c_cohort.is_some() {
                // (2) Reading line.
                is_deleted = false;
                match self.got_reading(
                    &mut cleaned,
                    &mut line,
                    &mut indents,
                    &mut all_mappings,
                    &mut variables_set,
                    &mut variables_rem,
                    &mut variables_output,
                    &mut c_swindow,
                    c_cohort.unwrap(),
                    &mut l_swindow,
                    &mut did_soft_lookback,
                    is_deleted,
                ) {
                    GotReading::Continue => {
                        // C++ `cReading = nullptr; continue;` — the `continue`
                        // re-enters the read loop WITHOUT running the trailing
                        // `++numLines; line[0]=cleaned[0]=0;`.
                        c_reading = None;
                        continue 'mainloop;
                    }
                    GotReading::Istext => {
                        is_text = true;
                    }
                    GotReading::Normal => {}
                }
            } else if self.pipe_deleted
                && cleaned[0] == ';'
                && cleaned[1] == ' '
                && cleaned[2] == '"'
                && c_cohort.is_some()
            {
                // (3) Deleted-reading line: strip the leading ';' and fall into (2).
                is_deleted = true;
                cleaned.remove(0);
                line.remove(0);
                match self.got_reading(
                    &mut cleaned,
                    &mut line,
                    &mut indents,
                    &mut all_mappings,
                    &mut variables_set,
                    &mut variables_rem,
                    &mut variables_output,
                    &mut c_swindow,
                    c_cohort.unwrap(),
                    &mut l_swindow,
                    &mut did_soft_lookback,
                    is_deleted,
                ) {
                    GotReading::Continue => {
                        // C++ `cReading = nullptr; continue;` (skips the trailing
                        // `++numLines; line[0]=cleaned[0]=0;`).
                        c_reading = None;
                        continue 'mainloop;
                    }
                    GotReading::Istext => {
                        is_text = true;
                    }
                    GotReading::Normal => {}
                }
            } else {
                // "looked like a reading but there was no containing cohort": deferred.
                is_text = true;
            }

            if is_text {
                // (4) istext: plain text + stream commands.
                if line[0] != '\0' {
                    let mut is_cmd = false;
                    let cleaned_str: String =
                        cleaned.iter().take_while(|&&c| c != '\0').collect();

                    if cleaned_str == crate::strings::STR_CMD_FLUSH {
                        // "FLUSH encountered … Flushing…": deferred.
                        is_cmd = true;
                        let back_swindow = self.gWindow.back();
                        if let Some(bsw) = back_swindow {
                            self.store.single_windows.get_mut(bsw.0).flush_after = true;
                        }
                        if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
                            self.split_all_mappings(&mut all_mappings, cc, true);
                            crate::single_window::append_cohort(
                                &mut self.gWindow,
                                &mut self.store,
                                sw,
                                cc,
                            );
                            // C++ SingleWindow::appendCohort: if (cohort->dep_self)
                            // parent->parent->dep_highest_seen = cohort->dep_self;
                            {
                                let ds = self.store.cohorts.get(cc.0).dep_self;
                                if ds != 0 {
                                    self.dep_highest_seen = ds;
                                }
                            }
                            if self.store.cohorts.get(cc.0).readings.is_empty() {
                                self.init_empty_cohort(cc);
                            }
                            let rs = self.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let tid = super::core::tag_by_hash(&self.grammar, self.endtag);
                                self.add_tag_to_reading(r, tid);
                            }
                            c_reading = None;
                            l_reading = None;
                            c_cohort = None;
                            l_cohort = None;
                            c_swindow = None;
                            l_swindow = None;
                        }
                        while !self.gWindow.next.is_empty() {
                            self.shuffle_windows_down();
                            
                            self.run_grammar_on_window(output);
                            if self.numWindows % reset_after == 0 {
                                self.reset_indexes();
                            }
                        }
                        self.shuffle_windows_down();
                        while !self.gWindow.previous.is_empty() {
                            let tmp = self.gWindow.previous[0];
                            let mut store = std::mem::take(&mut self.store);
                            self.print_single_window(&mut store, tmp, output, false);
                            self.store = store;
                            let mut t = Some(tmp);
                            crate::single_window::free_swindow(
                                &mut self.gWindow,
                                &mut self.store,
                                &mut t,
                            );
                            self.gWindow.previous.remove(0);
                        }
                        if back_swindow.is_none() {
                            self.print_stream_command(crate::strings::STR_CMD_FLUSH, output);
                        }
                        line[0] = '\0';
                        self.variables.clear(0);
                        crate::uextras::u_fflush(output);
                    } else if cleaned_str == crate::strings::STR_CMD_IGNORE {
                        // "IGNORE encountered …": deferred.
                        is_cmd = true;
                        ignoreinput = true;
                        self.print_stream_command(crate::strings::STR_CMD_IGNORE, output);
                        line[0] = '\0';
                    } else if cleaned_str == crate::strings::STR_CMD_RESUME {
                        // "RESUME encountered …": deferred.
                        is_cmd = true;
                        ignoreinput = false;
                        self.print_stream_command(crate::strings::STR_CMD_RESUME, output);
                        line[0] = '\0';
                    } else if cleaned_str == crate::strings::STR_CMD_EXIT {
                        // "EXIT encountered …": deferred.
                        is_cmd = true;
                        self.print_stream_command(crate::strings::STR_CMD_EXIT, output);
                        break 'mainloop;
                    } else if cleaned_str.starts_with(crate::strings::STR_CMD_SETVAR) {
                        // <STREAMCMD:SETVAR:...> — inline parse (no parseSetVar method).
                        is_cmd = true;
                        cleaned[packoff - 1] = '\0';
                        line[0] = '\0';

                        // UChar* s = &cleaned[STR_CMD_SETVAR.size()];
                        let mut s: Option<usize> = Some(crate::strings::STR_CMD_SETVAR.chars().count());
                        let mut c = u_strchr(&cleaned, s.unwrap(), ',');
                        let mut d = u_strchr(&cleaned, s.unwrap(), '=');
                        if c.is_none() && d.is_none() {
                            let s_text: String =
                                cleaned[s.unwrap()..].iter().take_while(|&&ch| ch != '\0').collect();
                            let tag = self.add_tag(&s_text, 0);
                            let h = self.grammar.single_tags_list[tag.0].hash;
                            *variables_set.index_or_insert(h) = self.grammar.tag_any;
                            variables_rem.erase(h);
                            variables_output.insert(h);
                            if c_swindow.is_none() {
                                *self.variables.index_or_insert(h) = self.grammar.tag_any;
                            }
                        } else {
                            let mut a: u32;
                            let mut b: u32;
                            while c.is_some() || d.is_some() {
                                if d.is_some() && (c.is_none() || d.unwrap() < c.unwrap()) {
                                    let di = d.unwrap();
                                    cleaned[di] = '\0';
                                    if cleaned[s.unwrap()] == '\0' {
                                        // "no identifier before the =": default *.
                                        a = self.grammar.tag_any;
                                    } else {
                                        let s_text: String = cleaned[s.unwrap()..]
                                            .iter()
                                            .take_while(|&&ch| ch != '\0')
                                            .collect();
                                        let atag = self.add_tag(&s_text, 0);
                                        a = self.grammar.single_tags_list[atag.0].hash;
                                    }
                                    if let Some(ci) = c {
                                        cleaned[ci] = '\0';
                                        s = Some(ci + 1);
                                    }
                                    if cleaned[di + 1] == '\0' {
                                        // "no value after the =": default *.
                                        b = self.grammar.tag_any;
                                    } else {
                                        let d_text: String = cleaned[di + 1..]
                                            .iter()
                                            .take_while(|&&ch| ch != '\0')
                                            .collect();
                                        let btag = self.add_tag(&d_text, 0);
                                        b = self.grammar.single_tags_list[btag.0].hash;
                                    }
                                    if c.is_none() {
                                        d = None;
                                        s = None;
                                    }
                                    *variables_set.index_or_insert(a) = b;
                                    variables_rem.erase(a);
                                    variables_output.insert(a);
                                } else if c.is_some() && (d.is_none() || c.unwrap() < d.unwrap()) {
                                    let ci = c.unwrap();
                                    cleaned[ci] = '\0';
                                    if cleaned[s.unwrap()] == '\0' {
                                        // "no identifier after the ,": default *.
                                        a = self.grammar.tag_any;
                                    } else {
                                        let s_text: String = cleaned[s.unwrap()..]
                                            .iter()
                                            .take_while(|&&ch| ch != '\0')
                                            .collect();
                                        let atag = self.add_tag(&s_text, 0);
                                        a = self.grammar.single_tags_list[atag.0].hash;
                                    }
                                    s = Some(ci + 1);
                                    *variables_set.index_or_insert(a) = self.grammar.tag_any;
                                    variables_rem.erase(a);
                                    variables_output.insert(a);
                                }
                                if let Some(si) = s {
                                    c = u_strchr(&cleaned, si, ',');
                                    d = u_strchr(&cleaned, si, '=');
                                    if c.is_none() && d.is_none() {
                                        let s_text: String =
                                            cleaned[si..].iter().take_while(|&&ch| ch != '\0').collect();
                                        let atag = self.add_tag(&s_text, 0);
                                        a = self.grammar.single_tags_list[atag.0].hash;
                                        *variables_set.index_or_insert(a) = self.grammar.tag_any;
                                        variables_rem.erase(a);
                                        variables_output.insert(a);
                                        s = None;
                                    }
                                }
                            }
                        }
                    } else if cleaned_str.starts_with(crate::strings::STR_CMD_REMVAR) {
                        // <STREAMCMD:REMVAR:...> — inline parse (no parseRemVar method).
                        is_cmd = true;
                        cleaned[packoff - 1] = '\0';
                        line[0] = '\0';

                        // UChar* s = &cleaned[STR_CMD_REMVAR.size()];
                        let mut s: usize = crate::strings::STR_CMD_REMVAR.chars().count();
                        let mut c = u_strchr(&cleaned, s, ',');
                        while let Some(ci) = c {
                            if cleaned[ci] == '\0' {
                                break;
                            }
                            cleaned[ci] = '\0';
                            if cleaned[s] != '\0' {
                                let s_text: String =
                                    cleaned[s..].iter().take_while(|&&ch| ch != '\0').collect();
                                let atag = self.add_tag(&s_text, 0);
                                let a = self.grammar.single_tags_list[atag.0].hash;
                                variables_set.erase(a);
                                variables_rem.insert(a);
                                variables_output.insert(a);
                            }
                            s = ci + 1;
                            c = u_strchr(&cleaned, s, ',');
                        }
                        if cleaned[s] != '\0' {
                            let s_text: String =
                                cleaned[s..].iter().take_while(|&&ch| ch != '\0').collect();
                            let atag = self.add_tag(&s_text, 0);
                            let a = self.grammar.single_tags_list[atag.0].hash;
                            variables_set.erase(a);
                            variables_rem.insert(a);
                            variables_output.insert(a);
                        }
                    }

                    if line[0] != '\0' {
                        let line_str: String = line.iter().take_while(|&&c| c != '\0').collect();
                        if l_swindow.is_some()
                            && l_cohort.is_some()
                            && test_string_against(&line_str, &mut self.text_delimiters)
                        {
                            // Text-delimiter line.
                            let lsw = l_swindow.unwrap();
                            self.store.single_windows.get_mut(lsw.0).text_post.push_str(&line_str);
                            let cc = c_cohort.unwrap();
                            let rs = self.store.cohorts.get(cc.0).readings.clone();
                            for r in rs {
                                let tid = super::core::tag_by_hash(&self.grammar, self.endtag);
                                self.add_tag_to_reading(r, tid);
                            }
                            self.split_all_mappings(&mut all_mappings, cc, true);
                            let sw = c_swindow.unwrap();
                            crate::single_window::append_cohort(
                                &mut self.gWindow,
                                &mut self.store,
                                sw,
                                cc,
                            );
                            // C++ SingleWindow::appendCohort: if (cohort->dep_self)
                            // parent->parent->dep_highest_seen = cohort->dep_self;
                            {
                                let ds = self.store.cohorts.get(cc.0).dep_self;
                                if ds != 0 {
                                    self.dep_highest_seen = ds;
                                }
                            }
                            self.store.cohorts.get_mut(cc.0).line_number = self.numLines;
                            l_swindow = Some(sw);
                            c_swindow = None;
                            c_cohort = None;
                            l_cohort = None;
                            self.numCohorts += 1;
                            did_soft_lookback = false;
                        } else if let Some(lc) = l_cohort {
                            self.store.cohorts.get_mut(lc.0).text.push_str(&line_str);
                        } else if let Some(lsw) = l_swindow {
                            let sww = self.store.single_windows.get_mut(lsw.0);
                            if !sww.text_post.is_empty() {
                                sww.text_post.push_str(&line_str);
                            } else {
                                sww.text.push_str(&line_str);
                            }
                        } else if !is_cmd {
                            self.print_plain_text_line(&line_str, output);
                        }
                    }
                }
            }

            self.numLines += 1;
            line[0] = '\0';
            cleaned[0] = '\0';

            // Loop termination: the C++ `while(!input.eof())` re-check at the
            // top of the loop, using the EOF state sampled after get_line_clean.
            if hit_eof {
                break 'mainloop;
            }
        }

        self.input_eof = true;

        // Pending cCohort + cSWindow at EOF.
        if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
            self.split_all_mappings(&mut all_mappings, cc, true);
            crate::single_window::append_cohort(&mut self.gWindow, &mut self.store, sw, cc);
            // C++ SingleWindow::appendCohort: if (cohort->dep_self)
            // parent->parent->dep_highest_seen = cohort->dep_self;
            {
                let ds = self.store.cohorts.get(cc.0).dep_self;
                if ds != 0 {
                    self.dep_highest_seen = ds;
                }
            }
            if self.store.cohorts.get(cc.0).readings.is_empty() {
                self.init_empty_cohort(cc);
            }
            let rs = self.store.cohorts.get(cc.0).readings.clone();
            for r in rs {
                let tid = super::core::tag_by_hash(&self.grammar, self.endtag);
                self.add_tag_to_reading(r, tid);
            }
            c_reading = None;
            c_cohort = None;
            c_swindow = None;
        }

        if self.fmt_output == super::cg3_sformat::CG3SF_BINARY && !variables_output.empty() {
            // binary_maybe_window() + adopt_variables() [inlined]
            if self.fmt_output == super::cg3_sformat::CG3SF_BINARY {
                let sw = self.gWindow.alloc_append_single_window(&mut self.store);
                self.init_empty_single_window(sw);
                l_swindow = Some(sw);
                c_swindow = Some(sw);
            }
            if let Some(sw) = c_swindow {
                self.adopt_variables(
                    sw,
                    &mut variables_set,
                    &mut variables_rem,
                    &mut variables_output,
                );
            }
        }

        // Drain the remaining windows.
        while !self.gWindow.next.is_empty() {
            self.shuffle_windows_down();
            
            self.run_grammar_on_window(output);
            // verbose progress: deferred.
        }
        self.shuffle_windows_down();
        while !self.gWindow.previous.is_empty() {
            let tmp = self.gWindow.previous[0];
            let mut store = std::mem::take(&mut self.store);
            self.print_single_window(&mut store, tmp, output, false);
            self.store = store;
            let mut t = Some(tmp);
            crate::single_window::free_swindow(&mut self.gWindow, &mut self.store, &mut t);
            self.gWindow.previous.remove(0);
        }

        crate::uextras::u_fflush(output);

        // Emit final SETVAR/REMVAR stream commands for each output variable.
        for &var in variables_output.as_slice() {
            let key = {
                let tid = super::core::tag_by_hash(&self.grammar, var);
                self.grammar.single_tags_list.get(tid.0).tag.clone()
            };
            let mut cmd_buf = String::new();
            let found = variables_set.find(var);
            if found != variables_set.end() {
                let val = found.get().1;
                if val != self.grammar.tag_any {
                    let value = {
                        let tid = super::core::tag_by_hash(&self.grammar, val);
                        self.grammar.single_tags_list.get(tid.0).tag.clone()
                    };
                    cmd_buf.push_str(crate::strings::STR_CMD_SETVAR);
                    cmd_buf.push_str(&key);
                    cmd_buf.push('=');
                    cmd_buf.push_str(&value);
                    cmd_buf.push('>');
                } else {
                    cmd_buf.push_str(crate::strings::STR_CMD_SETVAR);
                    cmd_buf.push_str(&key);
                    cmd_buf.push('>');
                }
            } else {
                cmd_buf.push_str(crate::strings::STR_CMD_REMVAR);
                cmd_buf.push_str(&key);
                cmd_buf.push('>');
            }
            self.print_stream_command(&cmd_buf, output);
        }

        // CGCMD_EXIT: verbose "Did N lines, N windows, ..." summary: deferred.
        let _ = (lines, l_reading);
    }

    /// The retire loop at the head of C++ `runGrammarOnWindow()`
    /// (`label_runGrammarOnWindow_begin:`): while `gWindow->previous` holds more
    /// than `num_windows` windows, `printSingleWindow(front, *ux_stdout)`,
    /// `free_swindow(front)`, pop front. The Rust `run_grammar_on_window` cannot
    /// reach the driver's output stream (the `ux_stdout` placeholder) and renders
    /// retiring windows into a discarded sink, so the driver performs the
    /// identical retire-print here immediately before entering it — same bytes,
    /// same order, and the inner sink loop is left a no-op (`previous.len() <=
    /// num_windows` afterwards; nothing inside a window run grows `previous`).
    /// C++ `Window::shuffleWindowsDown()` as invoked from `runGrammarOnText`.
    /// The C++ method does `current->variables_set = parent->variables;` through
    /// the Window's applicator back-pointer before retiring `current`; the Rust
    /// `Window` has no such back-pointer (placeholder), so the driver performs
    /// that snapshot here before delegating to `gWindow.shuffle_windows_down`
    /// (which still clears `variables_rem` and shuffles the lists).
    fn shuffle_windows_down(&mut self) {
        if let Some(current) = self.gWindow.current {
            let vars = live_map_pairs(&mut self.variables);
            let sww = self.store.single_windows.get_mut(current.0);
            sww.variables_set.clear(0);
            for (k, v) in vars {
                sww.variables_set.insert((k, v));
            }
        }
        self.gWindow.shuffle_windows_down(&mut self.store);
    }

    /// C++ `adopt_variables` lambda from `runGrammarOnText`: move the pending
    /// stream-command variable deltas onto `cSWindow`, then clear the locals.
    /// `cSWindow->variables_set.insert(begin, end)` etc. — only the live slots of
    /// the flat maps/sets are transferred (the `EMPTY`/`DEL` sentinels skipped).
    fn adopt_variables(
        &mut self,
        c_swindow: crate::arena::SwId,
        variables_set: &mut crate::flat_unordered_map::Uint32FlatHashMap,
        variables_rem: &mut crate::flat_unordered_set::Uint32FlatHashSet,
        variables_output: &mut crate::sorted_vector::uint32SortedVector,
    ) {
        let vs = live_map_pairs(variables_set);
        let vr = live_set_values(variables_rem);
        let vo: Vec<u32> = variables_output.as_slice().to_vec();
        let sww = self.store.single_windows.get_mut(c_swindow.0);
        for (k, v) in vs {
            sww.variables_set.insert((k, v));
        }
        for v in vr {
            sww.variables_rem.insert(v);
        }
        for v in vo {
            sww.variables_output.insert(v);
        }
        variables_set.clear(0);
        variables_rem.clear(0);
        variables_output.clear();
    }
}
