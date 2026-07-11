//! Port of `src/BinaryGrammar.cpp` + `src/BinaryGrammar_read.cpp` +
//! `src/BinaryGrammar_write.cpp` + `src/BinaryGrammar.hpp` — the `.cg3b`
//! binary-grammar (de)serializer.
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). The on-disk format MUST stay
//! BYTE-COMPATIBLE with the CURRENT revision (`CG3_FEATURE_REV` = 13898); byte
//! parity is the contract.
//!
//! ## Wire layout (big-endian ints via [`crate::inlines::read_be`] /
//! [`crate::inlines::write_be`]; strings are a 4-byte length prefix + UTF-8 bytes
//! — NOT the 16-bit-prefixed `writeUTF8` form):
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
//!   from iterating `sets_by_contents` in `reindex` step (10) — a
//!   `std::collections::HashMap` in the port vs `std::unordered_map` in C++ —
//!   so the relative order of top-level used sets (and thus the exact number
//!   assignment) is neither run-stable nor libstdc++-bucket-identical.
//! * **`rehash`/`hash` on tags & contexts** already differ from the C++ for
//!   non-ASCII text / set `tmpl` (documented in `crate::tag` / `crate::contextual_test`
//!   — UTF-8 vs UTF-16 hashing, `CtxId` folded for the run-varying `tmpl`
//!   pointer). Because `.cg3b` stores those hashes verbatim, a grammar
//!   round-tripped THROUGH the port is self-consistent but its stored hashes
//!   differ from a C++-produced file for such content.
//! * **regex tags** store the PATTERN text only; the case-insensitive flag is
//!   re-derived on read from `T_CASE_INSENSITIVE` in `type` (compiled with
//!   `RegexBuilder::case_insensitive`, so `Regex::as_str` round-trips the bare
//!   pattern — matching `uregex_pattern`).
//! * **comparison_val** is a 12-byte double (`u64` BE mantissa + `i32` BE
//!   exponent), via [`crate::inlines::write_be_f64`] / [`crate::inlines::read_be_f64`].
//! * **context record count** is `grammar.contexts.size()` — every context
//!   reachable via `tmpl`/`ors`/`linked` MUST also be a distinct `contexts` map
//!   entry, else more records emit than the count and the stream desyncs on read.
//!
//! ## Legacy reader OUT OF SCOPE
//! `read_binary_grammar_10043` / `read_contextual_test_10043` (the `*_10043`
//! methods) are the legacy pre-10298 reader, intentionally EXCLUDED from the
//! port. They are kept as ERRORING STUBS (carrying their spec ids) that refuse
//! legacy input with "legacy .cg3b rev <10373 not supported".

use std::collections::HashMap;
use std::io::{Read, Write};

use regex::{Regex, RegexBuilder};

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::contextual_test::POS_64BIT;
use crate::flat_unordered_set::Uint32FlatHashSet;
use crate::grammar::{Grammar, trie_unserialize};
use crate::igrammar_parser::IGrammarParser;
use crate::inlines::{cg3_quit, is_cg3b, read_be, read_be_f64, ui16, ui32, write_be, write_be_f64};
use crate::rule::Rule;
use crate::set::Set;
use crate::strings::KEYWORDS;
use crate::tag::{C_OPS, T_CASE_INSENSITIVE, T_CONTEXT, T_LOCAL_VARIABLE, T_VARIABLE, Tag};
use crate::tag_trie::trie_serialize;
use crate::types::SetNumber;

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
pub type deferred_t = HashMap<CtxId, u32>;

// [spec:cg3:def:binary-grammar.cg3.binary-grammar.deferred-ors-t]
/// C++ `typedef std::unordered_map<ContextualTest*, std::vector<uint32_t>> deferred_ors_t`.
pub type deferred_ors_t = HashMap<CtxId, Vec<u32>>;

// [spec:cg3:def:binary-grammar.cg3.binary-grammar]
/// C++ `class BinaryGrammar : public IGrammarParser`.
///
/// C++ holds `Grammar* grammar` aliasing the externally-owned `result`. The port
/// OWNS its result `grammar` (per the brief: "the struct holds/builds a
/// `grammar: Grammar` (read) or references one (write)"). The inherited
/// `IGrammarParser` members (`nrules`, `nrules_inv`, `verbosity`) live here as
/// fields (a Rust trait has no fields). The C++ base `std::ostream* ux_stderr`
/// has no field analogue: diagnostics are tracing events (wave 4).
pub struct BinaryGrammar {
    /// C++ `Grammar* grammar` (aliases `result`); OWNED here.
    pub grammar: Grammar,
    /// C++ base `URegularExpression* nrules` — the `--nrules` name filter.
    /// Public: C++ main.cpp sets `parser->nrules` on the IGrammarParser base
    /// for BOTH the textual and binary parsers.
    pub nrules: Option<Regex>,
    /// C++ base `URegularExpression* nrules_inv` — the `--nrules-inv` filter.
    pub nrules_inv: Option<Regex>,
    /// C++ base `uint32_t verbosity`.
    verbosity: u32,
    deferred_tmpls: deferred_t,
    deferred_ors: deferred_ors_t,
    seen_uint32: Uint32FlatHashSet,
}

impl BinaryGrammar {
    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.binary-grammar-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.binary-grammar-fn]
    /// C++ `BinaryGrammar(Grammar& res, std::ostream& ux_err)`. Delegates to the
    /// base `IGrammarParser(res, ux_err)` (sets `ux_stderr = &ux_err`, `result =
    /// &res`; `nrules`/`nrules_inv` null; `verbosity` 0), then sets `grammar =
    /// result`. The port OWNS `res` (so `grammar` == `result` == the owned
    /// field); the `ux_err` diagnostic sink is tracing (wave 4). No allocation
    /// or I/O occurs.
    pub fn binary_grammar(res: Grammar) -> BinaryGrammar {
        BinaryGrammar {
            grammar: res,
            nrules: None,
            nrules_inv: None,
            verbosity: 0,
            deferred_tmpls: deferred_t::new(),
            deferred_ors: deferred_ors_t::new(),
            seen_uint32: Uint32FlatHashSet::new(),
        }
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.set-compatible-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-compatible-fn]
    /// C++ `void setCompatible(bool)` — an empty body; the flag is discarded.
    pub fn set_compatible(&mut self, _compat: bool) {}

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.set-verbosity-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.set-verbosity-fn]
    /// C++ `void setVerbosity(uint32_t v)` — stores `verbosity = v`.
    pub fn set_verbosity(&mut self, level: u32) {
        self.verbosity = level;
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.parse-grammar-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.parse-grammar-fn]
    /// C++ `int parse_grammar(const char* filename)` — the file-path entry point.
    /// `stat`s the file into `grammar->grammar_size`, then reads it and delegates
    /// to the istream overload. The C++ null-`grammar` guard is moot here (the
    /// grammar is owned). The C++ ifstream exception mask (throw on short read) is
    /// not modelled — `read_be` swallows short reads (see `crate::inlines`).
    pub fn parse_grammar_filename(&mut self, filename: &str) -> i32 {
        let meta = match std::fs::metadata(filename) {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(
                    "Error: Cannot stat {} due to error {} - bailing out!",
                    filename,
                    e
                );
                cg3_quit(1, None, 0);
            }
        };
        self.grammar.grammar_size = meta.len() as usize;

        let data = match std::fs::read(filename) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!(
                    "Error: Cannot stat {} due to error {} - bailing out!",
                    filename,
                    e
                );
                cg3_quit(1, None, 0);
            }
        };
        let mut cur = std::io::Cursor::new(data);
        self.parse_grammar_reader(&mut cur)
    }

    /// C++ `int parse_grammar(const char* buffer, size_t length)`: writes the
    /// bytes into a stringstream, seeks to 0, and calls the istream overload.
    /// The port wraps the slice in a `Cursor`.
    pub fn parse_grammar_buffer(&mut self, buffer: &[u8]) -> i32 {
        let mut cur = std::io::Cursor::new(buffer);
        self.parse_grammar_reader(&mut cur)
    }

    /// C++ `int parse_grammar(const std::string& buffer)` → `(buffer.data(),
    /// buffer.size())`.
    pub fn parse_grammar_string(&mut self, buffer: &str) -> i32 {
        self.parse_grammar_buffer(buffer.as_bytes())
    }

    /// C++ `int parse_grammar(const UChar*, size_t)` — unconditionally throws
    /// ("UChar* interface doesn't make sense for binary grammars.").
    pub fn parse_grammar_uchar(&mut self, _buffer: &[char], _length: usize) -> i32 {
        panic!("UChar* interface doesn't make sense for binary grammars.");
    }

    /// C++ private `int parse_grammar(UString&)` — unconditionally throws
    /// ("UString interface doesn't make sense for binary grammars.").
    pub fn parse_grammar_ustring(&mut self, _buffer: &mut String) -> i32 {
        panic!("UString interface doesn't make sense for binary grammars.");
    }

    // [spec:cg3:def:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn]
    // [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.parse-grammar-fn]
    /// C++ `int parse_grammar(std::istream& input)` (BinaryGrammar_read.cpp).
    /// Reads a whole `.cg3b` blob into `grammar`. See the module docs for the
    /// exhaustive wire layout.
    pub fn parse_grammar_reader<R: Read>(&mut self, input: &mut R) -> i32 {
        // Header: 4 magic bytes.
        let mut magic = [0u8; 4];
        if input.read_exact(&mut magic).is_err() {
            tracing::error!("Error: Error reading first 4 bytes from grammar!");
            cg3_quit(1, None, 0);
        }
        if !is_cg3b(magic) {
            tracing::error!(
                "Error: Grammar does not begin with magic bytes - cannot load as binary!"
            );
            cg3_quit(1, None, 0);
        }

        let bin_revision = read_be::<u32, _>(input);
        if bin_revision <= BIN_REV_ANCIENT {
            if self.verbosity >= 1 {
                tracing::warn!(
                    "Warning: Grammar revision is {}, but current format is {} or later. Please recompile the binary grammar with latest CG-3.",
                    bin_revision,
                    CG3_FEATURE_REV
                );
            }
            // input.seekg(0) — OMITTED: the 10043 path is an erroring stub.
            return self.read_binary_grammar_10043(input);
        }
        if bin_revision < CG3_TOO_OLD {
            tracing::error!(
                "Error: Grammar revision is {}, but this loader requires {} or later!",
                bin_revision,
                CG3_TOO_OLD
            );
            cg3_quit(1, None, 0);
        }
        if bin_revision > CG3_FEATURE_REV {
            tracing::error!(
                "Error: Grammar revision is {}, but this loader only knows up to revision {}!",
                bin_revision,
                CG3_FEATURE_REV
            );
            cg3_quit(1, None, 0);
        }

        self.grammar.is_binary = true;

        let fields = read_be::<u32, _>(input);

        self.grammar.has_dep = (fields & BINF_DEP) != 0;
        self.grammar.sub_readings_ltr = (fields & BINF_SUB_LTR) != 0;
        self.grammar.has_relations = (fields & BINF_RELATIONS) != 0;
        self.grammar.has_bag_of_tags = (fields & BINF_BAG) != 0;
        self.grammar.ordered = (fields & BINF_ORDERED) != 0;
        self.grammar.addcohort_attach = (fields & BINF_ADDCOHORT_ATTACH) != 0;

        if fields & BINF_PREFIX != 0 {
            let len = read_be::<u32, _>(input);
            let mut buf = vec![0u8; len as usize];
            let _ = input.read_exact(&mut buf);
            // Decode into a single UChar (mapping_prefix has capacity 1).
            self.grammar.mapping_prefix =
                String::from_utf8_lossy(&buf).chars().next().unwrap_or('\0');
        }

        if bin_revision >= BIN_REV_CMDARGS {
            let len = read_be::<u32, _>(input);
            if len != 0 {
                let mut buf = vec![0u8; len as usize];
                let _ = input.read_exact(&mut buf);
                self.grammar.cmdargs = String::from_utf8_lossy(&buf).into_owned();
            }
            let len = read_be::<u32, _>(input);
            if len != 0 {
                let mut buf = vec![0u8; len as usize];
                let _ = input.read_exact(&mut buf);
                self.grammar.cmdargs_override = String::from_utf8_lossy(&buf).into_owned();
            }
        }

        // Deferred varstring-tag → set-number map (sets load AFTER tags).
        // C++ `std::map<uint32_t, uint32Vector> tag_varsets` keyed by tag number.
        let mut tag_varsets: HashMap<u32, Vec<u32>> = HashMap::new();

        // --- Tags ---
        let num_single_tags = if fields & BINF_TAGS != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        self.grammar.num_tags = num_single_tags as usize;
        // single_tags_list.resize(num): pre-allocate `num` slots so a tag can be
        // placed at its `number` (== arena slot).
        for _ in 0..num_single_tags {
            self.grammar.single_tags_list.alloc(Tag::default());
        }
        for _ in 0..num_single_tags {
            let mut t = Tag::default(); // allocateTag()
            let tfields = read_be::<u32, _>(input);

            if tfields & (1 << 0) != 0 {
                t.number = read_be(input);
            }
            if tfields & (1 << 1) != 0 {
                t.hash = read_be(input);
            }
            if tfields & (1 << 2) != 0 {
                t.plain_hash = read_be(input);
            }
            if tfields & (1 << 3) != 0 {
                t.seed = read_be(input);
            }
            if tfields & (1 << 4) != 0 {
                t.r#type = crate::tag::TagType::from_bits_retain(read_be(input));
            }
            if tfields & (1 << 5) != 0 {
                t.comparison_hash = read_be(input);
            }
            if tfields & (1 << 6) != 0 {
                t.comparison_op = c_ops_from_u32(read_be::<u32, _>(input));
            }
            if tfields & (1 << 7) != 0 {
                // Legacy integer comparison_val, never emitted by the current
                // writer. Clamp at the int32 extremes to NUMERIC_MIN/MAX.
                let v = read_be::<i32, _>(input);
                t.comparison_val = v as f64;
                if v <= i32::MIN {
                    t.comparison_val = crate::inlines::NUMERIC_MIN;
                }
                if v >= i32::MAX {
                    t.comparison_val = crate::inlines::NUMERIC_MAX;
                }
            }
            if tfields & (1 << 12) != 0 {
                // 12-byte double: u64 BE mantissa + i32 BE exponent.
                t.comparison_val = read_be_f64(input);
            }
            if tfields & (1 << 8) != 0 {
                let len = read_be::<u32, _>(input);
                if len != 0 {
                    let mut buf = vec![0u8; len as usize];
                    let _ = input.read_exact(&mut buf);
                    t.tag = String::from_utf8_lossy(&buf).into_owned();
                }
            }
            if tfields & (1 << 9) != 0 {
                let len = read_be::<u32, _>(input);
                if len != 0 {
                    let mut buf = vec![0u8; len as usize];
                    let _ = input.read_exact(&mut buf);
                    let pattern = String::from_utf8_lossy(&buf).into_owned();
                    // Flags re-derived from type (NOT stored): case-insensitive iff
                    // T_CASE_INSENSITIVE. RegexBuilder keeps `as_str()` == the bare
                    // pattern (matching uregex_pattern round-trip).
                    let built = RegexBuilder::new(&pattern)
                        .case_insensitive(t.r#type.intersects(T_CASE_INSENSITIVE))
                        .build();
                    match built {
                        Ok(re) => t.regexp = Some(re),
                        Err(e) => {
                            tracing::error!(
                                "Error: uregex_open returned {} trying to parse tag {} - cannot continue!",
                                e,
                                t.tag
                            );
                            cg3_quit(1, None, 0);
                        }
                    }
                }
            }
            if tfields & (1 << 10) != 0 {
                let num = read_be::<u32, _>(input);
                t.allocate_vs_sets();
                let entry = tag_varsets.entry(t.number).or_default();
                for _ in 0..num {
                    entry.push(read_be(input));
                }
            }
            if tfields & (1 << 11) != 0 {
                let num = read_be::<u32, _>(input);
                t.allocate_vs_names();
                for _ in 0..num {
                    let len = read_be::<u32, _>(input);
                    if len != 0 {
                        let mut buf = vec![0u8; len as usize];
                        let _ = input.read_exact(&mut buf);
                        t.vs_names
                            .as_mut()
                            .unwrap()
                            .push(String::from_utf8_lossy(&buf).into_owned());
                    }
                }
            }
            // 1 << 12 used above.
            if tfields & (1 << 13) != 0 {
                // variable_hash (the C++ union member).
                let v = read_be(input);
                t.set_variable_hash(v);
            }
            if tfields & (1 << 14) != 0 {
                // context_ref_pos (the C++ union member).
                let v = read_be(input);
                t.set_context_ref_pos(v);
            }

            let hash = t.hash;
            let number = t.number;
            let is_star = t.tag == "*";
            // single_tags[t->hash] = t (id == arena slot `number`).
            self.grammar.single_tags.insert((hash, TagId(number)));
            if is_star {
                self.grammar.tag_any = hash;
            }
            // single_tags_list[t->number] = t.
            self.grammar.single_tags_list[number] = t;
        }

        // --- reopen_mappings ---
        let num_remaps = if fields & BINF_REOPEN_MAP != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        for _ in 0..num_remaps {
            let v = read_be::<u32, _>(input);
            self.grammar.reopen_mappings.insert(v);
        }

        // --- preferred_targets ---
        let num_pref = if fields & BINF_PREF_TARGETS != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        for _ in 0..num_pref {
            let v = read_be::<u32, _>(input);
            self.grammar.preferred_targets.push(v);
        }

        // --- parentheses ---
        let num_par = if fields & BINF_ENCLS != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        for _ in 0..num_par {
            let left = read_be::<u32, _>(input);
            let right = read_be::<u32, _>(input);
            self.grammar.parentheses.insert(left, right);
            self.grammar.parentheses_reverse.insert(right, left);
        }

        // --- anchors ---
        let num_anchors = if fields & BINF_ANCHORS != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        for _ in 0..num_anchors {
            let left = read_be::<u32, _>(input);
            let right = read_be::<u32, _>(input);
            self.grammar.anchors.insert((left, right));
        }

        // --- Sets ---
        let num_sets = if fields & BINF_SETS != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        // sets_list.resize(num_sets): pre-allocate `num_sets` slots (each
        // registered in sets_all, like the loop's allocateSet()).
        for _ in 0..num_sets {
            self.grammar.allocate_set();
        }
        for _ in 0..num_sets {
            let mut s = Set::default(); // allocateSet()
            let sfields = read_be::<u32, _>(input);

            if sfields & (1 << 0) != 0 {
                s.number = SetNumber(read_be(input));
            }
            if sfields & (1 << 1) != 0 {
                s.r#type = crate::set::SetType::from_bits_retain(ui16(read_be::<u32, _>(input)));
            }
            if sfields & (1 << 2) != 0 {
                s.r#type = crate::set::SetType::from_bits_retain(read_be::<u8, _>(input) as u16);
            }
            if sfields & (1 << 3) != 0 {
                let n1 = read_be::<u32, _>(input);
                if n1 != 0 {
                    trie_unserialize(&mut s.trie, input, &self.grammar, n1);
                }
                let n2 = read_be::<u32, _>(input);
                if n2 != 0 {
                    trie_unserialize(&mut s.trie_special, input, &self.grammar, n2);
                }
            }
            if sfields & (1 << 4) != 0 {
                let n = read_be::<u32, _>(input);
                for _ in 0..n {
                    s.set_ops.push(read_be(input));
                }
            }
            if sfields & (1 << 5) != 0 {
                let n = read_be::<u32, _>(input);
                for _ in 0..n {
                    s.sets.push(read_be(input));
                }
            }
            if sfields & (1 << 6) != 0 {
                let len = read_be::<u32, _>(input);
                if len != 0 {
                    let mut buf = vec![0u8; len as usize];
                    let _ = input.read_exact(&mut buf);
                    // C++ s->setName(UChar*) (assign directly); the port's Set has
                    // only the u32 setName overload, so inline the assignment.
                    s.name = String::from_utf8_lossy(&buf).into_owned();
                }
            }
            let number = s.number.get();
            self.grammar.sets_list[number] = s; // sets_list[s->number] = s
        }
        // The dense sets_list vector: the reader stores each set at its own
        // (dense) number, so slot == number and the order is the identity.
        self.grammar.sets_list_order = (0..num_sets).map(SetId).collect();

        // Resolve deferred varstring-tag sets now that sets are loaded.
        for (tagnum, setnums) in tag_varsets {
            for num in setnums {
                self.grammar.single_tags_list[tagnum]
                    .vs_sets
                    .as_mut()
                    .unwrap()
                    .push(SetId(num));
            }
        }

        if fields & BINF_DELIMS != 0 {
            let n = read_be::<u32, _>(input);
            self.grammar.delimiters = Some(SetId(n));
        }
        if fields & BINF_SOFT_DELIMS != 0 {
            let n = read_be::<u32, _>(input);
            self.grammar.soft_delimiters = Some(SetId(n));
        }
        if fields & BINF_TEXT_DELIMS != 0 {
            let n = read_be::<u32, _>(input);
            self.grammar.text_delimiters = Some(SetId(n));
        }

        // --- Contexts ---
        let num_contexts = if fields & BINF_CONTEXTS != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        for _ in 0..num_contexts {
            let t = self.read_contextual_test(input);
            let hash = self.grammar.contexts_arena[t.0].hash;
            self.grammar.contexts.insert(hash, t);
        }

        // --- Rules ---
        let num_rules = if fields & BINF_RULES != 0 {
            read_be::<u32, _>(input)
        } else {
            0
        };
        // rule_by_number.resize(num_rules): pre-allocate `num_rules` slots.
        for _ in 0..num_rules {
            self.grammar.rule_by_number.alloc(Rule::default());
        }
        for _ in 0..num_rules {
            let mut r = Rule::default(); // allocateRule()
            let rfields = read_be::<u32, _>(input);

            if rfields & (1 << 0) != 0 {
                r.section = read_be(input);
            }
            if rfields & (1 << 1) != 0 {
                r.r#type = keywords_from_u32(read_be::<u32, _>(input));
            }
            if rfields & (1 << 2) != 0 {
                r.line = read_be(input);
            }
            if rfields & (1 << 3) != 0 {
                if rfields & (1 << 16) != 0 {
                    r.flags = crate::rule::RuleFlags::from_bits_retain(read_be::<u64, _>(input));
                } else {
                    r.flags =
                        crate::rule::RuleFlags::from_bits_retain(read_be::<u32, _>(input) as u64);
                }
            }
            if rfields & (1 << 4) != 0 {
                let len = read_be::<u32, _>(input);
                if len != 0 {
                    let mut buf = vec![0u8; len as usize];
                    let _ = input.read_exact(&mut buf);
                    let nm = String::from_utf8_lossy(&buf).into_owned();
                    r.set_name(Some(nm.as_str()));
                }
            }
            if rfields & (1 << 5) != 0 {
                r.target = SetNumber(read_be(input));
            }
            if rfields & (1 << 6) != 0 {
                let n = read_be::<u32, _>(input);
                r.wordform = Some(TagId(n)); // single_tags_list[u32]
            }
            if rfields & (1 << 7) != 0 {
                r.varname = read_be(input);
            }
            if rfields & (1 << 8) != 0 {
                r.varvalue = read_be(input);
            }
            if rfields & (1 << 9) != 0 {
                let mut u = read_be::<u32, _>(input);
                let mut v = u as i32;
                if u & (1 << 31) != 0 {
                    u &= !(1u32 << 31);
                    v = -(u as i32);
                }
                r.sub_reading = v;
            }
            if rfields & (1 << 10) != 0 {
                r.childset1 = SetNumber(read_be(input));
            }
            if rfields & (1 << 11) != 0 {
                r.childset2 = SetNumber(read_be(input));
            }
            if rfields & (1 << 12) != 0 {
                let n = read_be::<u32, _>(input);
                r.maplist = Some(SetId(n)); // sets_list[u32]
            }
            if rfields & (1 << 13) != 0 {
                let n = read_be::<u32, _>(input);
                r.sublist = Some(SetId(n));
            }
            if rfields & (1 << 14) != 0 {
                r.number = read_be(input);
            }

            // dep_target: contexts[hash] (inline; only when nonzero).
            let dep = read_be::<u32, _>(input);
            if dep != 0 {
                r.dep_target = self.grammar.contexts.get(&dep).copied();
            }

            let num_dep_tests = read_be::<u32, _>(input);
            for _ in 0..num_dep_tests {
                let h = read_be::<u32, _>(input);
                let ctx = self.grammar.contexts[&h]; // operator[]: missing → panic
                Rule::add_contextual_test(ctx, &mut r.dep_tests);
            }

            let num_tests = read_be::<u32, _>(input);
            for _ in 0..num_tests {
                let h = read_be::<u32, _>(input);
                let ctx = self.grammar.contexts[&h];
                Rule::add_contextual_test(ctx, &mut r.tests);
            }

            if rfields & (1 << 15) != 0 {
                let n = read_be::<u32, _>(input);
                for _ in 0..n {
                    let num = read_be::<u32, _>(input);
                    r.sub_rules.push(RuleId(num)); // rule_by_number[u32]
                }
            }

            // --nrules / --nrules-inv name filters (K_IGNORE the rule).
            if let Some(re) = &self.nrules {
                if !re.is_match(&r.name) {
                    r.r#type = KEYWORDS::K_IGNORE;
                }
            }
            if let Some(re) = &self.nrules_inv {
                if re.is_match(&r.name) {
                    r.r#type = KEYWORDS::K_IGNORE;
                }
            }

            let number = r.number;
            self.grammar.rule_by_number[number] = r; // rule_by_number[r->number] = r
        }

        // Bind deferred template refs.
        let tmpls: Vec<(CtxId, u32)> = self.deferred_tmpls.iter().map(|(&k, &v)| (k, v)).collect();
        for (t, hash) in tmpls {
            let ctx = self.grammar.contexts[&hash]; // find(hash)->second (no end-check)
            self.grammar.contexts_arena[t.0].tmpl = Some(ctx);
        }

        // Bind deferred OR'ed contexts.
        let ors_list: Vec<(CtxId, Vec<u32>)> = self
            .deferred_ors
            .iter()
            .map(|(&k, v)| (k, v.clone()))
            .collect();
        for (t, hashes) in ors_list {
            let mut resolved = Vec::with_capacity(hashes.len());
            for h in hashes {
                resolved.push(self.grammar.contexts[&h]);
            }
            self.grammar.contexts_arena[t.0].ors.extend(resolved);
        }

        0
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-contextual-test-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-fn]
    // [spec:cg3:def:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn]
    // [spec:cg3:sem:binary-grammar-read.cg3.binary-grammar.read-contextual-test-fn]
    /// C++ `ContextualTest* readContextualTest(std::istream& input)`. Reads one
    /// test record (a fresh `allocateContextualTest`) in the exact source field
    /// order: bit12 (jump_pos) is read BEFORE bit10 (ors) / bit11 (linked).
    /// `tmpl`/`ors` refs are DEFERRED; `linked` resolves inline via
    /// `contexts[hash]` (present because the writer emits linked children first).
    fn read_contextual_test<R: Read>(&mut self, input: &mut R) -> CtxId {
        let t = self.grammar.allocate_contextual_test();
        let fields = read_be::<u32, _>(input);

        if fields & (1 << 0) != 0 {
            self.grammar.contexts_arena[t.0].hash = read_be(input);
        }
        if fields & (1 << 1) != 0 {
            let mut pos = read_be::<u32, _>(input) as u64;
            if pos & POS_64BIT.bits() != 0 {
                let hi = read_be::<u32, _>(input);
                pos |= (hi as u64) << 32;
            }
            self.grammar.contexts_arena[t.0].pos =
                crate::contextual_test::PosFlags::from_bits_retain(pos);
        }
        if fields & (1 << 2) != 0 {
            self.grammar.contexts_arena[t.0].offset = read_be(input);
        }
        if fields & (1 << 3) != 0 {
            let h = read_be::<u32, _>(input);
            self.deferred_tmpls.insert(t, h);
        }
        if fields & (1 << 4) != 0 {
            self.grammar.contexts_arena[t.0].target = SetNumber(read_be(input));
        }
        if fields & (1 << 5) != 0 {
            self.grammar.contexts_arena[t.0].line = read_be(input);
        }
        if fields & (1 << 6) != 0 {
            self.grammar.contexts_arena[t.0].relation = read_be(input);
        }
        if fields & (1 << 7) != 0 {
            self.grammar.contexts_arena[t.0].barrier = SetNumber(read_be(input));
        }
        if fields & (1 << 8) != 0 {
            self.grammar.contexts_arena[t.0].cbarrier = SetNumber(read_be(input));
        }
        if fields & (1 << 9) != 0 {
            self.grammar.contexts_arena[t.0].offset_sub = read_be(input);
        }
        if fields & (1 << 12) != 0 {
            self.grammar.contexts_arena[t.0].jump_pos = read_be::<i8, _>(input);
        }
        if fields & (1 << 10) != 0 {
            let num_ors = read_be::<u32, _>(input);
            let entry = self.deferred_ors.entry(t).or_default();
            for _ in 0..num_ors {
                entry.push(read_be(input));
            }
        }
        if fields & (1 << 11) != 0 {
            let h = read_be::<u32, _>(input);
            // grammar->contexts[u32]: operator[] (missing → null; here None).
            self.grammar.contexts_arena[t.0].linked = self.grammar.contexts.get(&h).copied();
        }

        t
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-binary-grammar-10043-fn]
    /// OUT OF SCOPE (the legacy pre-10298 `.cg3b` reader). ERRORING STUB: refuses
    /// legacy input and returns error code 1 instead of parsing. The real reader
    /// lived in BinaryGrammar_read_10043.cpp and is intentionally excluded.
    fn read_binary_grammar_10043<R: Read>(&mut self, _input: &mut R) -> i32 {
        tracing::error!(
            "Error: legacy .cg3b rev <10373 not supported (readBinaryGrammar_10043 not ported)."
        );
        1
    }

    // [spec:cg3:def:binary-grammar.cg3.binary-grammar.read-contextual-test-10043-fn]
    // [spec:cg3:sem:binary-grammar.cg3.binary-grammar.read-contextual-test-10043-fn]
    /// OUT OF SCOPE (legacy `_10043` contextual-test reader). ERRORING STUB:
    /// never invoked (its only caller, `read_binary_grammar_10043`, errors out
    /// first); refuses legacy input and returns error code 1.
    fn read_contextual_test_10043<R: Read>(&mut self, _input: &mut R) -> i32 {
        tracing::error!(
            "Error: legacy .cg3b rev <10373 not supported (readContextualTest_10043 not ported)."
        );
        1
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
    pub fn write_binary_grammar<W: Write>(&mut self, output: &mut W) -> i32 {
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
            if hash != 0 {
                tfields |= 1 << 1;
                write_be(&mut buffer, hash);
            }
            if plain_hash != 0 {
                tfields |= 1 << 2;
                write_be(&mut buffer, plain_hash);
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
            if comparison_op != C_OPS::OP_NOP {
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
                for sid in vs {
                    let n = self.grammar.sets_list[sid.0].number.get();
                    write_be(&mut buffer, n);
                }
            }
            if let Some(vn) = &vs_names {
                tfields |= 1 << 11;
                write_be(&mut buffer, vn.len() as u32);
                for name in vn {
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
            self.write_contextual_test(cid, output);
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
            if rtype != KEYWORDS::K_IGNORE {
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

        0
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
    fn write_contextual_test<W: Write>(&mut self, t: CtxId, output: &mut W) {
        let hash = self.grammar.contexts_arena[t.0].hash;
        if self.seen_uint32.contains(hash) {
            return;
        }
        self.seen_uint32.insert(hash);

        // Snapshot child ids so the recursive `&mut self` calls can re-borrow.
        let (tmpl, ors, linked) = {
            let ct = &self.grammar.contexts_arena[t.0];
            (ct.tmpl, ct.ors.clone(), ct.linked)
        };
        if let Some(tm) = tmpl {
            self.write_contextual_test(tm, output);
        }
        for o in &ors {
            self.write_contextual_test(*o, output);
        }
        if let Some(l) = linked {
            self.write_contextual_test(l, output);
        }

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
            tracing::error!("Error: Context on line {} had hash 0!", line);
            cg3_quit(1, None, 0);
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
    }
}

// C++ `static_cast<C_OPS>(uint32_t)` — map a serialized operator id back to the
// enum. Out-of-range ids (never emitted by a valid writer) fall to OP_NOP.
fn c_ops_from_u32(v: u32) -> C_OPS {
    match v {
        0 => C_OPS::OP_NOP,
        1 => C_OPS::OP_EQUALS,
        2 => C_OPS::OP_LESSTHAN,
        3 => C_OPS::OP_GREATERTHAN,
        4 => C_OPS::OP_LESSEQUALS,
        5 => C_OPS::OP_GREATEREQUALS,
        6 => C_OPS::OP_NOTEQUALS,
        7 => C_OPS::NUM_OPS,
        _ => C_OPS::OP_NOP,
    }
}

// C++ `static_cast<KEYWORDS>(uint32_t)`. `KEYWORDS` is `#[repr(u32)]` with
// contiguous discriminants `0..=KEYWORD_COUNT`; the transmute is sound for that
// range (out-of-range ids — never emitted by a valid writer — fall to K_IGNORE).
fn keywords_from_u32(v: u32) -> KEYWORDS {
    // Safe table lookup (wave 4; was a transmute). Out-of-range ids — never
    // emitted by a valid writer — fall to K_IGNORE, as before. NOTE: the old
    // bound was `v <= KEYWORD_COUNT` inclusive; `v == KEYWORD_COUNT` would have
    // transmuted to the sentinel variant itself, which the table cannot
    // produce — it now falls to K_IGNORE (unreachable from any valid stream).
    crate::strings::KEYWORDS_BY_ID
        .get(v as usize)
        .copied()
        .unwrap_or(KEYWORDS::K_IGNORE)
}

impl IGrammarParser for BinaryGrammar {
    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    /// C++ `int parse_grammar(const char* buffer, size_t length)` override.
    /// RECONCILIATION: the C++ writes into the member `grammar` (bound at
    /// construction), so the trait's per-call `grammar` param is unused here — the
    /// port owns its result (see the `binary_grammar` ctor and the module's
    /// `IGrammarParser` note). Delegates to `parse_grammar_buffer`.
    fn parse_grammar(&mut self, _grammar: &mut Grammar, input: &[u8]) -> i32 {
        self.parse_grammar_buffer(input)
    }

    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-compatible-fn]
    fn set_compatible(&mut self, compat: bool) {
        BinaryGrammar::set_compatible(self, compat);
    }

    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.set-verbosity-fn]
    fn set_verbosity(&mut self, level: u32) {
        BinaryGrammar::set_verbosity(self, level);
    }

    fn get_grammar(&self) -> &Grammar {
        &self.grammar
    }
}
