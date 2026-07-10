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
//! ## UChar / NUL DIVERGENCE (faithful-with-a-flag)
//! RapidJSON `json::Value(cstr, allocator)` builds each string from a C string,
//! so a tag/text containing an embedded NUL (`\0`) is TRUNCATED at the NUL. This
//! port stores full Rust `String`s into `serde_json::Value::String`, which keep
//! everything after the NUL. Every such call site is flagged `DIVERGENCE(NUL)`.

use std::io::{BufRead, Read, Seek, Write};

use serde_json::{json, Map, Value};

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::grammar::Grammar;
use crate::grammar_applicator::GrammarApplicator;
use crate::sorted_vector::uint32SortedVector;
use crate::tag::{TagList, T_DEPENDENCY, T_MAPPING, T_RELATION};
use crate::types::{UString, UStringView};
use crate::uextras::u_fprintf;

/// C++ `grammar->single_tags[hash]` (operator[]) — resolve a hash to its
/// `TagId`. operator[] would default-insert a null `Tag*` on a miss (deref
/// crash); a miss here returns `TagId(0)` which cannot crash — benign for the
/// always-present hashes the call sites use. Reproduces
/// `grammar_applicator::core::tag_by_hash` (which is `pub(super)`).
fn tag_by_hash(grammar: &Grammar, hash: u32) -> TagId {
    let it = grammar.single_tags.find(hash);
    if it != grammar.single_tags.end() {
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

const CT_REMOVED: u8 = crate::cohort::CT_REMOVED;
const DEP_NO_PARENT: u32 = crate::cohort::DEP_NO_PARENT;

// [spec:cg3:def:jsonl-applicator.cg3.ustring-to-utf8-fn]
// [spec:cg3:sem:jsonl-applicator.cg3.ustring-to-utf8-fn]
/// C++ free fn `std::string ustring_to_utf8(UStringView ustr)`. In this UTF-8
/// port the internal representation is already UTF-8, so this is the identity on
/// the string content (the ICU two-pass `u_strToUTF8` preflight collapses to a
/// plain copy). Returns an owned `String`.
pub fn ustring_to_utf8(ustr: UStringView) -> String {
    ustr.to_string()
}

// [spec:cg3:def:jsonl-applicator.cg3.json-to-ustring-fn]
// [spec:cg3:sem:jsonl-applicator.cg3.json-to-ustring-fn]
/// C++ free fn `UString json_to_ustring(const json::Value& val)`. If `val` is a
/// JSON string, decode its UTF-8 bytes to the internal (UTF-8) representation;
/// for any non-string value (null / number / bool / array / object / missing),
/// return an empty string.
pub fn json_to_ustring(val: &Value) -> UString {
    match val {
        Value::String(s) => s.clone(),
        _ => UString::new(),
    }
}

// [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator]
/// C++ `class JsonlApplicator : public virtual GrammarApplicator` — modelled as
/// composition over [`GrammarApplicator`]. No JSONL-specific data members (the
/// C++ subclass adds none), so `base` is the only field.
pub struct JsonlApplicator {
    pub base: GrammarApplicator,
}

impl JsonlApplicator {
    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.jsonl-applicator-fn]
    /// C++ `JsonlApplicator::JsonlApplicator(std::ostream& ux_err)` — delegates to
    /// the base `GrammarApplicator(ux_err)` with an empty body and no subclass
    /// data. Here the caller constructs the base applicator (which owns the
    /// grammar); `new` just wraps it. (The C++ explicit empty destructor exists
    /// only to anchor the vtable and has no Rust analog.)
    pub fn new(base: GrammarApplicator) -> Self {
        JsonlApplicator { base }
    }

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
    fn build_json_tags(&self, reading: ReadingId) -> Vec<Value> {
        let mut tags_json: Vec<Value> = Vec::new();

        let (tags_list, baseform, parent_wf_hash) = {
            let r = self.base.store.readings.get(reading.0);
            let parent_wf_hash = r.parent.and_then(|cid| {
                self.base
                    .store
                    .cohorts
                    .get(cid.0)
                    .wordform
                    .map(|wf| self.base.grammar.single_tags_list.get(wf.0).hash)
            });
            (r.tags_list.clone(), r.baseform, parent_wf_hash)
        };

        let mut unique = uint32SortedVector::new();
        for tter in tags_list {
            if (!self.base.show_end_tags && tter == self.base.endtag) || tter == self.base.begintag {
                continue;
            }
            if tter == baseform || parent_wf_hash == Some(tter) {
                continue;
            }

            if self.base.unique_tags {
                if unique.find(tter) != unique.end() {
                    continue;
                }
                unique.insert(tter);
            }

            let tag = tag_by_hash(&self.base.grammar, tter);
            let (ttype, ttag) = {
                let t = self.base.grammar.single_tags_list.get(tag.0);
                (t.r#type, t.tag.clone())
            };

            if ttype & T_DEPENDENCY != 0 && self.base.has_dep && !self.base.dep_original {
                continue;
            }
            if ttype & T_RELATION != 0 && self.base.has_relations {
                continue;
            }

            // DIVERGENCE(NUL): RapidJSON truncates `tag->tag` at an embedded NUL;
            // this keeps the whole string.
            tags_json.push(Value::String(ustring_to_utf8(&ttag)));
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
    fn build_json_reading(&self, reading: ReadingId) -> Map<String, Value> {
        let mut reading_json = Map::new();

        // Baseform ("l").
        let baseform = self.base.store.readings.get(reading.0).baseform;
        let mut baseform_utf8 = String::new();
        if baseform != 0 {
            let it = self.base.grammar.single_tags.find(baseform);
            if it != self.base.grammar.single_tags.end() {
                let tid = it.get().1;
                let tag = &self.base.grammar.single_tags_list.get(tid.0).tag;
                // tag.size() >= 2 && tag.front()=='"' && tag.back()=='"'
                let chars: Vec<char> = tag.chars().collect();
                if chars.len() >= 2 && chars[0] == '"' && chars[chars.len() - 1] == '"' {
                    let inner: String = chars[1..chars.len() - 1].iter().collect();
                    baseform_utf8 = ustring_to_utf8(&inner);
                } else {
                    baseform_utf8 = ustring_to_utf8(tag);
                }
            }
        }
        // DIVERGENCE(NUL): RapidJSON truncates the c-string at NUL.
        reading_json.insert("l".to_string(), Value::String(baseform_utf8));

        // Tags ("ts").
        let tags_json = self.build_json_tags(reading);
        if !tags_json.is_empty() {
            reading_json.insert("ts".to_string(), Value::Array(tags_json));
        }

        // Subreading ("s").
        let next = self.base.store.readings.get(reading.0).next;
        if let Some(next) = next {
            let sub = self.build_json_reading(next);
            if !sub.is_empty() {
                reading_json.insert("s".to_string(), Value::Object(sub));
            }
        }

        reading_json
    }

    // =======================================================================
    // parseJsonReading / parseJsonCohort — deserialisation
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn]
    /// C++ `Reading* parseJsonReading(const json::Value& reading_obj, Cohort*
    /// parentCohort)`. Parses one reading object `{"l", "ts", "s"}`, recursing on
    /// subreadings; returns the new `ReadingId` or `None` on a non-object input.
    fn parse_json_reading<W: Write>(
        &mut self,
        reading_obj: &Value,
        parent_cohort: CohortId,
        output_err: &mut W,
    ) -> Option<ReadingId> {
        let obj = match reading_obj {
            Value::Object(m) => m,
            _ => {
                u_fprintf(
                    output_err,
                    format_args!(
                        "Error: Expected reading object, but got different type on line {}.\n",
                        self.base.numLines
                    ),
                );
                return None;
            }
        };

        let c_reading = crate::reading::alloc_reading(&mut self.base.store, Some(parent_cohort));
        // addTagToReading(*cReading, parentCohort->wordform); [Tag* overload]
        let wordform = self
            .base
            .store
            .cohorts
            .get(parent_cohort.0)
            .wordform
            .expect("parseJsonReading: cohort has no wordform");
        self.base.add_tag_to_reading(c_reading, wordform);

        // Baseform ("l").
        if let Some(l_val) = obj.get("l") {
            let base_str = json_to_ustring(l_val);
            if !base_str.is_empty() {
                let mut base_tag = UString::new();
                base_tag.push('"');
                base_tag.push_str(&base_str);
                base_tag.push('"');
                let tid = self.base.add_tag(&base_tag, 0);
                self.base.add_tag_to_reading(c_reading, tid);
            } else {
                u_fprintf(
                    output_err,
                    format_args!(
                        "Warning: Empty 'l' (baseform) in reading on line {}.\n",
                        self.base.numLines
                    ),
                );
            }
        } else {
            u_fprintf(
                output_err,
                format_args!(
                    "Warning: Reading missing 'l' (baseform) on line {}.\n",
                    self.base.numLines
                ),
            );
        }

        // Tags ("ts").
        if let Some(Value::Array(tags_arr)) = obj.get("ts") {
            let mut mappings: TagList = TagList::new();
            let mapping_prefix = self.base.grammar.mapping_prefix;
            for tag_val in tags_arr {
                let tag_str = json_to_ustring(tag_val);
                if !tag_str.is_empty() {
                    let tag = self.base.add_tag(&tag_str, 0);
                    let (ttype, first_char) = {
                        let t = self.base.grammar.single_tags_list.get(tag.0);
                        (t.r#type, tag_str.chars().next().unwrap_or('\0'))
                    };
                    if ttype & T_MAPPING != 0 || (!tag_str.is_empty() && first_char == mapping_prefix)
                    {
                        mappings.push(tag);
                    } else {
                        self.base.add_tag_to_reading(c_reading, tag);
                    }
                }
            }
            if !mappings.is_empty() {
                self.base
                    .split_mappings(&mut mappings, parent_cohort, c_reading, true);
            }
        }

        // Subreading ("s").
        if let Some(sub_reading_val) = obj.get("s") {
            if sub_reading_val.is_object() {
                let sub = self.parse_json_reading(sub_reading_val, parent_cohort, output_err);
                if let Some(sub) = sub {
                    self.base.store.readings.get_mut(c_reading.0).next = Some(sub);
                } else {
                    u_fprintf(
                        output_err,
                        format_args!(
                            "Error: Failed to parse subreading object on line {}.\n",
                            self.base.numLines
                        ),
                    );
                }
            } else {
                u_fprintf(
                    output_err,
                    format_args!(
                        "Warning: Value for 's' (sub_reading) is not an object on line {}. Skipping.\n",
                        self.base.numLines
                    ),
                );
            }
        }

        // Ensure baseform exists.
        if self.base.store.readings.get(c_reading.0).baseform == 0 {
            let wf_hash = self.base.grammar.single_tags_list.get(wordform.0).hash;
            self.base.store.readings.get_mut(c_reading.0).baseform = wf_hash;
            u_fprintf(
                output_err,
                format_args!(
                    "Warning: Reading on line {} ended up with no baseform. Using wordform.\n",
                    self.base.numLines
                ),
            );
        }

        Some(c_reading)
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn]
    /// C++ `void parseJsonCohort(const json::Value& obj, SingleWindow* cSWindow,
    /// Cohort*& cCohort)`. Parses one cohort object into a new cohort, assigning
    /// it into the returned value.
    fn parse_json_cohort<W: Write>(
        &mut self,
        obj: &Map<String, Value>,
        c_swindow: SwId,
        output_err: &mut W,
    ) -> CohortId {
        let c_cohort = crate::cohort::alloc_cohort(&mut self.base.store, Some(c_swindow));
        let gn = self.base.gWindow.cohort_counter;
        self.base.gWindow.cohort_counter = self.base.gWindow.cohort_counter.wrapping_add(1);
        self.base.store.cohorts.get_mut(c_cohort.0).global_number = gn;
        self.base.numCohorts = self.base.numCohorts.wrapping_add(1);

        // Wordform ("w").
        let wform_str = if let Some(w) = obj.get("w") {
            json_to_ustring(w)
        } else {
            u_fprintf(
                output_err,
                format_args!(
                    "Warning: JSON cohort on line {} missing 'w' (wordform). Using empty.\n",
                    self.base.numLines
                ),
            );
            UString::new()
        };
        let mut wform_tag = UString::new();
        wform_tag.push_str("\"<");
        wform_tag.push_str(&wform_str);
        wform_tag.push_str(">\"");
        let wf = self.base.add_tag(&wform_tag, 0);
        self.base.store.cohorts.get_mut(c_cohort.0).wordform = Some(wf);

        // Text ("z").
        self.base.store.cohorts.get_mut(c_cohort.0).wblank.clear();
        if let Some(z) = obj.get("z") {
            self.base.store.cohorts.get_mut(c_cohort.0).text = json_to_ustring(z);
        }

        // Static tags ("sts").
        if let Some(Value::Array(sts)) = obj.get("sts") {
            if self.base.store.cohorts.get(c_cohort.0).wread.is_none() {
                let wread = crate::reading::alloc_reading(&mut self.base.store, Some(c_cohort));
                self.base.store.cohorts.get_mut(c_cohort.0).wread = Some(wread);
                self.base.add_tag_to_reading(wread, wf);
                let wf_hash = self.base.grammar.single_tags_list.get(wf.0).hash;
                self.base.store.readings.get_mut(wread.0).baseform = wf_hash;
            }
            let wread = self.base.store.cohorts.get(c_cohort.0).wread.unwrap();
            for tag_val in sts {
                let tag_str = json_to_ustring(tag_val);
                if !tag_str.is_empty() {
                    let tag = self.base.add_tag(&tag_str, 0);
                    let hash = self.base.grammar.single_tags_list.get(tag.0).hash;
                    // Pushed directly to the list, NOT via addTagToReading.
                    self.base.store.readings.get_mut(wread.0).tags_list.push(hash);
                }
            }
        }

        // Readings ("rs").
        if let Some(Value::Array(readings_arr)) = obj.get("rs") {
            for reading_val in readings_arr {
                if !reading_val.is_object() {
                    u_fprintf(
                        output_err,
                        format_args!(
                            "Warning: Non-object found in 'rs' (readings) array on line {}. Skipping.\n",
                            self.base.numLines
                        ),
                    );
                    continue;
                }
                let c_reading = self.parse_json_reading(reading_val, c_cohort, output_err);
                if let Some(c_reading) = c_reading {
                    crate::cohort::append_reading(&mut self.base.store, c_cohort, c_reading);
                    self.base.numReadings = self.base.numReadings.wrapping_add(1);
                } else {
                    u_fprintf(
                        output_err,
                        format_args!(
                            "Error: Failed to parse main reading on line {}.\n",
                            self.base.numLines
                        ),
                    );
                }
            }
        }

        if self.base.store.cohorts.get(c_cohort.0).readings.is_empty() {
            self.base.init_empty_cohort(c_cohort);
        }
        crate::inlines::insert_if_exists(
            &mut self.base.store.cohorts.get_mut(c_cohort.0).possible_sets,
            self.base.grammar.sets_any.as_ref(),
        );

        // Dependency ("ds" / "dp").
        if let Some(ds) = obj.get("ds") {
            if let Some(v) = as_uint(ds) {
                self.base.store.cohorts.get_mut(c_cohort.0).dep_self = v;
            }
        }
        if let Some(dp) = obj.get("dp") {
            if let Some(v) = as_uint(dp) {
                self.base.store.cohorts.get_mut(c_cohort.0).dep_parent = v;
            }
        }

        // Deleted readings ("drs").
        if let Some(Value::Array(drs)) = obj.get("drs") {
            for dr_val in drs {
                if !dr_val.is_object() {
                    continue;
                }
                let del_r = self.parse_json_reading(dr_val, c_cohort, output_err);
                if let Some(del_r) = del_r {
                    self.base.store.readings.get_mut(del_r.0).deleted = true;
                    self.base.store.cohorts.get_mut(c_cohort.0).deleted.push(del_r);
                } else {
                    u_fprintf(
                        output_err,
                        format_args!(
                            "Error: Failed to parse deleted reading on line {}.\n",
                            self.base.numLines
                        ),
                    );
                }
            }
        }

        c_cohort
    }

    // =======================================================================
    // printStreamCommand / printPlainTextLine — single-object JSONL lines
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn]
    /// C++ `void printStreamCommand(UStringView cmd, std::ostream& output)`. Emits
    /// `{"cmd": <cmd>}` + `"\n"`. Does NOT flush.
    pub fn print_stream_command<W: Write>(&self, cmd: UStringView, output: &mut W) {
        // DIVERGENCE(NUL): RapidJSON truncates the c-string at NUL.
        let doc = json!({ "cmd": ustring_to_utf8(cmd) });
        let s = serde_json::to_string(&doc).unwrap();
        u_fprintf(output, format_args!("{s}\n"));
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn]
    /// C++ `void printPlainTextLine(UStringView line, std::ostream& output)`.
    /// Emits `{"t": <line>}` + `"\n"`. Does NOT flush. Newlines embedded in
    /// `line` are JSON-escaped by the writer, so the output stays one physical
    /// line.
    pub fn print_plain_text_line<W: Write>(&self, line: UStringView, output: &mut W) {
        // DIVERGENCE(NUL): RapidJSON truncates the c-string at NUL.
        let doc = json!({ "t": ustring_to_utf8(line) });
        let s = serde_json::to_string(&doc).unwrap();
        u_fprintf(output, format_args!("{s}\n"));
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
    pub fn print_cohort<W: Write>(&mut self, cohort: CohortId, output: &mut W, profiling: bool) {
        let (local_number, ctype) = {
            let c = self.base.store.cohorts.get(cohort.0);
            (c.local_number, c.r#type)
        };
        if local_number == 0 || (ctype & CT_REMOVED != 0) {
            return;
        }

        if !profiling {
            crate::cohort::unignore_all(&mut self.base.store, cohort);
        }

        let mut doc = Map::new();

        // Wordform ("w").
        let wform_tag = {
            let wf = self
                .base
                .store
                .cohorts
                .get(cohort.0)
                .wordform
                .expect("printCohort: cohort has no wordform");
            self.base.grammar.single_tags_list.get(wf.0).tag.clone()
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
                ustring_to_utf8(&inner)
            } else {
                ustring_to_utf8(&wform_tag)
            }
        };
        // DIVERGENCE(NUL).
        doc.insert("w".to_string(), Value::String(wform_utf8));

        // Static tags ("sts").
        let wread = self.base.store.cohorts.get(cohort.0).wread;
        if let Some(wread) = wread {
            let (tags_list, wf_hash) = {
                let tl = self.base.store.readings.get(wread.0).tags_list.clone();
                let wf_hash = self
                    .base
                    .store
                    .cohorts
                    .get(cohort.0)
                    .wordform
                    .map(|wf| self.base.grammar.single_tags_list.get(wf.0).hash);
                (tl, wf_hash)
            };
            if !tags_list.is_empty() {
                let mut static_tags_json: Vec<Value> = Vec::new();
                let mut unique_sts = uint32SortedVector::new();
                for tag_hash in tags_list {
                    if wf_hash == Some(tag_hash) {
                        continue;
                    }
                    if self.base.unique_tags {
                        if unique_sts.find(tag_hash) != unique_sts.end() {
                            continue;
                        }
                        unique_sts.insert(tag_hash);
                    }
                    let it = self.base.grammar.single_tags.find(tag_hash);
                    if it != self.base.grammar.single_tags.end() {
                        let tid = it.get().1;
                        let ttag = self.base.grammar.single_tags_list.get(tid.0).tag.clone();
                        // DIVERGENCE(NUL).
                        static_tags_json.push(Value::String(ustring_to_utf8(&ttag)));
                    }
                }
                if !static_tags_json.is_empty() {
                    doc.insert("sts".to_string(), Value::Array(static_tags_json));
                }
            }
        }

        // Text ("z").
        let text = self.base.store.cohorts.get(cohort.0).text.clone();
        if !text.is_empty() {
            let mut z_text = text;
            if z_text.ends_with('\n') {
                z_text.pop();
            }
            if !z_text.is_empty() {
                // DIVERGENCE(NUL).
                doc.insert("z".to_string(), Value::String(ustring_to_utf8(&z_text)));
            }
        }

        // Dependency ("ds" / "dp").
        if self.base.has_dep && (ctype & CT_REMOVED == 0) {
            let (dep_self, global_number, dep_parent) = {
                let c = self.base.store.cohorts.get(cohort.0);
                (c.dep_self, c.global_number, c.dep_parent)
            };
            let self_id = if dep_self == 0 { global_number } else { dep_self };
            doc.insert("ds".to_string(), json!(self_id));
            if dep_parent != DEP_NO_PARENT {
                doc.insert("dp".to_string(), json!(dep_parent));
            }
        }

        // Readings ("rs").
        let mut readings = self.base.store.cohorts.get(cohort.0).readings.clone();
        sort_readings(&self.base.store, &mut readings);
        self.base.store.cohorts.get_mut(cohort.0).readings = readings.clone();
        let mut readings_json: Vec<Value> = Vec::new();
        for reading in readings {
            if self.base.store.readings.get(reading.0).noprint {
                continue;
            }
            let reading_json = self.build_json_reading(reading);
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
        let deleted = self.base.store.cohorts.get(cohort.0).deleted.clone();
        if !deleted.is_empty() {
            let mut deleted_readings_json: Vec<Value> = Vec::new();
            let mut deleted_sorted = deleted;
            sort_readings(&self.base.store, &mut deleted_sorted);
            self.base.store.cohorts.get_mut(cohort.0).deleted = deleted_sorted.clone();
            for reading in deleted_sorted {
                // noprint flag NOT checked here (faithful).
                let reading_json = self.build_json_reading(reading);
                if !reading_json.is_empty() {
                    deleted_readings_json.push(Value::Object(reading_json));
                }
            }
            if !deleted_readings_json.is_empty() {
                doc.insert("drs".to_string(), Value::Array(deleted_readings_json));
            }
        }

        let s = serde_json::to_string(&Value::Object(doc)).unwrap();
        u_fprintf(output, format_args!("{s}\n"));
        let _ = output.flush();
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn]
    /// C++ `void printSingleWindow(SingleWindow* window, std::ostream& output,
    /// bool profiling)`. Emits: (1) SETVAR/REMVAR commands for `variables_output`;
    /// (2) pre-text; (3) each cohort; (4) post-text; (5) FLUSH if `flush_after`.
    pub fn print_single_window<W: Write>(&mut self, window: SwId, output: &mut W, profiling: bool) {
        let (vars_output, text, all_cohorts, text_post, flush_after) = {
            let w = self.base.store.single_windows.get(window.0);
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
                let key = tag_by_hash(&self.base.grammar, var);
                self.base.grammar.single_tags_list.get(key.0).tag.clone()
            };
            let value_hash: Option<u32> = {
                let w = self.base.store.single_windows.get(window.0);
                let it = w.variables_set.find(var);
                if it != w.variables_set.end() {
                    Some(it.get().1)
                } else {
                    None
                }
            };
            let mut cmd_buf = UString::new();
            match value_hash {
                Some(vh) => {
                    if vh != self.base.grammar.tag_any {
                        let value_tag = {
                            let value = tag_by_hash(&self.base.grammar, vh);
                            self.base.grammar.single_tags_list.get(value.0).tag.clone()
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
            self.print_stream_command(&cmd_buf, output);
        }

        // (2) Pre-text.
        if !text.is_empty() {
            self.print_plain_text_line(&text, output);
        }

        // (3) Cohorts.
        for cohort in all_cohorts {
            self.print_cohort(cohort, output, profiling);
        }

        // (4) Post-text.
        if !text_post.is_empty() {
            self.print_plain_text_line(&text_post, output);
        }

        // (5) Flush command.
        if flush_after {
            self.print_stream_command(STR_CMD_FLUSH, output);
        }
    }

    // =======================================================================
    // runGrammarOnText — the JSONL stream driver
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn]
    /// C++ `void runGrammarOnText(std::istream& input, std::ostream& output)`.
    /// Reads JSON-Lines input (one JSON object per line), builds windows, runs the
    /// grammar, and prints JSONL output.
    ///
    /// PORT NOTES:
    /// * `input` is `Read + Seek` (needs `Seek` for [`ux_strip_bom`]); line
    ///   reading uses a [`BufReader`](std::io::BufReader). The C++ `ux_stdin` /
    ///   `ux_stdout` assignments are elided (`Option<()>` placeholders). Output
    ///   validity checks (`!output`) have no analog.
    /// * The `good()`/`eof()`-guard `CG3Quit(1)` branches and the delimiter
    ///   warnings write to `ux_stderr` — deferred (placeholder); the fatal quits
    ///   are not triggered here.
    /// * `variables.clear()` (the member map) is reproduced; the LOCAL
    ///   `variables_set/rem/output` are NOT cleared on FLUSH (faithful).
    pub fn run_grammar_on_text<R, W>(&mut self, input: &mut R, output: &mut W)
    where
        R: Read + Seek,
        W: Write,
    {
        // ux_stdin/ux_stdout assignments elided (Option<()> placeholders).
        // good()/eof()/output/grammar validity CG3Quit(1) checks: deferred I/O.
        // No-delimiter warnings: deferred I/O.

        self.base.index();

        let reset_after: u32 = (self.base.num_windows + 4) * 2 + 1;

        let mut ignoreinput = false;
        let mut c_swindow: Option<SwId> = None;
        // `cCohort` is written after each cohort/delimit (faithful to the C++); the
        // reads are elided because the C++ `if (!cCohort)` null-check is
        // unreachable here (alloc always succeeds).
        #[allow(unused_assignments, unused_variables)]
        let mut c_cohort: Option<CohortId> = None;
        #[allow(unused_assignments)]
        let mut l_swindow: Option<SwId> = None;
        let mut l_cohort: Option<CohortId> = None;

        self.base.gWindow.window_span = self.base.num_windows;

        // LOCAL variable-tracking state.
        let mut variables_set = crate::flat_unordered_map::Uint32FlatHashMap::default();
        let mut variables_rem = crate::flat_unordered_set::Uint32FlatHashSet::default();
        let mut variables_output = uint32SortedVector::new();

        crate::uextras::ux_strip_bom(input);

        // getline loop. `BufReader::read_line` reads up to and including '\n'.
        let mut reader = std::io::BufReader::new(input);
        let mut exit_requested = false;

        'mainloop: loop {
            let mut line_str = String::new();
            let n = match reader.read_line(&mut line_str) {
                Ok(0) => break 'mainloop, // EOF: getline fails, loop ends.
                Ok(n) => n,
                Err(_) => break 'mainloop,
            };
            let _ = n;
            // std::getline strips the trailing '\n' (but keeps '\r' etc).
            if line_str.ends_with('\n') {
                line_str.pop();
            }

            self.base.numLines = self.base.numLines.wrapping_add(1);

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
                    u_fprintf(
                        output,
                        // Note: C++ writes to ux_stderr; output-stream placeholder
                        // stands in (no separate stderr handle threaded here).
                        format_args!(
                            "Warning: Failed to parse JSON on line {}: {} (offset {}). Skipping line.\n",
                            self.base.numLines,
                            e,
                            e.column()
                        ),
                    );
                    continue;
                }
            };

            let obj = match &doc {
                Value::Object(m) => m,
                _ => {
                    u_fprintf(
                        output,
                        format_args!(
                            "Warning: JSON on line {} is not an object. Skipping line.\n",
                            self.base.numLines
                        ),
                    );
                    continue;
                }
            };

            // Command handling.
            if let Some(cmd_v) = obj.get("cmd") {
                let cmd_ustr = json_to_ustring(cmd_v);
                if !cmd_ustr.is_empty() {
                    if cmd_ustr == STR_CMD_FLUSH {
                        // verbose Info line: deferred.
                        let back_swindow = self.base.gWindow.back();
                        if let Some(bsw) = back_swindow {
                            self.base.store.single_windows.get_mut(bsw.0).flush_after = true;
                        }

                        // If lCohort is the last cohort of cSWindow, add endtag.
                        if let (Some(lc), Some(sw)) = (l_cohort, c_swindow) {
                            let is_last = {
                                let cohorts = &self.base.store.single_windows.get(sw.0).cohorts;
                                !cohorts.is_empty() && *cohorts.last().unwrap() == lc
                            };
                            if is_last {
                                let rs = self.base.store.cohorts.get(lc.0).readings.clone();
                                for r in rs {
                                    self.add_endtag(r);
                                }
                            }
                        }

                        l_cohort = None;
                        c_swindow = None;
                        l_swindow = None;

                        // Drain buffered windows.
                        while !self.base.gWindow.next.is_empty() {
                            self.base.gWindow.shuffle_windows_down(&mut self.base.store);
                            self.base.run_grammar_on_window(output);
                            if self.base.numWindows % reset_after == 0 {
                                self.base.reset_indexes();
                            }
                            // verbose progress: deferred.
                        }
                        self.base.gWindow.shuffle_windows_down(&mut self.base.store);
                        while !self.base.gWindow.previous.is_empty() {
                            let tmp = self.base.gWindow.previous[0];
                            self.print_single_window(tmp, output, false);
                            let mut t = Some(tmp);
                            crate::single_window::free_swindow(
                                &mut self.base.gWindow,
                                &mut self.base.store,
                                &mut t,
                            );
                            self.base.gWindow.previous.remove(0);
                        }

                        if back_swindow.is_none() {
                            self.print_stream_command(&cmd_ustr, output);
                        }

                        self.base.variables.clear(0);
                        let _ = output.flush();
                        // u_fflush(*ux_stderr): deferred.
                    } else if cmd_ustr == STR_CMD_IGNORE {
                        ignoreinput = true;
                        self.print_stream_command(&cmd_ustr, output);
                    } else if cmd_ustr == STR_CMD_RESUME {
                        ignoreinput = false;
                        self.print_stream_command(&cmd_ustr, output);
                    } else if cmd_ustr == STR_CMD_EXIT {
                        self.print_stream_command(&cmd_ustr, output);
                        exit_requested = true;
                        break 'mainloop; // goto CGCMD_EXIT_JSONL
                    } else if cmd_ustr.starts_with(STR_CMD_SETVAR) {
                        // payload = cmd_ustr.substr(SETVAR.size(), size - SETVAR.size() - 1)
                        let payload = substr_strip_prefix_and_last(&cmd_ustr, STR_CMD_SETVAR);
                        let key_tag: TagId;
                        let value_hash: u32;
                        if let Some(eq) = payload.find('=') {
                            let key_str = &payload[..eq];
                            let value_str = &payload[eq + '='.len_utf8()..];
                            key_tag = self.base.add_tag(key_str, 0);
                            let vt = self.base.add_tag(value_str, 0);
                            value_hash = self.base.grammar.single_tags_list.get(vt.0).hash;
                        } else {
                            key_tag = self.base.add_tag(&payload, 0);
                            value_hash = self.base.grammar.tag_any;
                        }
                        let key_hash = self.base.grammar.single_tags_list.get(key_tag.0).hash;
                        // variables_set[key_hash] = value_hash; (operator[] overwrites)
                        *variables_set.index_or_insert(key_hash) = value_hash;
                        variables_rem.erase(key_hash);
                        variables_output.insert(key_hash);
                    } else if cmd_ustr.starts_with(STR_CMD_REMVAR) {
                        let payload = substr_strip_prefix_and_last(&cmd_ustr, STR_CMD_REMVAR);
                        let key_tag = self.base.add_tag(&payload, 0);
                        let key_hash = self.base.grammar.single_tags_list.get(key_tag.0).hash;
                        variables_set.erase(key_hash);
                        variables_rem.insert(key_hash);
                        variables_output.insert(key_hash);
                    }
                } else {
                    u_fprintf(
                        output,
                        format_args!("Warning: Empty 'cmd' value on line {}.\n", self.base.numLines),
                    );
                }
                continue;
            }

            // Ignore mode.
            if ignoreinput {
                if let Some(t_v) = obj.get("t") {
                    let t_ustr = json_to_ustring(t_v);
                    if !t_ustr.is_empty() {
                        self.print_plain_text_line(&t_ustr, output);
                    }
                }
                continue;
            }

            // Plain text: has "t" and NOT "w".
            if obj.contains_key("t") && !obj.contains_key("w") {
                let t_ustr = json_to_ustring(obj.get("t").unwrap());
                if !t_ustr.is_empty() {
                    // verbose Info: deferred.
                    if let Some(lc) = l_cohort {
                        self.base.store.cohorts.get_mut(lc.0).text.push_str(&t_ustr);
                    } else if let Some(lsw) = l_swindow {
                        self.base.store.single_windows.get_mut(lsw.0).text.push_str(&t_ustr);
                    } else {
                        self.print_plain_text_line(&t_ustr, output);
                    }
                } else {
                    u_fprintf(
                        output,
                        format_args!("Warning: Empty 't' value on line {}.\n", self.base.numLines),
                    );
                }
                continue;
            } else if obj.contains_key("w") {
                // Cohort.
                if c_swindow.is_none() {
                    let sw = self.base.gWindow.alloc_append_single_window(&mut self.base.store);
                    self.base.init_empty_single_window(sw);

                    // Transfer local variable state into the window, then clear
                    // locals. C++: `cSWindow->variables_set = variables_set;
                    // variables_set.clear();` — copy then clear. The window is
                    // freshly allocated (its maps/vec are empty), so swapping the
                    // local into the window and leaving the local empty is exactly
                    // equivalent to copy-then-clear. (FlatUnorderedMap/Set are not
                    // Clone; `swap` is the faithful, allocation-free transfer.)
                    {
                        let sww = self.base.store.single_windows.get_mut(sw.0);
                        sww.variables_set.swap(&mut variables_set);
                        sww.variables_rem.swap(&mut variables_rem);
                        sww.variables_output.swap(&mut variables_output);
                    }

                    self.base.numWindows = self.base.numWindows.wrapping_add(1);
                    c_swindow = Some(sw);
                    l_swindow = Some(sw);
                }

                let sw = c_swindow.unwrap();
                let cc = self.parse_json_cohort(obj, sw, output);
                c_cohort = Some(cc);
                // cCohort is never null in this port (alloc always succeeds), so the
                // "Failed to create cohort" branch is unreachable.

                crate::single_window::append_cohort(
                    &mut self.base.gWindow,
                    &mut self.base.store,
                    sw,
                    cc,
                );
                l_cohort = Some(cc);

                let mut did_delim = false;
                let cohorts_len = self.base.store.single_windows.get(sw.0).cohorts.len();
                let soft_hit = cohorts_len >= self.base.soft_limit as usize
                    && self.base.grammar.soft_delimiters.is_some()
                    && {
                        let sd = self.base.grammar.sets_list
                            [self.base.grammar.soft_delimiters.unwrap().0]
                            .number;
                        self.base.does_set_match_cohort_normal(cc, sd, None)
                    };
                if soft_hit {
                    // verbose Info: deferred.
                    let rs = self.base.store.cohorts.get(cc.0).readings.clone();
                    for r in rs {
                        self.add_endtag(r);
                    }
                    c_swindow = None;
                    c_cohort = None;
                    did_delim = true;
                } else {
                    let hard_hit = cohorts_len >= self.base.hard_limit as usize
                        || (self.base.grammar.delimiters.is_some() && {
                            let d = self.base.grammar.sets_list
                                [self.base.grammar.delimiters.unwrap().0]
                                .number;
                            self.base.does_set_match_cohort_normal(cc, d, None)
                        });
                    if hard_hit {
                        if cohorts_len >= self.base.hard_limit as usize {
                            u_fprintf(
                                output,
                                format_args!(
                                    "Warning: Hard limit of {} cohorts reached at line {} - forcing break.\n",
                                    self.base.hard_limit, self.base.numLines
                                ),
                            );
                        }
                        let rs = self.base.store.cohorts.get(cc.0).readings.clone();
                        for r in rs {
                            self.add_endtag(r);
                        }
                        c_swindow = None;
                        c_cohort = None;
                        did_delim = true;
                    }
                }

                if did_delim
                    || self.base.gWindow.next.len() > self.base.num_windows as usize
                {
                    self.base.gWindow.shuffle_windows_down(&mut self.base.store);
                    self.base.run_grammar_on_window(output);
                    if self.base.numWindows % reset_after == 0 {
                        self.base.reset_indexes();
                    }
                    // verbose progress: deferred.
                }
                c_cohort = None;
            }
        }

        // End of stream (skipped entirely on EXIT).
        if !exit_requested {
            if let Some(sw) = c_swindow {
                let cohorts = self.base.store.single_windows.get(sw.0).cohorts.clone();
                if let Some(&last) = cohorts.last() {
                    let rs = self.base.store.cohorts.get(last.0).readings.clone();
                    for r in rs {
                        self.add_endtag(r);
                    }
                }
            }

            while !self.base.gWindow.next.is_empty() {
                self.base.gWindow.shuffle_windows_down(&mut self.base.store);
                self.base.run_grammar_on_window(output);
            }
            if self.base.gWindow.current.is_some() {
                self.base.run_grammar_on_window(output);
            }

            self.base.gWindow.shuffle_windows_down(&mut self.base.store);
            while !self.base.gWindow.previous.is_empty() {
                let tmp = self.base.gWindow.previous[0];
                self.print_single_window(tmp, output, false);
                let mut t = Some(tmp);
                crate::single_window::free_swindow(
                    &mut self.base.gWindow,
                    &mut self.base.store,
                    &mut t,
                );
                self.base.gWindow.previous.remove(0);
            }

            let _ = output.flush();

            // Emit any still-pending GLOBAL variable commands.
            for &var in variables_output.as_slice() {
                let key = {
                    let key_tag = tag_by_hash(&self.base.grammar, var);
                    self.base.grammar.single_tags_list.get(key_tag.0).tag.clone()
                };
                let it = variables_set.find(var);
                let mut cmd_buf = UString::new();
                if it != variables_set.end() {
                    let val = it.get().1;
                    if val != self.base.grammar.tag_any {
                        let value = {
                            let vt = tag_by_hash(&self.base.grammar, val);
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
                self.print_stream_command(&cmd_buf, output);
            }
        }

        // CGCMD_EXIT_JSONL: verbose "Progress: ... - Done." line: deferred.
    }

    /// `addTagToReading(*iter, endtag)` — the C++ `uint32_t` overload: resolve the
    /// `endtag` hash to its `TagId` (via `grammar->single_tags[hash]`), then add.
    /// Not a manifest symbol — a helper deduplicating the repeated end-tagging.
    fn add_endtag(&mut self, reading: ReadingId) {
        let endtag_id = tag_by_hash(&self.base.grammar, self.base.endtag);
        self.base.add_tag_to_reading(reading, endtag_id);
    }
}

/// C++ `obj["ds"].IsUint()` + `GetUint()`: accept only a JSON unsigned integer
/// that fits in `u32`. serde_json numbers are queried via `as_u64`; reject
/// negatives / non-integers / out-of-range (RapidJSON `IsUint` is a 32-bit
/// unsigned check).
fn as_uint(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|u| u32::try_from(u).ok())
}

/// C++ `cmd_ustr.substr(prefix.size(), cmd_ustr.size() - prefix.size() - 1)` —
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
