//! Port of `src/BinaryApplicator.cpp` + `src/BinaryApplicator.hpp` — the binary
//! STREAM protocol applicator (distinct from the `.cg3b` grammar format: the
//! stream magic is `CGBF`, the grammar magic is `CG3B`).
//!
//! ## Composition (task design)
//! C++ `class BinaryApplicator : public virtual GrammarApplicator` becomes
//! [`BinaryApplicator`] holding a [`GrammarApplicator`](crate::grammar_applicator::GrammarApplicator)
//! `base` by value plus `header_done` and the reusable `text` member. Every
//! engine/core call goes through `self.base.<method>` (or the core free fns
//! threaded `&mut self.base.doc.store` / `&mut self.base.doc.stream` /
//! `&self.base.grammar`). No `src/*` module other than this file is edited.
//!
//! ## Wire format
//! All multi-byte integers are LITTLE-ENDIAN, via [`crate::inlines`]
//! `read_le`/`write_le` and the length-prefixed `read_utf8_le`/`write_utf8_le`
//! (`[u16 LE byte-length][UTF-8 bytes]`). REPRODUCED FLAGGED BUGS:
//!   * Header version is read NATIVELY (`reinterpret_cast<uint32_t*>` — NOT
//!     byte-swapped) even though the writer emits it little-endian → a
//!     big-endian host would spuriously fail the version check.
//!   * A text packet's byte-length prefix truncates to `u16` (`>65535`-byte
//!     lines wrap).
//!   * Deleted readings are NOT written by `printSingleWindow` (only
//!     `cohort->readings` are traversed).
//!   * An unknown stream command writes only the `BFP_COMMAND` type byte with no
//!     command byte following (malformed packet).
//!
//! DIVERGENCE: window packets go through the checked primitives in `wire`.
//! The reader refuses a truncated packet, a field past its body, a tag index
//! past the window's tag table and a reserved dependency or relation number;
//! the writer refuses a window whose counts or string lengths do not fit the
//! format's fields. The C++ reads past its buffer and wraps its counts.
//!
//! ## I/O model
//! [`read_packet`](BinaryApplicator::read_packet) / `read_window` / `read_command`
//! / `read_text` and the three writers ([`print_single_window`](BinaryApplicator::print_single_window),
//! [`print_stream_command`](BinaryApplicator::print_stream_command),
//! [`print_plain_text_line`](BinaryApplicator::print_plain_text_line)) each take
//! the `input`/`output` handle as a generic `Read`/`Write` param rather than
//! reading the C++ base's stream members.
//!
//! [`run_grammar_on_text`](BinaryApplicator::run_grammar_on_text) is a genuine
//! port: it wraps `input` in a [`std::io::BufReader`] so it can peek for
//! end-of-stream (reproducing the C++ `while (!input.eof())`, where `eof()`
//! becomes true only after a failed read), then threads that reader through
//! `read_packet` and the print methods.

use std::io::{Read, Write};

use crate::arena::{CohortId, SwId, TagId};
use crate::cohort::{CT_RELATED, CT_REMOVED, DEP_NO_PARENT};
use crate::error::RunError;
use crate::grammar::Grammar;
use crate::grammar_applicator::{Engine, GrammarApplicator};
use crate::inlines::{read_le, ui8, write_le, write_utf8_le};
use crate::reading::Reading;
use crate::tag::{T_DEPENDENCY, T_MAPPING, T_RELATION};
use crate::types::{GlobalNumber, TagHash};

mod wire;

pub use wire::{BinaryCount, BinaryField, BinaryStreamFault};
use wire::{PacketWriter, WindowBody, read_window_body};

/// C++ `version.hpp` `constexpr uint32_t CG3_BINARY_STREAM = 1`. `version.hpp`
/// is not yet ported, so the constant is reproduced here verbatim (its only
/// stream users are this file's reader/writer).
pub const CG3_BINARY_STREAM: u32 = 1;

/// C++ `grammar->single_tags[hash]` — resolves a tag hash to its `TagId`, else
/// `TagId(0)`. Reproduces `grammar_applicator::core::tag_by_hash` (which is
/// `pub(super)`, not reachable here); the module cannot be edited.
fn tag_by_hash(grammar: &Grammar, hash: TagHash) -> TagId {
    let it = grammar.single_tags().find(hash.get());
    if it != grammar.single_tags().end() {
        it.get().1
    } else {
        TagId(0)
    }
}

// C++ `Strings.hpp` stream-command name strings — now sourced from the fully
// ported `crate::strings` module.
use crate::strings::{STR_CMD_EXIT, STR_CMD_FLUSH, STR_CMD_IGNORE, STR_CMD_RESUME};

// [spec:cg3:def:binary-applicator.cg3.binary-format-flags]
// C++ `enum BinaryFormatFlags` — OR-combinable, so `u32` bit constants.
/// Window flag: dependency span present.
pub const BFW_DEP_SPAN: u32 = 1 << 0;
/// Cohort flag: has relations.
pub const BFC_RELATED: u32 = 1 << 0;
/// Reading flag: subreading link.
pub const BFR_SUBREADING: u32 = 1 << 0;
/// Reading flag: deleted reading.
pub const BFR_DELETED: u32 = 1 << 1;
/// Variable op: set to a concrete value.
pub const BFV_SETVAR: u32 = 1;
/// Variable op: set to ANY.
pub const BFV_SETVAR_ANY: u32 = 2;
/// Variable op: remove.
pub const BFV_REMVAR: u32 = 3;

// [spec:cg3:def:binary-applicator.cg3.binary-packet-type]
/// C++ `enum BinaryPacketType : uint8_t`; the variants camel-case the C++
/// `BFP_INVALID`/`BFP_WINDOW`/`BFP_COMMAND`/`BFP_TEXT` enumerators.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum BinaryPacketType {
    #[default]
    BfpInvalid = 0,
    BfpWindow = 1,
    BfpCommand = 2,
    BfpText = 3,
}

impl BinaryPacketType {
    fn from_u8(v: u8) -> BinaryPacketType {
        match v {
            1 => BinaryPacketType::BfpWindow,
            2 => BinaryPacketType::BfpCommand,
            3 => BinaryPacketType::BfpText,
            _ => BinaryPacketType::BfpInvalid,
        }
    }
}

// [spec:cg3:def:binary-applicator.cg3.binary-command-type]
/// C++ `enum BinaryCommandType : uint8_t`.
pub const BFC_FLUSH: u8 = 1;
pub const BFC_EXIT: u8 = 2;
pub const BFC_IGNORE: u8 = 3;
pub const BFC_RESUME: u8 = 4;

// [spec:cg3:def:binary-applicator.cg3.binary-packet]
/// C++ `struct BinaryPacket { BinaryPacketType type = BFP_INVALID; void* payload
/// = nullptr; }`.
///
/// The C++ `void* payload` is overloaded: a `SingleWindow*` for a WINDOW packet,
/// a command byte stuffed INTO the pointer for a COMMAND packet, and a
/// string pointer (→ the `text` member) for a TEXT packet. In the arena model those
/// become explicit variants, tracked here as the parsed [`SwId`], the raw
/// command byte, or a `text`-is-set marker.
#[derive(Default)]
pub struct BinaryPacket {
    pub r#type: BinaryPacketType,
    /// WINDOW: the parsed single-window id (C++ `payload = cSWindow`); `None`
    /// for the other packet types.
    pub window: Option<SwId>,
    /// COMMAND: the single command byte (C++ stuffs it into the `void*`).
    pub command: u8,
    /// TEXT: true when the packet decoded into the `text` member (C++ `payload =
    /// &text`).
    pub text_set: bool,
}

// [spec:cg3:def:binary-applicator.cg3.binary-applicator]
/// C++ `class BinaryApplicator : public virtual GrammarApplicator`. Composition
/// port (wave 4): the shared base engine is BORROWED for the run (the C++
/// virtual-base subobject is shared with the most-derived object); `text`
/// takes its C++ in-class default. C++ `header_done` lives on
/// [`BinaryFormat`], the binary print-vtable strategy.
pub struct BinaryApplicator<'a> {
    /// The `GrammarApplicator` base (C++ `public virtual` inheritance).
    pub base: &'a mut GrammarApplicator,
    /// C++ `text` string, reused across TEXT packets.
    pub text: String,
}

impl<'a> BinaryApplicator<'a> {
    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.binary-applicator-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.binary-applicator-fn]
    /// C++ `BinaryApplicator::BinaryApplicator`. Delegates
    /// to the base ctor with an empty body; `header_done = false`, `text` empty.
    pub fn new(base: &'a mut GrammarApplicator) -> Self {
        BinaryApplicator {
            base,
            text: String::new(),
        }
    }

    // =======================================================================
    // Readers
    // =======================================================================

    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-packet-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-packet-fn]
    /// C++ `BinaryPacket BinaryApplicator::readPacket()`. Reads one wire packet.
    /// Reads the type byte, dispatches WINDOW/COMMAND, then in a SEPARATE `if`
    /// (not chained, faithful) dispatches TEXT. The C++ input stream member is
    /// threaded as an explicit `input` param.
    pub fn read_packet<R: Read>(
        &mut self,
        input: &mut R,
    ) -> Result<BinaryPacket, crate::error::RunError> {
        let mut packet = BinaryPacket::default();
        let ty: u8 = read_le(input);
        packet.r#type = BinaryPacketType::from_u8(ty);
        if packet.r#type == BinaryPacketType::BfpWindow {
            packet.window = Some(self.read_window(input)?);
        } else if packet.r#type == BinaryPacketType::BfpCommand {
            packet.command = self.read_command(input);
        }
        if packet.r#type == BinaryPacketType::BfpText {
            self.read_text(input);
            packet.text_set = true;
        }
        Ok(packet)
    }

    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-command-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-command-fn]
    /// C++ `void BinaryApplicator::readCommand(void*& payload)`. Reads exactly one
    /// command byte and (in C++) stuffs it into the `void*`; here it is returned.
    pub fn read_command<R: Read>(&mut self, input: &mut R) -> u8 {
        read_le(input)
    }

    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-text-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-text-fn]
    /// C++ `void BinaryApplicator::readText(void*& payload)`. Reads
    /// `[u16 LE byte-length][UTF-8 bytes]` into the reusable `text` member.
    pub fn read_text<R: Read>(&mut self, input: &mut R) {
        crate::inlines::read_utf8_le(input, &mut self.text);
    }

    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-window-fn+1]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-window-fn+1]
    // [spec:cg3:req:robustness.binary-stream-validated]
    /// C++ `void BinaryApplicator::readWindow(void*& payload)`. Reads a `u32 LE`
    /// body length and the body, then parses the body through a bounds-checked
    /// cursor into a fresh `SingleWindow` (all integers LE) and returns its id.
    ///
    /// DIVERGENCE: the C++ returns no window when the stream ends inside the
    /// length, parses whatever a short body holds, and indexes the window's tag
    /// table unchecked. Each of those is a [`RunError::BinaryStreamWindow`]
    /// here, as is a reserved dependency or relation number.
    pub fn read_window<R: Read>(&mut self, input: &mut R) -> Result<SwId, RunError> {
        let window = self.base.doc.stream.window_counter.wrapping_add(1);
        let bytes = read_window_body(input, window)?;
        let mut body = WindowBody {
            bytes: &bytes,
            pos: 0,
            window,
        };

        let c_swindow = self
            .base
            .doc
            .stream
            .alloc_append_single_window(&mut self.base.doc.store);
        self.base.engine().init_empty_single_window(c_swindow)?;

        // 1. Window flags.
        let flags = body.u16(BinaryField::WindowFlags)?;
        if flags & (BFW_DEP_SPAN as u16) != 0 {
            self.base.doc.dep_has_spanned = true;
        }

        // 2-3. Tag table, then the variables that index it.
        let window_tags = self.read_tag_table(&mut body)?;
        self.read_variables(&mut body, &window_tags, c_swindow)?;

        // 4. Window text / text_post.
        let text = body.string(BinaryField::WindowText)?;
        let text_post = body.string(BinaryField::WindowText)?;
        let sw = self.base.doc.store.single_windows.get_mut(c_swindow.0);
        sw.text = text;
        sw.text_post = text_post;

        // 5. Cohorts.
        let cohort_count = body.u16(BinaryField::CohortCount)?;
        for cn in 0..cohort_count {
            let last = cn + 1 == cohort_count;
            self.read_cohort(&mut body, &window_tags, c_swindow, last)?;
        }

        Ok(c_swindow)
    }

    /// `readWindow` step 2: the window's tag table. Every later tag reference
    /// in the window is a 0-based index into it.
    fn read_tag_table(&mut self, body: &mut WindowBody<'_>) -> Result<Vec<TagId>, RunError> {
        let tag_count = body.u16(BinaryField::TagTable)?;
        let mut window_tags: Vec<TagId> = Vec::new();
        for _ in 0..tag_count {
            let tg = body.string(BinaryField::TagTable)?;
            let first = tg.chars().next().unwrap_or('\0');
            let tid = self.base.add_tag(&tg, crate::tag::TagType::empty())?;
            // tg[0] == grammar->mapping_prefix ? |= T_MAPPING : &= ~T_MAPPING.
            // The only place the engine CLEARS a type bit, and it does so per
            // window: a tag mapped in one window and not the next changes
            // meaning mid-stream, which is exactly why the flags are the run's.
            if first == self.base.grammar.mapping_prefix {
                self.base.grammar.tag_type_insert(tid, T_MAPPING);
            } else {
                self.base.grammar.tag_type_remove(tid, T_MAPPING);
            }
            window_tags.push(tid);
        }
        Ok(window_tags)
    }

    /// `readWindow` step 3: `[1 byte mode][u16 key][u16 value]` per variable.
    /// Only `BFV_SETVAR` reads its value slot, so only there is it resolved
    /// against the tag table; the other modes carry a placeholder.
    fn read_variables(
        &mut self,
        body: &mut WindowBody<'_>,
        window_tags: &[TagId],
        c_swindow: SwId,
    ) -> Result<(), RunError> {
        let var_count = body.u16(BinaryField::Variable)?;
        for _ in 0..var_count {
            let mode = u32::from(body.u8(BinaryField::Variable)?);
            let key = body.tag(window_tags, BinaryField::Variable)?;
            let value = if mode == BFV_SETVAR {
                Some(body.tag(window_tags, BinaryField::Variable)?)
            } else {
                body.u16(BinaryField::Variable)?;
                None
            };
            let hash1 = self.base.grammar.single_tags_list[key.0].hash.get();
            let vh = value.map_or(self.base.grammar.tag_any, |v| {
                self.base.grammar.single_tags_list[v.0].hash.get()
            });
            let sw = self.base.doc.store.single_windows.get_mut(c_swindow.0);
            if mode == BFV_SETVAR || mode == BFV_SETVAR_ANY {
                sw.variables_set.insert((hash1, vh));
                sw.variables_rem.erase(hash1);
                sw.variables_output.insert(hash1);
            } else if mode == BFV_REMVAR {
                sw.variables_set.erase(hash1);
                sw.variables_rem.insert(hash1);
                sw.variables_output.insert(hash1);
            }
        }
        Ok(())
    }

    /// `readWindow` step 5: one cohort record, appended to `c_swindow`. Every
    /// reading of the window's `last` cohort also gets the end tag.
    fn read_cohort(
        &mut self,
        body: &mut WindowBody<'_>,
        window_tags: &[TagId],
        c_swindow: SwId,
        last: bool,
    ) -> Result<(), RunError> {
        let c_cohort = crate::cohort::alloc_cohort(&mut self.base.doc.store, Some(c_swindow));
        let gn = self.base.doc.cohorts.next_cohort_number();
        self.base
            .doc
            .store
            .cohorts
            .get_mut(c_cohort.0)
            .global_number = gn;
        self.base.doc.num_cohorts = self.base.doc.num_cohorts.wrapping_add(1);

        let cflags = body.u16(BinaryField::CohortFlags)?;
        if cflags & (BFC_RELATED as u16) != 0 {
            self.base.doc.store.cohorts.get_mut(c_cohort.0).r#type |= CT_RELATED;
            self.base.doc.deps.has_relations = true;
        }

        let wf = body.tag(window_tags, BinaryField::Wordform)?;
        self.base.doc.store.cohorts.get_mut(c_cohort.0).wordform = Some(wf);
        self.read_static_tags(body, window_tags, c_cohort, wf)?;
        self.read_links(body, window_tags, c_cohort, gn)?;

        // Cohort text / wblank.
        let text = body.string(BinaryField::CohortText)?;
        let wblank = body.string(BinaryField::CohortText)?;
        let c = self.base.doc.store.cohorts.get_mut(c_cohort.0);
        c.text = text;
        c.wblank = wblank;

        // Readings.
        let reading_count = body.u16(BinaryField::Reading)?;
        if reading_count == 0 {
            self.base.engine().init_empty_cohort(c_cohort)?;
        }
        let mut prev: Option<crate::arena::ReadingId> = None;
        for _ in 0..reading_count {
            prev = Some(self.read_reading(body, window_tags, c_cohort, wf, prev)?);
        }

        if last {
            self.add_endtag(c_cohort)?;
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
        crate::single_window::append_cohort(
            &mut self.base.doc.store,
            &mut self.base.doc.cohorts,
            &mut self.base.doc.deps,
            c_swindow,
            c_cohort,
        );
        Ok(())
    }

    /// A cohort's static tags, which go on a `wread` that also carries the
    /// wordform. Only the last add rehashes the reading.
    fn read_static_tags(
        &mut self,
        body: &mut WindowBody<'_>,
        window_tags: &[TagId],
        c_cohort: CohortId,
        wf: TagId,
    ) -> Result<(), RunError> {
        let stag_count = body.u16(BinaryField::StaticTag)?;
        if stag_count == 0 {
            return Ok(());
        }
        let wread = crate::reading::alloc_reading(&mut self.base.doc.store, Some(c_cohort));
        self.base.doc.store.cohorts.get_mut(c_cohort.0).wread = Some(wread);
        self.base.engine().add_tag_to_reading(wread, wf)?;
        for tn in 0..stag_count {
            let tag = body.tag(window_tags, BinaryField::StaticTag)?;
            let rehash = tn + 1 == stag_count;
            self.base
                .engine()
                .add_tag_to_reading_rehash(wread, tag, rehash)?;
        }
        Ok(())
    }

    // [spec:cg3:req:robustness.reserved-keys]
    /// A cohort's `[u32 self][u32 parent]` dependency and its `[u16 tag][u32
    /// head]` relations. [`WindowBody::number`] refuses the numbers the flat
    /// hash containers reserve before `relation_map` or a relation lookup sees
    /// them.
    fn read_links(
        &mut self,
        body: &mut WindowBody<'_>,
        window_tags: &[TagId],
        c_cohort: CohortId,
        gn: GlobalNumber,
    ) -> Result<(), RunError> {
        let dep_self = body.number(BinaryField::Dependency, None)?;
        let dep_parent = body.number(BinaryField::Dependency, Some(DEP_NO_PARENT))?;
        {
            let c = self.base.doc.store.cohorts.get_mut(c_cohort.0);
            c.dep_self = (dep_self != 0).then_some(GlobalNumber(dep_self));
            c.dep_parent = (dep_parent != DEP_NO_PARENT).then_some(GlobalNumber(dep_parent));
        }
        self.base.doc.deps.relation_map.insert((dep_self, gn.get()));
        if dep_parent != DEP_NO_PARENT {
            self.base.doc.deps.has_dep = true;
        }

        let rel_count = body.u16(BinaryField::Relation)?;
        for _ in 0..rel_count {
            let tag = body.tag(window_tags, BinaryField::Relation)?;
            let head = body.number(BinaryField::Relation, None)?;
            let rhash = self.base.grammar.single_tags_list[tag.0].hash;
            self.base
                .doc
                .store
                .cohorts
                .get_mut(c_cohort.0)
                .relations_input
                .entry(rhash.get())
                .or_default()
                .insert(head);
        }
        if rel_count != 0 {
            self.base.doc.deps.has_relations = true;
            self.base.doc.deps.relation_map.insert((dep_self, gn.get()));
            self.base.doc.store.cohorts.get_mut(c_cohort.0).r#type |= CT_RELATED;
        }
        Ok(())
    }

    /// One `[u16 flags][u16 baseform][u16 count][u16 tag]...` reading, placed
    /// by its flags as a subreading of `prev`, a deleted reading, or a live one.
    fn read_reading(
        &mut self,
        body: &mut WindowBody<'_>,
        window_tags: &[TagId],
        c_cohort: CohortId,
        wf: TagId,
        prev: Option<crate::arena::ReadingId>,
    ) -> Result<crate::arena::ReadingId, RunError> {
        let c_reading = crate::reading::alloc_reading(&mut self.base.doc.store, Some(c_cohort));
        self.base.engine().add_tag_to_reading(c_reading, wf)?;

        let rflags = body.u16(BinaryField::Reading)?;
        let baseform = body.tag(window_tags, BinaryField::Baseform)?;
        self.base.engine().add_tag_to_reading(c_reading, baseform)?;

        let rtag_count = body.u16(BinaryField::ReadingTag)?;
        let mut mappings = crate::tag::TagList::new();
        for _ in 0..rtag_count {
            let tid = body.tag(window_tags, BinaryField::ReadingTag)?;
            if self.base.grammar.tag_type(tid).intersects(T_MAPPING) {
                mappings.push(tid);
            } else {
                self.base.engine().add_tag_to_reading(c_reading, tid)?;
            }
        }
        if !mappings.is_empty() {
            self.base
                .engine()
                .split_mappings(&mut mappings, c_cohort, c_reading, true)?;
        }

        if let Some(prev_reading) = prev
            && (rflags & (BFR_SUBREADING as u16) != 0)
        {
            self.base.doc.store.readings.get_mut(prev_reading.0).next = Some(c_reading);
        } else if rflags & (BFR_DELETED as u16) != 0 {
            self.base
                .doc
                .store
                .cohorts
                .get_mut(c_cohort.0)
                .deleted
                .push(c_reading);
        } else {
            crate::cohort::append_reading(&mut self.base.doc.store, c_cohort, c_reading);
        }
        self.base.doc.num_readings = self.base.doc.num_readings.wrapping_add(1);
        Ok(c_reading)
    }

    /// Every reading of the window's last cohort carries the end tag. `endtag`
    /// is a tag HASH (C++ `addTagToReading(*iter, endtag)` uint32 overload), so
    /// the TagId is resolved via `single_tags[endtag]` for the Tag* overload.
    fn add_endtag(&mut self, c_cohort: CohortId) -> Result<(), RunError> {
        let endtag = self.base.cfg.endtag;
        let endtag_id = tag_by_hash(&self.base.grammar, endtag);
        let readings = self.base.doc.store.cohorts.get(c_cohort.0).readings.clone();
        for r in readings {
            let tags = &self.base.doc.store.readings.get(r.0).tags;
            if tags.find(endtag.get()) == tags.end() {
                self.base.engine().add_tag_to_reading(r, endtag_id)?;
            }
        }
        Ok(())
    }

    // =======================================================================
    // Writers
    // =======================================================================
}

// [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-plain-text-line-fn]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-plain-text-line-fn]
// [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-stream-command-fn]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-stream-command-fn]
// [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-single-window-fn+1]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-single-window-fn+1]
/// The binary print vtable (wave 4): C++ `BinaryApplicator`'s three print
/// virtuals (`printPlainTextLine` / `printStreamCommand` /
/// `printSingleWindow`), with the C++ `bool header_done` member as strategy
/// state (the literal port had hoisted it onto the base as a `Cell`).
#[derive(Default)]
pub struct BinaryFormat {
    /// C++ `bool header_done = false;`.
    pub header_done: bool,
}

impl BinaryFormat {
    /// Shared stream-header prologue: `"CGBF" + writeLE(CG3_BINARY_STREAM)` once,
    /// then set `header_done`. NOT a manifest symbol — factors the identical
    /// prologue duplicated across the three writers.
    fn bin_write_header<W: Write>(&mut self, output: &mut W) {
        if !self.header_done {
            let _ = output.write_all(b"CGBF");
            write_le(output, CG3_BINARY_STREAM);
            self.header_done = true;
        }
    }

    /// Body of C++ `BinaryApplicator::printPlainTextLine` (spec anchors on
    /// [`BinaryFormat`]). Writes the stream header if needed, then a
    /// `BFP_TEXT` byte + the line via `writeUTF8_LE`. No flush.
    pub fn bin_print_plain_text_line<W: Write>(&mut self, line: &str, output: &mut W) {
        self.bin_write_header(output);
        write_le(output, ui8(BinaryPacketType::BfpText as u32));
        write_utf8_le(output, line);
    }

    /// Body of C++ `BinaryApplicator::printStreamCommand` (spec anchors on
    /// [`BinaryFormat`]). Header if needed, then `BFP_COMMAND` byte,
    /// then the mapped command byte. QUIRK (faithful): an unrecognised `cmd`
    /// writes ONLY the type byte (malformed packet). No flush.
    pub fn bin_print_stream_command<W: Write>(&mut self, cmd: &str, output: &mut W) {
        self.bin_write_header(output);
        write_le(output, ui8(BinaryPacketType::BfpCommand as u32));
        if cmd == STR_CMD_FLUSH {
            write_le(output, BFC_FLUSH);
        } else if cmd == STR_CMD_EXIT {
            write_le(output, BFC_EXIT);
        } else if cmd == STR_CMD_IGNORE {
            write_le(output, BFC_IGNORE);
        } else if cmd == STR_CMD_RESUME {
            write_le(output, BFC_RESUME);
        }
        // else: no command byte follows (malformed packet) — faithful.
    }

    // [spec:cg3:req:robustness.checked-arithmetic]
    /// Body of C++ `BinaryApplicator::printSingleWindow` (spec anchors on
    /// [`BinaryFormat`]) — the exact inverse of `readWindow`.
    /// `profiling` is ignored. All integers LITTLE-ENDIAN.
    ///
    /// DIVERGENCE: the packet is assembled in full before any of it is written,
    /// and a window whose counts or string lengths do not fit the format's
    /// fields is refused with [`RunError::BinaryStreamOverflow`], writing
    /// nothing. The C++ wraps the count and emits a corrupt packet.
    pub fn bin_print_single_window<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        _profiling: bool,
    ) -> Result<(), RunError> {
        let mut packet = PacketWriter::new(e.doc.store.single_windows.get(window.0).number);
        let (var_count, var_buffer) = packet.variables(e, window)?;
        reflow_removed_text(e, window);
        let (cohort_count, cohort_buffer) = packet.cohorts(e, window)?;

        // Header buffer (assembled AFTER the cohort buffer so the tag table is
        // complete).
        let mut header_buffer: Vec<u8> = Vec::new();
        let wflags: u16 = if e.doc.dep_has_spanned {
            BFW_DEP_SPAN as u16
        } else {
            0
        };
        header_buffer.extend_from_slice(&wflags.to_le_bytes());
        packet.count(&mut header_buffer, packet.tags.len(), BinaryCount::Tags)?;
        for &tag in &packet.tags {
            packet.string(&mut header_buffer, &e.grammar.single_tags_list[tag.0].tag)?;
        }
        packet.count(&mut header_buffer, var_count, BinaryCount::Variables)?;
        header_buffer.extend_from_slice(&var_buffer);
        let w = e.doc.store.single_windows.get(window.0);
        packet.string(&mut header_buffer, &w.text)?;
        packet.string(&mut header_buffer, &w.text_post)?;
        packet.count(&mut header_buffer, cohort_count, BinaryCount::Cohorts)?;
        let flush_after = w.flush_after;

        let body_len = header_buffer.len() + cohort_buffer.len();
        let total_size = u32::try_from(body_len)
            .map_err(|_| packet.overflow(BinaryCount::BodyBytes, body_len, u32::MAX as usize))?;

        // Emit: packet type, total_size (u32 LE), header buffer, cohort buffer.
        self.bin_write_header(output);
        write_le(output, ui8(BinaryPacketType::BfpWindow as u32));
        write_le(output, total_size);
        let _ = output.write_all(&header_buffer);
        let _ = output.write_all(&cohort_buffer);

        if flush_after {
            // C++ virtual printStreamCommand — only ever reached with binary
            // output active, so the binary writer is the dispatch target.
            self.bin_print_stream_command(STR_CMD_FLUSH, output);
        }
        let _ = output.flush();
        Ok(())
    }
}

/// Reflow removed-cohort text to the nearest prior non-removed cohort (or the
/// window). QUIRK: the inner loop has NO break — after clearing, later
/// iterations append the now-empty string (no-op).
fn reflow_removed_text(e: &mut Engine<'_>, window: SwId) {
    let all_cohorts: Vec<CohortId> = e.doc.store.single_windows.get(window.0).all_cohorts.clone();
    for i in 0..all_cohorts.len() {
        let cohort = all_cohorts[i];
        let (ln, ty, has_text) = {
            let c = e.doc.store.cohorts.get(cohort.0);
            (c.local_number, c.r#type, !c.text.is_empty())
        };
        if (ln == 0 || (ty.intersects(CT_REMOVED))) && has_text {
            for j in (1..=i).rev() {
                let prior = all_cohorts[j - 1];
                let (pln, pty) = {
                    let c = e.doc.store.cohorts.get(prior.0);
                    (c.local_number, c.r#type)
                };
                if pln == 0 || (pty.intersects(CT_REMOVED)) {
                    continue;
                }
                let txt = e.doc.store.cohorts.get(cohort.0).text.clone();
                e.doc.store.cohorts.get_mut(prior.0).text.push_str(&txt);
                e.doc.store.cohorts.get_mut(cohort.0).text.clear();
            }
            let txt = e.doc.store.cohorts.get(cohort.0).text.clone();
            e.doc
                .store
                .single_windows
                .get_mut(window.0)
                .text
                .push_str(&txt);
            e.doc.store.cohorts.get_mut(cohort.0).text.clear();
        }
    }
}

/// The parent slot of a cohort's dependency pair: the raw field when it is 0
/// or `DEP_NO_PARENT`; else, when `cohort_map` holds the parent, 0 for the
/// `>>>` cohort and its global number otherwise; else `DEP_NO_PARENT`.
fn dep_parent_slot(e: &Engine<'_>, dep_parent: Option<GlobalNumber>) -> u32 {
    let Some(dp) = dep_parent else {
        return DEP_NO_PARENT;
    };
    if dp == GlobalNumber(0) {
        return 0;
    }
    match e.doc.cohorts.cohort_map.get(&dp) {
        Some(&pr) => {
            let parent = e.doc.store.cohorts.get(pr.0);
            if parent.local_number == 0 {
                0
            } else {
                parent.global_number.get()
            }
        }
        None => DEP_NO_PARENT,
    }
}

impl PacketWriter {
    /// The window's `variables_output` as `[1 byte mode][u16 key][u16 value]`
    /// records (C++ `var_buffer`), and how many there are.
    fn variables(&mut self, e: &Engine<'_>, window: SwId) -> Result<(usize, Vec<u8>), RunError> {
        let sw = e.doc.store.single_windows.get(window.0);
        let mut buffer: Vec<u8> = Vec::new();
        let mut count = 0usize;
        for var in sw.variables_output.iter().copied() {
            count += 1;
            let key = tag_by_hash(e.grammar, TagHash(var));
            let it = sw.variables_set.find(var);
            let value = (it != sw.variables_set.end()).then(|| it.get().1);
            match value {
                Some(vh) if vh != e.grammar.tag_any => {
                    buffer.push(BFV_SETVAR as u8);
                    self.tag(&mut buffer, key)?;
                    self.tag(&mut buffer, tag_by_hash(e.grammar, TagHash(vh)))?;
                }
                Some(_) => {
                    buffer.push(BFV_SETVAR_ANY as u8);
                    self.tag(&mut buffer, key)?;
                    buffer.extend_from_slice(&0u16.to_le_bytes());
                }
                None => {
                    buffer.push(BFV_REMVAR as u8);
                    self.tag(&mut buffer, key)?;
                    buffer.extend_from_slice(&0u16.to_le_bytes());
                }
            }
        }
        Ok((count, buffer))
    }

    /// Every kept cohort's record (C++ `cohort_buffer`), and how many there
    /// are. The `>>>` cohort and removed cohorts are not written.
    fn cohorts(&mut self, e: &mut Engine<'_>, window: SwId) -> Result<(usize, Vec<u8>), RunError> {
        let mut buffer: Vec<u8> = Vec::new();
        let mut count = 0usize;
        let all_cohorts = e.doc.store.single_windows.get(window.0).all_cohorts.clone();
        for cohort in all_cohorts {
            let c = e.doc.store.cohorts.get(cohort.0);
            if c.local_number == 0 || c.r#type.intersects(CT_REMOVED) {
                continue;
            }
            crate::cohort::unignore_all(&mut e.doc.store, cohort);
            count += 1;
            self.cohort(e, cohort, &mut buffer)?;
        }
        Ok((count, buffer))
    }

    /// One cohort record: flags, wordform, static tags (the `wread`'s, less
    /// the wordform), dependency, relations, text and blank, then readings.
    fn cohort(
        &mut self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        buffer: &mut Vec<u8>,
    ) -> Result<(), RunError> {
        let c = e.doc.store.cohorts.get(cohort.0);
        let cflags: u16 = if c.r#type.intersects(CT_RELATED) {
            BFC_RELATED as u16
        } else {
            0
        };
        buffer.extend_from_slice(&cflags.to_le_bytes());

        let wf = c.wordform.expect("cohort wordform");
        let wf_hash = e.grammar.single_tags_list[wf.0].hash;
        self.tag(buffer, wf)?;

        let mut tag_buf: Vec<u8> = Vec::new();
        let mut stag_count = 0usize;
        if let Some(wr) = c.wread {
            for &tter in &e.doc.store.readings.get(wr.0).tags_list {
                if TagHash(tter) == wf_hash {
                    continue;
                }
                self.tag(&mut tag_buf, tag_by_hash(e.grammar, TagHash(tter)))?;
                stag_count += 1;
            }
        }
        self.count(buffer, stag_count, BinaryCount::StaticTags)?;
        buffer.extend_from_slice(&tag_buf);

        buffer.extend_from_slice(&c.global_number.get().to_le_bytes());
        buffer.extend_from_slice(&dep_parent_slot(e, c.dep_parent).to_le_bytes());

        let mut rel_buffer: Vec<u8> = Vec::new();
        let mut rel_count = 0usize;
        for (&name_hash, targets) in &c.relations {
            let tid = tag_by_hash(e.grammar, TagHash(name_hash));
            for &target in targets.iter() {
                rel_count += 1;
                self.tag(&mut rel_buffer, tid)?;
                rel_buffer.extend_from_slice(&target.to_le_bytes());
            }
        }
        self.count(buffer, rel_count, BinaryCount::Relations)?;
        buffer.extend_from_slice(&rel_buffer);

        self.string(buffer, &c.text)?;
        self.string(buffer, &c.wblank)?;
        self.readings(e, cohort, buffer)
    }

    /// A cohort's readings sorted by `cmp_number`: each printable top reading
    /// followed by its subreading chain. Deleted readings are NOT written.
    fn readings(
        &mut self,
        e: &mut Engine<'_>,
        cohort: CohortId,
        buffer: &mut Vec<u8>,
    ) -> Result<(), RunError> {
        let mut readings: Vec<crate::arena::ReadingId> =
            e.doc.store.cohorts.get(cohort.0).readings.clone();
        readings.sort_by(|&a, &b| {
            let ra = e.doc.store.readings.get(a.0);
            let rb = e.doc.store.readings.get(b.0);
            if Reading::cmp_number(ra, rb) {
                std::cmp::Ordering::Less
            } else if Reading::cmp_number(rb, ra) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        e.doc.store.cohorts.get_mut(cohort.0).readings = readings.clone();

        let mut reading_buffer: Vec<u8> = Vec::new();
        let mut reading_count = 0usize;
        for top_reading in readings {
            if e.doc.store.readings.get(top_reading.0).noprint {
                continue;
            }
            let mut reading = Some(top_reading);
            while let Some(rid) = reading {
                reading_count += 1;
                let rflags: u16 = if rid != top_reading {
                    BFR_SUBREADING as u16
                } else {
                    0
                };
                reading_buffer.extend_from_slice(&rflags.to_le_bytes());
                self.reading(e, rid, &mut reading_buffer)?;
                reading = e.doc.store.readings.get(rid.0).next;
            }
        }
        self.count(buffer, reading_count, BinaryCount::Readings)?;
        buffer.extend_from_slice(&reading_buffer);
        Ok(())
    }

    /// One reading's baseform, then its tags less the baseform, the wordform,
    /// dependency and relation tags, and (under `unique_tags`) repeats.
    fn reading(
        &mut self,
        e: &Engine<'_>,
        rid: crate::arena::ReadingId,
        buffer: &mut Vec<u8>,
    ) -> Result<(), RunError> {
        let r = e.doc.store.readings.get(rid.0);
        let baseform = r.baseform.unwrap_or(TagHash(0));
        self.tag(buffer, tag_by_hash(e.grammar, baseform))?;

        let parent_wf_hash = {
            let w = e.doc.store.cohorts.get(r.parent.unwrap().0).wordform;
            w.map(|t| e.grammar.single_tags_list[t.0].hash)
                .unwrap_or(TagHash(0))
        };
        let mut tag_buf: Vec<u8> = Vec::new();
        let mut tag_count = 0usize;
        let mut unique = crate::sorted_vector::Uint32SortedVector::new();
        for &tter in &r.tags_list {
            let tter = TagHash(tter);
            if tter == baseform || tter == parent_wf_hash {
                continue;
            }
            let tid = tag_by_hash(e.grammar, tter);
            if e.grammar
                .tag_type(tid)
                .intersects(T_DEPENDENCY | T_RELATION)
            {
                continue;
            }
            if e.cfg.unique_tags {
                if unique.find(tter.get()) != unique.end() {
                    continue;
                }
                unique.insert(tter.get());
            }
            self.tag(&mut tag_buf, tid)?;
            tag_count += 1;
        }
        self.count(buffer, tag_count, BinaryCount::ReadingTags)?;
        buffer.extend_from_slice(&tag_buf);
        Ok(())
    }
}

impl crate::grammar_applicator::stream_format::StreamFormat for BinaryFormat {
    fn print_cohort<W: Write>(
        &mut self,
        _e: &mut Engine<'_>,
        _cohort: CohortId,
        _output: &mut W,
        _profiling: bool,
    ) -> Result<(), crate::error::RunError> {
        // Binary streams are emitted as whole-window packets.
        Ok(())
    }

    fn print_single_window<W: Write>(
        &mut self,
        e: &mut Engine<'_>,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) -> Result<(), crate::error::RunError> {
        self.bin_print_single_window(e, window, output, profiling)?;
        Ok(())
    }

    fn print_stream_command<W: Write>(&mut self, _e: &mut Engine<'_>, cmd: &str, output: &mut W) {
        self.bin_print_stream_command(cmd, output);
    }

    fn print_plain_text_line<W: Write>(&mut self, _e: &mut Engine<'_>, line: &str, output: &mut W) {
        self.bin_print_plain_text_line(line, output);
    }
}

impl<'x> BinaryApplicator<'x> {
    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.run-grammar-on-text-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.run-grammar-on-text-fn]
    /// C++ `void BinaryApplicator::runGrammarOnText(std::istream& input,
    /// std::ostream& output)`. Reads the 8-byte header (magic `CGBF` + native
    /// u32 version), then a packet sequence (window/command/text), running the
    /// grammar over windows and printing results.
    ///
    /// The C++ `while (!input.eof())` (eof becomes true only after a failed read)
    /// is reproduced by wrapping `input` in a [`std::io::BufReader`] and peeking
    /// `fill_buf()` before each packet: an empty fill means end-of-stream.
    pub fn run_grammar_on_text<F, R, W>(
        &mut self,
        fmt: &mut F,
        input: &mut R,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: std::io::Read,
        W: std::io::Write,
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
        R: std::io::Read,
        W: std::io::Write,
    {
        use std::io::BufRead;
        // good()/eof()/output/grammar validity checks: deferred I/O.

        let mut input = std::io::BufReader::new(input);

        {
            let mut header = [0u8; 8];
            if input.read_exact(&mut header).is_err() {
                // "Error: Could not read stream header!" + CG3Quit(1): deferred.
                return Ok(());
            }
            if !crate::inlines::is_cg3bsf(header) {
                // "Stream does not start with magic bytes" + CG3Quit(1): deferred.
                return Ok(());
            }
            // BUG (faithful): version read NATIVELY, not byte-swapped.
            let version = u32::from_ne_bytes([header[4], header[5], header[6], header[7]]);
            if version != CG3_BINARY_STREAM {
                // "Stream is version %u..." + CG3Quit(1): deferred.
                return Ok(());
            }
        }

        self.base.index();
        let reset_after: u32 = (self.base.cfg.num_windows + 4) * 2 + 1;
        self.base.doc.stream.window_span = self.base.cfg.num_windows;

        // flush(flush_after) lambda: drain the pipeline + print buffered windows.
        // Reproduced inline at each call site (Rust closures can't borrow `self`
        // mutably across the loop and also be re-entrant here) — see below.

        // while (!input.eof())
        loop {
            // Peek for end-of-stream (eof() true after a failed read in C++).
            let at_eof = match input.fill_buf() {
                Ok(buf) => buf.is_empty(),
                Err(_) => true,
            };
            if at_eof {
                break;
            }
            let packet = self.read_packet(&mut input)?;
            match packet.r#type {
                BinaryPacketType::BfpWindow => {
                    self.base.doc.num_windows = self.base.doc.num_windows.wrapping_add(1);
                    if self.base.doc.stream.next.len() > self.base.cfg.num_windows as usize {
                        self.base.engine().shuffle_windows_down();
                        self.base.engine().run_grammar_on_window_with(fmt, output)?;
                        if self.base.doc.num_windows.is_multiple_of(reset_after) {
                            self.base.engine().reset_indexes();
                        }
                    }
                }
                BinaryPacketType::BfpCommand => {
                    let cmd = packet.command;
                    if cmd == BFC_FLUSH {
                        let back = self.flush(fmt, output, true)?;
                        if back.is_none() {
                            fmt.print_stream_command(
                                &mut self.base.engine(),
                                STR_CMD_FLUSH,
                                output,
                            );
                        }
                    } else if cmd == BFC_EXIT {
                        fmt.print_stream_command(&mut self.base.engine(), STR_CMD_EXIT, output);
                        return Ok(());
                    } else if cmd == BFC_IGNORE {
                        fmt.print_stream_command(&mut self.base.engine(), STR_CMD_IGNORE, output);
                    } else if cmd == BFC_RESUME {
                        fmt.print_stream_command(&mut self.base.engine(), STR_CMD_RESUME, output);
                    }
                }
                BinaryPacketType::BfpText => {
                    let text = self.text.clone();
                    fmt.print_plain_text_line(&mut self.base.engine(), &text, output);
                }
                BinaryPacketType::BfpInvalid => {}
            }
        }
        self.flush(fmt, output, false)?;
        Ok(())
    }

    /// C++ local `flush(flush_after)` lambda: set `flush_after` on the back
    /// window, drain `gWindow->next` through the grammar, then print + free every
    /// buffered `previous` window. Returns the back window (null → the caller
    /// emits a bare FLUSH command).
    fn flush<F, W>(
        &mut self,
        fmt: &mut F,
        output: &mut W,
        flush_after: bool,
    ) -> Result<Option<SwId>, crate::error::RunError>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        W: std::io::Write,
    {
        let back = self.base.doc.stream.back();
        if let Some(bsw) = back {
            self.base
                .doc
                .store
                .single_windows
                .get_mut(bsw.0)
                .flush_after = flush_after;
        }
        while self.base.engine().rotate_next().is_some() {
            self.base.engine().run_grammar_on_window_with(fmt, output)?;
        }
        self.base.engine().shuffle_windows_down();
        while !self.base.doc.stream.previous.is_empty() {
            let tmp = self.base.doc.stream.previous[0];
            // C++ virtual printSingleWindow — the most-derived format decides.
            fmt.print_single_window(&mut self.base.engine(), tmp, output, false)?;
            let t = Some(tmp);
            crate::single_window::free_swindow(
                &mut self.base.doc.store,
                &mut self.base.doc.cohorts,
                &mut self.base.doc.deps,
                t,
            );
            self.base.doc.stream.previous.remove(0);
        }
        Ok(back)
    }
}
