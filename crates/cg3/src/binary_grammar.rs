//! Port of `src/BinaryGrammar.cpp` + `src/BinaryGrammar_read.cpp` +
//! `src/BinaryGrammar_write.cpp` + `src/BinaryGrammar.hpp` — the `.cg3b`
//! binary-grammar (de)serializer.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). The on-disk format MUST stay
//! BYTE-COMPATIBLE with the CURRENT revision (`CG3_FEATURE_REV` = 13898); byte
//! parity is the contract.
//!
//! DIVERGENCE: reading is not bug-for-bug. The C++ trusts the bytes; the port
//! reads them through a bounds-checked cursor and checks every number against
//! what it indexes before storing it, so a truncated or crafted `.cg3b` is a
//! load error rather than a grammar that panics later
//! (`[spec:cg3:req:robustness.binary-grammar-validated]`).
//!
//! ## Wire layout (big-endian ints, read through the checked cursor and
//! written with [`crate::inlines::write_be`]; strings are a 4-byte length
//! prefix + UTF-8 bytes — NOT the 16-bit-prefixed `writeUTF8` form):
//!   1. 4 raw magic bytes `"CG3B"`.
//!   2. `u32` feature revision (`CG3_FEATURE_REV`).
//!   3. `u32` top-level `BINF_*` feature bitset built from grammar state.
//!   4. mapping-prefix (`u32` len + UTF-8) when `BINF_PREFIX`.
//!   5. `cmdargs` and `cmdargs_override` (each `u32` len + raw bytes, always).
//!   6. tag table: `u32` count, then per-tag `u32` field mask + fields
//!      (comparison_val as a 12-byte double, regex PATTERN only, vs_sets/vs_names).
//!   7. reopen_mappings, preferred_targets, parentheses, anchors.
//!   8. set table: `u32` count, then per-set `u32` mask + fields (serialized
//!      tries with `u32` size prefixes).
//!   9. delimiters / soft_delimiters / text_delimiters set NUMBERS.
//!  10. context table: `u32` `contexts.size()`, each via `write_contextual_test`
//!      (dependency-first + dedup by hash).
//!  11. rule table: `u32` count, then per-rule mask + fields, then dep_target
//!      hash, then dep_tests / tests hash lists (after `reverse_contextual_tests`).
//!
//! ## Arena / pointer model
//! Follows `crate::grammar`: `Tag*`/`Set*`/`Rule*`/`ContextualTest*` are
//! `TagId`/`SetId`/`RuleId`/`CtxId` arena indices. The C++ `single_tags_list`,
//! `sets_list`, `rule_by_number` VECTORS (which the reader `resize`s then indexes
//! by `t->number`) are reconstructed by pre-allocating `count` arena slots and
//! then OVERWRITING `arena[number] = value` — so in a READ grammar a
//! tag/set/rule's `number` equals its arena slot, and the reader sets
//! `Grammar::sets_list_order` to the identity over those slots. References read
//! as raw numbers (`wordform`/`maplist`/`sublist`/`target`) become
//! `TagId`/`SetId(number)` directly (valid because slot == number post-read).
//!
//! ## BYTE-PARITY RISKS (documented, per the port brief)
//! * **Set numbering** is DENSE like the C++ (`add_set_to_list` pushes onto
//!   `Grammar::sets_list_order` and numbers by push position); the writer emits
//!   set records over that order with the dense count, so a port-written `.cg3b`
//!   re-reads cleanly. Remaining parity caveat: the DFS numbering order derives
//!   from iterating `sets_by_contents` in `reindex` step (10) — a `BTreeMap`
//!   in the port vs `std::unordered_map` in C++ — so the relative order of
//!   top-level used sets (and thus the exact number assignment) is stable
//!   across runs but not identical to any C++ stdlib's bucket order.
//! * **Context hashes** that involve a template differ from a C++-produced
//!   file: the C++ folds the `tmpl` POINTER into the hash (not reproducible
//!   even between two C++ runs), and the port folds its `CtxId` instead (see
//!   `crate::contextual_test`). `.cg3b` stores those hashes verbatim, so a
//!   grammar round-tripped through the port is self-consistent for them.
//! * **regex tags** store the PATTERN text only; the case-insensitive flag is
//!   re-derived on read from `T_CASE_INSENSITIVE` in `type` (compiled with
//!   `RegexBuilder::case_insensitive`, so `Regex::as_str` round-trips the bare
//!   pattern, as the C++ does).
//! * **comparison_val** is a 12-byte double (`u64` BE mantissa + `i32` BE
//!   exponent), via [`crate::inlines::write_be_f64`] / [`crate::inlines::read_be_f64`].
//! * **context record count** is `grammar.contexts.size()` — every context
//!   reachable via `tmpl`/`ors`/`linked` MUST also be a distinct `contexts` map
//!   entry, else more records emit than the count and the stream desyncs on read.
//!
//! ## Legacy reader OUT OF SCOPE
//! The C++ `readBinaryGrammar_10043` / `readContextualTest_10043` methods (the
//! legacy pre-10298 reader) are intentionally EXCLUDED from the port. The entry
//! point `read_binary_grammar_10043` is an ERRORING STUB that refuses legacy
//! input with "legacy .cg3b rev <10373 not supported"; the contextual-test
//! reader behind it has no port item at all (PORT DIVERGENCE note in
//! `docs/spec/port/src/BinaryGrammar.md`).

use std::collections::HashMap;
use std::io::{Read, Write};

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::contextual_test::{ContextualTest, POS_64BIT};
use crate::error::GrammarError;
use crate::flat_unordered_set::Uint32FlatHashSet;
use crate::grammar::{GrammarCore, trie_unserialize};
use crate::igrammar_parser::IGrammarParser;
use crate::inlines::{is_cg3b, ui16, ui32, write_be, write_be_f64};
use crate::rule::Rule;
use crate::set::Set;
use crate::strings::Keywords;
use crate::tag::{COps, T_CASE_INSENSITIVE, T_CONTEXT, T_LOCAL_VARIABLE, T_VARIABLE, Tag};
use crate::tag_regex::TagRegex;
use crate::tag_trie::trie_serialize;
use crate::types::{SetNumber, TagHash};

mod checks;
mod cursor;
mod read;

pub(crate) use cursor::Cg3bCursor;
use cursor::malformed;
use read::Load;

// C++ `BinaryGrammar.hpp` `enum : uint32_t { BINF_* }` — the top-level feature
// bitset. Reproduced verbatim (no `[spec:cg3:def]` id: an unnamed header enum).
const BINF_DEP: u32 = 1 << 0;
const BINF_PREFIX: u32 = 1 << 1;
const BINF_SUB_LTR: u32 = 1 << 2;
const BINF_TAGS: u32 = 1 << 3;
const BINF_REOPEN_MAP: u32 = 1 << 4;
const BINF_PREF_TARGETS: u32 = 1 << 5;
const BINF_ENCLS: u32 = 1 << 6;
const BINF_ANCHORS: u32 = 1 << 7;
const BINF_SETS: u32 = 1 << 8;
const BINF_DELIMS: u32 = 1 << 9;
const BINF_SOFT_DELIMS: u32 = 1 << 10;
const BINF_CONTEXTS: u32 = 1 << 11;
const BINF_RULES: u32 = 1 << 12;
const BINF_RELATIONS: u32 = 1 << 13;
const BINF_BAG: u32 = 1 << 14;
const BINF_ORDERED: u32 = 1 << 15;
const BINF_TEXT_DELIMS: u32 = 1 << 16;
const BINF_ADDCOHORT_ATTACH: u32 = 1 << 17;

// C++ `BinaryGrammar.hpp` `constexpr uint32_t BIN_REV_ANCIENT / BIN_REV_CMDARGS`.
const BIN_REV_ANCIENT: u32 = 10297;
const BIN_REV_CMDARGS: u32 = 13898;

// C++ `version.hpp` `constexpr uint32_t CG3_FEATURE_REV / CG3_TOO_OLD`.
// Reproduced locally (the port has no `version.hpp` module and this pass may
// create only `binary_grammar.rs`).
const CG3_FEATURE_REV: u32 = 13898;
const CG3_TOO_OLD: u32 = 10373;

// [spec:cg3:def:binary-grammar.cg3.binary-grammar.deferred-t]
/// C++ `typedef std::unordered_map<ContextualTest*, uint32_t> deferred_t`.
/// The `ContextualTest*` key becomes a `CtxId`.
pub type DeferredTests = HashMap<CtxId, u32>;

// [spec:cg3:def:binary-grammar.cg3.binary-grammar.deferred-ors-t]
/// C++ `typedef std::unordered_map<ContextualTest*, std::vector<uint32_t>> deferred_ors_t`.
pub type DeferredOrs = HashMap<CtxId, Vec<u32>>;

// [spec:cg3:def:binary-grammar.cg3.binary-grammar]
/// C++ `class BinaryGrammar : public IGrammarParser`.
///
/// C++ holds `Grammar* grammar` aliasing the externally-owned `result`. The port
/// OWNS its result `grammar` (per the brief: "the struct holds/builds a
/// `grammar: Grammar` (read) or references one (write)"). The inherited
/// `IGrammarParser` members (`nrules`, `nrules_inv`, `verbosity`) live here as
/// fields (a Rust trait has no fields). The C++ base error-stream pointer
/// has no field analogue: diagnostics are tracing events (wave 4).
pub struct BinaryGrammar {
    /// C++ `Grammar* grammar` (aliases `result`); OWNED here.
    pub grammar: GrammarCore,
    /// C++ base `nrules` — the `--nrules` name filter, compiled through the
    /// tag-regex seam like every other user-authored pattern
    /// (`[spec:cg3:req:tag-regex.single-seam]`).
    /// Public: C++ main.cpp sets `parser->nrules` on the IGrammarParser base
    /// for BOTH the textual and binary parsers.
    pub nrules: Option<TagRegex>,
    /// C++ base `nrules_inv` — the `--nrules-inv` filter.
    pub nrules_inv: Option<TagRegex>,
    /// C++ base `uint32_t verbosity`.
    verbosity: u32,
    deferred_tmpls: DeferredTests,
    deferred_ors: DeferredOrs,
    seen_uint32: Uint32FlatHashSet,
}

impl BinaryGrammar {
    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.binary-grammar-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.binary-grammar-fn]
    /// C++ `BinaryGrammar` constructor. Delegates to the base `IGrammarParser`
    /// ctor (stores the error stream, `result = &res`; `nrules`/`nrules_inv`
    /// null; `verbosity` 0), then sets `grammar = result`. The port OWNS `res`
    /// (so `grammar` == `result` == the owned field); diagnostics are tracing
    /// events. No allocation or I/O occurs.
    pub fn new(res: GrammarCore) -> BinaryGrammar {
        BinaryGrammar {
            grammar: res,
            nrules: None,
            nrules_inv: None,
            verbosity: 0,
            deferred_tmpls: DeferredTests::new(),
            deferred_ors: DeferredOrs::new(),
            seen_uint32: Uint32FlatHashSet::new(),
        }
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.parse-grammar-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.parse-grammar-fn]
    /// C++ `int parse_grammar(const char* filename)` — the file-path entry point.
    /// `stat`s the file into `grammar->grammar_size`, then reads it and delegates
    /// to the istream overload. The C++ null-`grammar` guard is moot here (the
    /// grammar is owned). The C++ ifstream exception mask makes a short read
    /// throw; here it is a [`GrammarError::Truncated`](crate::error::GrammarError::Truncated).
    pub fn parse_grammar_filename(&mut self, filename: &str) -> Result<(), crate::error::Cg3Error> {
        let meta = std::fs::metadata(filename).map_err(|source| {
            crate::error::GrammarError::Unreadable {
                path: filename.to_string(),
                source,
            }
        })?;
        self.grammar.grammar_size = meta.len() as usize;

        let data =
            std::fs::read(filename).map_err(|source| crate::error::GrammarError::Unreadable {
                path: filename.to_string(),
                source,
            })?;
        let rv = self.parse_cg3b(&data);
        // [spec:cg3:req:diagnostics.source-lazy]
        // The path, not the text: it is what lets a runtime failure find the
        // companion source file, and it costs nothing until one happens. Only
        // this entry point can supply it — the buffer one is handed bytes with
        // no file behind them.
        self.grammar.binary_path = Some(filename.to_string());
        rv
    }

    /// C++ `int parse_grammar(const char* buffer, size_t length)`: writes the
    /// bytes into a stringstream, seeks to 0, and calls the istream overload.
    /// The port reads the slice in place.
    pub fn parse_grammar_buffer(&mut self, buffer: &[u8]) -> Result<(), crate::error::Cg3Error> {
        self.parse_cg3b(buffer)
    }

    // [spec:cg3:def:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn+1]
    // [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn+1]
    /// C++ `int parse_grammar(std::istream& input)` (BinaryGrammar_read.cpp).
    /// Reads the stream to its end, then the `.cg3b` it held; see
    /// `parse_cg3b`.
    pub fn parse_grammar_reader<R: Read>(
        &mut self,
        input: &mut R,
    ) -> Result<(), crate::error::Cg3Error> {
        let mut data = Vec::new();
        input
            .read_to_end(&mut data)
            .map_err(|source| GrammarError::BinaryUnreadable { source })?;
        self.parse_cg3b(&data)
    }

    // [spec:cg3:def:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn+1]
    // [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn+1]
    // [spec:cg3:req:robustness.binary-grammar-validated]
    /// Reads a whole `.cg3b` blob into `grammar`, one section at a time in the
    /// wire order of the module docs. Every read is bounds-checked and every
    /// number is checked against what it indexes as it is stored; what needs a
    /// whole table — cycles, template references, crowded hashes — is checked
    /// once that table is in. A grammar this accepts is one reindexing, both
    /// writers and a run can take.
    fn parse_cg3b(&mut self, data: &[u8]) -> Result<(), crate::error::Cg3Error> {
        let mut cur = Cg3bCursor::new(data);
        // Header: 4 magic bytes.
        let magic = cur
            .bytes(4, "magic bytes")
            .map_err(|_| GrammarError::TruncatedHeader)?;
        if !is_cg3b(magic) {
            return Err(GrammarError::NotBinary.into());
        }

        let bin_revision: u32 = cur.be("grammar revision")?;
        if bin_revision <= BIN_REV_ANCIENT {
            if self.verbosity >= 1 {
                tracing::warn!(
                    "Warning: GrammarCore revision is {}, but current format is {} or later. Please recompile the binary grammar with latest CG-3.",
                    bin_revision,
                    CG3_FEATURE_REV
                );
            }
            // Rewinding to the start is moot: the 10043 path is an erroring stub.
            let mut input = data;
            return Err(self
                .read_binary_grammar_10043(&mut input, bin_revision)
                .into());
        }
        if !(CG3_TOO_OLD..=CG3_FEATURE_REV).contains(&bin_revision) {
            return Err(GrammarError::Revision {
                found: bin_revision,
                min: CG3_TOO_OLD,
                max: CG3_FEATURE_REV,
            }
            .into());
        }

        self.grammar.is_binary = true;

        let fields: u32 = cur.be("feature bits")?;

        self.grammar.has_dep = (fields & BINF_DEP) != 0;
        self.grammar.sub_readings_ltr = (fields & BINF_SUB_LTR) != 0;
        self.grammar.has_relations = (fields & BINF_RELATIONS) != 0;
        self.grammar.has_bag_of_tags = (fields & BINF_BAG) != 0;
        self.grammar.ordered = (fields & BINF_ORDERED) != 0;
        self.grammar.addcohort_attach = (fields & BINF_ADDCOHORT_ATTACH) != 0;

        self.read_prefix_and_cmdargs(&mut cur, fields, bin_revision)?;
        let mut load = Load::default();
        self.read_tags(&mut cur, fields, &mut load)?;
        self.read_tag_tables(&mut cur, fields, &mut load)?;
        self.read_sets(&mut cur, fields, &mut load)?;
        self.read_delimiters(&mut cur, fields, &load)?;
        self.read_contexts(&mut cur, fields, &mut load)?;
        self.read_rules(&mut cur, fields, &mut load)?;
        self.bind_deferred_tests(&load)?;
        Ok(())
    }

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// Read one tag record: the `tfields` bitmap followed by whatever
    /// fields it advertises, in the exact C++ order.
    ///
    /// A tag whose pattern will not compile is pushed to `bad_regexes` rather
    /// than aborting: the record has been read whole, so the read can continue
    /// and report every bad tag in one pass. A short record, or a number that
    /// fits nothing, ends the read.
    fn read_tag_record(
        cur: &mut Cg3bCursor<'_>,
        num_tags: u32,
        tag_varsets: &mut HashMap<u32, Vec<u32>>,
        bad_regexes: &mut Vec<crate::tag_regex::TagRegexError>,
    ) -> Result<Tag, GrammarError> {
        let at = cur.offset();
        let mut t = Tag::default(); // allocateTag()
        let tfields: u32 = cur.be("tag field mask")?;
        read::tag_scalars(cur, tfields, num_tags, &mut t)?;
        read::tag_text(cur, tfields, &mut t, bad_regexes)?;
        read::tag_varstring(cur, tfields, &mut t, tag_varsets)?;
        read::tag_role(cur, tfields, &mut t, at)?;
        Ok(t)
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-contextual-test-fn+1]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-fn+1]
    // [spec:cg3:def:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn+1]
    // [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn+1]
    /// C++ `ContextualTest* readContextualTest(std::istream& input)`. Reads one
    /// test record (a fresh `allocateContextualTest`) in the exact source field
    /// order: bit12 (jump_pos) is read BEFORE bit10 (ors) / bit11 (linked).
    /// `tmpl`/`ors` refs are DEFERRED; `linked` resolves inline via
    /// `contexts[hash]` (present because the writer emits linked children first).
    /// DIVERGENCE: a `linked` hash naming no test read so far, a missing hash,
    /// a set number past the set table and a relation naming no tag are
    /// refused; the C++ stores a null link or the unchecked number.
    fn read_contextual_test(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        num_sets: u32,
    ) -> Result<CtxId, GrammarError> {
        let at = cur.offset();
        let t = self.grammar.allocate_contextual_test();
        let fields: u32 = cur.be("contextual test field mask")?;
        let mut ct = ContextualTest::default();
        if let Some(h) = read::context_fields(cur, fields, &mut ct)? {
            self.deferred_tmpls.insert(t, h);
        }
        if fields & (1 << 10) != 0 {
            let num_ors = cur.count("OR'd test count", 4)?;
            let entry = self.deferred_ors.entry(t).or_default();
            for _ in 0..num_ors {
                entry.push(cur.be("OR'd test hash")?);
            }
        }
        if fields & (1 << 11) != 0 {
            ct.linked = Some(self.context_by_hash(cur, "linked test")?);
        }
        self.check_context(&ct, num_sets)
            .map_err(|fault| malformed(at, fault))?;
        self.grammar.contexts_arena[t.0] = ct;
        Ok(t)
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn]
    /// OUT OF SCOPE (the legacy pre-10298 `.cg3b` reader). ERRORING STUB: refuses
    /// legacy input instead of parsing. The real reader lived in
    /// BinaryGrammar_read_10043.cpp and is intentionally excluded.
    ///
    /// The refusal is an `Err`, not the old `Ok(1)`: it is a load failure, and
    /// encoding it in the success arm is exactly the C-ism this API shed.
    fn read_binary_grammar_10043<R: Read>(
        &mut self,
        _input: &mut R,
        found: u32,
    ) -> crate::error::GrammarError {
        crate::error::GrammarError::LegacyRevision { found }
    }

    /// The C++ dense `sets_list` VECTOR (`Grammar::sets_list_order`): position 0
    /// is the dummy (number 0), positions 1..k the sets numbered by
    /// `addSetToList`. Written in this exact order, with each set's dense
    /// `number`, matching `BinaryGrammar_write.cpp`.
    fn used_set_ids(&self) -> Vec<SetId> {
        self.grammar.sets_list_order.clone()
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.write-binary-grammar-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.write-binary-grammar-fn]
    // [spec:cg3:def:binary-grammar-write.cg3.binary-grammar.write-binary-grammar-fn]
    // [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-binary-grammar-fn]
    /// C++ `int writeBinaryGrammar(std::ostream& output)`. Serializes `grammar` as
    /// a byte-compatible `.cg3b` blob, returning 0. SIDE EFFECT: calls
    /// `reverseContextualTests()` on each rule (reverses `tests`/`dep_tests` in
    /// place). See the module docs for the full wire layout + byte-parity risks
    /// (esp. the set sparse-numbering divergence).
    pub fn write_binary_grammar<W: Write>(
        &mut self,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error> {
        // C++ guards: null output / null grammar. Both are owned here (moot); kept
        // as documentation.

        // The dense used-set list (C++ `grammar->sets_list`); computed up front so
        // the BINF_SETS bit + the set section agree.
        let used_sets = self.used_set_ids();

        let _ = output.write_all(b"CG3B");
        write_be(output, CG3_FEATURE_REV);

        let mut fields = 0u32;
        if self.grammar.has_dep {
            fields |= BINF_DEP;
        }
        if self.grammar.mapping_prefix != '\0' {
            fields |= BINF_PREFIX;
        }
        if self.grammar.sub_readings_ltr {
            fields |= BINF_SUB_LTR;
        }
        if self.grammar.num_tags != 0 {
            fields |= BINF_TAGS;
        }
        if !self.grammar.reopen_mappings.empty() {
            fields |= BINF_REOPEN_MAP;
        }
        if !self.grammar.preferred_targets.is_empty() {
            fields |= BINF_PREF_TARGETS;
        }
        if !self.grammar.parentheses.is_empty() {
            fields |= BINF_ENCLS;
        }
        if !self.grammar.anchors.empty() {
            fields |= BINF_ANCHORS;
        }
        if !used_sets.is_empty() {
            fields |= BINF_SETS;
        }
        if self.grammar.delimiters.is_some() {
            fields |= BINF_DELIMS;
        }
        if self.grammar.soft_delimiters.is_some() {
            fields |= BINF_SOFT_DELIMS;
        }
        if !self.grammar.contexts.is_empty() {
            fields |= BINF_CONTEXTS;
        }
        if self.grammar.rule_by_number.capacity() != 0 {
            fields |= BINF_RULES;
        }
        if self.grammar.has_relations {
            fields |= BINF_RELATIONS;
        }
        if self.grammar.has_bag_of_tags {
            fields |= BINF_BAG;
        }
        if self.grammar.ordered {
            fields |= BINF_ORDERED;
        }
        if self.grammar.text_delimiters.is_some() {
            fields |= BINF_TEXT_DELIMS;
        }
        if self.grammar.addcohort_attach {
            fields |= BINF_ADDCOHORT_ATTACH;
        }

        write_be(output, fields);

        if self.grammar.mapping_prefix != '\0' {
            let mut b = [0u8; 4];
            let s = self.grammar.mapping_prefix.encode_utf8(&mut b);
            write_be(output, s.len() as u32);
            let _ = output.write_all(s.as_bytes());
        }

        // cmdargs / cmdargs_override — always present (raw bytes, not transcoded).
        {
            let b = self.grammar.cmdargs.as_bytes();
            write_be(output, b.len() as u32);
            if !b.is_empty() {
                let _ = output.write_all(b);
            }
        }
        {
            let b = self.grammar.cmdargs_override.as_bytes();
            write_be(output, b.len() as u32);
            if !b.is_empty() {
                let _ = output.write_all(b);
            }
        }

        // --- Tags ---
        if self.grammar.num_tags != 0 {
            write_be(output, self.grammar.num_tags as u32);
        }
        for i in 0..(self.grammar.num_tags as u32) {
            // Snapshot the tag fields (release the arena borrow before reaching
            // into sets_list for vs_sets numbers).
            let (
                number,
                hash,
                plain_hash,
                seed,
                ttype,
                comparison_hash,
                comparison_op,
                comparison_val,
                tag_text,
                regex_pat,
                vs_sets,
                vs_names,
                dep_parent,
            ) = {
                let t = &self.grammar.single_tags_list[i];
                (
                    t.number,
                    t.hash,
                    t.plain_hash,
                    t.seed,
                    t.r#type,
                    t.comparison_hash,
                    t.comparison_op,
                    t.comparison_val,
                    t.tag.clone(),
                    t.regexp.as_ref().map(|r| r.as_str().to_string()),
                    t.vs_sets.clone(),
                    t.vs_names.clone(),
                    t.extra.raw(),
                )
            };

            let mut buffer: Vec<u8> = Vec::new();
            let mut tfields = 0u32;

            if number != 0 {
                tfields |= 1 << 0;
                write_be(&mut buffer, number);
            }
            if hash.get() != 0 {
                tfields |= 1 << 1;
                write_be(&mut buffer, hash.get());
            }
            if plain_hash.get() != 0 {
                tfields |= 1 << 2;
                write_be(&mut buffer, plain_hash.get());
            }
            if seed != 0 {
                tfields |= 1 << 3;
                write_be(&mut buffer, seed);
            }
            if !ttype.is_empty() {
                tfields |= 1 << 4;
                write_be(&mut buffer, ttype.bits());
            }
            if comparison_hash != 0 {
                tfields |= 1 << 5;
                write_be(&mut buffer, comparison_hash);
            }
            if comparison_op != COps::OpNop {
                tfields |= 1 << 6;
                write_be(&mut buffer, comparison_op as u32);
            }
            // Field 1<<7 is NOT reused (reserved until a hard format break).
            if comparison_val != 0.0 {
                tfields |= 1 << 12;
                write_be_f64(&mut buffer, comparison_val);
            }
            if !tag_text.is_empty() {
                tfields |= 1 << 8;
                let b = tag_text.as_bytes();
                write_be(&mut buffer, b.len() as i32);
                buffer.extend_from_slice(b);
            }
            if let Some(pat) = &regex_pat {
                tfields |= 1 << 9;
                let b = pat.as_bytes();
                write_be(&mut buffer, b.len() as i32);
                buffer.extend_from_slice(b);
            }
            if let Some(vs) = &vs_sets {
                tfields |= 1 << 10;
                write_be(&mut buffer, vs.len() as u32);
                for sid in vs.iter() {
                    let n = self.grammar.sets_list[sid.0].number.get();
                    write_be(&mut buffer, n);
                }
            }
            if let Some(vn) = &vs_names {
                tfields |= 1 << 11;
                write_be(&mut buffer, vn.len() as u32);
                for name in vn.iter() {
                    let b = name.as_bytes();
                    write_be(&mut buffer, b.len() as i32);
                    buffer.extend_from_slice(b);
                }
            }
            // 1<<12 used above.
            if ttype.intersects(T_VARIABLE | T_LOCAL_VARIABLE) && dep_parent != 0 {
                tfields |= 1 << 13;
                write_be(&mut buffer, dep_parent);
            }
            if ttype.intersects(T_CONTEXT) {
                tfields |= 1 << 14;
                write_be(&mut buffer, dep_parent);
            }

            write_be(output, tfields);
            let _ = output.write_all(&buffer);
        }

        // --- reopen_mappings ---
        if !self.grammar.reopen_mappings.empty() {
            write_be(output, self.grammar.reopen_mappings.size() as u32);
        }
        for &v in self.grammar.reopen_mappings.iter() {
            write_be(output, v);
        }

        // --- preferred_targets ---
        if !self.grammar.preferred_targets.is_empty() {
            write_be(output, self.grammar.preferred_targets.len() as u32);
        }
        for &v in &self.grammar.preferred_targets {
            write_be(output, v);
        }

        // --- parentheses ---
        if !self.grammar.parentheses.is_empty() {
            write_be(output, self.grammar.parentheses.len() as u32);
        }
        for (&k, &v) in &self.grammar.parentheses {
            write_be(output, k);
            write_be(output, v);
        }

        // --- anchors ---
        let anchors: Vec<(u32, u32)> = {
            let v: Vec<(u32, u32)> = self.grammar.anchors.iter().copied().collect();
            v
        };
        if !anchors.is_empty() {
            write_be(output, anchors.len() as u32);
        }
        for (a, b) in &anchors {
            write_be(output, *a);
            write_be(output, *b);
        }

        // --- Sets ---
        if !used_sets.is_empty() {
            write_be(output, used_sets.len() as u32);
        }
        for &sid in &used_sets {
            let (number, stype, trie, trie_special, set_ops, sets, name) = {
                let s = &self.grammar.sets_list[sid.0];
                (
                    s.number,
                    s.r#type,
                    s.trie.clone(),
                    s.trie_special.clone(),
                    s.set_ops.clone(),
                    s.sets.clone(),
                    s.name.clone(),
                )
            };

            let mut buffer: Vec<u8> = Vec::new();
            let mut sfields = 0u32;

            if number.get() != 0 {
                sfields |= 1 << 0;
                write_be(&mut buffer, number.get());
            }
            // ST_ORDERED == 1<<8: 16-bit type when >= it, else 8-bit (exactly one).
            if stype.bits() >= crate::set::ST_ORDERED.bits() {
                sfields |= 1 << 1;
                write_be(&mut buffer, stype.bits() as u32);
            } else {
                sfields |= 1 << 2;
                write_be(&mut buffer, stype.bits() as u8);
            }
            // getNonEmpty() non-empty == at least one trie non-empty.
            if !trie.is_empty() || !trie_special.is_empty() {
                sfields |= 1 << 3;
                write_be(&mut buffer, trie.len() as u32);
                trie_serialize(&trie, &mut buffer, &self.grammar);
                write_be(&mut buffer, trie_special.len() as u32);
                trie_serialize(&trie_special, &mut buffer, &self.grammar);
            }
            if !set_ops.is_empty() {
                sfields |= 1 << 4;
                write_be(&mut buffer, set_ops.len() as u32);
                for &v in &set_ops {
                    write_be(&mut buffer, v);
                }
            }
            if !sets.is_empty() {
                sfields |= 1 << 5;
                write_be(&mut buffer, sets.len() as u32);
                for &v in &sets {
                    write_be(&mut buffer, v);
                }
            }
            if stype.intersects(crate::set::ST_STATIC) {
                sfields |= 1 << 6;
                let b = name.as_bytes();
                write_be(&mut buffer, b.len() as i32);
                buffer.extend_from_slice(b);
            }

            write_be(output, sfields);
            let _ = output.write_all(&buffer);
        }

        // --- delimiters / soft_delimiters / text_delimiters (set NUMBERS) ---
        if let Some(d) = self.grammar.delimiters {
            let n = self.grammar.sets_list[d.0].number.get();
            write_be(output, n);
        }
        if let Some(d) = self.grammar.soft_delimiters {
            let n = self.grammar.sets_list[d.0].number.get();
            write_be(output, n);
        }
        if let Some(d) = self.grammar.text_delimiters {
            let n = self.grammar.sets_list[d.0].number.get();
            write_be(output, n);
        }

        // --- Contexts ---
        self.seen_uint32.clear(0);
        if !self.grammar.contexts.is_empty() {
            write_be(output, self.grammar.contexts.len() as u32);
        }
        let ctx_ids: Vec<CtxId> = self.grammar.contexts.values().copied().collect();
        for cid in ctx_ids {
            self.write_contextual_test(cid, output)?;
        }

        // --- Rules ---
        let num_rules = self.grammar.rule_by_number.capacity();
        if num_rules != 0 {
            write_be(output, num_rules);
        }
        for i in 0..num_rules {
            let (
                section,
                rtype,
                line,
                flags,
                name,
                target,
                wordform,
                varname,
                varvalue,
                sub_reading,
                childset1,
                childset2,
                maplist,
                sublist,
                number,
                sub_rules,
                dep_target,
            ) = {
                let r = &self.grammar.rule_by_number[i];
                (
                    r.section,
                    r.r#type,
                    r.line,
                    r.flags,
                    r.name.clone(),
                    r.target,
                    r.wordform,
                    r.varname,
                    r.varvalue,
                    r.sub_reading,
                    r.childset1,
                    r.childset2,
                    r.maplist,
                    r.sublist,
                    r.number,
                    r.sub_rules.clone(),
                    r.dep_target,
                )
            };

            let mut buffer: Vec<u8> = Vec::new();
            let mut rfields = 0u32;

            if section != 0 {
                rfields |= 1 << 0;
                write_be(&mut buffer, section);
            }
            if rtype != Keywords::KIgnore {
                rfields |= 1 << 1;
                write_be(&mut buffer, rtype as u32);
            }
            if line != 0 {
                rfields |= 1 << 2;
                write_be(&mut buffer, line);
            }
            if !flags.is_empty() {
                rfields |= 1 << 3;
                if flags.bits() > u32::MAX as u64 {
                    rfields |= 1 << 16;
                    write_be(&mut buffer, flags.bits());
                } else {
                    write_be(&mut buffer, flags.bits() as u32);
                }
            }
            if !name.is_empty() {
                rfields |= 1 << 4;
                let b = name.as_bytes();
                write_be(&mut buffer, b.len() as i32);
                buffer.extend_from_slice(b);
            }
            if target.get() != 0 {
                rfields |= 1 << 5;
                write_be(&mut buffer, target.get());
            }
            if let Some(wf) = wordform {
                rfields |= 1 << 6;
                write_be(&mut buffer, self.grammar.single_tags_list[wf.0].number);
            }
            if varname != 0 {
                rfields |= 1 << 7;
                write_be(&mut buffer, varname);
            }
            if varvalue != 0 {
                rfields |= 1 << 8;
                write_be(&mut buffer, varvalue);
            }
            if sub_reading != 0 {
                rfields |= 1 << 9;
                let mut v = sub_reading.unsigned_abs();
                if sub_reading < 0 {
                    v |= 1 << 31;
                }
                write_be(&mut buffer, v);
            }
            if childset1.get() != 0 {
                rfields |= 1 << 10;
                write_be(&mut buffer, childset1.get());
            }
            if childset2.get() != 0 {
                rfields |= 1 << 11;
                write_be(&mut buffer, childset2.get());
            }
            if let Some(m) = maplist {
                rfields |= 1 << 12;
                write_be(&mut buffer, self.grammar.sets_list[m.0].number.get());
            }
            if let Some(sl) = sublist {
                rfields |= 1 << 13;
                write_be(&mut buffer, self.grammar.sets_list[sl.0].number.get());
            }
            if number != 0 {
                rfields |= 1 << 14;
                write_be(&mut buffer, number);
            }
            if !sub_rules.is_empty() {
                rfields |= 1 << 15;
            }

            write_be(output, rfields);
            let _ = output.write_all(&buffer);

            // dep_target hash (0 if none).
            let dep_hash = dep_target
                .map(|dt| self.grammar.contexts_arena[dt.0].hash)
                .unwrap_or(0);
            write_be(output, dep_hash);

            // SIDE EFFECT: reverse the rule's tests/dep_tests in place.
            self.grammar
                .rule_by_number
                .get_mut(i)
                .reverse_contextual_tests();

            let dep_tests: Vec<CtxId> = self.grammar.rule_by_number[i]
                .dep_tests
                .iter()
                .copied()
                .collect();
            write_be(output, dep_tests.len() as u32);
            for cid in &dep_tests {
                write_be(output, self.grammar.contexts_arena[cid.0].hash);
            }

            let tests: Vec<CtxId> = self.grammar.rule_by_number[i]
                .tests
                .iter()
                .copied()
                .collect();
            write_be(output, tests.len() as u32);
            for cid in &tests {
                write_be(output, self.grammar.contexts_arena[cid.0].hash);
            }

            if !sub_rules.is_empty() {
                write_be(output, sub_rules.len() as u32);
                for rid in &sub_rules {
                    write_be(output, self.grammar.rule_by_number[rid.0].number);
                }
            }
        }

        Ok(())
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.write-contextual-test-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.write-contextual-test-fn]
    // [spec:cg3:def:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
    // [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
    /// C++ `void writeContextualTest(ContextualTest* t, std::ostream& output)`.
    /// Dedups via `seen_uint32` (return early if `t->hash` already written; else
    /// insert). Recurses to write dependencies FIRST (`tmpl`, each `ors`, then
    /// `linked`) so referenced contexts precede the referrer. Then a `u32` field
    /// mask + buffer; bit0 hash is REQUIRED (hash 0 → fatal). The trailing `ors`
    /// count + hashes and `linked->hash` come AFTER the fixed buffer (bit12
    /// jump_pos is inside the buffer).
    ///
    /// The C++ recurses to write the dependencies first. A `LINK` chain or
    /// nest of alternatives from a `.cg3b` is as deep as the input makes it, so
    /// the tests whose dependencies are still being written are kept on a heap
    /// stack instead, and each is written once its dependencies are, in the
    /// order the recursion writes them.
    // [spec:cg3:req:robustness.depth-bounded]
    fn write_contextual_test<W: Write>(
        &mut self,
        t: CtxId,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error> {
        let mut open: Vec<(CtxId, Vec<CtxId>, usize)> = Vec::new();
        self.begin_contextual_test(&mut open, t);
        while let Some((test, deps, next)) = open.last_mut() {
            if let Some(&dep) = deps.get(*next) {
                *next += 1;
                self.begin_contextual_test(&mut open, dep);
                continue;
            }
            let test = *test;
            open.pop();
            self.write_contextual_test_record(test, output)?;
        }
        Ok(())
    }

    // [spec:cg3:def:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
    // [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
    /// Begin writing `t`, unless it is written already (`seen_uint32`): its
    /// dependencies go onto `open`, to be written before it — `tmpl`, each
    /// `ors`, then `linked`.
    fn begin_contextual_test(&mut self, open: &mut Vec<(CtxId, Vec<CtxId>, usize)>, t: CtxId) {
        let hash = self.grammar.contexts_arena[t.0].hash;
        if self.seen_uint32.contains(hash) {
            return;
        }
        self.seen_uint32.insert(hash);
        let ct = &self.grammar.contexts_arena[t.0];
        let deps: Vec<CtxId> = ct
            .tmpl
            .into_iter()
            .chain(ct.ors.iter().copied())
            .chain(ct.linked)
            .collect();
        open.push((t, deps, 0));
    }

    // [spec:cg3:def:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
    // [spec:cg3:sem:binary-grammar-write.cg3.binary-grammar.write-contextual-test-fn]
    /// The record of one test, once its dependencies are written: a `u32`
    /// field mask, the fixed buffer, then the `ors` hashes and `linked->hash`.
    fn write_contextual_test_record<W: Write>(
        &mut self,
        t: CtxId,
        output: &mut W,
    ) -> Result<(), crate::error::Cg3Error> {
        let (tmpl, ors, linked) = {
            let ct = &self.grammar.contexts_arena[t.0];
            (ct.tmpl, ct.ors.clone(), ct.linked)
        };

        // Snapshot this node's scalar fields.
        let (hash, pos, offset, target, line, relation, barrier, cbarrier, offset_sub, jump_pos) = {
            let ct = &self.grammar.contexts_arena[t.0];
            (
                ct.hash,
                ct.pos,
                ct.offset,
                ct.target,
                ct.line,
                ct.relation,
                ct.barrier,
                ct.cbarrier,
                ct.offset_sub,
                ct.jump_pos,
            )
        };

        let mut buffer: Vec<u8> = Vec::new();
        let mut fields = 0u32;

        if hash != 0 {
            fields |= 1 << 0;
            write_be(&mut buffer, hash);
        } else {
            return Err(crate::error::GrammarError::ContextHashZero { line }.into());
        }
        if !pos.is_empty() {
            fields |= 1 << 1;
            write_be(&mut buffer, ui32(pos.bits() & 0xFFFFFFFF));
            if pos.intersects(POS_64BIT) {
                write_be(&mut buffer, ui32((pos.bits() >> 32) & 0xFFFFFFFF));
            }
        }
        if offset != 0 {
            fields |= 1 << 2;
            write_be(&mut buffer, offset);
        }
        if let Some(tm) = tmpl {
            fields |= 1 << 3;
            write_be(&mut buffer, self.grammar.contexts_arena[tm.0].hash);
        }
        if target.get() != 0 {
            fields |= 1 << 4;
            write_be(&mut buffer, target.get());
        }
        if line != 0 {
            fields |= 1 << 5;
            write_be(&mut buffer, line);
        }
        if relation != 0 {
            fields |= 1 << 6;
            write_be(&mut buffer, relation);
        }
        if barrier.get() != 0 {
            fields |= 1 << 7;
            write_be(&mut buffer, barrier.get());
        }
        if cbarrier.get() != 0 {
            fields |= 1 << 8;
            write_be(&mut buffer, cbarrier.get());
        }
        if offset_sub != 0 {
            fields |= 1 << 9;
            write_be(&mut buffer, offset_sub);
        }
        if !ors.is_empty() {
            fields |= 1 << 10;
        }
        if linked.is_some() {
            fields |= 1 << 11;
        }
        if jump_pos != 0 {
            fields |= 1 << 12;
            write_be(&mut buffer, jump_pos);
        }

        write_be(output, fields);
        let _ = output.write_all(&buffer);

        if !ors.is_empty() {
            write_be(output, ors.len() as u32);
            for o in &ors {
                write_be(output, self.grammar.contexts_arena[o.0].hash);
            }
        }
        if let Some(l) = linked {
            write_be(output, self.grammar.contexts_arena[l.0].hash);
        }
        Ok(())
    }
}

impl IGrammarParser for BinaryGrammar {
    /// Reads `input` as a `.cg3b` blob; see
    /// [`parse_grammar_buffer`](BinaryGrammar::parse_grammar_buffer).
    fn parse_grammar(&mut self, input: &[u8]) -> Result<(), crate::error::Cg3Error> {
        self.parse_grammar_buffer(input)
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.set-compatible-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-compatible-fn]
    /// C++ `void setCompatible(bool)` — an empty body; the flag is discarded.
    fn set_compatible(&mut self, _compat: bool) {}

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.set-verbosity-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-verbosity-fn]
    /// C++ `void setVerbosity(uint32_t v)` — stores `verbosity = v`.
    fn set_verbosity(&mut self, level: u32) {
        self.verbosity = level;
    }

    fn get_grammar(&self) -> &GrammarCore {
        &self.grammar
    }
}
