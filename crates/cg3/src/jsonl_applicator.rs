//! Port of `src/JsonlApplicator.cpp` + `src/JsonlApplicator.hpp` — the JSON-Lines
//! (JSONL) stream applicator.
//!
//! ## Composition, not inheritance
//! C++ `class JsonlApplicator : public virtual GrammarApplicator`. Rust has no
//! virtual inheritance, so this is modelled as COMPOSITION: [`JsonlApplicator`]
//! holds a [`GrammarApplicator`](crate::grammar_applicator::GrammarApplicator) in
//! its `base` field and every method reaches the engine via `self.base.<method>`
//! and the engine state via `self.base.{grammar, store, gWindow, …}`. The C++
//! `override`s of `printCohort` / `printSingleWindow` / `printStreamCommand` /
//! `printPlainTextLine` / `runGrammarOnText` become inherent methods here; the
//! format-specific behaviour is dispatched by the caller ([`FormatConverter`])
//! rather than by a vtable.
//!
//! ## RapidJSON → serde_json
//! The C++ builds/parses JSON with RapidJSON; this port uses [`serde_json`]. The
//! emitted object SHAPE is reproduced key-for-key and in insertion order
//! (RapidJSON emits members in insertion order; serde_json's
//! [`Map`](serde_json::Map) with the crate's default is order-preserving because
//! this crate does not enable the `preserve_order` feature — actually serde_json
//! sorts by BTreeMap when `preserve_order` is off; see the DIVERGENCE note on
//! [`JsonlApplicator::print_cohort`]).
//!
//! ## NUL DIVERGENCE (faithful-with-a-flag)
//! RapidJSON `json::Value(cstr, allocator)` builds each string from a C string,
//! so a tag/text containing an embedded NUL (`\0`) is TRUNCATED at the NUL. This
//! port stores full Rust `String`s into `serde_json::Value::String`, which keep
//! everything after the NUL. Every such call site is flagged `DIVERGENCE(NUL)`.

use std::io::{BufRead, Read, Seek, Write};

use serde_json::{Map, Value, json};

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::grammar::Grammar;
use crate::grammar_applicator::{Engine, GrammarApplicator};
use crate::sorted_vector::Uint32SortedVector;
use crate::tag::{T_DEPENDENCY, T_MAPPING, T_RELATION, TagList};
use crate::types::TagHash;

/// C++ `grammar->single_tags[hash]` (operator[]) — resolve a hash to its
/// `TagId`. operator[] would default-insert a null `Tag*` on a miss (deref
/// crash); a miss here returns `TagId(0)` which cannot crash — benign for the
/// always-present hashes the call sites use. Reproduces
/// `grammar_applicator::core::tag_by_hash` (which is `pub(super)`).
fn tag_by_hash(grammar: &Grammar, hash: TagHash) -> TagId {
    let it = grammar.single_tags().find(hash.get());
    if it != grammar.single_tags().end() {
        it.get().1
    } else {
        TagId(0)
    }
}

// C++ stream-command string constants (`Strings.hpp`). Only SETVAR/REMVAR/FLUSH
// are defined in `grammar_applicator::core` (privately); JsonlApplicator also
// needs IGNORE/RESUME/EXIT, so the full set is declared here verbatim from the
// C++ `Strings.cpp` `STR_CMD_*` table.
const STR_CMD_FLUSH: &str = "<STREAMCMD:FLUSH>";
const STR_CMD_EXIT: &str = "<STREAMCMD:EXIT>";
const STR_CMD_IGNORE: &str = "<STREAMCMD:IGNORE>";
const STR_CMD_RESUME: &str = "<STREAMCMD:RESUME>";
const STR_CMD_SETVAR: &str = "<STREAMCMD:SETVAR:";
const STR_CMD_REMVAR: &str = "<STREAMCMD:REMVAR:";

const CT_REMOVED: crate::cohort::CohortType = crate::cohort::CT_REMOVED;

// [spec:cg3:def:jsonl-applicator.cg3.json-to-ustring-fn]
// [spec:cg3:sem:jsonl-applicator.cg3.json-to-ustring-fn]
/// C++ free fn `json_to_ustring`. If `val` is a
/// JSON string, decode its UTF-8 bytes to the internal (UTF-8) representation;
/// for any non-string value (null / number / bool / array / object / missing),
/// return an empty string.
pub fn json_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

// [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator]
/// C++ `class JsonlApplicator : public virtual GrammarApplicator` — modelled as
/// composition over [`GrammarApplicator`] (wave 4: the shared base is BORROWED,
/// matching the C++ shared virtual-base subobject). No JSONL-specific data
/// members (the C++ subclass adds none), so `base` is the only field.
pub struct JsonlApplicator<'a> {
    pub base: &'a mut GrammarApplicator,
}

impl<'a> JsonlApplicator<'a> {
    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
    /// C++ `JsonlApplicator::JsonlApplicator` — delegates to the base
    /// `GrammarApplicator` ctor with an empty body and no subclass
    /// data. Here the caller constructs the base applicator (which owns the
    /// grammar); `new` just wraps it. (The C++ explicit empty destructor exists
    /// only to anchor the vtable and has no Rust analog.)
    pub fn new(base: &'a mut GrammarApplicator) -> Self {
        JsonlApplicator { base }
    }

    // =======================================================================
    // parseJsonReading / parseJsonCohort — deserialisation
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn]
    /// C++ `Reading* parseJsonReading(const json::Value& reading_obj, Cohort*
    /// parentCohort)`. Parses one reading object `{"l", "ts", "s"}`, recursing on
    /// subreadings; returns the new `ReadingId` or `None` on a non-object input.
    /// `wordform` is `parent_cohort`'s, which [`Self::parse_json_cohort`] gave it.
    fn parse_json_reading(
        &mut self,
        reading_obj: &Value,
        parent_cohort: CohortId,
        wordform: TagId,
    ) -> Result<Option<ReadingId>, crate::error::RunError> {
        let obj = match reading_obj {
            Value::Object(m) => m,
            _ => {
                tracing::error!(
                    "Error: Expected reading object, but got different type on line {}.",
                    self.base.doc.num_lines
                );
                return Ok(None);
            }
        };

        let c_reading =
            crate::reading::alloc_reading(&mut self.base.doc.store, Some(parent_cohort));
        // addTagToReading(*cReading, parentCohort->wordform); [Tag* overload]
        self.base.engine().add_tag_to_reading(c_reading, wordform)?;

        // Baseform ("l").
        if let Some(l_val) = obj.get("l") {
            let base_str = json_to_string(l_val);
            if !base_str.is_empty() {
                let mut base_tag = String::new();
                base_tag.push('"');
                base_tag.push_str(&base_str);
                base_tag.push('"');
                let tid = self.base.add_tag(&base_tag, crate::tag::TagType::empty())?;
                self.base.engine().add_tag_to_reading(c_reading, tid)?;
            } else {
                tracing::warn!(
                    "Warning: Empty 'l' (baseform) in reading on line {}.",
                    self.base.doc.num_lines
                );
            }
        } else {
            tracing::warn!(
                "Warning: Reading missing 'l' (baseform) on line {}.",
                self.base.doc.num_lines
            );
        }

        // Tags ("ts").
        if let Some(Value::Array(tags_arr)) = obj.get("ts") {
            let mut mappings: TagList = TagList::new();
            let mapping_prefix = self.base.grammar.mapping_prefix;
            for tag_val in tags_arr {
                let tag_str = json_to_string(tag_val);
                if !tag_str.is_empty() {
                    let tag = self.base.add_tag(&tag_str, crate::tag::TagType::empty())?;
                    let ttype = self.base.grammar.tag_type(tag);
                    let first_char = tag_str.chars().next().unwrap_or('\0');
                    if ttype.intersects(T_MAPPING)
                        || (!tag_str.is_empty() && first_char == mapping_prefix)
                    {
                        mappings.push(tag);
                    } else {
                        self.base.engine().add_tag_to_reading(c_reading, tag)?;
                    }
                }
            }
            if !mappings.is_empty() {
                self.base
                    .engine()
                    .split_mappings(&mut mappings, parent_cohort, c_reading, true)?;
            }
        }

        // Subreading ("s").
        if let Some(sub_reading_val) = obj.get("s") {
            if sub_reading_val.is_object() {
                let sub = self.parse_json_reading(sub_reading_val, parent_cohort, wordform)?;
                if let Some(sub) = sub {
                    self.base.doc.store.readings.get_mut(c_reading.0).next = Some(sub);
                } else {
                    tracing::error!(
                        "Error: Failed to parse subreading object on line {}.",
                        self.base.doc.num_lines
                    );
                }
            } else {
                tracing::warn!(
                    "Warning: Value for 's' (sub_reading) is not an object on line {}. Skipping.",
                    self.base.doc.num_lines
                );
            }
        }

        // Ensure baseform exists.
        if self
            .base
            .doc
            .store
            .readings
            .get(c_reading.0)
            .baseform
            .is_none()
        {
            let wf_hash = self.base.grammar.single_tags_list.get(wordform.0).hash;
            self.base.doc.store.readings.get_mut(c_reading.0).baseform = Some(wf_hash);
            tracing::warn!(
                "Warning: Reading on line {} ended up with no baseform. Using wordform.",
                self.base.doc.num_lines
            );
        }

        Ok(Some(c_reading))
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+1]
    /// C++ `void parseJsonCohort(const json::Value& obj, SingleWindow* cSWindow,
    /// Cohort*& cCohort)`. Parses one cohort object into a new cohort, assigning
    /// it into the returned value.
    fn parse_json_cohort(
        &mut self,
        obj: &Map<String, Value>,
        c_swindow: SwId,
    ) -> Result<CohortId, crate::error::RunError> {
        let c_cohort = crate::cohort::alloc_cohort(&mut self.base.doc.store, Some(c_swindow));
        let gn = self.base.doc.cohorts.next_cohort_number();
        self.base
            .doc
            .store
            .cohorts
            .get_mut(c_cohort.0)
            .global_number = gn;
        self.base.doc.num_cohorts = self.base.doc.num_cohorts.wrapping_add(1);

        // Wordform ("w").
        let wform_str = if let Some(w) = obj.get("w") {
            json_to_string(w)
        } else {
            tracing::warn!(
                "Warning: JSON cohort on line {} missing 'w' (wordform). Using empty.",
                self.base.doc.num_lines
            );
            String::new()
        };
        let mut wform_tag = String::new();
        wform_tag.push_str("\"<");
        wform_tag.push_str(&wform_str);
        wform_tag.push_str(">\"");
        let wf = self
            .base
            .add_tag(&wform_tag, crate::tag::TagType::empty())?;
        self.base.doc.store.cohorts.get_mut(c_cohort.0).wordform = Some(wf);

        // Text ("z").
        self.base
            .doc
            .store
            .cohorts
            .get_mut(c_cohort.0)
            .wblank
            .clear();
        if let Some(z) = obj.get("z") {
            self.base.doc.store.cohorts.get_mut(c_cohort.0).text = json_to_string(z);
        }

        // Static tags ("sts").
        if let Some(Value::Array(sts)) = obj.get("sts") {
            let wread = self.static_reading(c_cohort, wf)?;
            for tag_val in sts {
                let tag_str = json_to_string(tag_val);
                if !tag_str.is_empty() {
                    let tag = self.base.add_tag(&tag_str, crate::tag::TagType::empty())?;
                    let hash = self.base.grammar.single_tags_list.get(tag.0).hash;
                    // Pushed directly to the list, NOT via addTagToReading.
                    self.base
                        .doc
                        .store
                        .readings
                        .get_mut(wread.0)
                        .tags_list
                        .push(hash.get());
                }
            }
        }

        // Readings ("rs").
        if let Some(Value::Array(readings_arr)) = obj.get("rs") {
            for reading_val in readings_arr {
                if !reading_val.is_object() {
                    tracing::warn!(
                        "Warning: Non-object found in 'rs' (readings) array on line {}. Skipping.",
                        self.base.doc.num_lines
                    );
                    continue;
                }
                let c_reading = self.parse_json_reading(reading_val, c_cohort, wf)?;
                if let Some(c_reading) = c_reading {
                    crate::cohort::append_reading(&mut self.base.doc.store, c_cohort, c_reading);
                    self.base.doc.num_readings = self.base.doc.num_readings.wrapping_add(1);
                } else {
                    tracing::error!(
                        "Error: Failed to parse main reading on line {}.",
                        self.base.doc.num_lines
                    );
                }
            }
        }

        if self
            .base
            .doc
            .store
            .cohorts
            .get(c_cohort.0)
            .readings
            .is_empty()
        {
            self.base.engine().init_empty_cohort(c_cohort)?;
        }
        crate::inlines::insert_if_exists(
            &mut self
                .base
                .doc
                .store
                .cohorts
                .get_mut(c_cohort.0)
                .possible_sets,
            self.base.grammar.sets_any.as_ref(),
        );

        self.parse_json_dependency(obj, c_cohort)?;

        // Deleted readings ("drs").
        if let Some(Value::Array(drs)) = obj.get("drs") {
            for dr_val in drs {
                if !dr_val.is_object() {
                    continue;
                }
                let del_r = self.parse_json_reading(dr_val, c_cohort, wf)?;
                if let Some(del_r) = del_r {
                    self.base.doc.store.readings.get_mut(del_r.0).deleted = true;
                    self.base
                        .doc
                        .store
                        .cohorts
                        .get_mut(c_cohort.0)
                        .deleted
                        .push(del_r);
                } else {
                    tracing::error!(
                        "Error: Failed to parse deleted reading on line {}.",
                        self.base.doc.num_lines
                    );
                }
            }
        }

        Ok(c_cohort)
    }

    /// `c_cohort`'s static reading, made on first use with the wordform `wf`
    /// as its baseform.
    fn static_reading(
        &mut self,
        c_cohort: CohortId,
        wf: TagId,
    ) -> Result<ReadingId, crate::error::RunError> {
        if let Some(wread) = self.base.doc.store.cohorts.get(c_cohort.0).wread {
            return Ok(wread);
        }
        let wread = crate::reading::alloc_reading(&mut self.base.doc.store, Some(c_cohort));
        self.base.doc.store.cohorts.get_mut(c_cohort.0).wread = Some(wread);
        self.base.engine().add_tag_to_reading(wread, wf)?;
        let wf_hash = self.base.grammar.single_tags_list.get(wf.0).hash;
        self.base.doc.store.readings.get_mut(wread.0).baseform = Some(wf_hash);
        Ok(wread)
    }

    // [spec:cg3:req:robustness.reserved-keys]
    /// `parseJsonCohort`'s dependency members: `"ds"` and `"dp"`, each when it
    /// is an unsigned 32-bit integer. They key and probe the window's
    /// dependency map, so a number that map reserves is refused; a `"dp"` of
    /// `u32::MAX` is `DEP_NO_PARENT` and stays.
    fn parse_json_dependency(
        &mut self,
        obj: &Map<String, Value>,
        c_cohort: CohortId,
    ) -> Result<(), crate::error::RunError> {
        use crate::error::{NumberRole, ReservedNumber};
        let ds = obj.get("ds").and_then(as_uint);
        let dp = obj.get("dp").and_then(as_uint);
        let (input, line) = (&self.base.cfg.input_name, self.base.doc.num_lines);
        let refuse = |source| crate::error::RunError::ReservedNumber {
            input: input.clone(),
            line,
            source,
        };
        if let Some(v) = ds {
            ReservedNumber::check("ds", NumberRole::DependencySelf, v).map_err(refuse)?;
        }
        if let Some(v) = dp {
            ReservedNumber::check("dp", NumberRole::DependencyParent, v).map_err(refuse)?;
        }
        let cohort = self.base.doc.store.cohorts.get_mut(c_cohort.0);
        if let Some(v) = ds {
            cohort.dep_self = (v != 0).then_some(crate::types::GlobalNumber(v));
        }
        if let Some(v) = dp {
            cohort.dep_parent =
                (v != crate::cohort::DEP_NO_PARENT).then_some(crate::types::GlobalNumber(v));
        }
        Ok(())
    }

    // =======================================================================
    // runGrammarOnText — the JSONL stream driver
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+1]
    /// C++ `void runGrammarOnText(std::istream& input, std::ostream& output)`.
    /// Reads JSON-Lines input (one JSON object per line), builds windows, runs the
    /// grammar, and prints JSONL output.
    ///
    /// PORT NOTES:
    /// * `input` is `Read + Seek` (needs `Seek` for [`crate::uextras::strip_bom`]); line
    ///   reading uses a [`BufReader`](std::io::BufReader). Output validity
    ///   checks (`!output`) have no analog.
    /// * The `good()`/`eof()`-guard `CG3Quit(1)` branches and the delimiter
    ///   warnings write to stderr — deferred; the fatal quits
    ///   are not triggered here.
    /// * `variables.clear()` (the member map) is reproduced; the LOCAL
    ///   `variables_set/rem/output` are NOT cleared on FLUSH (faithful).
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
        self.run_grammar_on_text_impl(fmt, input, output)
            .map_err(crate::error::Cg3Error::from)
    }

    fn run_grammar_on_text_impl<F, R, W>(
        &mut self,
        fmt: &mut F,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::RunError>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: Read + Seek,
        W: Write,
    {
        // good()/eof()/output/grammar validity CG3Quit(1) checks: deferred I/O.
        // No-delimiter warnings: deferred I/O.

        self.base.index();

        let reset_after: u32 = self.base.cfg.reset_after();

        let mut ignoreinput = false;
        let mut c_swindow: Option<SwId> = None;
        // C++ `cCohort` exists only for the `if (!cCohort)` "Failed to create
        // cohort" null-check, which is unreachable here (alloc always succeeds),
        // so no binding is kept for it; the cohort id is used directly (`cc`).
        let mut l_swindow: Option<SwId> = None;
        let mut l_cohort: Option<CohortId> = None;

        self.base.doc.stream.window_span = self.base.cfg.num_windows;

        // LOCAL variable-tracking state.
        let mut variables_set = crate::flat_unordered_map::Uint32FlatHashMap::default();
        let mut variables_rem = crate::flat_unordered_set::Uint32FlatHashSet::default();
        let mut variables_output = Uint32SortedVector::new();

        crate::uextras::strip_bom(input);

        // getline loop. `BufReader::read_line` reads up to and including '\n'.
        let mut reader = std::io::BufReader::new(input);
        let mut exit_requested = false;

        'mainloop: loop {
            let line_no = self.base.doc.num_lines.saturating_add(1);
            let Some(line_str) = read_jsonl_line(&mut reader, &self.base.cfg.input_name, line_no)?
            else {
                break 'mainloop; // EOF: getline fails, loop ends.
            };

            self.base.doc.num_lines = self.base.doc.num_lines.wrapping_add(1);

            // Skip empty / all-whitespace lines.
            if line_str.is_empty()
                || !line_str
                    .chars()
                    .any(|c| !matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r'))
            {
                continue;
            }

            let doc: Value = match serde_json::from_str(&line_str) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        "Warning: Failed to parse JSON on line {}: {} (offset {}). Skipping line.",
                        self.base.doc.num_lines,
                        e,
                        e.column()
                    );
                    continue;
                }
            };

            let obj = match &doc {
                Value::Object(m) => m,
                _ => {
                    tracing::warn!(
                        "Warning: JSON on line {} is not an object. Skipping line.",
                        self.base.doc.num_lines
                    );
                    continue;
                }
            };

            // Command handling.
            if let Some(cmd_v) = obj.get("cmd") {
                let cmd = json_to_string(cmd_v);
                if !cmd.is_empty() {
                    if cmd == STR_CMD_FLUSH {
                        // verbose Info line: deferred.
                        let back_swindow = self.base.doc.stream.back();
                        if let Some(bsw) = back_swindow {
                            self.base
                                .doc
                                .store
                                .single_windows
                                .get_mut(bsw.0)
                                .flush_after = true;
                        }

                        // If lCohort is the last cohort of cSWindow, add endtag.
                        if let (Some(lc), Some(sw)) = (l_cohort, c_swindow) {
                            let is_last = {
                                let cohorts = &self.base.doc.store.single_windows.get(sw.0).cohorts;
                                cohorts.last() == Some(&lc)
                            };
                            if is_last {
                                let rs = self.base.doc.store.cohorts.get(lc.0).readings.clone();
                                for r in rs {
                                    self.add_endtag(r)?;
                                }
                            }
                        }

                        l_cohort = None;
                        c_swindow = None;
                        l_swindow = None;

                        // Drain buffered windows.
                        while self.base.engine().rotate_next().is_some() {
                            self.base.engine().run_grammar_on_window_with(fmt, output)?;
                            if self.base.doc.num_windows.is_multiple_of(reset_after) {
                                self.base.reset_indexes();
                            }
                            // verbose progress: deferred.
                        }
                        self.base.engine().shuffle_windows_down();
                        while !self.base.doc.stream.previous.is_empty() {
                            let tmp = self.base.doc.stream.previous[0];
                            fmt.print_single_window(&mut self.base.engine(), tmp, output, false)?;
                            crate::single_window::free_swindow(
                                &mut self.base.doc.store,
                                &mut self.base.doc.cohorts,
                                &mut self.base.doc.deps,
                                Some(tmp),
                            );
                            self.base.doc.stream.previous.remove(0);
                        }

                        if back_swindow.is_none() {
                            fmt.print_stream_command(&mut self.base.engine(), &cmd, output);
                        }

                        self.base.doc.variables.clear(0);
                        let _ = output.flush();
                    } else if cmd == STR_CMD_IGNORE {
                        ignoreinput = true;
                        fmt.print_stream_command(&mut self.base.engine(), &cmd, output);
                    } else if cmd == STR_CMD_RESUME {
                        ignoreinput = false;
                        fmt.print_stream_command(&mut self.base.engine(), &cmd, output);
                    } else if cmd == STR_CMD_EXIT {
                        fmt.print_stream_command(&mut self.base.engine(), &cmd, output);
                        exit_requested = true;
                        break 'mainloop; // goto CGCMD_EXIT_JSONL
                    } else if cmd.starts_with(STR_CMD_SETVAR) {
                        // payload = cmd.substr(SETVAR.size(), size - SETVAR.size() - 1)
                        let payload = substr_strip_prefix_and_last(&cmd, STR_CMD_SETVAR);
                        self.setvar_command(
                            &payload,
                            &mut variables_set,
                            &mut variables_rem,
                            &mut variables_output,
                        )?;
                    } else if cmd.starts_with(STR_CMD_REMVAR) {
                        let payload = substr_strip_prefix_and_last(&cmd, STR_CMD_REMVAR);
                        self.remvar_command(
                            &payload,
                            &mut variables_set,
                            &mut variables_rem,
                            &mut variables_output,
                        )?;
                    }
                } else {
                    tracing::warn!(
                        "Warning: Empty 'cmd' value on line {}.",
                        self.base.doc.num_lines
                    );
                }
                continue;
            }

            // Ignore mode.
            if ignoreinput {
                if let Some(t_v) = obj.get("t") {
                    let text = json_to_string(t_v);
                    if !text.is_empty() {
                        fmt.print_plain_text_line(&mut self.base.engine(), &text, output);
                    }
                }
                continue;
            }

            // Plain text: has "t" and NOT "w".
            if let Some(t_v) = obj.get("t")
                && !obj.contains_key("w")
            {
                let text = json_to_string(t_v);
                if !text.is_empty() {
                    // verbose Info: deferred.
                    if let Some(lc) = l_cohort {
                        self.base
                            .doc
                            .store
                            .cohorts
                            .get_mut(lc.0)
                            .text
                            .push_str(&text);
                    } else if let Some(lsw) = l_swindow {
                        self.base
                            .doc
                            .store
                            .single_windows
                            .get_mut(lsw.0)
                            .text
                            .push_str(&text);
                    } else {
                        fmt.print_plain_text_line(&mut self.base.engine(), &text, output);
                    }
                } else {
                    tracing::warn!(
                        "Warning: Empty 't' value on line {}.",
                        self.base.doc.num_lines
                    );
                }
                continue;
            } else if obj.contains_key("w") {
                // Cohort.
                let sw = if let Some(sw) = c_swindow {
                    sw
                } else {
                    let sw = self
                        .base
                        .doc
                        .stream
                        .alloc_append_single_window(&mut self.base.doc.store);
                    self.base.engine().init_empty_single_window(sw)?;

                    // Transfer local variable state into the window, then clear
                    // locals. C++: `cSWindow->variables_set = variables_set;
                    // variables_set.clear();` — copy then clear. The window is
                    // freshly allocated (its maps/vec are empty), so swapping the
                    // local into the window and leaving the local empty is exactly
                    // equivalent to copy-then-clear. (FlatUnorderedMap/Set are not
                    // Clone; `swap` is the faithful, allocation-free transfer.)
                    {
                        let sww = self.base.doc.store.single_windows.get_mut(sw.0);
                        sww.variables_set.swap(&mut variables_set);
                        sww.variables_rem.swap(&mut variables_rem);
                        sww.variables_output.swap(&mut variables_output);
                    }

                    self.base.doc.num_windows = self.base.doc.num_windows.wrapping_add(1);
                    c_swindow = Some(sw);
                    l_swindow = Some(sw);
                    sw
                };
                let cc = self.parse_json_cohort(obj, sw)?;
                // cCohort is never null in this port (alloc always succeeds), so the
                // "Failed to create cohort" branch is unreachable.

                crate::single_window::append_cohort(
                    &mut self.base.doc.store,
                    &mut self.base.doc.cohorts,
                    &mut self.base.doc.deps,
                    sw,
                    cc,
                );
                l_cohort = Some(cc);

                let mut did_delim = false;
                let cohorts_len = self.base.doc.store.single_windows.get(sw.0).cohorts.len();
                let soft_delimiters = self.base.grammar.soft_delimiters;
                let soft_hit = cohorts_len >= self.base.cfg.soft_limit as usize
                    && self
                        .base
                        .engine()
                        .matches_delimiter_set(cc, soft_delimiters)?;
                if soft_hit {
                    // verbose Info: deferred.
                    let rs = self.base.doc.store.cohorts.get(cc.0).readings.clone();
                    for r in rs {
                        self.add_endtag(r)?;
                    }
                    c_swindow = None;
                    did_delim = true;
                } else {
                    let delimiters = self.base.grammar.delimiters;
                    let hard_hit = cohorts_len >= self.base.cfg.hard_limit as usize
                        || self.base.engine().matches_delimiter_set(cc, delimiters)?;
                    if hard_hit {
                        if cohorts_len >= self.base.cfg.hard_limit as usize {
                            tracing::warn!(
                                "Warning: Hard limit of {} cohorts reached at line {} - forcing break.",
                                self.base.cfg.hard_limit,
                                self.base.doc.num_lines
                            );
                        }
                        let rs = self.base.doc.store.cohorts.get(cc.0).readings.clone();
                        for r in rs {
                            self.add_endtag(r)?;
                        }
                        c_swindow = None;
                        did_delim = true;
                    }
                }

                if did_delim || self.base.doc.stream.next.len() > self.base.cfg.num_windows as usize
                {
                    self.run_next_window(fmt, output, reset_after)?;
                }
            }
        }

        // End of stream (skipped entirely on EXIT).
        if !exit_requested {
            if let Some(sw) = c_swindow {
                let cohorts = self.base.doc.store.single_windows.get(sw.0).cohorts.clone();
                if let Some(&last) = cohorts.last() {
                    let rs = self.base.doc.store.cohorts.get(last.0).readings.clone();
                    for r in rs {
                        self.add_endtag(r)?;
                    }
                }
            }

            while self.base.engine().rotate_next().is_some() {
                self.base.engine().run_grammar_on_window_with(fmt, output)?;
            }
            if self.base.doc.stream.current.is_some() {
                self.base.engine().run_grammar_on_window_with(fmt, output)?;
            }

            self.base.engine().shuffle_windows_down();
            while !self.base.doc.stream.previous.is_empty() {
                let tmp = self.base.doc.stream.previous[0];
                fmt.print_single_window(&mut self.base.engine(), tmp, output, false)?;
                crate::single_window::free_swindow(
                    &mut self.base.doc.store,
                    &mut self.base.doc.cohorts,
                    &mut self.base.doc.deps,
                    Some(tmp),
                );
                self.base.doc.stream.previous.remove(0);
            }

            let _ = output.flush();

            // Emit any still-pending GLOBAL variable commands.
            for &var in variables_output.as_slice() {
                let key = {
                    let key_tag = tag_by_hash(&self.base.grammar, TagHash(var));
                    self.base
                        .grammar
                        .single_tags_list
                        .get(key_tag.0)
                        .tag
                        .clone()
                };
                let it = variables_set.find(var);
                let mut cmd_buf = String::new();
                if it != variables_set.end() {
                    let val = it.get().1;
                    if val != self.base.grammar.tag_any {
                        let value = {
                            let vt = tag_by_hash(&self.base.grammar, TagHash(val));
                            self.base.grammar.single_tags_list.get(vt.0).tag.clone()
                        };
                        cmd_buf.push_str(STR_CMD_SETVAR);
                        cmd_buf.push_str(&key);
                        cmd_buf.push('=');
                        cmd_buf.push_str(&value);
                        cmd_buf.push('>');
                    } else {
                        cmd_buf.push_str(STR_CMD_SETVAR);
                        cmd_buf.push_str(&key);
                        cmd_buf.push('>');
                    }
                } else {
                    cmd_buf.push_str(STR_CMD_REMVAR);
                    cmd_buf.push_str(&key);
                    cmd_buf.push('>');
                }
                fmt.print_stream_command(&mut self.base.engine(), &cmd_buf, output);
            }
        }

        // CGCMD_EXIT_JSONL: verbose "Progress: ... - Done." line: deferred.
        Ok(())
    }

    /// `addTagToReading(*iter, endtag)` — the C++ `uint32_t` overload: resolve the
    /// `endtag` hash to its `TagId` (via `grammar->single_tags[hash]`), then add.
    /// Not a manifest symbol — a helper deduplicating the repeated end-tagging.
    fn add_endtag(&mut self, reading: ReadingId) -> Result<(), crate::error::RunError> {
        let endtag_id = tag_by_hash(&self.base.grammar, self.base.cfg.endtag);
        self.base.engine().add_tag_to_reading(reading, endtag_id)?;
        Ok(())
    }

    /// The hash of `text` interned as a tag, or of `*` when `text` is empty,
    /// with the CG reader's warning for the SETVAR `side` left empty.
    fn setvar_side_hash(&mut self, text: &str, side: &str) -> Result<u32, crate::error::RunError> {
        if text.is_empty() {
            tracing::warn!(
                "Warning: SETVAR on line {} had no {side}! Defaulting to *.",
                self.base.doc.num_lines
            );
            return Ok(self.base.grammar.tag_any);
        }
        let tag = self.base.add_tag(text, crate::tag::TagType::empty())?;
        Ok(self.base.grammar.single_tags_list.get(tag.0).hash.get())
    }

    // [spec:cg3:req:robustness.empty-tag]
    /// `<STREAMCMD:SETVAR:key[=value]>` with its prefix and closing `>`
    /// stripped: set `key` to `value`, or to `*` when there is no `=`.
    ///
    /// DIVERGENCE: the C++ interned an empty tag for an empty key or value.
    /// An empty key or value beside an `=` defaults to `*`, as the CG and
    /// Apertium readers' C++ does; a command with neither is skipped.
    fn setvar_command(
        &mut self,
        payload: &str,
        variables_set: &mut crate::flat_unordered_map::Uint32FlatHashMap,
        variables_rem: &mut crate::flat_unordered_set::Uint32FlatHashSet,
        variables_output: &mut Uint32SortedVector,
    ) -> Result<(), crate::error::RunError> {
        let (key_hash, value_hash) = match payload.split_once('=') {
            Some((key, value)) => (
                self.setvar_side_hash(key, "identifier before the =")?,
                self.setvar_side_hash(value, "value after the =")?,
            ),
            None if payload.is_empty() => return Ok(()),
            None => (
                self.setvar_side_hash(payload, "identifier")?,
                self.base.grammar.tag_any,
            ),
        };
        // variables_set[key_hash] = value_hash; (operator[] overwrites)
        *variables_set.index_or_insert(key_hash) = value_hash;
        variables_rem.erase(key_hash);
        variables_output.insert(key_hash);
        Ok(())
    }

    // [spec:cg3:req:robustness.empty-tag]
    /// `<STREAMCMD:REMVAR:key>` with its prefix and closing `>` stripped.
    ///
    /// DIVERGENCE: an empty key is skipped, as the CG and Apertium readers'
    /// C++ skips one; the JSONL C++ interned an empty tag.
    fn remvar_command(
        &mut self,
        payload: &str,
        variables_set: &mut crate::flat_unordered_map::Uint32FlatHashMap,
        variables_rem: &mut crate::flat_unordered_set::Uint32FlatHashSet,
        variables_output: &mut Uint32SortedVector,
    ) -> Result<(), crate::error::RunError> {
        if payload.is_empty() {
            return Ok(());
        }
        let key_tag = self.base.add_tag(payload, crate::tag::TagType::empty())?;
        let key_hash = self.base.grammar.single_tags_list.get(key_tag.0).hash.get();
        variables_set.erase(key_hash);
        variables_rem.insert(key_hash);
        variables_output.insert(key_hash);
        Ok(())
    }

    /// Shuffle the windows down one and run the grammar on the new current
    /// window, resetting the indexes every `reset_after` windows.
    ///
    /// DIVERGENCE: with `--num-windows 0` a delimiter can shuffle the only
    /// window out, leaving none current; the C++ ran the grammar on the null
    /// window and crashed. There is nothing to run, so nothing runs.
    fn run_next_window<F, W>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        reset_after: u32,
    ) -> Result<(), crate::error::RunError>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        W: Write,
    {
        self.base.engine().shuffle_windows_down();
        if self.base.doc.stream.current.is_none() {
            return Ok(());
        }
        self.base.engine().run_grammar_on_window_with(fmt, output)?;
        if self.base.doc.num_windows.is_multiple_of(reset_after) {
            self.base.reset_indexes();
        }
        // verbose progress: deferred.
        Ok(())
    }
}

// [spec:cg3:req:robustness.stream-invalid-utf8]
/// C++ `std::getline`: the next line of `reader` without its `'\n'`, or `None`
/// at end of stream (or on a read error, which ends `getline` too). A line
/// that is not UTF-8 is a run error naming `input` and `line`, not the end of
/// the stream.
fn read_jsonl_line<R: BufRead>(
    reader: &mut R,
    input: &str,
    line: u32,
) -> Result<Option<String>, crate::error::RunError> {
    let mut buf = Vec::new();
    match reader.read_until(b'\n', &mut buf) {
        Ok(0) | Err(_) => return Ok(None),
        Ok(_) => {}
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    String::from_utf8(buf).map(Some).map_err(|e| {
        let bad = e.utf8_error();
        let start = bad.valid_up_to();
        let end = bad.error_len().map_or(e.as_bytes().len(), |n| start + n);
        crate::uextras::InvalidUtf8 {
            bytes: e.as_bytes()[start..end].to_vec(),
        }
        .at(input, line)
    })
}

/// Jsonl print-vtable strategy (no persistent state).
#[derive(Default)]
pub struct JsonlFormat;

impl JsonlFormat {
    // =======================================================================
    // buildJsonTags / buildJsonReading — serialisation helpers
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn]
    /// C++ `void buildJsonTags(const Reading* reading, json::Value& tags_json,
    /// json::Document::AllocatorType& allocator)`. Fills a JSON array with the
    /// reading's printable tags (as UTF-8 strings), in `tags_list` order, applying
    /// the skip rules (`endtag`/`begintag`/baseform/wordform, `unique_tags`
    /// dedup, dependency/relation suppression). Returns the array of tag strings.
    fn build_json_tags_e(&self, e: &Engine<'_>, reading: ReadingId) -> Vec<Value> {
        let mut tags_json: Vec<Value> = Vec::new();

        let (tags_list, baseform, parent_wf_hash) = {
            let r = e.doc.store.readings.get(reading.0);
            let parent_wf_hash = r.parent.and_then(|cid| {
                e.doc
                    .store
                    .cohorts
                    .get(cid.0)
                    .wordform
                    .map(|wf| e.grammar.single_tags_list.get(wf.0).hash)
            });
            (
                r.tags_list.clone(),
                r.baseform.unwrap_or(TagHash(0)),
                parent_wf_hash,
            )
        };

        let mut unique = Uint32SortedVector::new();
        for tter in tags_list {
            let tter = TagHash(tter);
            if (!e.cfg.show_end_tags && tter == e.cfg.endtag) || tter == e.cfg.begintag {
                continue;
            }
            if tter == baseform || parent_wf_hash == Some(tter) {
                continue;
            }

            if e.cfg.unique_tags {
                if unique.find(tter.get()) != unique.end() {
                    continue;
                }
                unique.insert(tter.get());
            }

            let tag = tag_by_hash(e.grammar, tter);
            let ttype = e.grammar.tag_type(tag);
            let ttag = e.grammar.single_tags_list.get(tag.0).tag.clone();

            if ttype.intersects(T_DEPENDENCY) && e.doc.deps.has_dep && !e.cfg.dep_original {
                continue;
            }
            if ttype.intersects(T_RELATION) && e.doc.deps.has_relations {
                continue;
            }

            // DIVERGENCE(NUL): RapidJSON truncates `tag->tag` at an embedded NUL;
            // this keeps the whole string.
            tags_json.push(Value::String(ttag.to_string()));
        }
        tags_json
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn]
    /// C++ `void buildJsonReading(const Reading* reading, json::Value&
    /// reading_json, json::Document::AllocatorType& allocator)`. Builds the object
    /// `{"l": <baseform>, "ts": [<tags>], "s": {<subreading>}}` — `"l"` always
    /// present (empty string when no baseform); `"ts"`/`"s"` only when non-empty.
    /// Recurses on `reading->next`. The returned `Map` preserves insertion order
    /// (l, ts, s).
    ///
    /// The C++ recurses down the chain; this builds each sub-reading's object
    /// from the tail of the chain back to `reading`, nesting each under the
    /// one before it, so a chain of any length costs no stack.
    // [spec:cg3:req:robustness.depth-bounded]
    fn build_json_reading_e(&self, e: &Engine<'_>, reading: ReadingId) -> Map<String, Value> {
        let chain = crate::reading::sub_reading_chain(&e.doc.store.readings, reading);
        let mut sub: Option<Map<String, Value>> = None;
        for r in chain.into_iter().rev() {
            let mut reading_json = self.build_one_json_reading_e(e, r);
            // Subreading ("s").
            if let Some(sub) = sub.take()
                && !sub.is_empty()
            {
                reading_json.insert("s".to_string(), Value::Object(sub));
            }
            sub = Some(reading_json);
        }
        sub.unwrap_or_default()
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn]
    /// One reading of [`Self::build_json_reading_e`]'s chain: its `"l"` and
    /// `"ts"`, without its sub-reading.
    fn build_one_json_reading_e(&self, e: &Engine<'_>, reading: ReadingId) -> Map<String, Value> {
        let mut reading_json = Map::new();

        // Baseform ("l").
        let baseform = e
            .doc
            .store
            .readings
            .get(reading.0)
            .baseform
            .unwrap_or(TagHash(0));
        let mut baseform_utf8 = String::new();
        if baseform != TagHash(0) {
            let it = e.grammar.single_tags().find(baseform.get());
            if it != e.grammar.single_tags().end() {
                let tid = it.get().1;
                let tag = &e.grammar.single_tags_list.get(tid.0).tag;
                // tag.size() >= 2 && tag.front()=='"' && tag.back()=='"'
                let chars: Vec<char> = tag.chars().collect();
                if chars.len() >= 2 && chars[0] == '"' && chars[chars.len() - 1] == '"' {
                    let inner: String = chars[1..chars.len() - 1].iter().collect();
                    baseform_utf8 = inner.to_string();
                } else {
                    baseform_utf8 = tag.to_string();
                }
            }
        }
        // DIVERGENCE(NUL): RapidJSON truncates the c-string at NUL.
        reading_json.insert("l".to_string(), Value::String(baseform_utf8));

        // Tags ("ts").
        let tags_json = self.build_json_tags_e(e, reading);
        if !tags_json.is_empty() {
            reading_json.insert("ts".to_string(), Value::Array(tags_json));
        }

        reading_json
    }

    // =======================================================================
    // printStreamCommand / printPlainTextLine — single-object JSONL lines
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn]
    /// C++ `printStreamCommand`. Emits
    /// `{"cmd": <cmd>}` + `"\n"`. Does NOT flush.
    pub(crate) fn print_stream_command_e<W: Write>(&self, cmd: &str, output: &mut W) {
        // DIVERGENCE(NUL): RapidJSON truncates the c-string at NUL.
        let doc = json!({ "cmd": cmd.to_string() });
        let _ = writeln!(output, "{doc}");
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn]
    /// C++ `printPlainTextLine`.
    /// Emits `{"t": <line>}` + `"\n"`. Does NOT flush. Newlines embedded in
    /// `line` are JSON-escaped by the writer, so the output stays one physical
    /// line.
    pub(crate) fn print_plain_text_line_e<W: Write>(&self, line: &str, output: &mut W) {
        // DIVERGENCE(NUL): RapidJSON truncates the c-string at NUL.
        let doc = json!({ "t": line.to_string() });
        let _ = writeln!(output, "{doc}");
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn]
    /// C++ `void printCohort(Cohort* cohort, std::ostream& output, bool
    /// profiling)`. Serialises one cohort as one JSON object per line, in
    /// insertion order `w, sts, z, ds, dp, rs, drs`.
    ///
    /// DIVERGENCE(ORDER): RapidJSON emits members in insertion order. serde_json's
    /// [`Map`] is a `BTreeMap` unless the `preserve_order` feature is enabled
    /// (this crate does NOT enable it), so [`Value::Object`] serialises members in
    /// LEXICOGRAPHIC key order (`ds, dp, drs, rs, sts, w, z`) rather than
    /// insertion order. Flagged; a Wave-4 concern (enable `preserve_order` for
    /// byte-exact parity).
    pub(crate) fn print_cohort_e<W: Write>(
        &self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) {
        let (local_number, ctype) = {
            let c = e.doc.store.cohorts.get(cohort.0);
            (c.local_number, c.r#type)
        };
        if local_number == 0 || (ctype.intersects(CT_REMOVED)) {
            return;
        }

        if !profiling {
            crate::cohort::unignore_all(&mut e.doc.store, cohort);
        }

        let mut doc = Map::new();

        // Wordform ("w").
        let wform_tag = {
            #[expect(
                clippy::expect_used,
                reason = "every cohort gets a wordform where it is made (each stream reader, the >>> cohort in run_grammar, ADDCOHORT and the splitting rules in restructure); only cohort_clear resets it"
            )]
            let wf = e
                .doc
                .store
                .cohorts
                .get(cohort.0)
                .wordform
                .expect("printCohort: cohort has no wordform");
            e.grammar.single_tags_list.get(wf.0).tag.clone()
        };
        let wform_utf8 = {
            let chars: Vec<char> = wform_tag.chars().collect();
            // size() >= 4 && substr(0,2)=="\"<" && substr(size-2)==">\""
            if chars.len() >= 4
                && chars[0] == '"'
                && chars[1] == '<'
                && chars[chars.len() - 2] == '>'
                && chars[chars.len() - 1] == '"'
            {
                let inner: String = chars[2..chars.len() - 2].iter().collect();
                inner.to_string()
            } else {
                wform_tag.to_string()
            }
        };
        // DIVERGENCE(NUL).
        doc.insert("w".to_string(), Value::String(wform_utf8));

        // Static tags ("sts").
        let wread = e.doc.store.cohorts.get(cohort.0).wread;
        if let Some(wread) = wread {
            let (tags_list, wf_hash) = {
                let tl = e.doc.store.readings.get(wread.0).tags_list.clone();
                let wf_hash = e
                    .doc
                    .store
                    .cohorts
                    .get(cohort.0)
                    .wordform
                    .map(|wf| e.grammar.single_tags_list.get(wf.0).hash);
                (tl, wf_hash)
            };
            if !tags_list.is_empty() {
                let mut static_tags_json: Vec<Value> = Vec::new();
                let mut unique_sts = Uint32SortedVector::new();
                for tag_hash in tags_list {
                    if wf_hash == Some(TagHash(tag_hash)) {
                        continue;
                    }
                    if e.cfg.unique_tags {
                        if unique_sts.find(tag_hash) != unique_sts.end() {
                            continue;
                        }
                        unique_sts.insert(tag_hash);
                    }
                    let it = e.grammar.single_tags().find(tag_hash);
                    if it != e.grammar.single_tags().end() {
                        let tid = it.get().1;
                        let ttag = e.grammar.single_tags_list.get(tid.0).tag.clone();
                        // DIVERGENCE(NUL).
                        static_tags_json.push(Value::String(ttag.to_string()));
                    }
                }
                if !static_tags_json.is_empty() {
                    doc.insert("sts".to_string(), Value::Array(static_tags_json));
                }
            }
        }

        // Text ("z").
        let text = e.doc.store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() {
            let mut z_text = text;
            if z_text.ends_with('\n') {
                z_text.pop();
            }
            if !z_text.is_empty() {
                // DIVERGENCE(NUL).
                doc.insert("z".to_string(), Value::String(z_text.to_string()));
            }
        }

        // Dependency ("ds" / "dp").
        if e.doc.deps.has_dep && (!ctype.intersects(CT_REMOVED)) {
            let (dep_self, global_number, dep_parent) = {
                let c = e.doc.store.cohorts.get(cohort.0);
                (c.dep_self, c.global_number, c.dep_parent)
            };
            let self_id = dep_self.unwrap_or(global_number);
            doc.insert("ds".to_string(), json!(self_id.get()));
            if let Some(dp) = dep_parent {
                doc.insert("dp".to_string(), json!(dp.get()));
            }
        }

        // Readings ("rs").
        let mut readings = e.doc.store.cohorts.get(cohort.0).readings.clone();
        sort_readings(&e.doc.store, &mut readings);
        e.doc.store.cohorts.get_mut(cohort.0).readings = readings.clone();
        let mut readings_json: Vec<Value> = Vec::new();
        for reading in readings {
            if e.doc.store.readings.get(reading.0).noprint {
                continue;
            }
            let reading_json = self.build_json_reading_e(e, reading);
            if !reading_json.is_empty() {
                readings_json.push(Value::Object(reading_json));
            }
            // (Quirk: the `break` for single-best-reading is commented out in
            // C++, so ALL non-noprint readings are emitted.)
        }
        if !readings_json.is_empty() {
            doc.insert("rs".to_string(), Value::Array(readings_json));
        }

        // Deleted readings ("drs").
        let deleted = e.doc.store.cohorts.get(cohort.0).deleted.clone();
        if !deleted.is_empty() {
            let mut deleted_readings_json: Vec<Value> = Vec::new();
            let mut deleted_sorted = deleted;
            sort_readings(&e.doc.store, &mut deleted_sorted);
            e.doc.store.cohorts.get_mut(cohort.0).deleted = deleted_sorted.clone();
            for reading in deleted_sorted {
                // noprint flag NOT checked here (faithful).
                let reading_json = self.build_json_reading_e(e, reading);
                if !reading_json.is_empty() {
                    deleted_readings_json.push(Value::Object(reading_json));
                }
            }
            if !deleted_readings_json.is_empty() {
                doc.insert("drs".to_string(), Value::Array(deleted_readings_json));
            }
        }

        let doc = Value::Object(doc);
        let s = json_to_line(&doc);
        drop_json(doc);
        let _ = writeln!(output, "{s}");
        let _ = output.flush();
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn]
    /// C++ `void printSingleWindow(SingleWindow* window, std::ostream& output,
    /// bool profiling)`. Emits: (1) SETVAR/REMVAR commands for `variables_output`;
    /// (2) pre-text; (3) each cohort; (4) post-text; (5) FLUSH if `flush_after`.
    pub(crate) fn print_single_window_e<W: Write>(
        &self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        let (vars_output, text, all_cohorts, text_post, flush_after) = {
            let w = e.doc.store.single_windows.get(window.0);
            (
                w.variables_output.iter().copied().collect::<Vec<u32>>(),
                w.text.clone(),
                w.all_cohorts.clone(),
                w.text_post.clone(),
                w.flush_after,
            )
        };

        // (1) Variables as commands.
        for var in vars_output {
            let key_tag = {
                let key = tag_by_hash(e.grammar, TagHash(var));
                e.grammar.single_tags_list.get(key.0).tag.clone()
            };
            let value_hash: Option<u32> = {
                let w = e.doc.store.single_windows.get(window.0);
                let it = w.variables_set.find(var);
                if it != w.variables_set.end() {
                    Some(it.get().1)
                } else {
                    None
                }
            };
            let mut cmd_buf = String::new();
            match value_hash {
                Some(vh) => {
                    if vh != e.grammar.tag_any {
                        let value_tag = {
                            let value = tag_by_hash(e.grammar, TagHash(vh));
                            e.grammar.single_tags_list.get(value.0).tag.clone()
                        };
                        cmd_buf.push_str(STR_CMD_SETVAR);
                        cmd_buf.push_str(&key_tag);
                        cmd_buf.push('=');
                        cmd_buf.push_str(&value_tag);
                        cmd_buf.push('>');
                    } else {
                        cmd_buf.push_str(STR_CMD_SETVAR);
                        cmd_buf.push_str(&key_tag);
                        cmd_buf.push('>');
                    }
                }
                None => {
                    cmd_buf.push_str(STR_CMD_REMVAR);
                    cmd_buf.push_str(&key_tag);
                    cmd_buf.push('>');
                }
            }
            self.print_stream_command_e(&cmd_buf, output);
        }

        // (2) Pre-text.
        if !text.is_empty() {
            self.print_plain_text_line_e(&text, output);
        }

        // (3) Cohorts.
        for cohort in all_cohorts {
            self.print_cohort_e(e, cohort, output, profiling);
        }

        // (4) Post-text.
        if !text_post.is_empty() {
            self.print_plain_text_line_e(&text_post, output);
        }

        // (5) Flush command.
        if flush_after {
            self.print_stream_command_e(STR_CMD_FLUSH, output);
        }
    }
}

impl crate::grammar_applicator::stream_format::StreamFormat for JsonlFormat {
    fn print_cohort<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), crate::error::RunError> {
        self.print_cohort_e(e, cohort, output, profiling);
        Ok(())
    }

    fn print_single_window<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), crate::error::RunError> {
        self.print_single_window_e(e, window, output, profiling);
        Ok(())
    }

    fn print_stream_command<W: Write>(&mut self, _e: &mut Engine<'_>, cmd: &str, output: &mut W) {
        self.print_stream_command_e(cmd, output);
    }

    fn print_plain_text_line<W: Write>(&mut self, _e: &mut Engine<'_>, line: &str, output: &mut W) {
        self.print_plain_text_line_e(line, output);
    }
}

/// C++ `obj["ds"].IsUint()` + `GetUint()`: accept only a JSON unsigned integer
/// that fits in `u32`. serde_json numbers are queried via `as_u64`; reject
/// negatives / non-integers / out-of-range (RapidJSON `IsUint` is a 32-bit
/// unsigned check).
/// An object or array a [`json_to_line`] walk is inside, and whether it has
/// written a member yet.
enum JsonLevel<'a> {
    Object(serde_json::map::Iter<'a>, bool),
    Array(std::slice::Iter<'a, Value>, bool),
}

// [spec:cg3:req:robustness.depth-bounded]
/// `v` as serde_json's compact writer spells it, walked on a heap stack: a
/// cohort's object nests one level per sub-reading, and serde_json's own
/// writer recurses once per level. Scalars and keys are still written by
/// serde_json, so every byte is the one it writes.
fn json_to_line(v: &Value) -> String {
    let mut out = String::new();
    let mut levels: Vec<JsonLevel<'_>> = Vec::new();
    let mut pending = Some(v);
    loop {
        match pending.take() {
            Some(Value::Object(m)) => {
                out.push('{');
                levels.push(JsonLevel::Object(m.iter(), false));
            }
            Some(Value::Array(a)) => {
                out.push('[');
                levels.push(JsonLevel::Array(a.iter(), false));
            }
            Some(scalar) => out.push_str(&serde_json::to_string(scalar).unwrap_or_default()),
            None => {}
        }
        let Some(level) = levels.last_mut() else {
            return out;
        };
        pending = match level {
            JsonLevel::Object(members, written) => members.next().map(|(k, v)| {
                if std::mem::replace(written, true) {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                v
            }),
            JsonLevel::Array(items, written) => items.next().inspect(|_| {
                if std::mem::replace(written, true) {
                    out.push(',');
                }
            }),
        };
        if pending.is_none() {
            out.push(match levels.pop() {
                Some(JsonLevel::Object(..)) => '}',
                _ => ']',
            });
        }
    }
}

// [spec:cg3:req:robustness.depth-bounded]
/// Drop `v` a level at a time: serde_json's `Value` drops its members by
/// recursion, one level per sub-reading of a cohort's object.
fn drop_json(v: Value) {
    let mut values = vec![v];
    while let Some(v) = values.pop() {
        match v {
            Value::Object(m) => values.extend(m.into_iter().map(|(_, v)| v)),
            Value::Array(a) => values.extend(a),
            _ => {}
        }
    }
}

fn as_uint(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|u| u32::try_from(u).ok())
}

/// C++ `cmd.substr(prefix.size(), cmd.size() - prefix.size() - 1)` —
/// strip the leading `prefix` and the single final char (the assumed `>`).
fn substr_strip_prefix_and_last(cmd: &str, prefix: &str) -> String {
    // Work in chars to mirror the UTF-16-length arithmetic faithfully enough for
    // the ASCII prefixes/`>` used here.
    let chars: Vec<char> = cmd.chars().collect();
    let plen = prefix.chars().count();
    if chars.len() <= plen {
        return String::new();
    }
    // Drop the prefix and the last char.
    chars[plen..chars.len() - 1].iter().collect()
}

/// C++ `std::sort(list, Reading::cmp_number)` over a reading-id list, resolving
/// each id through `store`. Mirrors `grammar_applicator::core::sort_readings`
/// (which is private to that module).
fn sort_readings(store: &crate::store::RuntimeStore, list: &mut [ReadingId]) {
    list.sort_by(|&a, &b| {
        let ra = store.readings.get(a.0);
        let rb = store.readings.get(b.0);
        if crate::reading::Reading::cmp_number(ra, rb) {
            std::cmp::Ordering::Less
        } else if crate::reading::Reading::cmp_number(rb, ra) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
}
