//! Port of `src/GrammarWriter.cpp` / `src/GrammarWriter.hpp` — serializes a
//! loaded [`Grammar`] back into CG-3 TEXT source form (the `--dump-ast` / grammar
//! round-trip path).
//!
//! Literal, bug-for-bug 1:1 translation (Wave 2). Each method carries its
//! `[spec:cg3:def]` + `[spec:cg3:sem]` ids verbatim from
//! `docs/spec/port/src/GrammarWriter.md`.
//!
//! ## Representation decisions (parity notes)
//!
//! * **Output → `std::io::Write`.** The C++ `u_fprintf(std::ostream&, ...)` calls
//!   become `write!(output, ...)` over a generic `W: Write`. Our `UString` is
//!   already UTF-8, so `%S`/`%C`/`%s`/`%d`/`%u` format specifiers map to Rust's
//!   `{}` and the bytes written are the same UTF-8 the ICU `u_fprintf` produced.
//!   Write errors are swallowed (the C++ ignores the stream failbit).
//!
//! * **Arena-model signature reconciliation.** The C++ class stores a
//!   `const Grammar* grammar` member and mutates the pointed-to `Set` objects
//!   through the non-const `Set*` held in `sets_list` (the classic const-pointer /
//!   mutable-pointee trick). In the arena port a `Set` lives *inside*
//!   `grammar.sets_list` (an `Arena<Set>` by value), so mutating a set's name in
//!   `write_grammar`'s naming pass genuinely needs `&mut Grammar`, which cannot
//!   coexist with an immutable `grammar` field. Following the established
//!   convention used throughout this crate (`Set::rehash(grammar, id)`,
//!   `ContextualTest::rehash(contexts, id)`, the `trie_*(…, grammar)` helpers),
//!   `grammar` is therefore THREADED as a parameter rather than stored: the print
//!   methods take `grammar: &Grammar`, `write_grammar` takes `grammar: &mut
//!   Grammar` (naming pass, then reborrowed immutably for the print phase). The
//!   struct retains only the writer's own state (`used_sets`, `seen_rules`,
//!   `anchors`, `ux_stderr`). The ctor still consumes `res` (to build `anchors`)
//!   but does not retain it.
//!
//! * **Name tables local stand-ins.** `keywords[]`, `g_flags[]`, `stringbits[]`,
//!   the `FL_*` flag indices and the `STR_*` set-name constants live in
//!   `src/Strings.hpp`, which the `crate::strings` port covers only for the
//!   `KEYWORDS` enum. They are reproduced verbatim here as private constants (same
//!   precedent as the local `STR_*` stand-ins in `crate::grammar`). To reconcile:
//!   move to `crate::strings` when that module grows.
//!
//! * **`std::multimap` anchors.** The C++ `std::multimap<uint32_t, uint32_t>
//!   anchors` (built by INVERTING `grammar.anchors`) becomes a
//!   `BTreeMap<u32, Vec<u32>>` (values pushed in inversion order, so
//!   `equal_range` iteration order is preserved).

use std::collections::BTreeMap;
use std::io::Write;

use crate::arena::{CtxId, RuleId, SetId};
use crate::contextual_test::{
    ContextualTest, GSR_SPECIALS, POS_ABSOLUTE, POS_ACTIVE, POS_ALL, POS_ATTACH_TO,
    POS_BAG_OF_TAGS, POS_CAREFUL, POS_DEP_CHILD, POS_DEP_DEEP, POS_DEP_GLOB, POS_DEP_PARENT,
    POS_DEP_SIBLING, POS_INACTIVE, POS_JUMP, POS_JUMP_POS, POS_LEFT, POS_LEFT_PAR, POS_LEFTMOST,
    POS_LOOK_DELAYED, POS_LOOK_DELETED, POS_LOOK_IGNORED, POS_MARK_SET, POS_NEGATE, POS_NO_BARRIER,
    POS_NO_PASS_ORIGIN, POS_NONE, POS_NOT, POS_PASS_ORIGIN, POS_RELATION, POS_RIGHT, POS_RIGHT_PAR,
    POS_RIGHTMOST, POS_SCANALL, POS_SCANFIRST, POS_SELF, POS_SPAN_BOTH, POS_SPAN_LEFT,
    POS_SPAN_RIGHT, POS_TMPL_OVERRIDE, POS_UNKNOWN, POS_WITH,
};
use crate::flat_unordered_set::Uint32FlatHashSet;
use crate::grammar::Grammar;
use crate::inlines::{is_internal, si32};
use crate::rule::{FLAGS_COUNT, RF_AFTER, RF_BEFORE, RF_WITHCHILD, Rule};
use crate::set::ST_ORDERED;
use crate::strings::KEYWORDS;
use crate::tag::Tag;
use crate::tag_trie::trie_get_tags_ordered;

// ---------------------------------------------------------------------------
// Local `Strings.hpp` stand-ins (see module note).
// ---------------------------------------------------------------------------

const STR_DELIMITSET: &str = "_S_DELIMITERS_";
const STR_SOFTDELIMITSET: &str = "_S_SOFT_DELIMITERS_";
const STR_TEXTDELIMITSET: &str = "_S_TEXT_DELIMITERS_";

// `Strings.hpp` `enum { FL_* }` — only the indices `printRule` special-cases.
const FL_WITHCHILD: usize = 17;
const FL_SUB: usize = 23;
const FL_BEFORE: usize = 27;
const FL_AFTER: usize = 28;

// `Strings.hpp` `constexpr UStringView keywords[KEYWORD_COUNT]` (72 entries),
// indexed by the `KEYWORDS` enum value.
const KEYWORDS_NAMES: [&str; 72] = [
    "__CG3_DUMMY_KEYWORD__",
    "SETS",
    "LIST",
    "SET",
    "DELIMITERS",
    "SOFT-DELIMITERS",
    "PREFERRED-TARGETS",
    "MAPPING-PREFIX",
    "MAPPINGS",
    "CONSTRAINTS",
    "CORRECTIONS",
    "SECTION",
    "BEFORE-SECTIONS",
    "AFTER-SECTIONS",
    "NULL-SECTION",
    "ADD",
    "MAP",
    "REPLACE",
    "SELECT",
    "REMOVE",
    "IFF",
    "APPEND",
    "SUBSTITUTE",
    "START",
    "END",
    "ANCHOR",
    "EXECUTE",
    "JUMP",
    "REMVARIABLE",
    "SETVARIABLE",
    "DELIMIT",
    "MATCH",
    "SETPARENT",
    "SETCHILD",
    "ADDRELATION",
    "SETRELATION",
    "REMRELATION",
    "ADDRELATIONS",
    "SETRELATIONS",
    "REMRELATIONS",
    "TEMPLATE",
    "MOVE",
    "MOVE-AFTER",
    "MOVE-BEFORE",
    "SWITCH",
    "REMCOHORT",
    "STATIC-SETS",
    "UNMAP",
    "COPY",
    "ADDCOHORT",
    "ADDCOHORT-AFTER",
    "ADDCOHORT-BEFORE",
    "EXTERNAL",
    "EXTERNAL-ONCE",
    "EXTERNAL-ALWAYS",
    "OPTIONS",
    "STRICT-TAGS",
    "REOPEN-MAPPINGS",
    "SUBREADINGS",
    "SPLITCOHORT",
    "PROTECT",
    "UNPROTECT",
    "MERGECOHORTS",
    "RESTORE",
    "WITH",
    "OLIST",
    "OSET",
    "CMDARGS",
    "CMDARGS-OVERRIDE",
    "COPYCOHORT",
    "REMPARENT",
    "SWITCHPARENT",
];

// `Strings.hpp` `constexpr UStringView g_flags[FLAGS_COUNT]` (34 entries).
const G_FLAGS: [&str; 34] = [
    "NEAREST",
    "ALLOWLOOP",
    "DELAYED",
    "IMMEDIATE",
    "LOOKDELETED",
    "LOOKDELAYED",
    "UNSAFE",
    "SAFE",
    "REMEMBERX",
    "RESETX",
    "KEEPORDER",
    "VARYORDER",
    "ENCL_INNER",
    "ENCL_OUTER",
    "ENCL_FINAL",
    "ENCL_ANY",
    "ALLOWCROSS",
    "WITHCHILD",
    "NOCHILD",
    "ITERATE",
    "NOITERATE",
    "UNMAPLAST",
    "REVERSE",
    "SUB",
    "OUTPUT",
    "CAPTURE_UNIF",
    "REPEAT",
    "BEFORE",
    "AFTER",
    "IGNORED",
    "LOOKIGNORED",
    "NOMAPPED",
    "NOPARENT",
    "DETACH",
];

// `Strings.hpp` `constexpr UStringView stringbits[]` (9 entries), indexed by the
// set-operator code held in `Set::set_ops`.
const STRINGBITS: [&str; 9] = ["", "", "", "OR", "+", "-", "", "", "^"];

/// C++ `const UChar* data()[i]` / `UString::operator[](i)` NUL-terminator
/// semantics: index at/after the length reads the `'\0'` terminator (as a byte)
/// rather than panicking, reproducing the flagged unguarded `name[i]` reads.
#[inline]
fn byte_at(s: &str, i: usize) -> u8 {
    s.as_bytes().get(i).copied().unwrap_or(0)
}

/// The C++ `sets_list` VECTOR (the numbered used-set list) in dense number order
/// (`Grammar::sets_list_order`; see the reconciliation note in `crate::grammar`).
/// Mirrors `Grammar::used_set_ids` (which is private there).
fn used_set_ids(grammar: &Grammar) -> Vec<SetId> {
    grammar.sets_list_order.clone()
}

/// Recovers the C++ `rule_by_number` VECTOR order from the arena (rules are never
/// freed, so every slot is live; `RuleId.0 == number == slot`).
fn rule_ids(grammar: &Grammar) -> Vec<RuleId> {
    (0..grammar.rule_by_number.capacity())
        .filter(|&i| grammar.rule_by_number.try_get(i).is_some())
        .map(RuleId)
        .collect()
}

/// `write!` to the sink, swallowing the error (the C++ `u_fprintf` ignores the
/// stream failbit).
macro_rules! w {
    ($o:expr, $($arg:tt)*) => {{ let _ = write!($o, $($arg)*); }};
}

// [spec:cg3:def:grammar-writer.cg3.grammar-writer]
/// C++ `class GrammarWriter`. Serializes a [`Grammar`] to CG-3 TEXT form.
///
/// The C++ `const Grammar* grammar` member is NOT stored (see the module note on
/// arena-model signature reconciliation); `grammar` is threaded through the
/// methods instead. The C++ `ux_stderr` diagnostics sink has no field analogue:
/// diagnostics are tracing events (wave 4).
pub struct GrammarWriter {
    used_sets: Uint32FlatHashSet,
    seen_rules: Uint32FlatHashSet,
    /// C++ `std::multimap<uint32_t, uint32_t> anchors`: rule-number → anchor-tag
    /// hashes (in inversion order, preserving `equal_range` iteration order).
    anchors: BTreeMap<u32, Vec<u32>>,
}

impl GrammarWriter {
    // [spec:cg3:def:grammar-writer.cg3.grammar-writer.grammar-writer-fn]
    // [spec:cg3:sem:grammar-writer.cg3.grammar-writer.grammar-writer-fn]
    /// Constructor `GrammarWriter(Grammar& res, std::ostream& ux_err)`. Builds
    /// the `anchors` multimap by INVERTING
    /// `res.anchors` (anchor-tag-hash → rule-number): for each pair
    /// `(first, second)` it inserts `(second, first)`, so the multimap is keyed by
    /// rule number with anchor-tag-hash values, which `print_rule` later queries
    /// via `equal_range(rule.number)`. (The non-specced destructor merely nulls
    /// `grammar`; the arena port keeps no such pointer, so it is a no-op.)
    pub fn grammar_writer(res: &Grammar) -> GrammarWriter {
        let mut anchors: BTreeMap<u32, Vec<u32>> = BTreeMap::new();

        // for (auto at : res.anchors) anchors.insert(make_pair(at.second, at.first));
        for &(first, second) in res.anchors.iter() {
            anchors.entry(second).or_default().push(first);
        }

        GrammarWriter {
            used_sets: Uint32FlatHashSet::new(),
            seen_rules: Uint32FlatHashSet::new(),
            anchors,
        }
    }

    // [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-set-fn]
    // [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-set-fn]
    /// Emits one set in CG-3 text form to `output`, recursively and deduplicated.
    /// If `curset.number` is already in `used_sets`, return. Otherwise mark it and
    /// emit: a leaf (`sets` empty) prints a `LIST`, a composite prints a `SET`
    /// (dependencies first). QUIRK reproduced: the SET branch reads `name[0]` /
    /// `name[1]` without a length guard (NUL-terminator semantics via [`byte_at`]);
    /// the LIST branch ends with a single "\n", the SET branch with two.
    fn print_set<W: Write>(&mut self, grammar: &Grammar, output: &mut W, id: SetId) {
        let number = grammar.sets_list[id.0].number;
        if self.used_sets.find(number) != self.used_sets.end() {
            return;
        }

        let sets_empty = grammar.sets_list[id.0].sets.is_empty();
        if sets_empty {
            self.used_sets.insert(number);
            if grammar.sets_list[id.0].r#type.intersects(ST_ORDERED) {
                w!(output, "O");
            }
            w!(output, "LIST {} = ", grammar.sets_list[id.0].name);
            let tagsets = [
                trie_get_tags_ordered(&grammar.sets_list[id.0].trie, grammar),
                trie_get_tags_ordered(&grammar.sets_list[id.0].trie_special, grammar),
            ];
            for tvs in &tagsets {
                for tags in tvs {
                    if tags.len() > 1 {
                        w!(output, "(");
                    }
                    for &tag in tags {
                        self.print_tag(output, &grammar.single_tags_list[tag.0]);
                        w!(output, " ");
                    }
                    if tags.len() > 1 {
                        w!(output, ") ");
                    }
                }
            }
            w!(output, " ;\n");
        } else {
            self.used_sets.insert(number);
            let sets = grammar.sets_list[id.0].sets.clone();
            for &s in &sets {
                // printSet(*grammar->sets_list[s]) — `s` is a set NUMBER.
                self.print_set(grammar, output, grammar.set_id_by_number(s));
            }
            let name = grammar.sets_list[id.0].name.clone();
            // const UChar* n = curset.name.data(); n[0]/n[1] read without a guard.
            let n0 = byte_at(&name, 0);
            let n1 = byte_at(&name, 1);
            if (n0 == b'$' && n1 == b'$') || (n0 == b'&' && n1 == b'&') {
                w!(output, "# ");
            }
            if grammar.sets_list[id.0].r#type.intersects(ST_ORDERED) {
                w!(output, "O");
            }
            w!(output, "SET {} = ", name);
            w!(output, "{} ", grammar.set_by_number(sets[0]).name);
            let set_ops = grammar.sets_list[id.0].set_ops.clone();
            for i in 0..sets.len() - 1 {
                w!(
                    output,
                    "{} {} ",
                    STRINGBITS[set_ops[i] as usize],
                    grammar.set_by_number(sets[i + 1]).name
                );
            }
            w!(output, " ;\n\n");
        }
    }

    // [spec:cg3:def:grammar-writer.cg3.grammar-writer.write-grammar-fn]
    // [spec:cg3:sem:grammar-writer.cg3.grammar-writer.write-grammar-fn]
    /// Writes the whole grammar to `output` in CG-3 source text; returns 0. The
    /// C++ null-checks (`!output` / `!grammar` → diagnostic + `CG3Quit(1)`) are
    /// structurally unreachable here — `output: &mut W` and `grammar: &mut Grammar`
    /// are non-null references — so they are elided (deferred-I/O precedent).
    /// Preamble → set-naming pass (the one grammar mutation) → set / template /
    /// rule-section emission.
    pub fn write_grammar<W: Write>(&mut self, grammar: &mut Grammar, output: &mut W) -> i32 {
        // (!output) / (!grammar): non-null references in the port; checks elided.

        w!(
            output,
            "# DELIMITERS and SOFT-DELIMITERS do not exist. Instead, look for the sets _S_DELIMITERS_ and _S_SOFT_DELIMITERS_.\n"
        );

        w!(output, "MAPPING-PREFIX = {} ;\n", grammar.mapping_prefix);

        if grammar.sub_readings_ltr {
            w!(output, "SUBREADINGS = LTR ;\n");
        } else {
            w!(output, "SUBREADINGS = RTL ;\n");
        }

        if !grammar.cmdargs.is_empty() {
            w!(output, "CMDARGS += {} ;\n", grammar.cmdargs);
        }
        if !grammar.cmdargs_override.is_empty() {
            w!(
                output,
                "CMDARGS-OVERRIDE += {} ;\n",
                grammar.cmdargs_override
            );
        }

        if !grammar.static_sets.is_empty() {
            w!(output, "STATIC-SETS =");
            for str in &grammar.static_sets {
                w!(output, " {}", str);
            }
            w!(output, " ;\n");
        }

        if !grammar.preferred_targets.is_empty() {
            w!(output, "PREFERRED-TARGETS = ");
            for &iter in &grammar.preferred_targets {
                let tid = grammar.single_tags.find(iter).get().1;
                self.print_tag(output, &grammar.single_tags_list[tid.0]);
                w!(output, " ");
            }
            w!(output, " ;\n");
        }

        if !grammar.parentheses.is_empty() {
            w!(output, "PARENTHESES = ");
            for (&first, &second) in &grammar.parentheses {
                w!(output, "(");
                let ftid = grammar.single_tags.find(first).get().1;
                self.print_tag(output, &grammar.single_tags_list[ftid.0]);
                w!(output, " ");
                let stid = grammar.single_tags.find(second).get().1;
                self.print_tag(output, &grammar.single_tags_list[stid.0]);
                w!(output, ") ");
            }
            w!(output, ";\n");
        }

        if grammar.ordered {
            w!(output, "OPTIONS += ordered ;\n");
        }
        if grammar.addcohort_attach {
            w!(output, "OPTIONS += addcohort-attach ;\n");
        }

        w!(output, "\n");

        // Set-naming pass — the ONE grammar mutation (needs &mut Grammar).
        self.used_sets.clear(0);
        for id in used_set_ids(grammar) {
            if grammar.sets_list[id.0].name.is_empty() {
                if grammar.delimiters == Some(id) {
                    grammar.sets_list[id.0].name = STR_DELIMITSET.to_string();
                } else if grammar.soft_delimiters == Some(id) {
                    grammar.sets_list[id.0].name = STR_SOFTDELIMITSET.to_string();
                } else if grammar.text_delimiters == Some(id) {
                    grammar.sets_list[id.0].name = STR_TEXTDELIMITSET.to_string();
                } else {
                    // s->name.resize(12); s->name.resize(u_sprintf("S%u", number)).
                    let number = grammar.sets_list[id.0].number;
                    grammar.sets_list[id.0].name = format!("S{number}");
                }
            }
            if is_internal(&grammar.sets_list[id.0].name) {
                // insert '3', 'G', 'C' at the front (yielding a "CG3" prefix).
                let old = grammar.sets_list[id.0].name.clone();
                grammar.sets_list[id.0].name = format!("CG3{old}");
            }
        }

        // Print phase: reborrow the grammar immutably.
        for id in used_set_ids(grammar) {
            self.print_set(grammar, output, id);
        }
        w!(output, "\n");

        let tmpls: Vec<CtxId> = grammar.templates.values().copied().collect();
        for tmpl in tmpls {
            w!(
                output,
                "TEMPLATE {} = ",
                grammar.contexts_arena[tmpl.0].hash
            );
            self.print_contextual_test(grammar, output, &grammar.contexts_arena[tmpl.0]);
            w!(output, " ;\n");
        }

        let rids = rule_ids(grammar);

        let mut found = false;
        for rid in &rids {
            if grammar.rule_by_number[rid.0].section == -1 {
                if !found {
                    w!(output, "\nBEFORE-SECTIONS\n");
                    found = true;
                }
                self.print_rule(grammar, output, &grammar.rule_by_number[rid.0]);
                w!(output, " ;\n");
            }
        }
        for &isec in &grammar.sections {
            found = false;
            for rid in &rids {
                if grammar.rule_by_number[rid.0].section == si32(isec) {
                    if !found {
                        w!(output, "\nSECTION\n");
                        found = true;
                    }
                    self.print_rule(grammar, output, &grammar.rule_by_number[rid.0]);
                    w!(output, " ;\n");
                }
            }
        }
        found = false;
        for rid in &rids {
            if grammar.rule_by_number[rid.0].section == -2 {
                if !found {
                    w!(output, "\nAFTER-SECTIONS\n");
                    found = true;
                }
                self.print_rule(grammar, output, &grammar.rule_by_number[rid.0]);
                w!(output, " ;\n");
            }
        }
        found = false;
        for rid in &rids {
            if grammar.rule_by_number[rid.0].section == -3 {
                if !found {
                    w!(output, "\nNULL-SECTION\n");
                    found = true;
                }
                self.print_rule(grammar, output, &grammar.rule_by_number[rid.0]);
                w!(output, " ;\n");
            }
        }

        0
    }

    // [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-rule-fn]
    // [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-rule-fn]
    /// Emits one rule in CG-3 text form to `to`; deduplicated via `seen_rules`.
    /// Emits anchors, wordform, the (collapsed) type keyword + optional `:name`,
    /// flags, mapping/sublist/childset operands, target, tests, the trailing
    /// keyword, dep-target + dep-tests, and (for `WITH`) the brace-wrapped
    /// sub-rules. QUIRK reproduced: `rule.name[1]`/`name[2]` read without a length
    /// guard (NUL-terminator semantics via [`byte_at`]).
    fn print_rule<W: Write>(&mut self, grammar: &Grammar, to: &mut W, rule: &Rule) {
        if self.seen_rules.count(rule.number) != 0 {
            return;
        }
        self.seen_rules.insert(rule.number);

        // anchors.equal_range(rule.number)
        let anchor_hashes: Vec<u32> = self.anchors.get(&rule.number).cloned().unwrap_or_default();
        for h in anchor_hashes {
            let tid = grammar.single_tags.find(h).get().1;
            let tag = &grammar.single_tags_list[tid.0].tag;
            if tag == KEYWORDS_NAMES[KEYWORDS::K_START as usize]
                || tag == KEYWORDS_NAMES[KEYWORDS::K_END as usize]
                || *tag == rule.name
            {
                continue;
            }
            w!(to, "ANCHOR {tag} ;\n");
        }

        if let Some(wf) = rule.wordform {
            self.print_tag(to, &grammar.single_tags_list[wf.0]);
            w!(to, " ");
        }

        let mut type_kw = rule.r#type;
        if rule.r#type == KEYWORDS::K_MOVE_BEFORE || rule.r#type == KEYWORDS::K_MOVE_AFTER {
            type_kw = KEYWORDS::K_MOVE;
        }
        if rule.r#type == KEYWORDS::K_ADDCOHORT_BEFORE || rule.r#type == KEYWORDS::K_ADDCOHORT_AFTER
        {
            type_kw = KEYWORDS::K_ADDCOHORT;
        }
        if rule.r#type == KEYWORDS::K_EXTERNAL_ONCE || rule.r#type == KEYWORDS::K_EXTERNAL_ALWAYS {
            type_kw = KEYWORDS::K_EXTERNAL;
        }

        w!(to, "{}", KEYWORDS_NAMES[type_kw as usize]);

        // !name.empty() && !(name[0]=='_' && name[1]=='R' && name[2]=='_')
        if !rule.name.is_empty()
            && !(byte_at(&rule.name, 0) == b'_'
                && byte_at(&rule.name, 1) == b'R'
                && byte_at(&rule.name, 2) == b'_')
        {
            w!(to, ":{}", rule.name);
        }
        w!(to, " ");

        for i in 0..FLAGS_COUNT {
            if i == FL_BEFORE || i == FL_AFTER || i == FL_WITHCHILD {
                continue;
            }
            if rule
                .flags
                .intersects(crate::rule::RuleFlags::from_bits_retain(1u64 << i))
            {
                if i == FL_SUB {
                    w!(to, "{}:{} ", G_FLAGS[i], rule.sub_reading);
                } else {
                    w!(to, "{} ", G_FLAGS[i]);
                }
            }
        }

        if rule.flags.intersects(RF_WITHCHILD) {
            w!(
                to,
                "WITHCHILD {} ",
                grammar.set_by_number(rule.childset1).name
            );
        }

        if rule.r#type == KEYWORDS::K_SUBSTITUTE || rule.r#type == KEYWORDS::K_EXECUTE {
            w!(to, "{} ", grammar.sets_list[rule.sublist.unwrap().0].name);
        }

        if let Some(ml) = rule.maplist {
            w!(to, "{} ", grammar.sets_list[ml.0].name);
        }

        if rule.sublist.is_some()
            && (rule.r#type == KEYWORDS::K_ADDRELATIONS
                || rule.r#type == KEYWORDS::K_SETRELATIONS
                || rule.r#type == KEYWORDS::K_REMRELATIONS
                || rule.r#type == KEYWORDS::K_SETVARIABLE
                || rule.r#type == KEYWORDS::K_COPY
                || rule.r#type == KEYWORDS::K_COPYCOHORT)
        {
            if rule.r#type == KEYWORDS::K_COPY || rule.r#type == KEYWORDS::K_COPYCOHORT {
                w!(to, "EXCEPT ");
            }
            w!(to, "{} ", grammar.sets_list[rule.sublist.unwrap().0].name);
        }

        if rule.r#type == KEYWORDS::K_ADD
            || rule.r#type == KEYWORDS::K_MAP
            || rule.r#type == KEYWORDS::K_SUBSTITUTE
            || rule.r#type == KEYWORDS::K_COPY
            || rule.r#type == KEYWORDS::K_COPYCOHORT
        {
            if rule.flags.intersects(RF_BEFORE) {
                w!(to, "BEFORE ");
            }
            if rule.flags.intersects(RF_AFTER) {
                w!(to, "AFTER ");
            }
            if rule.childset1 != 0 {
                if rule.r#type == KEYWORDS::K_COPYCOHORT {
                    w!(to, "WITHCHILD ");
                }
                w!(to, "{} ", grammar.set_by_number(rule.childset1).name);
            }
        }

        if rule.r#type == KEYWORDS::K_ADDCOHORT_BEFORE {
            w!(to, "BEFORE ");
        } else if rule.r#type == KEYWORDS::K_ADDCOHORT_AFTER {
            w!(to, "AFTER ");
        }

        if rule.target != 0 {
            w!(to, "{} ", grammar.set_by_number(rule.target).name);
        }

        for it in &rule.tests {
            w!(to, "(");
            self.print_contextual_test(grammar, to, &grammar.contexts_arena[it.0]);
            w!(to, ") ");
        }

        if rule.r#type == KEYWORDS::K_SETPARENT
            || rule.r#type == KEYWORDS::K_SETCHILD
            || rule.r#type == KEYWORDS::K_ADDRELATIONS
            || rule.r#type == KEYWORDS::K_ADDRELATION
            || rule.r#type == KEYWORDS::K_SETRELATIONS
            || rule.r#type == KEYWORDS::K_SETRELATION
            || rule.r#type == KEYWORDS::K_REMRELATIONS
            || rule.r#type == KEYWORDS::K_REMRELATION
            || rule.r#type == KEYWORDS::K_COPYCOHORT
        {
            w!(to, "TO ");
        } else if rule.r#type == KEYWORDS::K_MOVE_AFTER {
            w!(to, "AFTER ");
        } else if rule.r#type == KEYWORDS::K_MOVE_BEFORE {
            w!(to, "BEFORE ");
        } else if rule.r#type == KEYWORDS::K_SWITCH || rule.r#type == KEYWORDS::K_MERGECOHORTS {
            w!(to, "WITH ");
        }

        if let Some(dt) = rule.dep_target {
            if rule.childset2 != 0 {
                w!(
                    to,
                    "WITHCHILD {} ",
                    grammar.set_by_number(rule.childset2).name
                );
            }
            w!(to, "(");
            self.print_contextual_test(grammar, to, &grammar.contexts_arena[dt.0]);
            w!(to, ") ");
        }
        for it in &rule.dep_tests {
            w!(to, "(");
            self.print_contextual_test(grammar, to, &grammar.contexts_arena[it.0]);
            w!(to, ") ");
        }

        if rule.r#type == KEYWORDS::K_WITH {
            w!(to, "{{\n");
            let sub_rules = rule.sub_rules.clone();
            for r in sub_rules {
                w!(to, "\t");
                self.print_rule(grammar, to, &grammar.rule_by_number[r.0]);
                w!(to, " ;\n");
            }
            w!(to, "}}\n");
        }
    }

    // [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-contextual-test-fn]
    // [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-contextual-test-fn]
    /// Emits a contextual test's position, target, barriers, and linked chain in
    /// CG-3 syntax to `to`. The position atom is emitted only when
    /// `POS_TMPL_OVERRIDE` is set OR the test has neither a template nor ORs; the
    /// reference part prints the template, the OR-group, the target, the
    /// (C)BARRIER, and recurses into `linked`.
    fn print_contextual_test<W: Write>(
        &mut self,
        grammar: &Grammar,
        to: &mut W,
        test: &ContextualTest,
    ) {
        if test.pos.intersects(POS_NEGATE) {
            w!(to, "NEGATE ");
        }
        if (test.pos.intersects(POS_TMPL_OVERRIDE)) || (test.tmpl.is_none() && test.ors.is_empty())
        {
            if test.pos.intersects(POS_ALL) {
                w!(to, "ALL ");
            }
            if test.pos.intersects(POS_NONE) {
                w!(to, "NONE ");
            }
            if test.pos.intersects(POS_NOT) {
                w!(to, "NOT ");
            }
            if test.pos.intersects(POS_ABSOLUTE) {
                w!(to, "@");
            }
            if test.pos.intersects(POS_SCANALL) {
                w!(to, "**");
            } else if test.pos.intersects(POS_SCANFIRST | POS_DEP_DEEP) {
                w!(to, "*");
            }

            if test.pos.intersects(POS_LEFTMOST) {
                w!(to, "ll");
            }
            if test.pos.intersects(POS_LEFT) {
                w!(to, "l");
            }
            if test.pos.intersects(POS_RIGHTMOST) {
                w!(to, "rr");
            }
            if test.pos.intersects(POS_RIGHT) {
                w!(to, "r");
            }
            if test.pos.intersects(POS_DEP_CHILD) {
                w!(to, "c");
            }
            if test.pos.intersects(POS_DEP_PARENT) {
                if test.pos.intersects(POS_DEP_GLOB) {
                    w!(to, "p");
                }
                w!(to, "p");
            } else if test.pos.intersects(POS_DEP_GLOB) {
                w!(to, "cc");
            }
            if test.pos.intersects(POS_DEP_SIBLING) {
                w!(to, "s");
            }
            if test.pos.intersects(POS_SELF) {
                w!(to, "S");
            }
            if test.pos.intersects(POS_NO_BARRIER) {
                w!(to, "N");
            }

            if test.pos.intersects(POS_UNKNOWN) {
                w!(to, "?");
            } else if !test.pos.intersects(
                POS_DEP_CHILD
                    | POS_DEP_SIBLING
                    | POS_DEP_PARENT
                    | POS_DEP_GLOB
                    | POS_LEFT_PAR
                    | POS_RIGHT_PAR
                    | POS_RELATION
                    | POS_BAG_OF_TAGS,
            ) {
                w!(to, "{}", test.offset);
            }

            if test.pos.intersects(POS_CAREFUL) {
                w!(to, "C");
            }
            if test.pos.intersects(POS_SPAN_BOTH) {
                w!(to, "W");
            }
            if test.pos.intersects(POS_SPAN_LEFT) {
                w!(to, "<");
            }
            if test.pos.intersects(POS_SPAN_RIGHT) {
                w!(to, ">");
            }
            if test.pos.intersects(POS_PASS_ORIGIN) {
                w!(to, "o");
            }
            if test.pos.intersects(POS_NO_PASS_ORIGIN) {
                w!(to, "O");
            }
            if test.pos.intersects(POS_LEFT_PAR) {
                w!(to, "L");
            }
            if test.pos.intersects(POS_RIGHT_PAR) {
                w!(to, "R");
            }
            if test.pos.intersects(POS_MARK_SET) {
                w!(to, "X");
            }
            if test.pos.intersects(POS_JUMP) {
                if test.jump_pos == POS_JUMP_POS::JUMP_MARK as i8 {
                    w!(to, "x");
                } else if test.jump_pos == POS_JUMP_POS::JUMP_ATTACH as i8 {
                    w!(to, "jA");
                } else if test.jump_pos == POS_JUMP_POS::JUMP_TARGET as i8 {
                    w!(to, "jT");
                } else {
                    w!(to, "jC{}", test.jump_pos);
                }
            }
            if test.pos.intersects(POS_LOOK_DELETED) {
                w!(to, "D");
            }
            if test.pos.intersects(POS_LOOK_DELAYED) {
                w!(to, "d");
            }
            if test.pos.intersects(POS_ACTIVE) {
                w!(to, "T");
            }
            if test.pos.intersects(POS_INACTIVE) {
                w!(to, "t");
            }
            if test.pos.intersects(POS_LOOK_IGNORED) {
                w!(to, "I");
            }
            if test.pos.intersects(POS_ATTACH_TO) {
                w!(to, "A");
            }
            if test.pos.intersects(POS_WITH) {
                w!(to, "w");
            }
            if test.pos.intersects(POS_BAG_OF_TAGS) {
                w!(to, "B");
            }
            if test.pos.intersects(POS_RELATION) {
                w!(to, "r:");
                let tid = grammar.single_tags.find(test.relation).get().1;
                self.print_tag(to, &grammar.single_tags_list[tid.0]);
            }
            if test.offset_sub != 0 {
                if test.offset_sub == GSR_SPECIALS::GSR_ANY as i32 {
                    w!(to, "/*");
                } else {
                    w!(to, "/{}", test.offset_sub);
                }
            }

            w!(to, " ");
        }

        if let Some(t) = test.tmpl {
            w!(to, "T:{} ", grammar.contexts_arena[t.0].hash);
        } else if !test.ors.is_empty() {
            let ors = test.ors.clone();
            let mut i = 0;
            while i < ors.len() {
                w!(to, "(");
                self.print_contextual_test(grammar, to, &grammar.contexts_arena[ors[i].0]);
                w!(to, ")");
                i += 1;
                if i != ors.len() {
                    w!(to, " OR ");
                } else {
                    w!(to, " ");
                }
            }
        }

        if test.target != 0 {
            w!(to, "{} ", grammar.set_by_number(test.target).name);
        }
        if test.cbarrier != 0 {
            w!(
                to,
                "CBARRIER {} ",
                grammar.set_by_number(test.cbarrier).name
            );
        }
        if test.barrier != 0 {
            w!(to, "BARRIER {} ", grammar.set_by_number(test.barrier).name);
        }

        if let Some(l) = test.linked {
            w!(to, "LINK ");
            self.print_contextual_test(grammar, to, &grammar.contexts_arena[l.0]);
        }
    }

    // [spec:cg3:def:grammar-writer.cg3.grammar-writer.print-tag-fn]
    // [spec:cg3:sem:grammar-writer.cg3.grammar-writer.print-tag-fn]
    /// Converts the tag to its CG-3 textual form via `tag.to_u_string(true)` (the
    /// `true` requests the escaped/round-trippable rendering) and prints it. No
    /// trailing space or separator is added here — callers add those.
    fn print_tag<W: Write>(&self, to: &mut W, tag: &Tag) {
        let str = tag.to_u_string(true);
        w!(to, "{str}");
    }
}
