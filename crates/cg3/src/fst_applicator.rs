//! Port of `src/FSTApplicator.cpp` + `src/FSTApplicator.hpp` — the FST-lookup
//! text-format applicator (`hfst`/`apertium`-style `wordform<TAB>analysis` I/O).
//!
//! ## Composition (task design)
//! C++ `class FSTApplicator : public virtual GrammarApplicator` becomes
//! [`FSTApplicator`] owning or borrowing a
//! [`GrammarApplicator`](crate::grammar_applicator::GrammarApplicator) plus the
//! four FST-only members. Every engine/core call goes through
//! `self.base.<method>` (or the core free fns threaded through its arenas).
//!
//! ## Parser and serializers
//! The three serialisers ([`FSTApplicator::print_reading`],
//! [`print_cohort`](FSTApplicator::print_cohort),
//! [`print_single_window`](FSTApplicator::print_single_window)) build only on
//! ported base helpers + the runtime store.
//!
//! [`run_grammar_on_text`](FSTApplicator::run_grammar_on_text) is now a genuine
//! port: the `input`/`output` streams are threaded as method params (mirroring
//! the sibling `apertium_applicator.rs` — `input: &mut R (Read + Seek)` /
//! `output: &mut W (Write)`), the `ux_stdin`/`ux_stdout` `Option<()>` fields are
//! elided, and the C++ `u_strchr` / `u_strspn` / `u_strcspn` `UChar*` walks are
//! reproduced over a `Vec<char>` scratch buffer with `usize` indices. The
//! `reverse(cReading)` sub-reading reversal maps to [`reverse_reading`] over the
//! arena `next` chain. `strtof` → `str::parse::<f32>`; the delimiter/warning
//! diagnostics are emitted to a discard sink (`ux_stderr` placeholder).

use std::io::Write;
use std::ops::DerefMut;

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::cohort::append_reading;
use crate::cohort::{CT_REMOVED, alloc_cohort, free_cohort};
use crate::grammar::Grammar;
use crate::grammar_applicator::GrammarApplicator;
use crate::inlines::{
    NUMERIC_MAX, insert_if_exists, isnl, isspace, reversed, skipto_nospan_raw_chars,
};
use crate::reading::alloc_reading;
use crate::single_window::{append_cohort, free_swindow};
use crate::tag::{T_DEPENDENCY, T_MAPPING, T_RELATION, TagList};
use crate::types::{TagHash, UString};
use crate::uextras::{get_line_clean_chars, u_fputc, ux_strip_bom};

/// C++ `grammar->single_tags[hash]` — resolves a tag hash to its `TagId`, else
/// `TagId(0)`. Reproduces `grammar_applicator::core::tag_by_hash` (which is
/// `pub(super)`, not reachable here); the module cannot be edited.
fn tag_by_hash(grammar: &Grammar, hash: TagHash) -> TagId {
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

// [spec:cg3:def:fst-applicator.cg3.fst-applicator]
/// C++ `class FSTApplicator : public virtual GrammarApplicator`. Composition
/// port: the base engine is owned by default and borrowed by `FormatConverter`;
/// the four FST-only members take their C++ in-class defaults.
pub struct FSTApplicator<B = Box<GrammarApplicator>> {
    /// The `GrammarApplicator` base (C++ `public virtual` inheritance).
    pub base: B,
    pub did_warn_statictags: bool,
    pub wfactor: f64,
    pub wtag: UString,
    pub sub_delims: UString,
}

impl FSTApplicator<Box<GrammarApplicator>> {
    // [spec:cg3:def:fst-applicator.cg3.fst-applicator.fst-applicator-fn]
    // [spec:cg3:sem:fst-applicator.cg3.fst-applicator.fst-applicator-fn]
    /// C++ `FSTApplicator::FSTApplicator(std::ostream& ux_err)`. Delegates to the
    /// base `GrammarApplicator(ux_err)` ctor with an empty body; the FST members
    /// take their in-class defaults (`did_warn_statictags = false`,
    /// `wfactor = 1.0`, `wtag = "W"`, `sub_delims = "#"`). No other side effects.
    pub fn new(base: GrammarApplicator) -> Self {
        Self::with_base(Box::new(base))
    }
}

impl<'a> FSTApplicator<&'a mut GrammarApplicator> {
    /// Borrow the shared virtual-base analogue used by [`FormatConverter`](crate::format_converter::FormatConverter).
    pub fn borrowing(base: &'a mut GrammarApplicator) -> Self {
        Self::with_base(base)
    }
}

impl<B> FSTApplicator<B>
where
    B: DerefMut<Target = GrammarApplicator>,
{
    fn with_base(base: B) -> Self {
        FSTApplicator {
            base,
            did_warn_statictags: false,
            wfactor: 1.0,
            wtag: "W".to_string(),
            sub_delims: "#".to_string(),
        }
    }

    // [spec:cg3:def:fst-applicator.cg3.fst-applicator.print-reading-fn]
    // [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-reading-fn]
    /// C++ `void FSTApplicator::printReading(const Reading* reading,
    /// std::ostream& output)`. Serialises one reading in FST-lookup syntax
    /// (`+`-joined baseform + tags), recursing into subreadings so the innermost
    /// prints first, joined by `sub_delims` (`"#"`). No trailing newline (the
    /// caller adds it).
    ///
    /// `Reading*`/`Cohort*` resolve through `self.base.store`; the store is
    /// threaded as a parameter so the caller can split the `&mut self.base` /
    /// `&mut store` borrows (matching the base print methods).
    pub fn print_reading<W: Write>(&self, reading: ReadingId, output: &mut W) {
        let (noprint, deleted, next, baseform, parent) = {
            let r = self.base.store.readings.get(reading.0);
            (r.noprint, r.deleted, r.next, r.baseform, r.parent)
        };
        if noprint {
            return;
        }
        if deleted {
            return;
        }

        if let Some(next_id) = next {
            self.print_reading(next_id, output);
            let _ = write!(output, "{}", self.sub_delims);
        }

        if let Some(baseform) = baseform {
            // grammar->single_tags[baseform]->tag, stripped of the surrounding
            // quotes: print `tag.size() - 2` chars starting at `tag.data() + 1`.
            let tid = tag_by_hash(&self.base.grammar, baseform);
            let tag = &self.base.grammar.single_tags_list[tid.0].tag;
            let chars: Vec<char> = tag.chars().collect();
            if chars.len() >= 2 {
                let inner: String = chars[1..chars.len() - 1].iter().collect();
                let _ = write!(output, "{inner}");
            }
        }

        // parent->wordform->hash for the skip test.
        let parent_wf_hash = {
            let cid = parent.expect("reading has no parent cohort");
            let wf = self.base.store.cohorts.get(cid.0).wordform;
            wf.map(|t| self.base.grammar.single_tags_list[t.0].hash)
                .unwrap_or(TagHash(0))
        };

        let tags_list: Vec<u32> = self.base.store.readings.get(reading.0).tags_list.clone();
        let mut unique: crate::sorted_vector::uint32SortedVector =
            crate::sorted_vector::uint32SortedVector::new();
        for tter in tags_list {
            let tter = TagHash(tter);
            if (!self.base.cfg.show_end_tags && tter == self.base.cfg.endtag) || tter == self.base.cfg.begintag
            {
                continue;
            }
            if baseform == Some(tter) || tter == parent_wf_hash {
                continue;
            }
            if self.base.cfg.unique_tags {
                if unique.find(tter.get()) != unique.end() {
                    continue;
                }
                unique.insert(tter.get());
            }
            let tid = tag_by_hash(&self.base.grammar, tter);
            let tag = &self.base.grammar.single_tags_list[tid.0];
            if tag.r#type.intersects(T_DEPENDENCY) && self.base.has_dep && !self.base.cfg.dep_original {
                continue;
            }
            if tag.r#type.intersects(T_RELATION) && self.base.has_relations {
                continue;
            }
            let _ = write!(output, "+{}", tag.tag);
        }
    }

    // [spec:cg3:def:fst-applicator.cg3.fst-applicator.print-cohort-fn]
    // [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-cohort-fn]
    /// C++ `void FSTApplicator::printCohort(Cohort* cohort, std::ostream& output,
    /// bool profiling)`. Uses a `removed:` label so removed cohorts still print
    /// their trailing text. Static tags trigger a one-shot stderr warning and are
    /// otherwise dropped from FST output.
    pub fn print_cohort<W: Write>(&mut self, cohort: CohortId, output: &mut W, profiling: bool) {
        let (local_number, ctype) = {
            let c = self.base.store.cohorts.get(cohort.0);
            (c.local_number, c.r#type)
        };
        // if (local_number == 0 || (type & CT_REMOVED)) goto removed;
        let goto_removed = local_number == 0 || (ctype.intersects(CT_REMOVED));

        if !goto_removed {
            let wblank = self.base.store.cohorts.get(cohort.0).wblank.clone();
            if !wblank.is_empty() {
                let _ = write!(output, "{wblank}");
                if !isnl(wblank.chars().next_back().unwrap_or('\0')) {
                    u_fputc('\n', output);
                }
            }

            if self.base.store.cohorts.get(cohort.0).wread.is_some() && !self.did_warn_statictags {
                // u_fprintf(ux_stderr, "Warning: FST CG format cannot output
                // static tags! You are losing information!\n"); ux_stderr is a
                // placeholder — emission deferred; the one-shot flag is set.
                self.did_warn_statictags = true;
            }

            if !profiling {
                crate::cohort::unignore_all(&mut self.base.store, cohort);
                if !self.base.cfg.split_mappings {
                    self.base.merge_mappings(cohort);
                }
            }

            // wform = cohort->wordform->tag; print stripped of `"<` and `>"`:
            // wform.size() - 4 chars starting at wform.data() + 2.
            let wform: Vec<char> = {
                let wf = self
                    .base
                    .store
                    .cohorts
                    .get(cohort.0)
                    .wordform
                    .expect("cohort wordform");
                self.base.grammar.single_tags_list[wf.0]
                    .tag
                    .chars()
                    .collect()
            };
            let wform_inner: String = if wform.len() >= 4 {
                wform[2..wform.len() - 2].iter().collect()
            } else {
                String::new()
            };

            let readings: Vec<ReadingId> = self.base.store.cohorts.get(cohort.0).readings.clone();
            let only_noprint =
                readings.len() == 1 && self.base.store.readings.get(readings[0].0).noprint;
            if readings.is_empty() || only_noprint {
                // "<wordform>\t+?\n" — the FST "no analysis" marker.
                let _ = writeln!(output, "{wform_inner}\t+?");
            } else {
                // NOTE: printCohort does NOT sort readings (unlike the base CG
                // applicator) — iterate the current vector order verbatim.
                for rter in readings {
                    let _ = write!(output, "{wform_inner}\t");
                    self.print_reading(rter, output);
                    u_fputc('\n', output);
                }
            }
            u_fputc('\n', output);
        }

        // removed:
        let text = self.base.store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() && text.chars().any(|c| !self.is_ws(c)) {
            let _ = write!(output, "{text}");
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }
    }

    /// C++ `UString::find_first_not_of(ws)` membership over the base's `ws`
    /// whitespace set (space, tab, [newline], NUL). Mirrors the base's private
    /// `is_ws`.
    fn is_ws(&self, c: char) -> bool {
        for &w in &self.base.cfg.ws {
            if w == '\0' {
                break;
            }
            if w == c {
                return true;
            }
        }
        false
    }

    // [spec:cg3:def:fst-applicator.cg3.fst-applicator.print-single-window-fn]
    // [spec:cg3:sem:fst-applicator.cg3.fst-applicator.print-single-window-fn]
    /// C++ `void FSTApplicator::printSingleWindow(SingleWindow* window,
    /// std::ostream& output, bool profiling)`. Pre-window text, then each cohort,
    /// then post-window text, then one blank line, then flush. `profiling` is
    /// forwarded to `printCohort` only.
    pub fn print_single_window<W: Write>(&mut self, window: SwId, output: &mut W, profiling: bool) {
        let (text, all_cohorts, text_post) = {
            let w = self.base.store.single_windows.get(window.0);
            (w.text.clone(), w.all_cohorts.clone(), w.text_post.clone())
        };

        if !text.is_empty() {
            let _ = write!(output, "{text}");
            if !isnl(text.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }

        for cohort in all_cohorts {
            self.print_cohort(cohort, output, profiling);
        }

        if !text_post.is_empty() {
            let _ = write!(output, "{text_post}");
            if !isnl(text_post.chars().next_back().unwrap_or('\0')) {
                u_fputc('\n', output);
            }
        }

        u_fputc('\n', output);
        let _ = output.flush();
    }
}

impl<B> FSTApplicator<B>
where
    B: DerefMut<Target = GrammarApplicator>,
{
    // [spec:cg3:def:fst-applicator.cg3.fst-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:fst-applicator.cg3.fst-applicator.run-grammar-on-text-fn]
    /// C++ `void FSTApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`. Reads the FST-lookup text format
    /// (`wordform<TAB>analysis[<TAB>weight]` per line; a wordform's readings on
    /// consecutive lines; blank/other line ends the cohort), builds windows, runs
    /// the grammar, prints results. No regex — manual char scanning + `strtof`.
    pub fn run_grammar_on_text<R, W>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        let mut fmt = FstFormat::from_app(self);
        let result =
            crate::error::catch_fatal(|| self.run_grammar_on_text_impl(&mut fmt, input, output));
        self.did_warn_statictags = fmt.did_warn_statictags;
        result
    }

    /// Run the FST parser while routing output through a most-derived stream
    /// format, matching C++ virtual dispatch in `FormatConverter`.
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

    fn run_grammar_on_text_impl<F, R, W>(&mut self, fmt: &mut F, input: &mut R, output: &mut W)
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: std::io::Read + std::io::Seek,
        W: std::io::Write,
    {
        // ux_stdin = &input; ux_stdout = &output; (elided: Option<()> placeholders)
        // good()/eof()/output/grammar validity checks (each CG3Quit(1) with a
        // u_fprintf diagnostic) elided — the grammar is assumed present.

        // No-hard/soft-delimiter warnings (emitted to the discard sink).
        let no_hard = self.base.grammar.delimiters.is_none();
        let no_soft = self.base.grammar.soft_delimiters.is_none();
        if no_hard {
            if no_soft {
                tracing::warn!(
                    "Warning: No soft or hard delimiters defined in grammar. Hard limit of {} cohorts may break windows in unintended places.",
                    self.base.cfg.hard_limit
                );
            } else {
                tracing::warn!(
                    "Warning: No hard delimiters defined in grammar. Soft limit of {} cohorts may break windows in unintended places.",
                    self.base.cfg.soft_limit
                );
            }
        }

        // UString line(1024, 0); UString cleaned(line.size(), 0);
        let mut line: Vec<char> = vec!['\0'; 1024];
        let mut cleaned: Vec<char> = vec!['\0'; line.len()];
        let ignoreinput = false;
        let mut did_soft_lookback = false;

        self.base.index();

        let reset_after: u32 = (self.base.cfg.num_windows + 4) * 2 + 1;
        let mut lines: u32 = 0;

        let mut c_swindow: Option<SwId> = None;
        let mut c_cohort: Option<CohortId> = None;

        let mut l_swindow: Option<SwId> = None;
        let mut l_cohort: Option<CohortId> = None;

        self.base.window.window_span = self.base.cfg.num_windows;

        ux_strip_bom(input);

        // C++ `while (!input.eof())`: reproduced by breaking when a read makes no
        // progress (get_line_clean returns 0 and the line buffer stays empty).
        'mainloop: loop {
            lines += 1;
            let mut packoff = get_line_clean_chars(&mut line, &mut cleaned, input, true);

            // C++ `while (!input.eof())`: eofbit is set when a read attempt hits
            // end-of-stream. `u_fgets` distinguishes a blank line (packoff == 0
            // but `line[0]` holds the newline) from true EOF (nothing stored, so
            // `line[0]` keeps the '\0' it was reset to) — only the latter ends
            // the loop. Sampled here, acted on at the bottom of the iteration
            // (matches the base run_grammar_on_text driver).
            let hit_eof = packoff == 0 && line[0] == '\0';

            // Trim trailing whitespace.
            while cleaned[0] != '\0' && packoff > 0 && isspace(cleaned[packoff - 1]) {
                cleaned[packoff - 1] = '\0';
                packoff -= 1;
            }

            // `is_text` is the `goto istext` flag; the C++ label body runs at the
            // bottom for every non-cohort line.
            let mut is_text = ignoreinput || cleaned[0] == '\0';

            if !is_text {
                // space = &cleaned[0]; SKIPTO_NOSPAN_RAW(space, '\t');
                let mut space = 0usize;
                skipto_nospan_raw_chars(&cleaned, &mut space, '\t');

                if cleaned[space] != '\t' {
                    // If this line looks like markup, don't warn about it.
                    if cleaned[0] != '<' {
                        tracing::warn!(
                            "Warning: {} on line {} looked like a cohort but wasn't - treated as text.",
                            cleaned[..space].iter().collect::<String>(),
                            self.base.numLines
                        );
                    }
                    is_text = true;
                } else {
                    cleaned[space] = '\0';

                    // tag = "\"<" + cleaned + ">\"";
                    let wf_body: String = cleaned[..space].iter().collect();
                    let mut tag = String::new();
                    tag.push_str("\"<");
                    tag.push_str(&wf_body);
                    tag.push_str(">\"");

                    if c_cohort.is_none() {
                        if c_swindow.is_none() {
                            let sw = {
                                let base = &mut *self.base;
                                base.window.alloc_append_single_window(&mut base.store)
                            };
                            self.base.init_empty_single_window(sw);
                            c_swindow = Some(sw);
                            l_swindow = Some(sw);
                            self.base.numWindows = self.base.numWindows.wrapping_add(1);
                            did_soft_lookback = false;
                        }
                        let cc = alloc_cohort(&mut self.base.store, c_swindow);
                        let gn = self.base.window.next_cohort_number();
                        let wf = self.base.add_tag(&tag, crate::tag::TagType::empty());
                        {
                            let c = self.base.store.cohorts.get_mut(cc.0);
                            c.global_number = gn;
                            c.wordform = Some(wf);
                        }
                        c_cohort = Some(cc);
                        l_cohort = Some(cc);
                        self.base.numCohorts = self.base.numCohorts.wrapping_add(1);
                    }
                    let cc = c_cohort.unwrap();

                    // ++space; while (space && *space && (space[0]!='+' ||
                    //   space[1]!='?' || space[2]!=0)) { ... }
                    // In C++ the inner `(space = u_strchr(space, '+')) != 0` scan
                    // sets `space` to nullptr when no '+' remains, which is what
                    // terminates THIS loop after the reading is finished; an index
                    // can't go null, so the nullptr state is a flag here.
                    space += 1;
                    let mut space_null = false;
                    while !space_null
                        && space < cleaned.len()
                        && cleaned[space] != '\0'
                        && !(cleaned[space] == '+'
                            && cleaned[space + 1] == '?'
                            && cleaned[space + 2] == '\0')
                    {
                        // tab = u_strchr(space, '\t'). FSTs sometimes echo the input
                        // twice for non-matches, so a `\t+?` tail ends the cohort.
                        let mut tab: Option<usize> = None;
                        {
                            let mut i = space;
                            while cleaned[i] != '\0' {
                                if cleaned[i] == '\t' {
                                    tab = Some(i);
                                    break;
                                }
                                i += 1;
                            }
                        }
                        if let Some(t) = tab
                            && cleaned[t + 1] == '+'
                            && cleaned[t + 2] == '?'
                        {
                            break;
                        }

                        // Reading* cReading = alloc_reading(cCohort);
                        let mut c_reading = alloc_reading(&mut self.base.store, Some(cc));
                        {
                            let base = &mut *self.base;
                            insert_if_exists(
                                &mut base.store.cohorts.get_mut(cc.0).possible_sets,
                                base.grammar.sets_any.as_ref(),
                            );
                        }
                        let wf = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                        self.base.add_tag_to_reading(c_reading, wf);

                        // const UChar* base = space; (index into cleaned). A quoted
                        // baseform reassignment (base = tag.data()) is tracked with
                        // `base_str = Some(...)`.
                        let mut base_idx = space;
                        let mut base_str: Option<String> = None;
                        let mut mappings = TagList::new();

                        let mut wtag_tag: Option<TagId> = None;
                        if let Some(mut t) = tab {
                            cleaned[t] = '\0';
                            t += 1;
                            // Replace the first comma with '.' (locale decimal).
                            {
                                let mut i = t;
                                while i < cleaned.len() && cleaned[i] != '\0' {
                                    if cleaned[i] == ',' {
                                        cleaned[i] = '.';
                                        break;
                                    }
                                    i += 1;
                                }
                            }
                            // char buf[32]; copy up to 31 units of the weight text.
                            let mut buf = String::new();
                            {
                                let mut i = 0;
                                while i < 31 && t + i < cleaned.len() && cleaned[t + i] != '\0' {
                                    buf.push(cleaned[t + i]);
                                    i += 1;
                                }
                            }
                            let formatted = if buf == "inf" {
                                // i = sprintf(buf, "%f", NUMERIC_MAX);
                                format!("{:.6}", NUMERIC_MAX)
                            } else {
                                // weight = strtof(buf, 0); weight *= wfactor;
                                let weight =
                                    (buf.parse::<f32>().unwrap_or(0.0) as f64) * self.wfactor;
                                // i = sprintf(buf, "%f", weight);
                                format!("{:.6}", weight)
                            };
                            // wtag_buf = "<" + wtag + ":" + buf + ">"
                            let mut wtag_buf: UString = String::new();
                            wtag_buf.push('<');
                            wtag_buf.push_str(&self.wtag);
                            wtag_buf.push(':');
                            wtag_buf.push_str(&formatted);
                            wtag_buf.push('>');
                            wtag_tag =
                                Some(self.base.add_tag(&wtag_buf, crate::tag::TagType::empty()));
                        }

                        // Initial baseform, because it may end on '+'.
                        // plus = u_strchr(space, '+');
                        {
                            let mut plus: Option<usize> = None;
                            let mut i = space;
                            while i < cleaned.len() && cleaned[i] != '\0' {
                                if cleaned[i] == '+' {
                                    plus = Some(i);
                                    break;
                                }
                                i += 1;
                            }
                            if let Some(p0) = plus {
                                let mut p = p0 + 1; // ++plus
                                // int32_t p = u_strspn(plus, "+"); span of '+'.
                                let mut f = 0usize;
                                while p + f < cleaned.len() && cleaned[p + f] == '+' {
                                    f += 1;
                                }
                                p += f; // space = plus + p
                                space = p - 1; // --space
                            }
                        }

                        // while (space && *space && (space = u_strchr(space,'+')))
                        loop {
                            // Advance space to the next '+' (u_strchr).
                            let mut found: Option<usize> = None;
                            {
                                let mut i = space;
                                while i < cleaned.len() && cleaned[i] != '\0' {
                                    if cleaned[i] == '+' {
                                        found = Some(i);
                                        break;
                                    }
                                    i += 1;
                                }
                            }
                            // C++ `(space = u_strchr(space, '+')) != 0`: a miss
                            // nulls `space` (exiting the enclosing reading loop too).
                            let Some(sp) = found else {
                                space_null = true;
                                break;
                            };
                            space = sp;

                            // if (base && base[0])
                            let base_first = match &base_str {
                                Some(s) => s.chars().next().unwrap_or('\0'),
                                None => cleaned.get(base_idx).copied().unwrap_or('\0'),
                            };
                            if base_first != '\0' {
                                // int32_t f = u_strcspn(base, sub_delims.data());
                                // (base is always a cleaned index at the top of the
                                // loop body — a reassignment to `tag` happens later).
                                let sub: Vec<char> = self.sub_delims.chars().collect();
                                let mut f = 0usize;
                                while base_idx + f < cleaned.len()
                                    && cleaned[base_idx + f] != '\0'
                                    && !sub.contains(&cleaned[base_idx + f])
                                {
                                    f += 1;
                                }
                                let mut hash: Option<usize> = None;
                                if f != 0 && base_idx + f < space {
                                    // cleaned.resize(size+1); copy_backward; hash[0]=0
                                    // — insert a NUL at base+f, shifting the tail right.
                                    let hidx = base_idx + f;
                                    cleaned.push('\0');
                                    let n = cleaned.len();
                                    for k in (hidx + 1..n).rev() {
                                        cleaned[k] = cleaned[k - 1];
                                    }
                                    cleaned[hidx] = '\0';
                                    hash = Some(hidx);
                                    space = hidx;
                                }
                                cleaned[space] = '\0';

                                // if (cReading->baseform == 0) { tag = '"'+base+'"';
                                //   base = tag.data(); }
                                if self.base.store.readings.get(c_reading.0).baseform.is_none() {
                                    let inner = cleaned_cstr(&cleaned, base_idx);
                                    tag.clear();
                                    tag.push('"');
                                    tag.push_str(&inner);
                                    tag.push('"');
                                    base_str = Some(tag.clone());
                                }
                                // if (base[0] == 0) { base = notag; warn; }
                                let cur_first = match &base_str {
                                    Some(s) => s.chars().next().unwrap_or('\0'),
                                    None => cleaned.get(base_idx).copied().unwrap_or('\0'),
                                };
                                if cur_first == '\0' {
                                    base_str = Some(String::from("_")); // notag {'_',0}
                                    tracing::warn!(
                                        "Warning: Line {} had empty tag.",
                                        self.base.numLines
                                    );
                                }
                                // Tag* tag2 = addTag(base);
                                let base_text = match &base_str {
                                    Some(s) => s.clone(),
                                    None => cleaned_cstr(&cleaned, base_idx),
                                };
                                let t = self.base.add_tag(&base_text, crate::tag::TagType::empty());
                                let (ttype, tfirst) = {
                                    let tg = self.base.grammar.single_tags_list.get(t.0);
                                    (tg.r#type, tg.tag.chars().next().unwrap_or('\0'))
                                };
                                if ttype.intersects(T_MAPPING)
                                    || tfirst == self.base.grammar.mapping_prefix
                                {
                                    mappings.push(t);
                                } else {
                                    self.base.add_tag_to_reading(c_reading, t);
                                }
                                // if (hash && hash[0] == 0) { ... new sub-reading ... }
                                if let Some(hidx) = hash
                                    && cleaned[hidx] == '\0'
                                {
                                    if let Some(wt) = wtag_tag {
                                        self.base.add_tag_to_reading(c_reading, wt);
                                    }
                                    let parent = self.base.store.readings.get(c_reading.0).parent;
                                    let nr = crate::reading::Reading::allocate_reading(
                                        &mut self.base.store,
                                        parent,
                                    );
                                    self.base.store.readings.get_mut(nr.0).next = Some(c_reading);
                                    c_reading = nr; // cReading = nr;
                                    space += 1; // ++space;
                                }
                            }
                            // base = ++space;
                            space += 1;
                            base_idx = space;
                            base_str = None;
                        }

                        // if (base && base[0]) — final trailing segment.
                        let base_first = match &base_str {
                            Some(s) => s.chars().next().unwrap_or('\0'),
                            None => cleaned.get(base_idx).copied().unwrap_or('\0'),
                        };
                        if base_first != '\0' {
                            if self.base.store.readings.get(c_reading.0).baseform.is_none() {
                                let inner = match &base_str {
                                    Some(s) => s.clone(),
                                    None => cleaned_cstr(&cleaned, base_idx),
                                };
                                tag.clear();
                                tag.push('"');
                                tag.push_str(&inner);
                                tag.push('"');
                                base_str = Some(tag.clone());
                            }
                            let base_text = match &base_str {
                                Some(s) => s.clone(),
                                None => cleaned_cstr(&cleaned, base_idx),
                            };
                            let t = self.base.add_tag(&base_text, crate::tag::TagType::empty());
                            let (ttype, tfirst) = {
                                let tg = self.base.grammar.single_tags_list.get(t.0);
                                (tg.r#type, tg.tag.chars().next().unwrap_or('\0'))
                            };
                            if ttype.intersects(T_MAPPING)
                                || tfirst == self.base.grammar.mapping_prefix
                            {
                                mappings.push(t);
                            } else {
                                self.base.add_tag_to_reading(c_reading, t);
                            }
                        }
                        if let Some(wt) = wtag_tag {
                            self.base.add_tag_to_reading(c_reading, wt);
                        }
                        // if (!cReading->baseform) { baseform = wordform->hash; warn }
                        if self.base.store.readings.get(c_reading.0).baseform.is_none() {
                            let wf = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                            let wf_hash = self.base.grammar.single_tags_list.get(wf.0).hash;
                            self.base.store.readings.get_mut(c_reading.0).baseform = Some(wf_hash);
                            tracing::warn!(
                                "Warning: Line {} had no valid baseform.",
                                self.base.numLines
                            );
                        }
                        // if (single_tags[baseform]->tag.size() == 2) { ... }
                        let bf_hash = self
                            .base
                            .store
                            .readings
                            .get(c_reading.0)
                            .baseform
                            .unwrap_or(TagHash(0));
                        let bf_size = {
                            let tid = tag_by_hash(&self.base.grammar, bf_hash);
                            self.base
                                .grammar
                                .single_tags_list
                                .get(tid.0)
                                .tag
                                .chars()
                                .count()
                        };
                        if bf_size == 2 {
                            self.base.del_tag_from_reading_hash(c_reading, bf_hash);
                            let wf = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                            let base = self.base.make_base_from_word(wf);
                            let h = self.base.grammar.single_tags_list.get(base.0).hash;
                            self.base.store.readings.get_mut(c_reading.0).baseform = Some(h);
                        }
                        if !mappings.is_empty() {
                            self.base.split_mappings(&mut mappings, cc, c_reading, true);
                        }
                        if self.base.grammar.sub_readings_ltr
                            && self.base.store.readings.get(c_reading.0).next.is_some()
                        {
                            c_reading = reverse_reading(&mut self.base.store, c_reading);
                        }
                        append_reading(&mut self.base.store, cc, c_reading);
                        self.base.numReadings = self.base.numReadings.wrapping_add(1);
                    }
                }
            }

            if is_text {
                self.istext(
                    fmt,
                    &line,
                    &cleaned,
                    output,
                    &mut c_swindow,
                    &mut c_cohort,
                    &mut l_swindow,
                    &mut l_cohort,
                    &mut did_soft_lookback,
                    reset_after,
                    lines,
                );
            }

            self.base.numLines = self.base.numLines.wrapping_add(1);
            line[0] = '\0';
            cleaned[0] = '\0';
            // Loop termination: the C++ `while(!input.eof())` re-check at the
            // top of the loop, using the EOF state sampled after get_line_clean.
            if hit_eof {
                break 'mainloop;
            }
        }

        // Trailing pending cohort at EOF.
        if let (Some(cc), Some(sw)) = (c_cohort, c_swindow) {
            {
                let base = &mut *self.base;
                append_cohort(&mut base.window, &mut base.store, sw, cc);
            }
            if self.base.store.cohorts.get(cc.0).readings.is_empty() {
                self.base.init_empty_cohort(cc);
            }
            let rs = self.base.store.cohorts.get(cc.0).readings.clone();
            for r in rs {
                let et = tag_by_hash(&self.base.grammar, self.base.cfg.endtag);
                self.base.add_tag_to_reading(r, et);
            }
        }

        // Drain buffered windows.
        while self.base.rotate_next().is_some() {
            self.base.run_grammar_on_window_with(fmt, output);
        }
        self.base.shuffle_windows_down();
        while !self.base.window.previous.is_empty() {
            let tmp = self.base.window.previous[0];
            fmt.print_single_window(&mut self.base, tmp, output, false);
            let t = Some(tmp);
            {
                let base = &mut *self.base;
                free_swindow(&mut base.window, &mut base.store, t);
            }
            self.base.window.previous.remove(0);
        }
        let _ = output.flush();
    }

    /// C++ `istext:` label body of `runGrammarOnText`. Runs for every non-cohort
    /// line: closes any pending cohort, handles the `is_conv` fast path, applies
    /// the soft/hard delimiter window breaks, allocates windows, drains the
    /// pipeline, and attaches trailing text.
    #[allow(clippy::too_many_arguments)]
    fn istext<F, W>(
        &mut self,
        fmt: &mut F,
        line: &[char],
        cleaned: &[char],
        output: &mut W,
        c_swindow: &mut Option<SwId>,
        c_cohort: &mut Option<CohortId>,
        l_swindow: &mut Option<SwId>,
        l_cohort: &mut Option<CohortId>,
        did_soft_lookback: &mut bool,
        reset_after: u32,
        lines: u32,
    ) where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        W: Write,
    {
        // if (cCohort && cCohort->readings.empty()) initEmptyCohort(*cCohort);
        if let Some(cc) = *c_cohort
            && self.base.store.cohorts.get(cc.0).readings.is_empty()
        {
            self.base.init_empty_cohort(cc);
        }

        // is_conv fast path.
        if self.base.cfg.is_conv {
            if let Some(cc) = *c_cohort {
                self.base.store.cohorts.get_mut(cc.0).local_number = 1;
                fmt.print_cohort(&mut self.base, cc, output, false);
                let opt = Some(cc);
                {
                    let base = &mut *self.base;
                    free_cohort(&mut base.store, Some(&mut base.window), opt);
                }
                *c_cohort = None;
            }
            if cleaned[0] != '\0' && line[0] != '\0' {
                let line_str: String = line.iter().take_while(|&&c| c != '\0').collect();
                fmt.print_plain_text_line(&mut self.base, &line_str, output);
            }
            return;
        }

        // Soft-limit lookback.
        if let Some(cs) = *c_swindow {
            let over_soft = self.base.store.single_windows.get(cs.0).cohorts.len() as u32
                >= self.base.cfg.soft_limit;
            if over_soft && self.base.grammar.soft_delimiters.is_some() && !*did_soft_lookback {
                *did_soft_lookback = true;
                let sd = self.base.grammar.sets_list[self.base.grammar.soft_delimiters.unwrap().0]
                    .number
                    .get();
                let cohorts = self.base.store.single_windows.get(cs.0).cohorts.clone();
                for &c in reversed(&cohorts) {
                    if self.base.does_set_match_cohort_normal(c, sd, None) {
                        *did_soft_lookback = false;
                        let cohort = self.base.delimit_at(cs, c);
                        // cSWindow = cohort->parent->next;
                        let parent = self.base.store.cohorts.get(cohort.0).parent.unwrap();
                        *c_swindow = self.base.store.single_windows.get(parent.0).next;
                        if let Some(cc) = *c_cohort {
                            self.base.store.cohorts.get_mut(cc.0).parent = *c_swindow;
                        }
                        // verbose soft-limit warning: discard sink.
                        break;
                    }
                }
            }
        }

        // Soft-delimiter on the current cohort.
        if let (Some(cc), Some(cs)) = (*c_cohort, *c_swindow) {
            let over_soft = self.base.store.single_windows.get(cs.0).cohorts.len() as u32
                >= self.base.cfg.soft_limit;
            let sd_hit = self.base.grammar.soft_delimiters.is_some() && {
                let sd = self.base.grammar.sets_list[self.base.grammar.soft_delimiters.unwrap().0]
                    .number
                    .get();
                self.base.does_set_match_cohort_normal(cc, sd, None)
            };
            if over_soft && sd_hit {
                let rs = self.base.store.cohorts.get(cc.0).readings.clone();
                for r in rs {
                    let et = tag_by_hash(&self.base.grammar, self.base.cfg.endtag);
                    self.base.add_tag_to_reading(r, et);
                }
                {
                    let base = &mut *self.base;
                    append_cohort(&mut base.window, &mut base.store, cs, cc);
                }
                *l_swindow = Some(cs);
                *l_cohort = Some(cc);
                *c_swindow = None;
                *did_soft_lookback = false;
            }
        }

        // Hard break.
        if let (Some(cc), Some(cs)) = (*c_cohort, *c_swindow) {
            let over_hard = self.base.store.single_windows.get(cs.0).cohorts.len() as u32
                >= self.base.cfg.hard_limit;
            let delim_hit =
                self.base.cfg.dep_delimit == 0 && self.base.grammar.delimiters.is_some() && {
                    let d = self.base.grammar.sets_list[self.base.grammar.delimiters.unwrap().0]
                        .number
                        .get();
                    self.base.does_set_match_cohort_normal(cc, d, None)
                };
            if over_hard || delim_hit {
                if !self.base.cfg.is_conv && over_hard {
                    let wf = self.base.store.cohorts.get(cc.0).wordform.unwrap();
                    let wftag = self.base.grammar.single_tags_list.get(wf.0).tag.clone();
                    tracing::warn!(
                        "Warning: Hard limit of {} cohorts reached at cohort {} (#{}) on line {} - forcing break.",
                        self.base.cfg.hard_limit,
                        wftag,
                        self.base.numCohorts,
                        self.base.numLines
                    );
                }
                let rs = self.base.store.cohorts.get(cc.0).readings.clone();
                for r in rs {
                    let et = tag_by_hash(&self.base.grammar, self.base.cfg.endtag);
                    self.base.add_tag_to_reading(r, et);
                }
                {
                    let base = &mut *self.base;
                    append_cohort(&mut base.window, &mut base.store, cs, cc);
                }
                *l_swindow = Some(cs);
                *l_cohort = Some(cc);
                *c_swindow = None;
                *did_soft_lookback = false;
            }
        }

        // No current window: allocate + init a fresh one.
        if c_swindow.is_none() {
            let sw = {
                let base = &mut *self.base;
                base.window.alloc_append_single_window(&mut base.store)
            };
            self.base.init_empty_single_window(sw);
            *l_swindow = Some(sw);
            // lCohort = cSWindow->cohorts[0];
            *l_cohort = self
                .base
                .store
                .single_windows
                .get(sw.0)
                .cohorts
                .first()
                .copied();
            *c_swindow = Some(sw);
            *c_cohort = None;
            self.base.numWindows = self.base.numWindows.wrapping_add(1);
            *did_soft_lookback = false;
        }

        // Pending cCohort: append it.
        if let (Some(cc), Some(cs)) = (*c_cohort, *c_swindow) {
            {
                let base = &mut *self.base;
                append_cohort(&mut base.window, &mut base.store, cs, cc);
            }
            *l_cohort = Some(cc);
        }

        // Drain a window if enough have queued up.
        if self.base.window.next.len() as u32 > self.base.cfg.num_windows {
            self.base.shuffle_windows_down();
            self.base.run_grammar_on_window_with(fmt, output);
            if self.base.numWindows.is_multiple_of(reset_after) {
                self.base.reset_indexes();
            }
            // verbose progress: discard sink.
            let _ = lines;
        }

        *c_cohort = None;

        // Attach trailing text.
        if cleaned[0] != '\0' && line[0] != '\0' {
            let line_str: String = line.iter().take_while(|&&c| c != '\0').collect();
            if let Some(lc) = *l_cohort {
                self.base
                    .store
                    .cohorts
                    .get_mut(lc.0)
                    .text
                    .push_str(&line_str);
            } else if let Some(ls) = *l_swindow {
                self.base
                    .store
                    .single_windows
                    .get_mut(ls.0)
                    .text
                    .push_str(&line_str);
            } else {
                fmt.print_plain_text_line(&mut self.base, &line_str, output);
            }
        }
    }
}

/// FST print-vtable state shared by `FormatConverter` input and output paths.
#[derive(Clone)]
pub struct FstFormat {
    pub did_warn_statictags: bool,
    pub wfactor: f64,
    pub wtag: UString,
    pub sub_delims: UString,
}

impl Default for FstFormat {
    fn default() -> Self {
        Self {
            did_warn_statictags: false,
            wfactor: 1.0,
            wtag: "W".to_string(),
            sub_delims: "#".to_string(),
        }
    }
}

impl FstFormat {
    fn from_app<B>(app: &FSTApplicator<B>) -> Self
    where
        B: DerefMut<Target = GrammarApplicator>,
    {
        Self {
            did_warn_statictags: app.did_warn_statictags,
            wfactor: app.wfactor,
            wtag: app.wtag.clone(),
            sub_delims: app.sub_delims.clone(),
        }
    }

    fn with_app<T>(
        &mut self,
        app: &mut GrammarApplicator,
        f: impl FnOnce(&mut FSTApplicator<&mut GrammarApplicator>) -> T,
    ) -> T {
        let mut fst = FSTApplicator::borrowing(app);
        fst.did_warn_statictags = self.did_warn_statictags;
        fst.wfactor = self.wfactor;
        fst.wtag.clone_from(&self.wtag);
        fst.sub_delims.clone_from(&self.sub_delims);
        let result = f(&mut fst);
        self.did_warn_statictags = fst.did_warn_statictags;
        result
    }
}

impl crate::grammar_applicator::stream_format::StreamFormat for FstFormat {
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

/// Read a NUL-terminated `UChar*` string starting at `start` out of the `cleaned`
/// scratch buffer as an owned `String` (the C++ `base`/`&cleaned[i]` reads).
fn cleaned_cstr(cleaned: &[char], start: usize) -> String {
    let mut s = String::new();
    let mut i = start;
    while i < cleaned.len() && cleaned[i] != '\0' {
        s.push(cleaned[i]);
        i += 1;
    }
    s
}
