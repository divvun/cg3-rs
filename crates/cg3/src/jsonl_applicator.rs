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
//! ## JSON
//! The C++ reads and writes JSON with RapidJSON; the port has its own small
//! reader and writer for it in [`json`]. A line is read as RapidJSON reads it,
//! so that a bad line's warning gives RapidJSON's message and offset and an
//! object keeps a repeated member name, and written as RapidJSON's `Writer`
//! writes it, byte for byte.

mod json;

use std::io::{BufRead, Read, Seek, Write};
use std::ops::ControlFlow;

use crate::arena::{CohortId, ReadingId, SwId, TagId};
use crate::error::{NumberRole, ReservedNumber, RunError};
use crate::flat_unordered_map::Uint32FlatHashMap;
use crate::flat_unordered_set::Uint32FlatHashSet;
use crate::grammar::Grammar;
use crate::grammar_applicator::stream_format::StreamFormat;
use crate::grammar_applicator::{Engine, GrammarApplicator};
use crate::sorted_vector::Uint32SortedVector;
use crate::strings::{
    STR_CMD_EXIT, STR_CMD_FLUSH, STR_CMD_IGNORE, STR_CMD_REMVAR, STR_CMD_RESUME, STR_CMD_SETVAR,
};
use crate::tag::{T_DEPENDENCY, T_MAPPING, T_RELATION, TagList, TagType};
use crate::types::{GlobalNumber, TagHash};

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

const CT_REMOVED: crate::cohort::CohortType = crate::cohort::CT_REMOVED;

// [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.get-json-string-fn]
// [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.get-json-string-fn]
/// C++ `bool getJsonString(const json::Value& obj, const char* key, UString&
/// out)`: the string member `key` of `obj`, read on input line `line`. `None`
/// when there is no such member, and with a warning when it is not a string.
fn get_json_string<'d>(obj: json::Value<'d>, key: &str, line: u32) -> Option<&'d str> {
    let value = obj.member(key)?;
    let s = value.as_str();
    if s.is_none() {
        tracing::warn!("Warning: '{key}' on line {line} is not a string - ignored.");
    }
    s
}

// [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator]
/// C++ `class JsonlApplicator : public virtual GrammarApplicator` — modelled as
/// composition over [`GrammarApplicator`], whose shared base is borrowed as the
/// C++ shares its virtual-base subobject. No JSONL-specific data members (the
/// C++ subclass adds none), so `base` is the only field.
pub struct JsonlApplicator<'a> {
    pub base: &'a mut GrammarApplicator,
}

/// `runGrammarOnText`'s local state, which the helpers the C++ writes as
/// lambdas share with its loop.
struct ReadState {
    /// The number of the line being read, or of the last one at the end.
    line: u32,
    reset_after: u32,
    ignoreinput: bool,
    did_soft_lookback: bool,
    /// C++ `cSWindow`: the window being filled, `None` once it is delimited.
    c_swindow: Option<SwId>,
    /// C++ `lSWindow` and `lCohort`: the last window and cohort, which take
    /// the text that follows them.
    l_swindow: Option<SwId>,
    l_cohort: Option<CohortId>,
    /// Variables set or removed since the last window was made.
    variables_set: Uint32FlatHashMap,
    variables_rem: Uint32FlatHashSet,
    variables_output: Uint32SortedVector,
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
    // parseJsonCohort / parseJsonReading — reading one cohort object
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.add-single-mapping-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.add-single-mapping-fn]
    /// C++ `void addSingleMapping(Reading& reading, TagList& mappings)`: give
    /// `reading`, which cannot be split, the first of `mappings`, warning for
    /// each of the others, and clear `mappings`.
    fn add_single_mapping(
        &mut self,
        reading: ReadingId,
        mappings: &mut TagList,
        line: u32,
    ) -> Result<(), RunError> {
        let Some(&first) = mappings.first() else {
            return Ok(());
        };
        for &tag in &mappings[1..] {
            tracing::warn!(
                "Warning: Mapping tag {} on line {line} will be discarded, as this reading can only have one.",
                self.base.grammar.single_tags_list[tag.0].tag
            );
        }
        self.base.engine().add_tag_to_reading(reading, first)?;
        mappings.clear();
        Ok(())
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-reading-fn+1]
    // [spec:cg3:req:robustness.depth-bounded]
    /// C++ `Reading* parseJsonReading(const json::Value& reading_obj, Cohort*
    /// cohort, TagList& mappings)`: a reading of `cohort`, whose wordform is
    /// `wordform`, with its chain of sub-readings. Its own mapping tags are
    /// appended to `mappings` for the caller.
    ///
    /// The C++ recurses on `"s"`. This reads each reading's own members going
    /// down the chain, then finishes the readings from the deepest back up,
    /// which is the order the recursion finishes them in, so the warnings come
    /// out in the same order and a chain of any depth costs no stack.
    fn parse_json_reading(
        &mut self,
        reading_obj: json::Value<'_>,
        cohort: CohortId,
        wordform: TagId,
        mappings: &mut TagList,
        line: u32,
    ) -> Result<ReadingId, RunError> {
        let top = self.parse_json_reading_tags(reading_obj, cohort, wordform, mappings, line)?;
        let mut subs: Vec<(ReadingId, TagList)> = Vec::new();
        let mut sub_obj = json_sub_reading(reading_obj, line);
        while let Some(obj) = sub_obj {
            let mut sub_mappings = TagList::new();
            let sub =
                self.parse_json_reading_tags(obj, cohort, wordform, &mut sub_mappings, line)?;
            subs.push((sub, sub_mappings));
            sub_obj = json_sub_reading(obj, line);
        }
        let mut below: Option<(ReadingId, TagList)> = None;
        for (reading, own_mappings) in subs.into_iter().rev() {
            if let Some((sub, sub_mappings)) = below.take() {
                self.attach_sub_reading(reading, sub, sub_mappings, line)?;
            }
            self.finish_json_reading(reading, wordform, line);
            below = Some((reading, own_mappings));
        }
        if let Some((sub, sub_mappings)) = below {
            self.attach_sub_reading(top, sub, sub_mappings, line)?;
        }
        self.finish_json_reading(top, wordform, line);
        Ok(top)
    }

    /// `parseJsonReading`'s steps up to its `"s"`: the reading, its wordform,
    /// its `"l"` and its `"ts"`.
    fn parse_json_reading_tags(
        &mut self,
        reading_obj: json::Value<'_>,
        cohort: CohortId,
        wordform: TagId,
        mappings: &mut TagList,
        line: u32,
    ) -> Result<ReadingId, RunError> {
        let reading = crate::reading::alloc_reading(&mut self.base.doc.store, Some(cohort));
        self.base.engine().add_tag_to_reading(reading, wordform)?;

        if reading_obj.member("l").is_none() {
            tracing::warn!("Warning: Reading missing 'l' (baseform) on line {line}.");
        } else if let Some(l) = get_json_string(reading_obj, "l", line) {
            if l.is_empty() {
                tracing::warn!("Warning: Empty 'l' (baseform) in reading on line {line}.");
            } else {
                let tag = self.base.add_tag(&format!("\"{l}\""), TagType::empty())?;
                self.base.engine().add_tag_to_reading(reading, tag)?;
            }
        }

        if let Some(ts) = reading_obj.member("ts") {
            let Some(tags) = ts.elements() else {
                tracing::warn!("Warning: 'ts' (tags) on line {line} is not an array - ignored.");
                return Ok(reading);
            };
            let mapping_prefix = self.base.grammar.mapping_prefix;
            for tag_val in tags {
                let Some(text) = tag_val.as_str() else {
                    tracing::warn!(
                        "Warning: Non-string found in 'ts' (tags) array on line {line} - skipping."
                    );
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                let tag = self.base.add_tag(text, TagType::empty())?;
                let first = self.base.grammar.single_tags_list[tag.0].tag.chars().next();
                if self.base.grammar.tag_type(tag).intersects(T_MAPPING)
                    || first == Some(mapping_prefix)
                {
                    self.base.grammar.tag_type_insert(tag, T_MAPPING);
                    mappings.push(tag);
                } else {
                    self.base.engine().add_tag_to_reading(reading, tag)?;
                }
            }
        }
        Ok(reading)
    }

    /// `parseJsonReading`'s `cReading->next = sub` and the
    /// `addSingleMapping(*cReading->next, sub_mappings)` after it.
    fn attach_sub_reading(
        &mut self,
        reading: ReadingId,
        sub: ReadingId,
        mut sub_mappings: TagList,
        line: u32,
    ) -> Result<(), RunError> {
        self.base.doc.store.readings.get_mut(reading.0).next = Some(sub);
        self.add_single_mapping(sub, &mut sub_mappings, line)
    }

    /// `parseJsonReading`'s steps after its `"s"`: a reading with no baseform
    /// takes the wordform's, and the reading is rehashed, since its hash
    /// covers the sub-reading attached after its tags were added.
    fn finish_json_reading(&mut self, reading: ReadingId, wordform: TagId, line: u32) {
        if self
            .base
            .doc
            .store
            .readings
            .get(reading.0)
            .baseform
            .is_none()
        {
            let hash = self.base.grammar.single_tags_list[wordform.0].hash;
            self.base.doc.store.readings.get_mut(reading.0).baseform = Some(hash);
            tracing::warn!(
                "Warning: Reading on line {line} ended up with no baseform. Using wordform."
            );
        }
        crate::reading::reading_rehash(
            &mut self.base.doc.store.readings,
            &self.base.grammar,
            reading,
        );
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+2]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.parse-json-cohort-fn+2]
    /// C++ `Cohort* parseJsonCohort(const json::Value& obj, SingleWindow*
    /// cSWindow)`: a cohort of `c_swindow` built from the object `obj` read on
    /// input line `line`, whose `"w"` the caller has read as `wform`. The
    /// caller appends it to the window.
    fn parse_json_cohort(
        &mut self,
        obj: json::Value<'_>,
        wform: &str,
        c_swindow: SwId,
        line: u32,
    ) -> Result<CohortId, RunError> {
        let c_cohort = crate::cohort::alloc_cohort(&mut self.base.doc.store, Some(c_swindow));
        let global_number = self.base.doc.cohorts.next_cohort_number();
        {
            let c = self.base.doc.store.cohorts.get_mut(c_cohort.0);
            c.global_number = global_number;
            c.line_number = line;
        }
        self.base.doc.num_cohorts = self.base.doc.num_cohorts.wrapping_add(1);

        let wordform = self
            .base
            .add_tag(&format!("\"<{wform}>\""), TagType::empty())?;
        self.base.doc.store.cohorts.get_mut(c_cohort.0).wordform = Some(wordform);

        if let Some(wb) = get_json_string(obj, "wb", line) {
            self.base.doc.store.cohorts.get_mut(c_cohort.0).wblank = wb.to_string();
        }
        // The CG reader keeps each line's newline, and the writer drops the
        // last one, so it is put back; an empty "z" is a blank line.
        if let Some(z) = get_json_string(obj, "z", line) {
            self.base.doc.store.cohorts.get_mut(c_cohort.0).text = format!("{z}\n");
        }

        self.parse_json_static_tags(obj, c_cohort, wordform, line)?;
        self.parse_json_readings(obj, c_cohort, wordform, line)?;
        // Before `init_empty_cohort`, since `set_related` would make the
        // placeholder reading printable.
        self.parse_json_relations(obj, c_cohort, global_number, line)?;

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

        self.parse_json_deleted_readings(obj, c_cohort, wordform, line)?;
        self.parse_json_dependency(obj, c_cohort, line)?;
        Ok(c_cohort)
    }

    /// `parseJsonCohort`'s `"sts"`: each non-empty string is a tag of the
    /// cohort's static reading, made with the wordform on first use.
    fn parse_json_static_tags(
        &mut self,
        obj: json::Value<'_>,
        c_cohort: CohortId,
        wordform: TagId,
        line: u32,
    ) -> Result<(), RunError> {
        let Some(sts) = obj.member("sts") else {
            return Ok(());
        };
        let Some(tags) = sts.elements() else {
            tracing::warn!(
                "Warning: 'sts' (static tags) on line {line} is not an array - ignored."
            );
            return Ok(());
        };
        for tag_val in tags {
            let Some(text) = tag_val.as_str() else {
                tracing::warn!(
                    "Warning: Non-string found in 'sts' (static tags) array on line {line} - skipping."
                );
                continue;
            };
            if text.is_empty() {
                continue;
            }
            let wread = match self.base.doc.store.cohorts.get(c_cohort.0).wread {
                Some(wread) => wread,
                None => {
                    let wread =
                        crate::reading::alloc_reading(&mut self.base.doc.store, Some(c_cohort));
                    self.base.doc.store.cohorts.get_mut(c_cohort.0).wread = Some(wread);
                    self.base.engine().add_tag_to_reading(wread, wordform)?;
                    wread
                }
            };
            let tag = self.base.add_tag(text, TagType::empty())?;
            self.base.engine().add_tag_to_reading(wread, tag)?;
        }
        Ok(())
    }

    /// `parseJsonCohort`'s `"rs"`: each object is a reading, and a reading
    /// with several mapping tags is then split into one reading per tag.
    fn parse_json_readings(
        &mut self,
        obj: json::Value<'_>,
        c_cohort: CohortId,
        wordform: TagId,
        line: u32,
    ) -> Result<(), RunError> {
        let Some(rs) = obj.member("rs") else {
            return Ok(());
        };
        let Some(readings) = rs.elements() else {
            tracing::warn!("Warning: 'rs' (readings) on line {line} is not an array - ignored.");
            return Ok(());
        };
        let mut all_mappings = crate::grammar_applicator::AllMappings::new();
        for reading_val in readings {
            if !reading_val.is_object() {
                tracing::warn!(
                    "Warning: Non-object found in 'rs' (readings) array on line {line} - skipping."
                );
                continue;
            }
            let mut mappings = TagList::new();
            let reading =
                self.parse_json_reading(reading_val, c_cohort, wordform, &mut mappings, line)?;
            crate::cohort::append_reading(&mut self.base.doc.store, c_cohort, reading);
            if !mappings.is_empty() {
                all_mappings.insert(reading, mappings);
            }
            self.base.doc.num_readings = self.base.doc.num_readings.wrapping_add(1);
        }
        self.base
            .engine()
            .split_all_mappings(&mut all_mappings, c_cohort, true)
    }

    // [spec:cg3:req:robustness.reserved-keys]
    // [spec:cg3:req:robustness.empty-tag]
    /// `parseJsonCohort`'s `"id"` and `"rels"`. The id keys the relation map
    /// and each relation target probes it, so a number the map reserves is
    /// refused; a relation with an empty name is refused as an empty tag.
    fn parse_json_relations(
        &mut self,
        obj: json::Value<'_>,
        c_cohort: CohortId,
        global_number: GlobalNumber,
        line: u32,
    ) -> Result<(), RunError> {
        if let Some(id) = obj.member("id") {
            match id.as_uint() {
                Some(id) if id != 0 => {
                    self.refuse_reserved("id", NumberRole::RelationId, id)?;
                    *self.base.doc.deps.relation_map.index_or_insert(id) = global_number.get();
                }
                _ => tracing::warn!(
                    "Warning: 'id' on line {line} is not a positive integer - ignored."
                ),
            }
        }

        let Some(rels) = obj.member("rels") else {
            return Ok(());
        };
        let Some(relations) = rels.members() else {
            tracing::warn!(
                "Warning: 'rels' (relations) on line {line} is not an object - ignored."
            );
            return Ok(());
        };
        for (name, targets) in relations {
            let Some(targets) = targets.elements() else {
                tracing::warn!(
                    "Warning: Relation targets on line {line} are not an array - ignored."
                );
                continue;
            };
            let name_tag = self.base.add_tag(name, TagType::empty())?;
            let name_hash = self.base.grammar.single_tags_list[name_tag.0].hash.get();
            for target in targets {
                let Some(target) = target.as_uint() else {
                    tracing::warn!(
                        "Warning: Relation target on line {line} is not a non-negative integer - skipping."
                    );
                    continue;
                };
                self.refuse_reserved(name, NumberRole::RelationTarget, target)?;
                // A target is looked up in the relation map as it is, so
                // `u32::MAX` means no "no parent" here and is refused too.
                if target == u32::MAX {
                    return Err(self.reserved(ReservedNumber {
                        text: name.into(),
                        role: NumberRole::RelationTarget,
                        value: target,
                    }));
                }
                self.base
                    .doc
                    .store
                    .cohorts
                    .get_mut(c_cohort.0)
                    .relations_input
                    .entry(name_hash)
                    .or_default()
                    .insert(target);
            }
        }
        self.base.doc.deps.has_relations = true;
        crate::cohort::set_related(&mut self.base.doc.store, c_cohort);
        Ok(())
    }

    /// `parseJsonCohort`'s `"drs"`, read only with `--deleted`: each object
    /// is a deleted reading, which cannot be split, so it keeps one mapping
    /// tag.
    fn parse_json_deleted_readings(
        &mut self,
        obj: json::Value<'_>,
        c_cohort: CohortId,
        wordform: TagId,
        line: u32,
    ) -> Result<(), RunError> {
        let Some(drs) = obj.member("drs") else {
            return Ok(());
        };
        if !self.base.cfg.pipe_deleted {
            if self.base.cfg.verbosity_level > 0 {
                tracing::info!(
                    "Info: Ignoring 'drs' (deleted readings) on line {line}; use --deleted to read them."
                );
            }
            return Ok(());
        }
        let Some(readings) = drs.elements() else {
            tracing::warn!(
                "Warning: 'drs' (deleted readings) on line {line} is not an array - ignored."
            );
            return Ok(());
        };
        for reading_val in readings {
            if !reading_val.is_object() {
                tracing::warn!(
                    "Warning: Non-object found in 'drs' (deleted readings) array on line {line} - skipping."
                );
                continue;
            }
            let mut mappings = TagList::new();
            let deleted =
                self.parse_json_reading(reading_val, c_cohort, wordform, &mut mappings, line)?;
            self.add_single_mapping(deleted, &mut mappings, line)?;
            for r in crate::reading::sub_reading_chain(&self.base.doc.store.readings, deleted) {
                self.base.doc.store.readings.get_mut(r.0).deleted = true;
            }
            // Appended and numbered as `Cohort::appendReading` does.
            let list = &mut self.base.doc.store.cohorts.get_mut(c_cohort.0).deleted;
            list.push(deleted);
            let size = list.len();
            let reading = self.base.doc.store.readings.get_mut(deleted.0);
            if reading.number == 0 {
                reading.number = crate::inlines::ui32(size.wrapping_mul(1000).wrapping_add(1000));
            }
            self.base.doc.num_readings = self.base.doc.num_readings.wrapping_add(1);
        }
        Ok(())
    }

    // [spec:cg3:req:robustness.reserved-keys]
    /// `parseJsonCohort`'s `"ds"` and `"dp"`. They key and probe the window's
    /// dependency map, so a number that map reserves is refused; a `"dp"` of
    /// `u32::MAX` is `DEP_NO_PARENT` and stays.
    fn parse_json_dependency(
        &mut self,
        obj: json::Value<'_>,
        c_cohort: CohortId,
        line: u32,
    ) -> Result<(), RunError> {
        let dp = obj.member("dp");
        let Some(ds) = obj.member("ds") else {
            if dp.is_some() {
                tracing::warn!(
                    "Warning: 'dp' (dependency parent) on line {line} without 'ds' (dependency self) - ignored."
                );
            }
            return Ok(());
        };
        let Some(dep_self) = ds.as_uint().filter(|&ds| ds != 0) else {
            tracing::warn!(
                "Warning: 'ds' (dependency self) on line {line} is not a positive integer - ignored."
            );
            return Ok(());
        };
        self.refuse_reserved("ds", NumberRole::DependencySelf, dep_self)?;
        self.base.doc.store.cohorts.get_mut(c_cohort.0).dep_self = Some(GlobalNumber(dep_self));
        if let Some(dp) = dp {
            match dp.as_uint() {
                None => tracing::warn!(
                    "Warning: 'dp' (dependency parent) on line {line} is not a non-negative integer - ignored."
                ),
                Some(dep_parent) => {
                    self.refuse_reserved("dp", NumberRole::DependencyParent, dep_parent)?;
                    if dep_parent != dep_self && dep_parent != crate::cohort::DEP_NO_PARENT {
                        self.base.doc.store.cohorts.get_mut(c_cohort.0).dep_parent =
                            Some(GlobalNumber(dep_parent));
                    }
                }
            }
        }
        self.base.doc.deps.has_dep = true;
        Ok(())
    }

    /// `value`, read from the member `member` as `role`, refused when the
    /// hash tables reserve it.
    fn refuse_reserved(&self, member: &str, role: NumberRole, value: u32) -> Result<(), RunError> {
        ReservedNumber::check(member, role, value).map_err(|source| self.reserved(source))
    }

    /// `source`, refused on the line being read.
    fn reserved(&self, source: ReservedNumber) -> RunError {
        RunError::ReservedNumber {
            input: self.base.cfg.input_name.clone(),
            line: self.base.doc.num_lines.saturating_add(1),
            source,
        }
    }

    // =======================================================================
    // runGrammarOnText — the JSONL stream driver
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+2]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.run-grammar-on-text-fn+2]
    /// C++ `void runGrammarOnText(std::istream& input, std::ostream& output)`:
    /// read a JSONL stream, one object per line, build windows from it, run
    /// the grammar on them, and write them in the format `fmt` prints.
    ///
    /// The C++ quits on an input that is not `good()` or already at its end,
    /// on an output that is not, and without a grammar. A Rust reader and
    /// writer have no such state, and the applicator owns its grammar.
    ///
    /// `doc.num_lines` counts the lines read to the end, as every reader's
    /// does, so a tag that fails while a line is read names that line; the
    /// C++ counts a line as it starts reading it.
    pub fn run_grammar_on_text<F, R, W>(
        &mut self,
        fmt: &mut F,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        F: StreamFormat,
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
    ) -> Result<(), RunError>
    where
        F: StreamFormat,
        R: Read + Seek,
        W: Write,
    {
        let delimiters = self.base.grammar.delimiters;
        let soft_delimiters = self.base.grammar.soft_delimiters;
        let empty = |set: Option<crate::arena::SetId>| {
            set.is_none_or(|set| self.base.grammar.sets_list[set.0].empty())
        };
        if empty(delimiters) {
            if empty(soft_delimiters) {
                tracing::warn!(
                    "Warning: No soft or hard delimiters defined in grammar. Hard limit of {} cohorts may break windows.",
                    self.base.cfg.hard_limit
                );
            } else {
                tracing::warn!(
                    "Warning: No hard delimiters defined in grammar. Soft limit of {} cohorts may break windows.",
                    self.base.cfg.soft_limit
                );
            }
        }

        self.base.index();

        let mut st = ReadState {
            line: 0,
            reset_after: self.base.cfg.reset_after(),
            ignoreinput: false,
            did_soft_lookback: false,
            c_swindow: None,
            l_swindow: None,
            l_cohort: None,
            variables_set: Uint32FlatHashMap::default(),
            variables_rem: Uint32FlatHashSet::default(),
            variables_output: Uint32SortedVector::new(),
        };
        self.base.doc.stream.window_span = self.base.cfg.num_windows;

        crate::uextras::strip_bom(input);
        let mut reader = std::io::BufReader::new(input);

        let mut exited = false;
        loop {
            let line = self.base.doc.num_lines.saturating_add(1);
            let Some(line_str) = read_jsonl_line(&mut reader, &self.base.cfg.input_name, line)?
            else {
                break;
            };
            st.line = line;
            let flow = self.read_jsonl_object(fmt, output, &mut st, &line_str, line)?;
            self.base.doc.num_lines = line;
            if flow.is_break() {
                exited = true;
                break;
            }
        }

        // An EXIT goes straight to the end, so buffered windows are neither
        // run nor written.
        if !exited {
            self.base.doc.input_eof = true;
            if let Some(sw) = st.c_swindow.take() {
                self.add_end_tag_to_last(sw)?;
            }
            self.flush_windows(fmt, output, &st)?;
            let _ = output.flush();
            // The variables set or removed after the last window was made.
            for cmd in
                variable_commands(&self.base.grammar, &st.variables_output, &st.variables_set)
            {
                fmt.print_stream_command(&mut self.base.engine(), &cmd, output);
            }
        }

        if self.base.cfg.verbosity_level > 0 {
            tracing::info!(
                "Progress: L:{}, W:{}, C:{}, R:{} - Done.",
                self.base.doc.num_lines,
                self.base.doc.num_windows,
                self.base.doc.num_cohorts,
                self.base.doc.num_readings
            );
        }
        Ok(())
    }

    /// One line of the stream, the `line`th: a command, a cohort or a line
    /// of text. `Break` on an EXIT.
    fn read_jsonl_object<F: StreamFormat, W: Write>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        st: &mut ReadState,
        line_str: &str,
        line: u32,
    ) -> Result<ControlFlow<()>, RunError> {
        if line_str
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r'))
        {
            return Ok(ControlFlow::Continue(()));
        }
        let doc = match json::parse(line_str) {
            Ok(doc) => doc,
            Err(e) => {
                tracing::warn!(
                    "Warning: Failed to parse JSON on line {line}: {} (offset {}). Skipping line.",
                    e.code,
                    e.offset
                );
                return Ok(ControlFlow::Continue(()));
            }
        };
        let obj = doc.root();
        if !obj.is_object() {
            tracing::warn!("Warning: JSON on line {line} is not an object. Skipping line.");
            return Ok(ControlFlow::Continue(()));
        }

        if obj.member("cmd").is_some() {
            return self.read_jsonl_command(fmt, output, st, obj, line);
        }
        if obj.member("w").is_some() {
            self.read_jsonl_cohort(fmt, output, st, obj, line_str, line)?;
        } else if obj.member("t").is_some() {
            if let Some(text) = get_json_string(obj, "t", line) {
                self.add_text(fmt, output, st, &format!("{text}\n"))?;
            }
        } else {
            tracing::warn!(
                "Warning: JSON object on line {line} has none of 'w', 't' or 'cmd'. Skipping line."
            );
        }
        Ok(ControlFlow::Continue(()))
    }

    /// A `"cmd"` object, handled whether or not input is ignored.
    fn read_jsonl_command<F: StreamFormat, W: Write>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        st: &mut ReadState,
        obj: json::Value<'_>,
        line: u32,
    ) -> Result<ControlFlow<()>, RunError> {
        let Some(cmd) = get_json_string(obj, "cmd", line) else {
            return Ok(ControlFlow::Continue(()));
        };
        if cmd.is_empty() {
            tracing::warn!("Warning: Empty 'cmd' value on line {line}.");
            return Ok(ControlFlow::Continue(()));
        }
        // The text between `prefix` and the closing `>`, when `cmd` has both.
        let payload = |prefix: &str| {
            (cmd.len() > prefix.len() && cmd.ends_with('>'))
                .then(|| cmd.strip_prefix(prefix))
                .flatten()
                .map(|rest| &rest[..rest.len() - 1])
        };
        let verbose = self.base.cfg.verbosity_level > 0;

        if cmd == STR_CMD_FLUSH {
            if verbose {
                tracing::info!("Info: FLUSH encountered on line {line}. Flushing...");
            }
            let back_swindow = self.base.doc.stream.back();
            if let Some(bsw) = back_swindow {
                self.base
                    .doc
                    .store
                    .single_windows
                    .get_mut(bsw.0)
                    .flush_after = true;
            }
            if let Some(sw) = st.c_swindow {
                self.add_end_tag_to_last(sw)?;
            }
            st.l_cohort = None;
            st.c_swindow = None;
            st.l_swindow = None;
            self.flush_windows(fmt, output, st)?;
            if back_swindow.is_none() {
                fmt.print_stream_command(&mut self.base.engine(), STR_CMD_FLUSH, output);
            }
            self.base.doc.variables.clear(0);
            let _ = output.flush();
        } else if cmd == STR_CMD_IGNORE {
            if verbose {
                tracing::info!(
                    "Info: IGNORE encountered on line {line}. Passing through all input..."
                );
            }
            st.ignoreinput = true;
            fmt.print_stream_command(&mut self.base.engine(), STR_CMD_IGNORE, output);
        } else if cmd == STR_CMD_RESUME {
            if verbose {
                tracing::info!("Info: RESUME encountered on line {line}. Resuming CG...");
            }
            st.ignoreinput = false;
            fmt.print_stream_command(&mut self.base.engine(), STR_CMD_RESUME, output);
        } else if cmd == STR_CMD_EXIT {
            if verbose {
                tracing::info!("Info: EXIT encountered on line {line}. Exiting...");
            }
            fmt.print_stream_command(&mut self.base.engine(), STR_CMD_EXIT, output);
            return Ok(ControlFlow::Break(()));
        } else if let Some(items) = payload(STR_CMD_SETVAR) {
            self.setvar_command(items, st, line)?;
        } else if let Some(items) = payload(STR_CMD_REMVAR) {
            self.remvar_command(items, st)?;
        } else {
            tracing::warn!(
                "Warning: Unknown or malformed stream command {cmd} on line {line} - treated as text."
            );
            self.add_text(fmt, output, st, &format!("{cmd}\n"))?;
        }
        Ok(ControlFlow::Continue(()))
    }

    /// `<STREAMCMD:SETVAR:...>` with its prefix and closing `>` stripped:
    /// `,`-separated items, each `key=value` or a bare `key` set to `*`. An
    /// empty key or value is `*`, the grammar's `tag_any`.
    fn setvar_command(
        &mut self,
        payload: &str,
        st: &mut ReadState,
        line: u32,
    ) -> Result<(), RunError> {
        let tag_any = self.base.grammar.tag_any;
        for item in payload.split(',') {
            let (key, value) = match item.split_once('=') {
                Some((key, value)) => (key, Some(value)),
                None => (item, None),
            };
            let key_hash = if key.is_empty() {
                tracing::warn!(
                    "Warning: SETVAR on line {line} had an empty identifier! Defaulting to identifier *."
                );
                tag_any
            } else {
                self.tag_hash(key)?
            };
            let value_hash = match value {
                Some("") => {
                    tracing::warn!(
                        "Warning: SETVAR on line {line} had no value after the =! Defaulting to value *."
                    );
                    tag_any
                }
                Some(value) => self.tag_hash(value)?,
                None => tag_any,
            };
            *st.variables_set.index_or_insert(key_hash) = value_hash;
            st.variables_rem.erase(key_hash);
            st.variables_output.insert(key_hash);
        }
        Ok(())
    }

    /// `<STREAMCMD:REMVAR:...>` with its prefix and closing `>` stripped:
    /// `,`-separated keys, empty ones skipped.
    fn remvar_command(&mut self, payload: &str, st: &mut ReadState) -> Result<(), RunError> {
        for key in payload.split(',').filter(|key| !key.is_empty()) {
            let key_hash = self.tag_hash(key)?;
            st.variables_set.erase(key_hash);
            st.variables_rem.insert(key_hash);
            st.variables_output.insert(key_hash);
        }
        Ok(())
    }

    fn tag_hash(&mut self, text: &str) -> Result<u32, RunError> {
        let tag = self.base.add_tag(text, TagType::empty())?;
        Ok(self.base.grammar.single_tags_list[tag.0].hash.get())
    }

    /// A `"w"` object: a cohort, or with input ignored the line as text.
    fn read_jsonl_cohort<F: StreamFormat, W: Write>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        st: &mut ReadState,
        obj: json::Value<'_>,
        line_str: &str,
        line: u32,
    ) -> Result<(), RunError> {
        if st.ignoreinput {
            return self.add_text(fmt, output, st, &format!("{line_str}\n"));
        }
        let wform = match get_json_string(obj, "w", line) {
            Some(wform) if !wform.is_empty() => wform,
            _ => {
                tracing::warn!(
                    "Warning: Cohort on line {line} has an empty or invalid 'w' (wordform). Skipping line."
                );
                return Ok(());
            }
        };

        // Once past the soft limit, delimit at the last soft delimiter in
        // the window, if there is one.
        if let Some(sw) = st.c_swindow
            && let Some(soft) = self.base.grammar.soft_delimiters
            && self.base.doc.store.single_windows.get(sw.0).cohorts.len()
                > self.base.cfg.soft_limit as usize
            && !st.did_soft_lookback
        {
            st.did_soft_lookback = true;
            let soft = self.base.grammar.sets_list[soft.0].number.get();
            let cohorts = self.base.doc.store.single_windows.get(sw.0).cohorts.clone();
            for &c in cohorts.iter().rev() {
                if self
                    .base
                    .engine()
                    .does_set_match_cohort_normal(c, soft, None)?
                {
                    st.did_soft_lookback = false;
                    let last = self.base.engine().delimit_at(sw, c)?;
                    st.c_swindow = self
                        .base
                        .doc
                        .store
                        .cohorts
                        .get(last.0)
                        .parent
                        .and_then(|p| self.base.doc.store.single_windows.get(p.0).next);
                    st.l_swindow = st.c_swindow;
                    if self.base.cfg.verbosity_level > 0 {
                        tracing::warn!(
                            "Warning: Soft limit of {} cohorts reached at line {line} but found suitable soft delimiter in buffer.",
                            self.base.cfg.soft_limit
                        );
                    }
                    break;
                }
            }
        }
        let mut sw = match st.c_swindow {
            Some(sw) => sw,
            None => self.new_window(st)?,
        };
        // Rules see `num_windows` windows ahead, as with the CG reader.
        if self.base.doc.stream.next.len() > (self.base.cfg.num_windows as usize).saturating_add(1)
        {
            self.run_window(fmt, output, st)?;
        }

        let c_cohort = self.parse_json_cohort(obj, wform, sw, line)?;

        // A cohort that no longer belongs to the window, by --dep-delimit,
        // starts the next one. A cohort without a "ds" never does.
        let dep_self = self
            .base
            .doc
            .store
            .cohorts
            .get(c_cohort.0)
            .dep_self
            .map_or(0, GlobalNumber::get);
        let dep_delimit = self.base.cfg.dep_delimit;
        let highest = self.base.doc.deps.dep_highest_seen.get();
        if dep_delimit != 0
            && highest != 0
            && dep_self != 0
            && self.base.doc.store.single_windows.get(sw.0).cohorts.len() > 1
            && (dep_self <= highest || dep_self - highest > dep_delimit)
        {
            let global_number = self.base.doc.store.cohorts.get(c_cohort.0).global_number;
            self.base
                .engine()
                .reflow_dependency_window(global_number.get());
            self.add_end_tag_to_last(sw)?;
            sw = self.new_window(st)?;
            self.base.doc.deps.dep_highest_seen = GlobalNumber(0);
            self.base.doc.store.cohorts.get_mut(c_cohort.0).parent = Some(sw);
            if self.base.grammar.has_bag_of_tags {
                let readings = self.base.doc.store.cohorts.get(c_cohort.0).readings.clone();
                for reading in readings {
                    self.base.engine().reflow_reading(reading)?;
                }
            }
        }

        crate::single_window::append_cohort(
            &mut self.base.doc.store,
            &mut self.base.doc.cohorts,
            &mut self.base.doc.deps,
            sw,
            c_cohort,
        );
        if dep_self != 0 {
            self.base.doc.deps.dep_highest_seen = GlobalNumber(dep_self);
        }
        st.l_cohort = Some(c_cohort);

        // The CG reader tests the limits before it appends the cohort, hence
        // `>` rather than `>=`.
        let size = self.base.doc.store.single_windows.get(sw.0).cohorts.len();
        let soft_delimiters = self.base.grammar.soft_delimiters;
        let delimiters = self.base.grammar.delimiters;
        if size > self.base.cfg.soft_limit as usize
            && self
                .base
                .engine()
                .matches_delimiter_set(c_cohort, soft_delimiters)?
        {
            if self.base.cfg.verbosity_level > 0 {
                tracing::warn!(
                    "Warning: Soft limit of {} cohorts reached at line {line} but found suitable soft delimiter.",
                    self.base.cfg.soft_limit
                );
            }
            self.add_end_tag(c_cohort)?;
            st.c_swindow = None;
        } else if size > self.base.cfg.hard_limit as usize
            || (dep_delimit == 0
                && self
                    .base
                    .engine()
                    .matches_delimiter_set(c_cohort, delimiters)?)
        {
            if !self.base.cfg.is_conv && size > self.base.cfg.hard_limit as usize {
                tracing::warn!(
                    "Warning: Hard limit of {} cohorts reached at line {line} - forcing break.",
                    self.base.cfg.hard_limit
                );
            }
            self.add_end_tag(c_cohort)?;
            st.c_swindow = None;
        }
        Ok(())
    }

    /// C++ `add_text`: text read on its own line goes where the CG reader
    /// puts it. After a cohort it is that cohort's, unless it matches
    /// `--text-delimit`, which ends the window; else it is the last window's;
    /// with no window yet it is written at once.
    fn add_text<F: StreamFormat, W: Write>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        st: &mut ReadState,
        text: &str,
    ) -> Result<(), RunError> {
        if let (Some(lsw), Some(_)) = (st.l_swindow, st.l_cohort)
            && crate::grammar_applicator::run_grammar::test_string_against(
                text,
                &self.base.cfg.text_delimiters,
            )
        {
            self.base
                .doc
                .store
                .single_windows
                .get_mut(lsw.0)
                .text_post
                .push_str(text);
            if st.c_swindow == Some(lsw) {
                self.add_end_tag_to_last(lsw)?;
                st.c_swindow = None;
            }
            st.l_cohort = None;
        } else if let Some(lc) = st.l_cohort {
            self.base
                .doc
                .store
                .cohorts
                .get_mut(lc.0)
                .text
                .push_str(text);
        } else if let Some(lsw) = st.l_swindow {
            let window = self.base.doc.store.single_windows.get_mut(lsw.0);
            if window.text_post.is_empty() {
                window.text.push_str(text);
            } else {
                window.text_post.push_str(text);
            }
        } else {
            fmt.print_plain_text_line(&mut self.base.engine(), text, output);
        }
        Ok(())
    }

    /// C++ `new_window`: append a window to fill, and move the variables set
    /// or removed since the last one into it.
    fn new_window(&mut self, st: &mut ReadState) -> Result<SwId, RunError> {
        let sw = self
            .base
            .doc
            .stream
            .alloc_append_single_window(&mut self.base.doc.store);
        self.base.engine().init_empty_single_window(sw)?;
        st.c_swindow = Some(sw);
        st.l_swindow = Some(sw);
        self.base.doc.num_windows = self.base.doc.num_windows.wrapping_add(1);
        st.did_soft_lookback = false;
        // The window is new and holds no variables, so swapping the locals
        // in leaves them empty, as the C++'s insert-then-clear does.
        let window = self.base.doc.store.single_windows.get_mut(sw.0);
        window.variables_set.swap(&mut st.variables_set);
        window.variables_rem.swap(&mut st.variables_rem);
        window.variables_output.swap(&mut st.variables_output);
        Ok(sw)
    }

    /// C++ `run_window`: run the grammar on the next window. JSONL has no
    /// marker for a dependency that spans windows, so once one is resolved
    /// across windows the writer is told to number dependencies as spanning;
    /// windows are written a few runs later, in time for that.
    fn run_window<F: StreamFormat, W: Write>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        st: &ReadState,
    ) -> Result<(), RunError> {
        self.base.engine().shuffle_windows_down();
        self.base.engine().run_grammar_on_window_with(fmt, output)?;
        if self.base.doc.deps.has_dep && !self.base.doc.dep_has_spanned {
            let windows: Vec<SwId> = self
                .base
                .doc
                .stream
                .current
                .into_iter()
                .chain(self.base.doc.stream.next.iter().copied())
                .collect();
            for window in windows {
                if self.dep_spans_windows(window) {
                    self.base.doc.dep_has_spanned = true;
                    break;
                }
            }
        }
        if self.base.doc.num_windows.is_multiple_of(st.reset_after) {
            self.base.reset_indexes();
        }
        if self.base.cfg.verbosity_level > 0 {
            tracing::info!(
                "Progress: L:{}, W:{}, C:{}, R:{}",
                st.line,
                self.base.doc.num_windows,
                self.base.doc.num_cohorts,
                self.base.doc.num_readings
            );
        }
        Ok(())
    }

    /// C++ `check_dep_span`: whether a cohort of `window` has a resolved
    /// dependency parent in another window.
    fn dep_spans_windows(&self, window: SwId) -> bool {
        let store = &self.base.doc.store;
        store.single_windows.get(window.0).cohorts.iter().any(|&c| {
            let cohort = store.cohorts.get(c.0);
            let Some(dep_parent) = cohort.dep_parent.filter(|p| p.get() != 0) else {
                return false;
            };
            cohort.r#type.intersects(crate::cohort::CT_DEP_DONE)
                && self
                    .base
                    .doc
                    .cohorts
                    .cohort_map
                    .get(&dep_parent)
                    .is_some_and(|&p| store.cohorts.get(p.0).parent != cohort.parent)
        })
    }

    /// C++ `flush_windows`: run the grammar on every window still to run,
    /// then write and free them all.
    fn flush_windows<F: StreamFormat, W: Write>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        st: &ReadState,
    ) -> Result<(), RunError> {
        while !self.base.doc.stream.next.is_empty() {
            self.run_window(fmt, output, st)?;
        }
        self.base.engine().shuffle_windows_down();
        while let Some(&tmp) = self.base.doc.stream.previous.first() {
            fmt.print_single_window(&mut self.base.engine(), tmp, output, false)?;
            crate::single_window::free_swindow(
                &mut self.base.doc.store,
                &mut self.base.doc.cohorts,
                &mut self.base.doc.deps,
                Some(tmp),
            );
            self.base.doc.stream.previous.remove(0);
        }
        Ok(())
    }

    /// C++ `add_end_tag(cohort)`: the end tag, added to each reading of
    /// `cohort` that does not have it yet.
    fn add_end_tag(&mut self, cohort: CohortId) -> Result<(), RunError> {
        let endtag = self.base.cfg.endtag;
        let endtag_id = tag_by_hash(&self.base.grammar, endtag);
        let readings = self.base.doc.store.cohorts.get(cohort.0).readings.clone();
        for reading in readings {
            if !self
                .base
                .doc
                .store
                .readings
                .get(reading.0)
                .tags
                .contains(endtag.get())
            {
                self.base.engine().add_tag_to_reading(reading, endtag_id)?;
            }
        }
        Ok(())
    }

    fn add_end_tag_to_last(&mut self, sw: SwId) -> Result<(), RunError> {
        match self.base.doc.store.single_windows.get(sw.0).cohorts.last() {
            Some(&last) => self.add_end_tag(last),
            None => Ok(()),
        }
    }
}

/// `parseJsonReading`'s `"s"`: the sub-reading object, if there is one.
fn json_sub_reading<'d>(reading_obj: json::Value<'d>, line: u32) -> Option<json::Value<'d>> {
    let sub = reading_obj.member("s")?;
    if sub.is_object() {
        return Some(sub);
    }
    tracing::warn!(
        "Warning: Value for 's' (sub-reading) is not an object on line {line} - skipping."
    );
    None
}

/// The stream commands that write out `vars_output`, in its order: a SETVAR
/// for each variable `vars_set` gives a value, without `=` for the value
/// `*`, and a REMVAR for each it does not.
fn variable_commands(
    grammar: &Grammar,
    vars_output: &Uint32SortedVector,
    vars_set: &Uint32FlatHashMap,
) -> Vec<String> {
    vars_output
        .as_slice()
        .iter()
        .map(|&var| {
            let key = tag_text(grammar, var);
            let found = vars_set.find(var);
            if found == vars_set.end() {
                format!("{STR_CMD_REMVAR}{key}>")
            } else if found.get().1 == grammar.tag_any {
                format!("{STR_CMD_SETVAR}{key}>")
            } else {
                format!(
                    "{STR_CMD_SETVAR}{key}={}>",
                    tag_text(grammar, found.get().1)
                )
            }
        })
        .collect()
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
) -> Result<Option<String>, RunError> {
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

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-tags-fn+1]
    /// C++ `void buildJsonTags(const Reading* reading, json::Value& tags_json,
    /// json::Document::AllocatorType& allocator)`: the text of each tag of
    /// `reading` the format prints, in `tags_list` order but with the mapping
    /// tags last, where the CG writer prints them.
    fn build_json_tags_e<'g>(&self, e: &'g Engine<'_>, reading: ReadingId) -> Vec<&'g str> {
        let r = e.doc.store.readings.get(reading.0);
        let baseform = r.baseform.unwrap_or(TagHash(0));
        let wordform = r.parent.and_then(|c| {
            e.doc
                .store
                .cohorts
                .get(c.0)
                .wordform
                .map(|wf| e.grammar.single_tags_list.get(wf.0).hash)
        });
        let mut unique = Uint32SortedVector::new();
        let mut tags: Vec<&'g str> = Vec::new();
        let mut mappings: Vec<&'g str> = Vec::new();
        for &tter in r.tags_list.iter() {
            let tter = TagHash(tter);
            if (!e.cfg.show_end_tags && tter == e.cfg.endtag) || tter == e.cfg.begintag {
                continue;
            }
            if tter == baseform || wordform == Some(tter) {
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
            if ttype.intersects(T_DEPENDENCY) && e.doc.deps.has_dep && !e.cfg.dep_original {
                continue;
            }
            if ttype.intersects(T_RELATION) && e.doc.deps.has_relations {
                continue;
            }
            let text = &*e.grammar.single_tags_list.get(tag.0).tag;
            if ttype.intersects(T_MAPPING) {
                mappings.push(text);
            } else {
                tags.push(text);
            }
        }
        tags.extend(mappings);
        tags
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.build-json-reading-fn+1]
    // [spec:cg3:req:robustness.depth-bounded]
    /// C++ `void buildJsonReading(const Reading* reading, json::Value&
    /// reading_json, json::Document::AllocatorType& allocator)`: `reading` as
    /// one object, its sub-reading nested in its `"s"`.
    ///
    /// The C++ recurses on the sub-reading. `"s"` is a reading's last member,
    /// so this writes each reading of the chain in turn, opening the next
    /// one's object in its `"s"`, and closes them all at the end; a chain of
    /// any length costs no stack.
    fn build_json_reading_e(&self, e: &Engine<'_>, reading: ReadingId, out: &mut json::Writer) {
        let chain = crate::reading::sub_reading_chain(&e.doc.store.readings, reading);
        out.start_object();
        for (i, &r) in chain.iter().enumerate() {
            if i > 0 {
                out.name("s");
                out.start_object();
            }

            let baseform = e.doc.store.readings.get(r.0).baseform;
            let mut lemma = "";
            if let Some(baseform) = baseform {
                let it = e.grammar.single_tags().find(baseform.get());
                if it != e.grammar.single_tags().end() {
                    let tag = &*e.grammar.single_tags_list.get(it.get().1.0).tag;
                    lemma = match tag.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
                        Some(inner) if tag.chars().count() >= 2 => inner,
                        _ => tag,
                    };
                }
            }
            out.string_member("l", lemma);

            let tags = self.build_json_tags_e(e, r);
            if !tags.is_empty() {
                out.strings_member("ts", tags);
            }

            let hit_by = &e.doc.store.readings.get(r.0).hit_by;
            if e.cfg.trace && !hit_by.is_empty() {
                out.name("tr");
                out.start_array();
                for &rule in hit_by.iter() {
                    let mut trace = Vec::new();
                    crate::grammar_applicator::core::print_trace(
                        e.grammar, e.cfg, &mut trace, rule,
                    );
                    out.string(&String::from_utf8_lossy(&trace));
                }
                out.end_array();
            }
        }
        for _ in &chain {
            out.end_object();
        }
    }

    // =======================================================================
    // printStreamCommand / printPlainTextLine / printText — non-cohort lines
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-stream-command-fn+1]
    /// C++ `printStreamCommand`: the whole of `cmd`, a NUL included, as the
    /// one member of an object. Does not flush.
    pub(crate) fn print_stream_command_e<W: Write>(&self, cmd: &str, output: &mut W) {
        let mut doc = json::Writer::new();
        doc.start_object();
        doc.string_member("cmd", cmd);
        doc.end_object();
        doc.write_line(output);
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-plain-text-line-fn+1]
    /// C++ `printPlainTextLine`: `{"t": ...}` for each line of `line`, a final
    /// newline dropped first. Only `\n` separates lines. Does not flush.
    pub(crate) fn print_plain_text_line_e<W: Write>(&self, line: &str, output: &mut W) {
        let line = match line.chars().next_back() {
            Some(last) if crate::inlines::isnl(last) => &line[..line.len() - last.len_utf8()],
            _ => line,
        };
        for piece in line.split('\n') {
            let mut doc = json::Writer::new();
            doc.start_object();
            doc.string_member("t", piece);
            doc.end_object();
            doc.write_line(output);
        }
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-text-fn]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-text-fn]
    /// C++ `printText`: `text` as `{"t": ...}` lines, unless it is only
    /// whitespace, as the CG writer skips it.
    fn print_text_e<W: Write>(&self, e: &Engine<'_>, text: &str, output: &mut W) {
        if has_non_ws(e, text) {
            self.print_plain_text_line_e(text, output);
        }
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-variables-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-variables-fn+1]
    /// C++ `printVariables`: a stream command for each variable of
    /// `vars_output`.
    fn print_variables_e<W: Write>(
        &self,
        e: &Engine<'_>,
        vars_output: &Uint32SortedVector,
        vars_set: &Uint32FlatHashMap,
        output: &mut W,
    ) {
        for cmd in variable_commands(e.grammar, vars_output, vars_set) {
            self.print_stream_command_e(&cmd, output);
        }
    }

    // =======================================================================
    // printCohort / printSingleWindow
    // =======================================================================

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-cohort-fn+1]
    /// C++ `void printCohort(Cohort* cohort, std::ostream& output, bool
    /// profiling)`: `cohort` as one object on its own line, with its members
    /// in the order `w, wb, sts, z, ds, dp, id, rels, rs, drs`. The `>>>`
    /// cohort and a removed cohort write only the text after them.
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
        if local_number == 0 || ctype.intersects(CT_REMOVED) {
            let text = e.doc.store.cohorts.get(cohort.0).text.clone();
            self.print_text_e(e, &text, output);
            return;
        }
        if !profiling {
            crate::cohort::unignore_all(&mut e.doc.store, cohort);
        }

        let mut readings = std::mem::take(&mut e.doc.store.cohorts.get_mut(cohort.0).readings);
        sort_readings(&e.doc.store, &mut readings);
        e.doc.store.cohorts.get_mut(cohort.0).readings = readings;
        let removed = e.cfg.trace && !e.cfg.trace_no_removed;
        if removed {
            let c = e.doc.store.cohorts.get_mut(cohort.0);
            let mut delayed = std::mem::take(&mut c.delayed);
            let mut deleted = std::mem::take(&mut c.deleted);
            sort_readings(&e.doc.store, &mut delayed);
            sort_readings(&e.doc.store, &mut deleted);
            let c = e.doc.store.cohorts.get_mut(cohort.0);
            c.delayed = delayed;
            c.deleted = deleted;
        }

        let e = &*e;
        let c = e.doc.store.cohorts.get(cohort.0);
        let mut doc = json::Writer::new();
        doc.start_object();

        #[expect(
            clippy::expect_used,
            reason = "every cohort gets a wordform where it is made (each stream reader, the >>> cohort in run_grammar, ADDCOHORT and the splitting rules in restructure); only cohort_clear resets it"
        )]
        let wordform = c.wordform.expect("printCohort: cohort has no wordform");
        let wordform_tag = e.grammar.single_tags_list.get(wordform.0);
        let w = &*wordform_tag.tag;
        let w = match w.strip_prefix("\"<").and_then(|t| t.strip_suffix(">\"")) {
            Some(inner) if w.chars().count() >= 4 => inner,
            _ => w,
        };
        doc.string_member("w", w);

        if !c.wblank.is_empty() {
            doc.string_member("wb", &c.wblank);
        }

        if let Some(wread) = c.wread {
            let static_tags: Vec<&str> = e
                .doc
                .store
                .readings
                .get(wread.0)
                .tags_list
                .iter()
                .filter(|&&tter| tter != wordform_tag.hash.get())
                .map(|&tter| tag_text(e.grammar, tter))
                .collect();
            if !static_tags.is_empty() {
                doc.strings_member("sts", static_tags);
            }
        }

        if has_non_ws(e, &c.text) {
            let text = match c.text.chars().next_back() {
                Some(last) if crate::inlines::isnl(last) => {
                    &c.text[..c.text.len() - last.len_utf8()]
                }
                _ => &c.text,
            };
            doc.string_member("z", text);
        }

        // Numbered as the CG writer numbers dependencies, except that the
        // root is always 0 and "dp" is left out when there is no parent.
        if e.doc.deps.has_dep {
            let parent = match c.dep_parent {
                None => None,
                Some(GlobalNumber(0)) => c.parent.and_then(|sw| {
                    e.doc
                        .store
                        .single_windows
                        .get(sw.0)
                        .cohorts
                        .first()
                        .copied()
                }),
                Some(dp) => e.doc.cohorts.cohort_map.get(&dp).copied(),
            };
            let parent = parent.map(|p| e.doc.store.cohorts.get(p.0));
            if e.cfg.dep_absolute || e.doc.dep_has_spanned {
                doc.name("ds");
                doc.uint(c.global_number.get());
                if let Some(p) = parent {
                    doc.name("dp");
                    doc.uint(if p.local_number == 0 {
                        0
                    } else {
                        p.global_number.get()
                    });
                }
            } else {
                doc.name("ds");
                doc.uint(c.local_number);
                if let Some(p) = parent {
                    doc.name("dp");
                    doc.uint(p.local_number);
                }
            }
        }

        // With relations in use every cohort has an id, so that a relation's
        // target can always be found.
        if e.doc.deps.has_relations || e.cfg.print_ids {
            doc.name("id");
            doc.uint(c.global_number.get());
        }
        if c.r#type.intersects(crate::cohort::CT_RELATED) {
            doc.name("rels");
            doc.start_object();
            for (&name, targets) in &c.relations {
                doc.name(tag_text(e.grammar, name));
                doc.start_array();
                for &target in targets.as_slice() {
                    doc.uint(target);
                }
                doc.end_array();
            }
            doc.end_object();
        }

        let printed = |list: &[ReadingId]| -> Vec<ReadingId> {
            list.iter()
                .copied()
                .filter(|r| !e.doc.store.readings.get(r.0).noprint)
                .collect()
        };
        let readings = printed(&c.readings);
        if !readings.is_empty() {
            doc.name("rs");
            doc.start_array();
            for reading in readings {
                self.build_json_reading_e(e, reading, &mut doc);
            }
            doc.end_array();
        }

        // Under the condition the CG writer prints delayed and deleted
        // readings on.
        if removed {
            let mut deleted = printed(&c.delayed);
            deleted.extend(printed(&c.deleted));
            if !deleted.is_empty() {
                doc.name("drs");
                doc.start_array();
                for reading in deleted {
                    self.build_json_reading_e(e, reading, &mut doc);
                }
                doc.end_array();
            }
        }

        doc.end_object();
        doc.write_line(output);
        let _ = output.flush();
    }

    // [spec:cg3:def:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn+1]
    // [spec:cg3:sem:jsonl-applicator.cg3.jsonl-applicator.print-single-window-fn+1]
    /// C++ `void printSingleWindow(SingleWindow* window, std::ostream& output,
    /// bool profiling)`: the window's variable commands, its text, each of its
    /// cohorts, the `>>>` and removed ones included, its trailing text, and a
    /// FLUSH if it was flushed.
    pub(crate) fn print_single_window_e<W: Write>(
        &self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        {
            let w = e.doc.store.single_windows.get(window.0);
            self.print_variables_e(e, &w.variables_output, &w.variables_set, output);
            self.print_text_e(e, &w.text, output);
        }
        let all_cohorts = e.doc.store.single_windows.get(window.0).all_cohorts.clone();
        for cohort in all_cohorts {
            self.print_cohort_e(e, cohort, output, profiling);
        }
        let w = e.doc.store.single_windows.get(window.0);
        self.print_text_e(e, &w.text_post, output);
        if w.flush_after {
            self.print_stream_command_e(STR_CMD_FLUSH, output);
        }
        let _ = output.flush();
    }
}

impl crate::grammar_applicator::stream_format::StreamFormat for JsonlFormat {
    fn print_cohort<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), RunError> {
        self.print_cohort_e(e, cohort, output, profiling);
        Ok(())
    }

    fn print_single_window<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), RunError> {
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

/// Whether `text` holds a character not in the whitespace set `ws`, the test
/// the CG writer applies to window and cohort text.
fn has_non_ws(e: &Engine<'_>, text: &str) -> bool {
    let ws = e.cfg.ws.iter().take_while(|&&w| w != '\0');
    text.chars().any(|c| !ws.clone().any(|&w| w == c))
}

/// The text of the tag `hash` names, or empty text when the grammar has no
/// such tag, where the C++ dereferences a null tag.
fn tag_text(grammar: &Grammar, hash: u32) -> &str {
    let it = grammar.single_tags().find(hash);
    if it == grammar.single_tags().end() {
        ""
    } else {
        &grammar.single_tags_list[it.get().1.0].tag
    }
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
