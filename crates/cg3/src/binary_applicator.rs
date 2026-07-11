//! Port of `src/BinaryApplicator.cpp` + `src/BinaryApplicator.hpp` — the binary
//! STREAM protocol applicator (distinct from the `.cg3b` grammar format: the
//! stream magic is `CGBF`, the grammar magic is `CG3B`).
//!
//! ## Composition (task design)
//! C++ `class BinaryApplicator : public virtual GrammarApplicator` becomes
//! [`BinaryApplicator`] holding a [`GrammarApplicator`](crate::grammar_applicator::GrammarApplicator)
//! `base` by value plus `header_done` and the reusable `text` member. Every
//! engine/core call goes through `self.base.<method>` (or the core free fns
//! threaded `&mut self.base.store` / `&mut self.base.gWindow` /
//! `&self.base.grammar`). No `src/*` module other than this file is edited.
//!
//! ## Wire format
//! All multi-byte integers are LITTLE-ENDIAN, via [`crate::inlines`]
//! `read_le`/`write_le` and the length-prefixed `read_utf8_le`/`write_utf8_le`
//! (`[u16 LE byte-length][UTF-8 bytes]`). REPRODUCED FLAGGED BUGS:
//!   * Header version is read NATIVELY (`reinterpret_cast<uint32_t*>` — NOT
//!     byte-swapped) even though the writer emits it little-endian → a
//!     big-endian host would spuriously fail the version check.
//!   * String byte-length prefix truncates to `u16` (`>65535`-byte strings wrap).
//!   * Deleted readings are NOT written by `printSingleWindow` (only
//!     `cohort->readings` are traversed).
//!   * An unknown stream command writes only the `BFP_COMMAND` type byte with no
//!     command byte following (malformed packet).
//!
//! ## I/O model
//! [`read_packet`](BinaryApplicator::read_packet) / `read_window` / `read_command`
//! / `read_text` and the three writers ([`print_single_window`](BinaryApplicator::print_single_window),
//! [`print_stream_command`](BinaryApplicator::print_stream_command),
//! [`print_plain_text_line`](BinaryApplicator::print_plain_text_line)) each take
//! the `input`/`output` handle as a generic `Read`/`Write` param (the base
//! `ux_stdin`/`ux_stdout` `Option<()>` fields are elided).
//!
//! [`run_grammar_on_text`](BinaryApplicator::run_grammar_on_text) is a genuine
//! port: it wraps `input` in a [`std::io::BufReader`] so it can peek for
//! end-of-stream (reproducing the C++ `while (!input.eof())`, where `eof()`
//! becomes true only after a failed read), then threads that reader through
//! `read_packet` and the print methods.

use std::io::{Read, Write};

use crate::arena::{CohortId, SwId, TagId};
use crate::cohort::{CT_RELATED, CT_REMOVED, DEP_NO_PARENT};
use crate::grammar::Grammar;
use crate::grammar_applicator::GrammarApplicator;
use crate::inlines::{read_le, ui8, ui16, ui32, write_le, write_utf8_le};
use crate::reading::Reading;
use crate::tag::{T_DEPENDENCY, T_MAPPING, T_RELATION};
use crate::types::UString;

/// C++ `version.hpp` `constexpr uint32_t CG3_BINARY_STREAM = 1`. `version.hpp`
/// is not yet ported, so the constant is reproduced here verbatim (its only
/// stream users are this file's reader/writer).
pub const CG3_BINARY_STREAM: u32 = 1;

/// C++ `grammar->single_tags[hash]` — resolves a tag hash to its `TagId`, else
/// `TagId(0)`. Reproduces `grammar_applicator::core::tag_by_hash` (which is
/// `pub(super)`, not reachable here); the module cannot be edited.
fn tag_by_hash(grammar: &Grammar, hash: u32) -> TagId {
    let it = grammar.single_tags.find(hash);
    if it != grammar.single_tags.end() {
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
/// C++ `enum BinaryPacketType : uint8_t`.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum BinaryPacketType {
    #[default]
    BFP_INVALID = 0,
    BFP_WINDOW = 1,
    BFP_COMMAND = 2,
    BFP_TEXT = 3,
}

impl BinaryPacketType {
    fn from_u8(v: u8) -> BinaryPacketType {
        match v {
            1 => BinaryPacketType::BFP_WINDOW,
            2 => BinaryPacketType::BFP_COMMAND,
            3 => BinaryPacketType::BFP_TEXT,
            _ => BinaryPacketType::BFP_INVALID,
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
/// `UString*` (→ the `text` member) for a TEXT packet. In the arena model those
/// become explicit variants, tracked here as the parsed [`SwId`], the raw
/// command byte, or a `text`-is-set marker.
#[derive(Default)]
pub struct BinaryPacket {
    pub r#type: BinaryPacketType,
    /// WINDOW: the parsed single-window id (C++ `payload = cSWindow`; may be
    /// `None` at EOF).
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
    /// C++ reusable `UString text` reused across TEXT packets.
    pub text: UString,
}

impl<'a> BinaryApplicator<'a> {
    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.binary-applicator-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.binary-applicator-fn]
    /// C++ `BinaryApplicator::BinaryApplicator(std::ostream& ux_err)`. Delegates
    /// to the base ctor with an empty body; `header_done = false`, `text` empty.
    pub fn new(base: &'a mut GrammarApplicator) -> Self {
        BinaryApplicator {
            base,
            text: UString::new(),
        }
    }

    // =======================================================================
    // Readers
    // =======================================================================

    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-packet-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-packet-fn]
    /// C++ `BinaryPacket BinaryApplicator::readPacket()`. Reads one wire packet.
    /// Reads the type byte, dispatches WINDOW/COMMAND, then in a SEPARATE `if`
    /// (not chained, faithful) dispatches TEXT. `ux_stdin` is threaded as an
    /// explicit `input` param (the base field is a placeholder).
    pub fn read_packet<R: Read>(&mut self, input: &mut R) -> BinaryPacket {
        let mut packet = BinaryPacket::default();
        let ty: u8 = read_le(input);
        packet.r#type = BinaryPacketType::from_u8(ty);
        if packet.r#type == BinaryPacketType::BFP_WINDOW {
            packet.window = self.read_window(input);
        } else if packet.r#type == BinaryPacketType::BFP_COMMAND {
            packet.command = self.read_command(input);
        }
        if packet.r#type == BinaryPacketType::BFP_TEXT {
            self.read_text(input);
            packet.text_set = true;
        }
        packet
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

    // [spec:cg3:def:binary-applicator.cg3.binary-applicator.read-window-fn]
    // [spec:cg3:sem:binary-applicator.cg3.binary-applicator.read-window-fn]
    /// C++ `void BinaryApplicator::readWindow(void*& payload)`. Reads a `u32 LE`
    /// body length, the body, and parses it with a `pos` cursor into a fresh
    /// `SingleWindow` (all integers LE). Returns the new window id, or `None` at
    /// EOF (C++ `payload = nullptr`). No bounds checking on tag indices (UB on a
    /// malformed index — faithful).
    pub fn read_window<R: Read>(&mut self, input: &mut R) -> Option<SwId> {
        let cs: u32 = read_le(input);

        // if (ux_stdin->eof()) { payload = nullptr; return; } — modelled as a
        // short read of the body (read_exact fails → EOF).
        let mut buf = vec![0u8; cs as usize];
        if input.read_exact(&mut buf).is_err() && cs != 0 {
            return None;
        }

        let c_swindow = self
            .base
            .gWindow
            .alloc_append_single_window(&mut self.base.store);
        self.base.init_empty_single_window(c_swindow);

        let mut pos = 0usize;

        // Primitives over `buf` at `pos`.
        macro_rules! read_u16 {
            () => {{
                let v = u16::from_le_bytes([buf[pos], buf[pos + 1]]);
                pos += 2;
                v
            }};
        }
        macro_rules! read_u32 {
            () => {{
                let v = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
                pos += 4;
                v
            }};
        }
        // READ_STR: u16 LE byte-length then that many UTF-8 bytes.
        macro_rules! read_str {
            () => {{
                let tl = read_u16!() as usize;
                let s = String::from_utf8_lossy(&buf[pos..pos + tl]).into_owned();
                pos += tl;
                s
            }};
        }

        // 1. Window flags.
        let flags = read_u16!();
        if flags & (BFW_DEP_SPAN as u16) != 0 {
            self.base.dep_has_spanned = true;
        }

        // 2. Tag table.
        let tag_count = read_u16!();
        let mut window_tags: Vec<TagId> = Vec::with_capacity(tag_count as usize);
        for _ in 0..tag_count {
            let tg = read_str!();
            let first = tg.chars().next().unwrap_or('\0');
            let tid = self.base.add_tag(&tg, crate::tag::TagType::empty());
            // tg[0] == grammar->mapping_prefix ? |= T_MAPPING : &= ~T_MAPPING.
            let t = self.base.grammar.single_tags_list.get_mut(tid.0);
            if first == self.base.grammar.mapping_prefix {
                t.r#type |= T_MAPPING;
            } else {
                t.r#type &= !T_MAPPING;
            }
            window_tags.push(tid);
        }

        // 3. Variables: [1 byte mode][u16 key][u16 value].
        let var_count = read_u16!();
        for _ in 0..var_count {
            let mode = buf[pos] as u32;
            pos += 1;
            let key = read_u16!() as usize;
            let value = read_u16!() as usize;
            let hash1 = self.base.grammar.single_tags_list[window_tags[key].0].hash;
            let sw = self.base.store.single_windows.get_mut(c_swindow.0);
            if mode == BFV_SETVAR {
                let vh = self.base.grammar.single_tags_list[window_tags[value].0].hash;
                sw.variables_set.insert((hash1, vh));
                sw.variables_rem.erase(hash1);
                sw.variables_output.insert(hash1);
            } else if mode == BFV_SETVAR_ANY {
                let any = self.base.grammar.tag_any;
                sw.variables_set.insert((hash1, any));
                sw.variables_rem.erase(hash1);
                sw.variables_output.insert(hash1);
            } else if mode == BFV_REMVAR {
                sw.variables_set.erase(hash1);
                sw.variables_rem.insert(hash1);
                sw.variables_output.insert(hash1);
            }
        }

        // 4. Window text / text_post.
        {
            let t = read_str!();
            self.base.store.single_windows.get_mut(c_swindow.0).text = t;
            let tp = read_str!();
            self.base
                .store
                .single_windows
                .get_mut(c_swindow.0)
                .text_post = tp;
        }

        // 5. Cohorts.
        let cohort_count = read_u16!();
        for cn in 0..cohort_count {
            let c_cohort = crate::cohort::alloc_cohort(&mut self.base.store, Some(c_swindow));
            let gn = self.base.gWindow.cohort_counter;
            self.base.gWindow.cohort_counter = self.base.gWindow.cohort_counter.wrapping_add(1);
            self.base.store.cohorts.get_mut(c_cohort.0).global_number = gn;
            self.base.numCohorts = self.base.numCohorts.wrapping_add(1);

            let cflags = read_u16!();
            if cflags & (BFC_RELATED as u16) != 0 {
                self.base.store.cohorts.get_mut(c_cohort.0).r#type |= CT_RELATED;
                self.base.has_relations = true;
            }

            let wf_idx = read_u16!() as usize;
            self.base.store.cohorts.get_mut(c_cohort.0).wordform = Some(window_tags[wf_idx]);

            // Static tags → wread.
            let stag_count = read_u16!();
            if stag_count != 0 {
                let wread = crate::reading::alloc_reading(&mut self.base.store, Some(c_cohort));
                self.base.store.cohorts.get_mut(c_cohort.0).wread = Some(wread);
                let wf = window_tags[wf_idx];
                self.base.add_tag_to_reading(wread, wf);
                for tn in 0..stag_count {
                    let ti = read_u16!() as usize;
                    let rehash = tn + 1 == stag_count;
                    self.base
                        .add_tag_to_reading_rehash(wread, window_tags[ti], rehash);
                }
            }

            // Dependency.
            let dep_self = read_u32!();
            let dep_parent = read_u32!();
            let dep_parent = if dep_parent == crate::cohort::DEP_NO_PARENT {
                None
            } else {
                Some(dep_parent)
            };
            {
                let c = self.base.store.cohorts.get_mut(c_cohort.0);
                c.dep_self = dep_self;
                c.dep_parent = dep_parent;
            }
            self.base.gWindow.relation_map.insert((dep_self, gn));
            if dep_parent.is_some() {
                self.base.has_dep = true;
            }

            // Relations: [u16 tag-index][u32 head].
            let rel_count = read_u16!();
            for _ in 0..rel_count {
                let ti = read_u16!() as usize;
                let head = read_u32!();
                let rhash = self.base.grammar.single_tags_list[window_tags[ti].0].hash;
                self.base
                    .store
                    .cohorts
                    .get_mut(c_cohort.0)
                    .relations_input
                    .entry(rhash)
                    .or_default()
                    .insert(head);
            }
            if rel_count != 0 {
                self.base.has_relations = true;
                self.base.gWindow.relation_map.insert((dep_self, gn));
                self.base.store.cohorts.get_mut(c_cohort.0).r#type |= CT_RELATED;
            }

            // Cohort text / wblank.
            {
                let t = read_str!();
                self.base.store.cohorts.get_mut(c_cohort.0).text = t;
                let wb = read_str!();
                self.base.store.cohorts.get_mut(c_cohort.0).wblank = wb;
            }

            // Readings.
            let reading_count = read_u16!();
            if reading_count == 0 {
                self.base.init_empty_cohort(c_cohort);
            }
            let mut prev: Option<crate::arena::ReadingId> = None;
            for _ in 0..reading_count {
                let c_reading = crate::reading::alloc_reading(&mut self.base.store, Some(c_cohort));
                let wf = self.base.store.cohorts.get(c_cohort.0).wordform.unwrap();
                self.base.add_tag_to_reading(c_reading, wf);

                let rflags = read_u16!();

                let base_idx = read_u16!() as usize;
                self.base
                    .add_tag_to_reading(c_reading, window_tags[base_idx]);

                let rtag_count = read_u16!();
                let mut mappings = crate::tag::TagList::new();
                for _ in 0..rtag_count {
                    let ti = read_u16!() as usize;
                    let tid = window_tags[ti];
                    if self.base.grammar.single_tags_list[tid.0]
                        .r#type
                        .intersects(T_MAPPING)
                    {
                        mappings.push(tid);
                    } else {
                        self.base.add_tag_to_reading(c_reading, tid);
                    }
                }
                if !mappings.is_empty() {
                    self.base
                        .split_mappings(&mut mappings, c_cohort, c_reading, true);
                }

                if prev.is_some() && (rflags & (BFR_SUBREADING as u16) != 0) {
                    self.base.store.readings.get_mut(prev.unwrap().0).next = Some(c_reading);
                } else if rflags & (BFR_DELETED as u16) != 0 {
                    self.base
                        .store
                        .cohorts
                        .get_mut(c_cohort.0)
                        .deleted
                        .push(c_reading);
                } else {
                    crate::cohort::append_reading(&mut self.base.store, c_cohort, c_reading);
                }
                prev = Some(c_reading);
                self.base.numReadings = self.base.numReadings.wrapping_add(1);
            }

            // Last cohort: ensure endtag on every reading. `endtag` is a tag
            // HASH (C++ `addTagToReading(*iter, endtag)` uint32 overload) → the
            // TagId is resolved via `single_tags[endtag]` for the Tag* overload.
            if cn + 1 == cohort_count {
                let endtag_id = tag_by_hash(&self.base.grammar, self.base.endtag);
                let readings = self.base.store.cohorts.get(c_cohort.0).readings.clone();
                for r in readings {
                    let has = self
                        .base
                        .store
                        .readings
                        .get(r.0)
                        .tags
                        .find(self.base.endtag)
                        != self.base.store.readings.get(r.0).tags.end();
                    if !has {
                        self.base.add_tag_to_reading(r, endtag_id);
                    }
                }
            }

            crate::inlines::insert_if_exists(
                &mut self.base.store.cohorts.get_mut(c_cohort.0).possible_sets,
                self.base.grammar.sets_any.as_ref(),
            );
            crate::single_window::append_cohort(
                &mut self.base.gWindow,
                &mut self.base.store,
                c_swindow,
                c_cohort,
            );
        }

        Some(c_swindow)
    }

    // =======================================================================
    // Writers
    // =======================================================================
}

// [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-plain-text-line-fn]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-plain-text-line-fn]
// [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-stream-command-fn]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-stream-command-fn]
// [spec:cg3:def:binary-applicator.cg3.binary-applicator.print-single-window-fn]
// [spec:cg3:sem:binary-applicator.cg3.binary-applicator.print-single-window-fn]
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
        write_le(output, ui8(BinaryPacketType::BFP_TEXT as u32));
        write_utf8_le(output, line);
    }

    /// Body of C++ `BinaryApplicator::printStreamCommand` (spec anchors on
    /// [`BinaryFormat`]). Header if needed, then `BFP_COMMAND` byte,
    /// then the mapped command byte. QUIRK (faithful): an unrecognised `cmd`
    /// writes ONLY the type byte (malformed packet). No flush.
    pub fn bin_print_stream_command<W: Write>(&mut self, cmd: &str, output: &mut W) {
        self.bin_write_header(output);
        write_le(output, ui8(BinaryPacketType::BFP_COMMAND as u32));
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

    /// Body of C++ `BinaryApplicator::printSingleWindow` (spec anchors on
    /// [`BinaryFormat`]) — the exact inverse of `readWindow`.
    /// `profiling` is ignored. All integers LITTLE-ENDIAN. `store` is threaded
    /// separately so the caller can split the `&mut app` / `&mut store`
    /// borrows (matching the base print methods).
    pub fn bin_print_single_window<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        window: SwId,
        output: &mut W,
        _profiling: bool,
    ) {
        self.bin_write_header(output);
        write_le(output, ui8(BinaryPacketType::BFP_WINDOW as u32));

        // Per-window tag table.
        let mut tags_to_write: Vec<TagId> = Vec::new();
        let mut tag_index: std::collections::HashMap<TagId, u16> = std::collections::HashMap::new();

        // WRITE_U16_INTO / WRITE_U32_INTO (little-endian bytes into a buffer).
        fn wu16(buffer: &mut Vec<u8>, n: u16) {
            buffer.extend_from_slice(&n.to_le_bytes());
        }
        fn wu32(buffer: &mut Vec<u8>, n: u32) {
            buffer.extend_from_slice(&n.to_le_bytes());
        }
        // WRITE_TAG_INTO: register a tag (assign next u16 index if new) + append.
        let write_tag = |tags_to_write: &mut Vec<TagId>,
                         tag_index: &mut std::collections::HashMap<TagId, u16>,
                         buffer: &mut Vec<u8>,
                         tag: TagId| {
            let idx = *tag_index.entry(tag).or_insert_with(|| {
                let i = ui16(tags_to_write.len());
                tags_to_write.push(tag);
                i
            });
            wu16(buffer, idx);
        };
        // WRITE_STR_INTO: [u16 LE byte-length][UTF-8 bytes] (u16 truncation quirk).
        fn write_str(buffer: &mut Vec<u8>, s: &str) {
            let bytes = s.as_bytes();
            let olen = ui16(bytes.len());
            wu16(buffer, olen);
            buffer.extend_from_slice(&bytes[..olen as usize]);
        }

        // Variables.
        let mut var_count: u16 = 0;
        let mut var_buffer: Vec<u8> = Vec::new();
        let vars_output: Vec<u32> = app
            .store
            .single_windows
            .get(window.0)
            .variables_output
            .iter()
            .copied()
            .collect();
        for var in vars_output {
            var_count += 1;
            let key = tag_by_hash(&app.grammar, var);
            let value: Option<u32> = {
                let sw = app.store.single_windows.get(window.0);
                let it = sw.variables_set.find(var);
                if it != sw.variables_set.end() {
                    Some(it.get().1)
                } else {
                    None
                }
            };
            match value {
                Some(vh) => {
                    if vh != app.grammar.tag_any {
                        var_buffer.push(BFV_SETVAR as u8);
                        write_tag(&mut tags_to_write, &mut tag_index, &mut var_buffer, key);
                        let vtag = tag_by_hash(&app.grammar, vh);
                        write_tag(&mut tags_to_write, &mut tag_index, &mut var_buffer, vtag);
                    } else {
                        var_buffer.push(BFV_SETVAR_ANY as u8);
                        write_tag(&mut tags_to_write, &mut tag_index, &mut var_buffer, key);
                        wu16(&mut var_buffer, 0);
                    }
                }
                None => {
                    var_buffer.push(BFV_REMVAR as u8);
                    write_tag(&mut tags_to_write, &mut tag_index, &mut var_buffer, key);
                    wu16(&mut var_buffer, 0);
                }
            }
        }

        // Reflow removed-cohort text to the nearest prior non-removed cohort (or
        // the window). QUIRK: the inner loop has NO break — after clearing, later
        // iterations append the now-empty string (no-op).
        let all_cohorts: Vec<CohortId> = app.store.single_windows.get(window.0).all_cohorts.clone();
        for i in 0..all_cohorts.len() {
            let cohort = all_cohorts[i];
            let (ln, ty, has_text) = {
                let c = app.store.cohorts.get(cohort.0);
                (c.local_number, c.r#type, !c.text.is_empty())
            };
            if (ln == 0 || (ty.intersects(CT_REMOVED))) && has_text {
                for j in (1..=i).rev() {
                    let prior = all_cohorts[j - 1];
                    let (pln, pty) = {
                        let c = app.store.cohorts.get(prior.0);
                        (c.local_number, c.r#type)
                    };
                    if pln == 0 || (pty.intersects(CT_REMOVED)) {
                        continue;
                    }
                    let txt = app.store.cohorts.get(cohort.0).text.clone();
                    app.store.cohorts.get_mut(prior.0).text.push_str(&txt);
                    app.store.cohorts.get_mut(cohort.0).text.clear();
                }
                let txt = app.store.cohorts.get(cohort.0).text.clone();
                app.store
                    .single_windows
                    .get_mut(window.0)
                    .text
                    .push_str(&txt);
                app.store.cohorts.get_mut(cohort.0).text.clear();
            }
        }

        // Cohorts.
        let mut cohort_buffer: Vec<u8> = Vec::new();
        let mut cohort_count: u16 = 0;
        for cohort in all_cohorts {
            let (ln, ty) = {
                let c = app.store.cohorts.get(cohort.0);
                (c.local_number, c.r#type)
            };
            if ln == 0 || (ty.intersects(CT_REMOVED)) {
                continue;
            }
            crate::cohort::unignore_all(&mut app.store, cohort);
            cohort_count += 1;

            let mut cflags: u16 = 0;
            if app
                .store
                .cohorts
                .get(cohort.0)
                .r#type
                .intersects(CT_RELATED)
            {
                cflags |= BFC_RELATED as u16;
            }
            wu16(&mut cohort_buffer, cflags);

            let wf = app
                .store
                .cohorts
                .get(cohort.0)
                .wordform
                .expect("cohort wordform");
            let wf_hash = app.grammar.single_tags_list[wf.0].hash;
            write_tag(&mut tags_to_write, &mut tag_index, &mut cohort_buffer, wf);

            // Static tags (wread), excluding the wordform hash.
            if let Some(wr) = app.store.cohorts.get(cohort.0).wread {
                let mut tag_buf: Vec<u8> = Vec::new();
                let mut stag_count: u16 = 0;
                let tags: Vec<u32> = app.store.readings.get(wr.0).tags_list.clone();
                for tter in tags {
                    if tter == wf_hash {
                        continue;
                    }
                    let tid = tag_by_hash(&app.grammar, tter);
                    write_tag(&mut tags_to_write, &mut tag_index, &mut tag_buf, tid);
                    stag_count += 1;
                }
                wu16(&mut cohort_buffer, stag_count);
                cohort_buffer.extend_from_slice(&tag_buf);
            } else {
                wu16(&mut cohort_buffer, 0);
            }

            // Dependency: self = global_number; parent per the cohort_map lookup.
            let (global_number, dep_parent) = {
                let c = app.store.cohorts.get(cohort.0);
                (c.global_number, c.dep_parent)
            };
            wu32(&mut cohort_buffer, global_number);
            if dep_parent == Some(0) || dep_parent.is_none() {
                // C++ writes the raw field (0 or DEP_NO_PARENT).
                wu32(
                    &mut cohort_buffer,
                    dep_parent.unwrap_or(crate::cohort::DEP_NO_PARENT),
                );
            } else if let Some(&pr) = app.gWindow.cohort_map.get(&dep_parent.unwrap()) {
                let pr_local = app.store.cohorts.get(pr.0).local_number;
                if pr_local == 0 {
                    wu32(&mut cohort_buffer, 0);
                } else {
                    wu32(
                        &mut cohort_buffer,
                        app.store.cohorts.get(pr.0).global_number,
                    );
                }
            } else {
                wu32(&mut cohort_buffer, DEP_NO_PARENT);
            }

            // Relations.
            let mut rel_buffer: Vec<u8> = Vec::new();
            let mut rel_count: u16 = 0;
            let relations: Vec<(u32, Vec<u32>)> = app
                .store
                .cohorts
                .get(cohort.0)
                .relations
                .iter()
                .map(|(k, v)| (*k, v.iter().copied().collect()))
                .collect();
            for (name_hash, targets) in relations {
                let tid = tag_by_hash(&app.grammar, name_hash);
                for target in targets {
                    rel_count += 1;
                    write_tag(&mut tags_to_write, &mut tag_index, &mut rel_buffer, tid);
                    wu32(&mut rel_buffer, target);
                }
            }
            wu16(&mut cohort_buffer, rel_count);
            cohort_buffer.extend_from_slice(&rel_buffer);

            let (ctext, cwblank) = {
                let c = app.store.cohorts.get(cohort.0);
                (c.text.clone(), c.wblank.clone())
            };
            write_str(&mut cohort_buffer, &ctext);
            write_str(&mut cohort_buffer, &cwblank);

            // Readings: sort by cmp_number; only top readings with !noprint, then
            // walk the subreading chain. Deleted readings are NOT written.
            let mut reading_buffer: Vec<u8> = Vec::new();
            let mut reading_count: u16 = 0;
            let mut readings: Vec<crate::arena::ReadingId> =
                app.store.cohorts.get(cohort.0).readings.clone();
            readings.sort_by(|&a, &b| {
                let ra = app.store.readings.get(a.0);
                let rb = app.store.readings.get(b.0);
                if Reading::cmp_number(ra, rb) {
                    std::cmp::Ordering::Less
                } else if Reading::cmp_number(rb, ra) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            app.store.cohorts.get_mut(cohort.0).readings = readings.clone();
            for top_reading in readings {
                if app.store.readings.get(top_reading.0).noprint {
                    continue;
                }
                let mut reading = Some(top_reading);
                while let Some(rid) = reading {
                    reading_count += 1;
                    let mut rflags: u16 = 0;
                    if rid != top_reading {
                        rflags |= BFR_SUBREADING as u16;
                    }
                    wu16(&mut reading_buffer, rflags);
                    let baseform = app.store.readings.get(rid.0).baseform.unwrap_or(0);
                    let btid = tag_by_hash(&app.grammar, baseform);
                    write_tag(
                        &mut tags_to_write,
                        &mut tag_index,
                        &mut reading_buffer,
                        btid,
                    );

                    let mut tag_buf: Vec<u8> = Vec::new();
                    let mut tag_count: u16 = 0;
                    let mut unique: crate::sorted_vector::uint32SortedVector =
                        crate::sorted_vector::uint32SortedVector::new();
                    let tags: Vec<u32> = app.store.readings.get(rid.0).tags_list.clone();
                    let parent_wf_hash = {
                        let cid = app.store.readings.get(rid.0).parent.unwrap();
                        let w = app.store.cohorts.get(cid.0).wordform;
                        w.map(|t| app.grammar.single_tags_list[t.0].hash)
                            .unwrap_or(0)
                    };
                    for tter in tags {
                        if tter == baseform || tter == parent_wf_hash {
                            continue;
                        }
                        let tid = tag_by_hash(&app.grammar, tter);
                        let tt = app.grammar.single_tags_list[tid.0].r#type;
                        if tt.intersects(T_DEPENDENCY | T_RELATION) {
                            continue;
                        }
                        if app.unique_tags {
                            if unique.find(tter) != unique.end() {
                                continue;
                            }
                            unique.insert(tter);
                        }
                        write_tag(&mut tags_to_write, &mut tag_index, &mut tag_buf, tid);
                        tag_count += 1;
                    }
                    wu16(&mut reading_buffer, tag_count);
                    reading_buffer.extend_from_slice(&tag_buf);
                    reading = app.store.readings.get(rid.0).next;
                }
            }
            wu16(&mut cohort_buffer, reading_count);
            cohort_buffer.extend_from_slice(&reading_buffer);
        }

        // Header buffer (assembled AFTER the cohort buffer so the tag table is
        // complete).
        let mut header_buffer: Vec<u8> = Vec::new();
        let mut wflags: u16 = 0;
        if app.dep_has_spanned {
            wflags |= BFW_DEP_SPAN as u16;
        }
        wu16(&mut header_buffer, wflags);
        wu16(&mut header_buffer, ui16(tags_to_write.len()));
        for &tag in &tags_to_write {
            let s = app.grammar.single_tags_list[tag.0].tag.clone();
            write_str(&mut header_buffer, &s);
        }
        wu16(&mut header_buffer, var_count);
        header_buffer.extend_from_slice(&var_buffer);
        let (wtext, wtext_post, flush_after) = {
            let w = app.store.single_windows.get(window.0);
            (w.text.clone(), w.text_post.clone(), w.flush_after)
        };
        write_str(&mut header_buffer, &wtext);
        write_str(&mut header_buffer, &wtext_post);
        wu16(&mut header_buffer, cohort_count);

        // Emit: total_size (u32 LE), header buffer, cohort buffer.
        let total_size = ui32(header_buffer.len() + cohort_buffer.len());
        write_le(output, total_size);
        let _ = output.write_all(&header_buffer);
        let _ = output.write_all(&cohort_buffer);

        if flush_after {
            // C++ virtual printStreamCommand — only ever reached with binary
            // output active, so the binary writer is the dispatch target.
            self.bin_print_stream_command(STR_CMD_FLUSH, output);
        }
        let _ = output.flush();
    }
}

impl crate::grammar_applicator::stream_format::StreamFormat for BinaryFormat {
    fn print_single_window<W: Write>(
        &mut self,
        app: &mut GrammarApplicator,
        window: SwId,
        output: &mut W,
        profiling: bool,
    ) {
        self.bin_print_single_window(app, window, output, profiling);
    }

    fn print_stream_command<W: Write>(
        &mut self,
        _app: &mut GrammarApplicator,
        cmd: &str,
        output: &mut W,
    ) {
        self.bin_print_stream_command(cmd, output);
    }

    fn print_plain_text_line<W: Write>(
        &mut self,
        _app: &mut GrammarApplicator,
        line: &str,
        output: &mut W,
    ) {
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
    pub fn run_grammar_on_text<F, R, W>(&mut self, fmt: &mut F, input: &mut R, output: &mut W)
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        R: std::io::Read,
        W: std::io::Write,
    {
        use std::io::BufRead;
        // ux_stdin = &input; ux_stdout = &output; (Option<()> placeholders).
        // good()/eof()/output/grammar validity checks: deferred I/O.

        let mut input = std::io::BufReader::new(input);

        {
            let mut header = [0u8; 8];
            if input.read_exact(&mut header).is_err() {
                // "Error: Could not read stream header!" + CG3Quit(1): deferred.
                return;
            }
            if !crate::inlines::is_cg3bsf(header) {
                // "Stream does not start with magic bytes" + CG3Quit(1): deferred.
                return;
            }
            // BUG (faithful): version read NATIVELY, not byte-swapped.
            let version = u32::from_ne_bytes([header[4], header[5], header[6], header[7]]);
            if version != CG3_BINARY_STREAM {
                // "Stream is version %u..." + CG3Quit(1): deferred.
                return;
            }
        }

        self.base.index();
        let reset_after: u32 = (self.base.num_windows + 4) * 2 + 1;
        self.base.gWindow.window_span = self.base.num_windows;

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
            let packet = self.read_packet(&mut input);
            match packet.r#type {
                BinaryPacketType::BFP_WINDOW => {
                    self.base.numWindows = self.base.numWindows.wrapping_add(1);
                    if self.base.gWindow.next.len() > self.base.num_windows as usize {
                        self.shuffle_windows_down();
                        self.base.run_grammar_on_window_with(fmt, output);
                        if self.base.numWindows % reset_after == 0 {
                            self.base.reset_indexes();
                        }
                    }
                }
                BinaryPacketType::BFP_COMMAND => {
                    let cmd = packet.command;
                    if cmd == BFC_FLUSH {
                        let back = self.flush(fmt, output, true);
                        if back.is_none() {
                            fmt.print_stream_command(self.base, STR_CMD_FLUSH, output);
                        }
                    } else if cmd == BFC_EXIT {
                        fmt.print_stream_command(self.base, STR_CMD_EXIT, output);
                        return;
                    } else if cmd == BFC_IGNORE {
                        fmt.print_stream_command(self.base, STR_CMD_IGNORE, output);
                    } else if cmd == BFC_RESUME {
                        fmt.print_stream_command(self.base, STR_CMD_RESUME, output);
                    }
                }
                BinaryPacketType::BFP_TEXT => {
                    let text = self.text.clone();
                    fmt.print_plain_text_line(self.base, &text, output);
                }
                BinaryPacketType::BFP_INVALID => {}
            }
        }
        self.flush(fmt, output, false);
    }

    /// C++ local `flush(flush_after)` lambda: set `flush_after` on the back
    /// window, drain `gWindow->next` through the grammar, then print + free every
    /// buffered `previous` window. Returns the back window (null → the caller
    /// emits a bare FLUSH command).
    fn flush<F, W>(&mut self, fmt: &mut F, output: &mut W, flush_after: bool) -> Option<SwId>
    where
        F: crate::grammar_applicator::stream_format::StreamFormat,
        W: std::io::Write,
    {
        let back = self.base.gWindow.back();
        if let Some(bsw) = back {
            self.base.store.single_windows.get_mut(bsw.0).flush_after = flush_after;
        }
        while !self.base.gWindow.next.is_empty() {
            self.shuffle_windows_down();
            self.base.run_grammar_on_window_with(fmt, output);
        }
        self.shuffle_windows_down();
        while !self.base.gWindow.previous.is_empty() {
            let tmp = self.base.gWindow.previous[0];
            // C++ virtual printSingleWindow — the most-derived format decides.
            fmt.print_single_window(self.base, tmp, output, false);
            let t = Some(tmp);
            crate::single_window::free_swindow(&mut self.base.gWindow, &mut self.base.store, t);
            self.base.gWindow.previous.remove(0);
        }
        back
    }

    /// C++ `Window::shuffleWindowsDown()` as invoked from the binary driver.
    /// The C++ method does `current->variables_set = parent->variables;` through
    /// the Window's applicator back-pointer before retiring `current`; the Rust
    /// `Window` has no such back-pointer (placeholder), so the driver performs
    /// that snapshot here before delegating (mirroring the CG driver's private
    /// wrapper in `grammar_applicator/run_grammar.rs`). Only the live slots of
    /// the flat map are copied (`EMPTY`/`DEL` sentinels skipped).
    fn shuffle_windows_down(&mut self) {
        if let Some(current) = self.base.gWindow.current {
            let vars: Vec<(u32, u32)> = self
                .base
                .variables
                .get()
                .iter()
                .copied()
                .filter(|&(k, _)| k != u32::MAX && k != u32::MAX - 1)
                .collect();
            let sww = self.base.store.single_windows.get_mut(current.0);
            sww.variables_set.clear(0);
            for (k, v) in vars {
                sww.variables_set.insert((k, v));
            }
        }
        self.base.gWindow.shuffle_windows_down(&mut self.base.store);
    }
}
