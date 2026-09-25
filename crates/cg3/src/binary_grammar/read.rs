//! The section readers behind [`BinaryGrammar::parse_grammar_reader`]. Each
//! reads one part of the wire layout (see the `binary_grammar` module docs) in
//! the C++ field order, and checks every number against what it indexes
//! before storing it: a `.cg3b` is untrusted bytes, and the C++ trusted them.

use std::collections::HashMap;

use super::cursor::{Cg3bCursor, malformed};
use super::*;
use crate::contextual_test::{ContextualTest, POS_RELATION, PosFlags};
use crate::error::{BinaryFault, GrammarError};
use crate::grammar::TRIE_ENTRY_SIZE;
use crate::set::{ST_SET_UNIFY, SetType};
use crate::strings::{KEYWORDS_BY_ID, S_FAILFAST, S_MINUS, S_OR, S_PLUS};
use crate::tag::TagUnion;
use crate::tag_regex::TagRegexError;

/// The smallest record of each table, for checking a count against the bytes
/// left: a tag, set or contextual test is at least its field mask; a rule is
/// its mask, dependency-target hash and two test counts.
const MIN_TAG_RECORD: usize = 4;
const MIN_SET_RECORD: usize = 4;
const MIN_CONTEXT_RECORD: usize = 4;
const MIN_RULE_RECORD: usize = 16;

/// The highest section number a `.cg3b` of `num_rules` rules may use: 1023,
/// or the rule count if that is higher.
///
/// A run makes a pass over every section up to the highest one used,
/// rerunning each earlier section's rules in it, so the number sets the cost
/// of loading and of every window. A textual grammar reaches a section only
/// through a `SECTION` header for each, and has no limit; tying this one to
/// the rule count lets its `.cg3b` load too, unless most of its sections are
/// empty, while a corrupt number costs no more than a textual grammar with
/// that many rules can.
fn max_section(num_rules: u32) -> i32 {
    i32::try_from(num_rules).unwrap_or(i32::MAX).max(1023)
}

/// The C++ `C_OPS` enumerators by their serialised id.
const C_OPS_BY_ID: [COps; 8] = [
    COps::OpNop,
    COps::OpEquals,
    COps::OpLessthan,
    COps::OpGreaterthan,
    COps::OpLessequals,
    COps::OpGreaterequals,
    COps::OpNotequals,
    COps::NumOps,
];

/// Marks a tag, set or rule number no record has claimed yet.
const UNCLAIMED: usize = usize::MAX;

/// What the reader carries from one section to the next: the counts later
/// numbers are checked against, and where each record starts, for the faults
/// only found once a whole table is in.
#[derive(Default)]
pub(super) struct Load {
    pub(super) num_tags: u32,
    pub(super) num_sets: u32,
    pub(super) num_rules: u32,
    /// Where each tag, set and rule record starts, by number.
    pub(super) tag_at: Vec<usize>,
    pub(super) set_at: Vec<usize>,
    pub(super) rule_at: Vec<usize>,
    /// Where each contextual test record starts.
    pub(super) ctx_at: HashMap<CtxId, usize>,
    /// C++ `std::map<uint32_t, uint32Vector> tag_varsets`: a varstring tag's
    /// set numbers by tag number, resolved once the sets are in.
    pub(super) tag_varsets: HashMap<u32, Vec<u32>>,
    /// The highest anchor position and where it was read; positions index
    /// the rules, which come last.
    pub(super) last_anchor: Option<(u32, usize)>,
}

impl Load {
    /// Where the contextual test `t` starts.
    pub(super) fn ctx_offset(&self, t: CtxId) -> usize {
        self.ctx_at.get(&t).copied().unwrap_or_default()
    }
}

/// Record that `number`'s record starts at `at`, refusing a second claim.
fn claim(
    slots: &mut [usize],
    number: u32,
    at: usize,
    what: &'static str,
) -> Result<(), GrammarError> {
    let Some(slot) = slots.get_mut(number as usize) else {
        let limit = slots.len() as u64;
        let value = number.into();
        return Err(malformed(
            at,
            BinaryFault::OutOfRange { what, value, limit },
        ));
    };
    if *slot != UNCLAIMED {
        let value = number;
        return Err(malformed(at, BinaryFault::Duplicate { what, value }));
    }
    *slot = at;
    Ok(())
}

/// The record count of an optional table: present when `bit` is set in the
/// feature bits, else zero.
fn flagged_count(
    cur: &mut Cg3bCursor<'_>,
    fields: u32,
    bit: u32,
    what: &'static str,
    min_size: usize,
) -> Result<u32, GrammarError> {
    if fields & bit == 0 {
        return Ok(0);
    }
    cur.count(what, min_size)
}

/// A set number held by a context or rule, which must name one of the
/// `num_sets` sets whether it was read or left at its default of 0.
fn check_set_number(what: &'static str, n: SetNumber, num_sets: u32) -> Result<(), BinaryFault> {
    if n.get() < num_sets {
        return Ok(());
    }
    let (value, limit) = (n.get().into(), num_sets.into());
    Err(BinaryFault::OutOfRange { what, value, limit })
}

/// A serialised enumerator id, refused when it names no entry of `table`
/// (C++ `static_cast<KEYWORDS>` / `static_cast<C_OPS>`).
fn enumerator<T: Copy>(
    cur: &mut Cg3bCursor<'_>,
    what: &'static str,
    table: &[T],
) -> Result<T, GrammarError> {
    let at = cur.offset();
    let id: u32 = cur.be(what)?;
    table.get(id as usize).copied().ok_or_else(|| {
        let (value, limit) = (id.into(), table.len() as u64);
        malformed(at, BinaryFault::OutOfRange { what, value, limit })
    })
}

// --- Tag records -----------------------------------------------------------

/// Tag bits 0-7 and 12: the number and hashes, type and comparison payload.
pub(super) fn tag_scalars(
    cur: &mut Cg3bCursor<'_>,
    tfields: u32,
    num_tags: u32,
    t: &mut Tag,
) -> Result<(), GrammarError> {
    if tfields & (1 << 0) != 0 {
        t.number = cur.index("tag number", num_tags)?;
    }
    if tfields & (1 << 1) != 0 {
        t.hash = TagHash(cur.hash("tag hash")?);
    }
    if tfields & (1 << 2) != 0 {
        t.plain_hash = TagHash(cur.hash("tag plain hash")?);
    }
    if tfields & (1 << 3) != 0 {
        t.seed = cur.be("tag seed")?;
    }
    if tfields & (1 << 4) != 0 {
        t.r#type = crate::tag::TagType::from_bits_retain(cur.be("tag type")?);
    }
    if tfields & (1 << 5) != 0 {
        t.comparison_hash = cur.hash("tag comparison hash")?;
    }
    if tfields & (1 << 6) != 0 {
        t.comparison_op = enumerator(cur, "comparison operator", &C_OPS_BY_ID)?;
    }
    if tfields & (1 << 7) != 0 {
        // Legacy integer comparison_val, never emitted by the current
        // writer. The C++ compares the double it just assigned from this
        // int32 against the int32 extremes, which only hits AT them.
        let v: i32 = cur.be("tag comparison value")?;
        t.comparison_val = match v {
            i32::MIN => crate::inlines::NUMERIC_MIN,
            i32::MAX => crate::inlines::NUMERIC_MAX,
            v => v as f64,
        };
    }
    if tfields & (1 << 12) != 0 {
        // 12-byte double: u64 BE mantissa + i32 BE exponent.
        t.comparison_val = cur.f64("tag comparison value")?;
    }
    Ok(())
}

/// Tag bits 8 and 9: the text and the regex pattern. A pattern that will not
/// compile is collected rather than ending the read, so one load reports
/// every bad tag.
pub(super) fn tag_text(
    cur: &mut Cg3bCursor<'_>,
    tfields: u32,
    t: &mut Tag,
    bad_regexes: &mut Vec<TagRegexError>,
) -> Result<(), GrammarError> {
    if tfields & (1 << 8) != 0 {
        t.tag = cur.text("tag text")?.into();
    }
    if tfields & (1 << 9) != 0 {
        let pattern = cur.text("tag regex")?;
        if !pattern.is_empty() {
            // Flags re-derived from type (NOT stored): case-insensitive iff
            // T_CASE_INSENSITIVE. RegexBuilder keeps `as_str()` == the bare
            // pattern, as the C++ round-trips it.
            let icase = t.r#type.intersects(T_CASE_INSENSITIVE);
            match crate::tag_regex::compile_tag_regex(&pattern, icase) {
                Ok(re) => t.regexp = Some(re),
                Err(e) => bad_regexes.push(e.with_tag(t.tag.clone())),
            }
        }
    }
    Ok(())
}

/// Tag bits 10 and 11: a varstring's set numbers (deferred until the sets are
/// in) and its names.
pub(super) fn tag_varstring(
    cur: &mut Cg3bCursor<'_>,
    tfields: u32,
    t: &mut Tag,
    tag_varsets: &mut HashMap<u32, Vec<u32>>,
) -> Result<(), GrammarError> {
    if tfields & (1 << 10) != 0 {
        let num = cur.count("varstring set count", 4)?;
        t.allocate_vs_sets();
        let entry = tag_varsets.entry(t.number).or_default();
        for _ in 0..num {
            entry.push(cur.be("varstring set")?);
        }
    }
    if tfields & (1 << 11) != 0 {
        let num = cur.count("varstring name count", 4)?;
        t.allocate_vs_names();
        let names = t.vs_names.get_or_insert_default();
        for _ in 0..num {
            let name = cur.text("varstring name")?;
            if !name.is_empty() {
                names.push(name);
            }
        }
    }
    Ok(())
}

/// Tag bits 13 and 14, the two roles of the C++ union the format stores, and
/// the check that the type gives the tag the role its value was stored for.
pub(super) fn tag_role(
    cur: &mut Cg3bCursor<'_>,
    tfields: u32,
    t: &mut Tag,
    at: usize,
) -> Result<(), GrammarError> {
    if tfields & (1 << 13) != 0 {
        // variable_hash (the C++ union member) — or, in a numeric-math
        // variable tag (`VAR:<x=5+1>`), the math offset the writer finds
        // there: a plain number, which may be any value, not a hash.
        if t.r#type.intersects(crate::tag::T_NUMERIC_MATH) {
            let v = cur.be("tag math offset")?;
            t.set_variable_member(v);
        } else {
            let v = cur.hash("tag variable value")?;
            t.set_variable_hash(v);
        }
    }
    if tfields & (1 << 14) != 0 {
        // context_ref_pos (the C++ union member).
        let v = cur.be("tag context position")?;
        t.set_context_ref_pos(v);
    }
    let fault = |problem| BinaryFault::TagRole {
        tag: t.number,
        type_bits: t.r#type.bits(),
        problem,
    };
    role_problem(tfields, t).map_or(Ok(()), |p| Err(malformed(at, fault(p))))
}

// [spec:cg3:req:robustness.binary-grammar-validated]
/// Why the union value a tag record stores does not fit its type, if it does
/// not. The engine reads each role's value through the type that gives the
/// tag that role, and the `Tag` role accessors panic on a read of any role
/// but the stored one; so a value must be stored for exactly the role its
/// type gives the tag.
fn role_problem(tfields: u32, t: &Tag) -> Option<&'static str> {
    let variable = tfields & (1 << 13) != 0;
    let context = tfields & (1 << 14) != 0;
    let ty = t.r#type;
    let other_roles =
        crate::tag::T_DEPENDENCY | crate::tag::T_RELATION | crate::tag::T_NUMERIC_MATH;
    // A numeric-math variable tag's slot holds its math offset, which the
    // writer stores in the variable field; that is the one other role the
    // variable field may carry.
    let variable_other_roles = crate::tag::T_DEPENDENCY | crate::tag::T_RELATION;
    let problems = [
        (
            variable && context,
            "stores both a variable value and a context position",
        ),
        (
            variable && !ty.intersects(T_VARIABLE | T_LOCAL_VARIABLE),
            "stores a variable value but is not a variable",
        ),
        (
            context && !ty.intersects(T_CONTEXT),
            "stores a context position but is not a context reference",
        ),
        (
            !context && ty.intersects(T_CONTEXT),
            "is a context reference but stores no position",
        ),
        (
            t.extra == TagUnion::ContextRefPos(0),
            "is a context reference to position 0",
        ),
        (
            (variable && ty.intersects(variable_other_roles))
                || (context && ty.intersects(other_roles)),
            "stores a value its type reads as another role",
        ),
    ];
    problems
        .iter()
        .find(|(bad, _)| *bad)
        .map(|&(_, problem)| problem)
}

// --- Set records -----------------------------------------------------------

/// One set record: the `sfields` bitmap followed by the fields it
/// advertises, each number checked against the tag and set counts.
pub(super) fn set_record(cur: &mut Cg3bCursor<'_>, load: &Load) -> Result<Set, GrammarError> {
    let at = cur.offset();
    let mut s = Set::default(); // allocateSet()
    let sfields: u32 = cur.be("set field mask")?;
    if sfields & (1 << 0) != 0 {
        s.number = SetNumber(cur.index("set number", load.num_sets)?);
    }
    if sfields & (1 << 1) != 0 {
        s.r#type = SetType::from_bits_retain(ui16(cur.be::<u32>("set type")?));
    }
    if sfields & (1 << 2) != 0 {
        s.r#type = SetType::from_bits_retain(u16::from(cur.be::<u8>("set type")?));
    }
    if sfields & (1 << 3) != 0 {
        let n = cur.count("set trie size", TRIE_ENTRY_SIZE)?;
        trie_unserialize(&mut s.trie, cur, n, load.num_tags)?;
        let n = cur.count("set special trie size", TRIE_ENTRY_SIZE)?;
        trie_unserialize(&mut s.trie_special, cur, n, load.num_tags)?;
    }
    if sfields & (1 << 4) != 0 {
        let n = cur.count("set operator count", 4)?;
        for _ in 0..n {
            s.set_ops.push(cur.be("set operator")?);
        }
    }
    if sfields & (1 << 5) != 0 {
        let n = cur.count("member set count", 4)?;
        for _ in 0..n {
            s.sets.push(cur.index("member set", load.num_sets)?);
        }
    }
    if sfields & (1 << 6) != 0 {
        // C++ s->setName(text) (assign directly); the port's Set has only the
        // u32 setName overload, so inline the assignment.
        s.name = cur.text("set name")?;
    }
    set_shape(&s).map_err(|f| malformed(at, f))?;
    Ok(s)
}

// [spec:cg3:req:robustness.binary-grammar-validated]
/// The shape the matcher and the grammar writer assume of a set: operators
/// only from the four they implement, one between each pair of member sets,
/// and a member set for a unified set to unify over.
fn set_shape(s: &Set) -> Result<(), BinaryFault> {
    let set = s.number.get();
    let known = |op: &u32| matches!(*op, S_OR | S_PLUS | S_MINUS | S_FAILFAST);
    if let Some(&op) = s.set_ops.iter().find(|op| !known(op)) {
        return Err(BinaryFault::SetOperator { set, op });
    }
    let (sets, ops) = (s.sets.len(), s.set_ops.len());
    if sets > 0 && ops < sets - 1 {
        return Err(BinaryFault::SetOperatorCount { set, sets, ops });
    }
    if s.r#type.intersects(ST_SET_UNIFY) && s.sets.is_empty() {
        return Err(BinaryFault::EmptyUnifiedSet { set });
    }
    Ok(())
}

// --- Contextual test records ------------------------------------------------

/// Contextual test bits 0-9 and 12, in the C++ read order (bit 12 before 10
/// and 11). Returns the template hash of bit 3, bound once every test is in.
pub(super) fn context_fields(
    cur: &mut Cg3bCursor<'_>,
    fields: u32,
    ct: &mut ContextualTest,
) -> Result<Option<u32>, GrammarError> {
    let mut tmpl = None;
    if fields & (1 << 0) != 0 {
        ct.hash = cur.hash("contextual test hash")?;
    }
    if fields & (1 << 1) != 0 {
        let mut pos = u64::from(cur.be::<u32>("test position")?);
        if pos & POS_64BIT.bits() != 0 {
            pos |= u64::from(cur.be::<u32>("test position")?) << 32;
        }
        ct.pos = PosFlags::from_bits_retain(pos);
    }
    if fields & (1 << 2) != 0 {
        ct.offset = cur.be("test offset")?;
    }
    if fields & (1 << 3) != 0 {
        tmpl = Some(cur.be("template hash")?);
    }
    if fields & (1 << 4) != 0 {
        ct.target = SetNumber(cur.be("test target set")?);
    }
    if fields & (1 << 5) != 0 {
        ct.line = cur.be("test line")?;
    }
    if fields & (1 << 6) != 0 {
        ct.relation = cur.hash("test relation")?;
    }
    if fields & (1 << 7) != 0 {
        ct.barrier = SetNumber(cur.be("BARRIER set")?);
    }
    if fields & (1 << 8) != 0 {
        ct.cbarrier = SetNumber(cur.be("CBARRIER set")?);
    }
    if fields & (1 << 9) != 0 {
        ct.offset_sub = cur.be("test sub-reading offset")?;
    }
    if fields & (1 << 12) != 0 {
        ct.jump_pos = cur.be("test jump position")?;
    }
    Ok(tmpl)
}

impl BinaryGrammar {
    /// The mapping prefix and the two `CMDARGS` strings that follow the
    /// feature bits.
    pub(super) fn read_prefix_and_cmdargs(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        fields: u32,
        bin_revision: u32,
    ) -> Result<(), GrammarError> {
        if fields & BINF_PREFIX != 0 {
            // Decode into a single char (mapping_prefix is one character).
            let prefix = cur.text("mapping prefix")?;
            self.grammar.mapping_prefix = prefix.chars().next().unwrap_or('\0');
        }
        if bin_revision >= BIN_REV_CMDARGS {
            self.grammar.cmdargs = cur.text("CMDARGS")?;
            self.grammar.cmdargs_override = cur.text("CMDARGS-OVERRIDE")?;
        }
        Ok(())
    }

    /// The tag table: each record placed at its own number, which must be
    /// unique, as must its hash.
    pub(super) fn read_tags(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        fields: u32,
        load: &mut Load,
    ) -> Result<(), GrammarError> {
        let num = flagged_count(cur, fields, BINF_TAGS, "tag count", MIN_TAG_RECORD)?;
        self.grammar.num_tags = num as usize;
        load.num_tags = num;
        load.tag_at = vec![UNCLAIMED; num as usize];
        // single_tags_list.resize(num): pre-allocate `num` slots so a tag can be
        // placed at its `number` (== arena slot).
        for _ in 0..num {
            self.grammar.single_tags_list.alloc(Tag::default());
        }
        let mut bad_regexes: Vec<TagRegexError> = Vec::new();
        for _ in 0..num {
            let at = cur.offset();
            let t = Self::read_tag_record(cur, num, &mut load.tag_varsets, &mut bad_regexes)?;
            self.place_tag(t, at, load)?;
        }
        if !bad_regexes.is_empty() {
            return Err(GrammarError::TagRegex(bad_regexes));
        }
        checks::tag_hashes(&self.grammar, load)
    }

    /// `single_tags[t->hash] = t; single_tags_list[t->number] = t`.
    fn place_tag(&mut self, t: Tag, at: usize, load: &mut Load) -> Result<(), GrammarError> {
        claim(&mut load.tag_at, t.number, at, "tag number")?;
        let hash = t.hash.get();
        if self.grammar.tags_by_hash.contains(hash) {
            let fault = BinaryFault::Duplicate {
                what: "tag hash",
                value: hash,
            };
            return Err(malformed(at, fault));
        }
        self.grammar.tags_by_hash.insert((hash, TagId(t.number)));
        if &*t.tag == "*" {
            self.grammar.tag_any = hash;
        }
        let number = t.number;
        self.grammar.single_tags_list[number] = t;
        Ok(())
    }

    /// A tag hash that must name a loaded tag.
    fn tag_hash(&self, cur: &mut Cg3bCursor<'_>, what: &'static str) -> Result<u32, GrammarError> {
        let at = cur.offset();
        let hash = cur.hash(what)?;
        if !self.grammar.tags_by_hash.contains(hash) {
            return Err(malformed(at, BinaryFault::UnknownTag { what, hash }));
        }
        Ok(hash)
    }

    /// reopen_mappings, preferred_targets, parentheses and anchors: tables of
    /// tag hashes, each of which must name a loaded tag.
    pub(super) fn read_tag_tables(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        fields: u32,
        load: &mut Load,
    ) -> Result<(), GrammarError> {
        let num = flagged_count(cur, fields, BINF_REOPEN_MAP, "reopen-mappings count", 4)?;
        for _ in 0..num {
            let h = self.tag_hash(cur, "reopen-mappings tag")?;
            self.grammar.reopen_mappings.insert(h);
        }
        let num = flagged_count(cur, fields, BINF_PREF_TARGETS, "preferred-targets count", 4)?;
        for _ in 0..num {
            let h = self.tag_hash(cur, "preferred target")?;
            self.grammar.preferred_targets.push(h);
        }
        let num = flagged_count(cur, fields, BINF_ENCLS, "parentheses count", 8)?;
        for _ in 0..num {
            let left = self.tag_hash(cur, "left parenthesis")?;
            let right = self.tag_hash(cur, "right parenthesis")?;
            self.grammar.parentheses.insert(left, right);
            self.grammar.parentheses_reverse.insert(right, left);
        }
        let num = flagged_count(cur, fields, BINF_ANCHORS, "anchor count", 8)?;
        for _ in 0..num {
            let name = self.tag_hash(cur, "anchor name")?;
            let at = cur.offset();
            let position: u32 = cur.be("anchor position")?;
            if load
                .last_anchor
                .is_none_or(|(highest, _)| position > highest)
            {
                load.last_anchor = Some((position, at));
            }
            self.grammar.anchors.insert((name, position));
        }
        Ok(())
    }

    /// The set table, then the varstring sets the tags were waiting on.
    pub(super) fn read_sets(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        fields: u32,
        load: &mut Load,
    ) -> Result<(), GrammarError> {
        let num = flagged_count(cur, fields, BINF_SETS, "set count", MIN_SET_RECORD)?;
        load.num_sets = num;
        load.set_at = vec![UNCLAIMED; num as usize];
        // sets_list.resize(num_sets): pre-allocate `num_sets` slots (each
        // registered in sets_all, like the loop's allocateSet()).
        for _ in 0..num {
            self.grammar.allocate_set();
        }
        for _ in 0..num {
            let at = cur.offset();
            let s = set_record(cur, load)?;
            let number = s.number.get();
            claim(&mut load.set_at, number, at, "set number")?;
            self.grammar.sets_list[number] = s; // sets_list[s->number] = s
        }
        // The dense sets_list vector: the reader stores each set at its own
        // (dense) number, so slot == number and the order is the identity.
        self.grammar.sets_list_order = (0..num).map(SetId).collect();
        self.resolve_tag_varsets(load)?;
        checks::set_cycles(&self.grammar, load)
    }

    /// Resolve deferred varstring-tag sets now that sets are loaded, in tag
    /// order so which bad number is reported does not depend on hashing.
    fn resolve_tag_varsets(&mut self, load: &Load) -> Result<(), GrammarError> {
        let mut tags: Vec<u32> = load.tag_varsets.keys().copied().collect();
        tags.sort_unstable();
        for tagnum in tags {
            let at = load
                .tag_at
                .get(tagnum as usize)
                .copied()
                .unwrap_or_default();
            for &num in &load.tag_varsets[&tagnum] {
                check_set_number("varstring set", SetNumber(num), load.num_sets)
                    .map_err(|f| malformed(at, f))?;
                let t = self.grammar.single_tags_list.get_mut(tagnum);
                t.vs_sets.get_or_insert_default().push(SetId(num));
            }
        }
        Ok(())
    }

    /// The three delimiter set numbers.
    pub(super) fn read_delimiters(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        fields: u32,
        load: &Load,
    ) -> Result<(), GrammarError> {
        if fields & BINF_DELIMS != 0 {
            let n = cur.index("DELIMITERS set", load.num_sets)?;
            self.grammar.delimiters = Some(SetId(n));
        }
        if fields & BINF_SOFT_DELIMS != 0 {
            let n = cur.index("SOFT-DELIMITERS set", load.num_sets)?;
            self.grammar.soft_delimiters = Some(SetId(n));
        }
        if fields & BINF_TEXT_DELIMS != 0 {
            let n = cur.index("TEXT-DELIMITERS set", load.num_sets)?;
            self.grammar.text_delimiters = Some(SetId(n));
        }
        Ok(())
    }

    /// The contextual test table, keyed by hash: one test per hash.
    pub(super) fn read_contexts(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        fields: u32,
        load: &mut Load,
    ) -> Result<(), GrammarError> {
        let num = flagged_count(
            cur,
            fields,
            BINF_CONTEXTS,
            "contextual test count",
            MIN_CONTEXT_RECORD,
        )?;
        for _ in 0..num {
            let at = cur.offset();
            let t = self.read_contextual_test(cur, load.num_sets)?;
            let hash = self.grammar.contexts_arena[t.0].hash;
            if self.grammar.contexts.contains_key(&hash) {
                let fault = BinaryFault::Duplicate {
                    what: "contextual test hash",
                    value: hash,
                };
                return Err(malformed(at, fault));
            }
            load.ctx_at.insert(t, at);
            self.grammar.contexts.insert(hash, t);
        }
        Ok(())
    }

    /// The contextual test a hash read at `at` names.
    fn context_named(
        &self,
        hash: u32,
        what: &'static str,
        at: usize,
    ) -> Result<CtxId, GrammarError> {
        let found = self.grammar.contexts.get(&hash).copied();
        found.ok_or_else(|| malformed(at, BinaryFault::UnknownContext { what, hash }))
    }

    /// A contextual test hash, which must name a test already read.
    pub(super) fn context_by_hash(
        &self,
        cur: &mut Cg3bCursor<'_>,
        what: &'static str,
    ) -> Result<CtxId, GrammarError> {
        let at = cur.offset();
        let hash: u32 = cur.be(what)?;
        self.context_named(hash, what, at)
    }

    // [spec:cg3:req:robustness.binary-grammar-validated]
    /// What a contextual test needs besides its links: a hash to be filed
    /// under, set numbers that name sets, and a relation that names a tag.
    pub(super) fn check_context(
        &self,
        ct: &ContextualTest,
        num_sets: u32,
    ) -> Result<(), BinaryFault> {
        if ct.hash == 0 {
            return Err(BinaryFault::ContextWithoutHash);
        }
        check_set_number("test target set", ct.target, num_sets)?;
        check_set_number("BARRIER set", ct.barrier, num_sets)?;
        check_set_number("CBARRIER set", ct.cbarrier, num_sets)?;
        let relational = ct.relation != 0 || ct.pos.intersects(POS_RELATION);
        if relational && !self.grammar.tags_by_hash.contains(ct.relation) {
            let (what, hash) = ("test relation", ct.relation);
            return Err(BinaryFault::UnknownTag { what, hash });
        }
        Ok(())
    }

    /// The rule table: each rule placed at its own number, which must be
    /// unique.
    pub(super) fn read_rules(
        &mut self,
        cur: &mut Cg3bCursor<'_>,
        fields: u32,
        load: &mut Load,
    ) -> Result<(), GrammarError> {
        let num = flagged_count(cur, fields, BINF_RULES, "rule count", MIN_RULE_RECORD)?;
        load.num_rules = num;
        if let Some((position, at)) = load.last_anchor
            && position > num
        {
            let (value, limit) = (position.into(), u64::from(num) + 1);
            let what = "anchor position";
            return Err(malformed(
                at,
                BinaryFault::OutOfRange { what, value, limit },
            ));
        }
        load.rule_at = vec![UNCLAIMED; num as usize];
        // rule_by_number.resize(num_rules): pre-allocate `num_rules` slots.
        for _ in 0..num {
            self.grammar.rule_by_number.alloc(Rule::default());
        }
        for _ in 0..num {
            let at = cur.offset();
            let r = self.read_rule_record(cur, load)?;
            claim(&mut load.rule_at, r.number, at, "rule number")?;
            let number = r.number;
            self.grammar.rule_by_number[number] = r; // rule_by_number[r->number] = r
        }
        checks::rule_cycles(&self.grammar, load)
    }

    /// One rule record: the `rfields` bitmap, the fields it advertises, then
    /// the dependency target, the two test lists and the sub-rules.
    fn read_rule_record(
        &self,
        cur: &mut Cg3bCursor<'_>,
        load: &Load,
    ) -> Result<Rule, GrammarError> {
        let at = cur.offset();
        let mut r = Rule::default(); // allocateRule()
        let rfields: u32 = cur.be("rule field mask")?;
        rule_head(cur, rfields, &mut r, max_section(load.num_rules))?;
        rule_refs(cur, rfields, &mut r, load)?;
        self.rule_tests(cur, rfields, &mut r, load.num_rules)?;
        self.check_rule(&r, load.num_sets)
            .map_err(|f| malformed(at, f))?;
        self.filter_by_name(&mut r);
        Ok(r)
    }

    /// The dependency target, the dependency and contextual test lists (each
    /// hash naming a test already read) and the `WITH` sub-rules.
    fn rule_tests(
        &self,
        cur: &mut Cg3bCursor<'_>,
        rfields: u32,
        r: &mut Rule,
        num_rules: u32,
    ) -> Result<(), GrammarError> {
        // dep_target: contexts[hash] (inline; only when nonzero).
        let at = cur.offset();
        let dep: u32 = cur.be("dependency target hash")?;
        if dep != 0 {
            r.dep_target = Some(self.context_named(dep, "dependency target", at)?);
        }
        let num = cur.count("dependency test count", 4)?;
        for _ in 0..num {
            let ctx = self.context_by_hash(cur, "dependency test")?;
            Rule::add_contextual_test(ctx, &mut r.dep_tests);
        }
        let num = cur.count("contextual test count", 4)?;
        for _ in 0..num {
            let ctx = self.context_by_hash(cur, "contextual test")?;
            Rule::add_contextual_test(ctx, &mut r.tests);
        }
        if rfields & (1 << 15) != 0 {
            let num = cur.count("sub-rule count", 4)?;
            for _ in 0..num {
                r.sub_rules.push(RuleId(cur.index("sub-rule", num_rules)?)); // rule_by_number[u32]
            }
        }
        Ok(())
    }

    // [spec:cg3:req:robustness.binary-grammar-validated]
    /// What a rule needs of the tables around it: set numbers that name sets,
    /// an `EXTERNAL` command (or any variable name) that names a tag, and the
    /// tag list `SUBSTITUTE` and `EXECUTE` act on.
    fn check_rule(&self, r: &Rule, num_sets: u32) -> Result<(), BinaryFault> {
        check_set_number("rule target set", r.target, num_sets)?;
        check_set_number("rule child set", r.childset1, num_sets)?;
        check_set_number("rule child set", r.childset2, num_sets)?;
        let external = matches!(
            r.r#type,
            Keywords::KExternalOnce | Keywords::KExternalAlways
        );
        if (external || r.varname != 0) && !self.grammar.tags_by_hash.contains(r.varname) {
            let (what, hash) = ("rule variable name", r.varname);
            return Err(BinaryFault::UnknownTag { what, hash });
        }
        let substitutes = matches!(r.r#type, Keywords::KSubstitute | Keywords::KExecute);
        if substitutes && r.sublist.is_none() {
            return Err(BinaryFault::MissingSublist { rule: r.number });
        }
        Ok(())
    }

    /// --nrules / --nrules-inv name filters (K_IGNORE the rule).
    fn filter_by_name(&self, r: &mut Rule) {
        if let Some(re) = &self.nrules
            && !re.is_match(&r.name)
        {
            r.r#type = Keywords::KIgnore;
        }
        if let Some(re) = &self.nrules_inv
            && re.is_match(&r.name)
        {
            r.r#type = Keywords::KIgnore;
        }
    }

    /// Bind the template and OR references deferred while the tests were
    /// read, then refuse any cycle they close and any `?` position left
    /// without an override. In test order, so which bad reference is
    /// reported does not depend on hashing.
    pub(super) fn bind_deferred_tests(&mut self, load: &Load) -> Result<(), GrammarError> {
        let mut tmpls: Vec<(CtxId, u32)> =
            self.deferred_tmpls.iter().map(|(&k, &v)| (k, v)).collect();
        tmpls.sort_unstable();
        for (t, hash) in tmpls {
            let ctx = self.context_named(hash, "template", load.ctx_offset(t))?;
            self.grammar.contexts_arena[t.0].tmpl = Some(ctx);
        }
        let mut ors_list: Vec<(CtxId, Vec<u32>)> = self
            .deferred_ors
            .iter()
            .map(|(&k, v)| (k, v.clone()))
            .collect();
        ors_list.sort_unstable();
        for (t, hashes) in ors_list {
            let mut resolved = Vec::with_capacity(hashes.len());
            for h in hashes {
                resolved.push(self.context_named(h, "OR'd test", load.ctx_offset(t))?);
            }
            self.grammar.contexts_arena[t.0].ors.extend(resolved);
        }
        checks::context_cycles(&self.grammar, load)?;
        checks::unknown_positions(&self.grammar, load)
    }
}

/// Rule bits 0-4: section, type, line, flags and name.
fn rule_head(
    cur: &mut Cg3bCursor<'_>,
    rfields: u32,
    r: &mut Rule,
    max: i32,
) -> Result<(), GrammarError> {
    if rfields & (1 << 0) != 0 {
        let at = cur.offset();
        r.section = cur.be("rule section")?;
        if !(-3..=max).contains(&r.section) {
            let fault = BinaryFault::Section {
                section: r.section,
                max,
            };
            return Err(malformed(at, fault));
        }
    }
    if rfields & (1 << 1) != 0 {
        r.r#type = enumerator(cur, "rule type", &KEYWORDS_BY_ID)?;
    }
    if rfields & (1 << 2) != 0 {
        r.line = cur.be("rule line")?;
    }
    if rfields & (1 << 3) != 0 {
        let bits = if rfields & (1 << 16) != 0 {
            cur.be::<u64>("rule flags")?
        } else {
            u64::from(cur.be::<u32>("rule flags")?)
        };
        r.flags = crate::rule::RuleFlags::from_bits_retain(bits);
    }
    if rfields & (1 << 4) != 0 {
        let name = cur.text("rule name")?;
        if !name.is_empty() {
            r.set_name(Some(name.as_str()));
        }
    }
    Ok(())
}

/// Rule bits 5-14: the sets, tags and numbers a rule refers to. Set numbers
/// that default to 0 when absent are checked with the rest of the rule.
fn rule_refs(
    cur: &mut Cg3bCursor<'_>,
    rfields: u32,
    r: &mut Rule,
    load: &Load,
) -> Result<(), GrammarError> {
    if rfields & (1 << 5) != 0 {
        r.target = SetNumber(cur.be("rule target set")?);
    }
    if rfields & (1 << 6) != 0 {
        r.wordform = Some(TagId(cur.index("rule wordform tag", load.num_tags)?)); // single_tags_list[u32]
    }
    if rfields & (1 << 7) != 0 {
        r.varname = cur.hash("rule variable name")?;
    }
    if rfields & (1 << 8) != 0 {
        r.varvalue = cur.be("rule variable value")?;
    }
    if rfields & (1 << 9) != 0 {
        // Sign in bit 31, magnitude below it.
        let u: u32 = cur.be("rule sub-reading")?;
        let magnitude = (u & !(1u32 << 31)) as i32;
        r.sub_reading = if u & (1 << 31) != 0 {
            -magnitude
        } else {
            magnitude
        };
    }
    if rfields & (1 << 10) != 0 {
        r.childset1 = SetNumber(cur.be("rule child set")?);
    }
    if rfields & (1 << 11) != 0 {
        r.childset2 = SetNumber(cur.be("rule child set")?);
    }
    if rfields & (1 << 12) != 0 {
        r.maplist = Some(SetId(cur.index("rule tag list set", load.num_sets)?)); // sets_list[u32]
    }
    if rfields & (1 << 13) != 0 {
        r.sublist = Some(SetId(cur.index("rule sub-list set", load.num_sets)?));
    }
    if rfields & (1 << 14) != 0 {
        r.number = cur.index("rule number", load.num_rules)?;
    }
    Ok(())
}

/// A tag's stored union role, for the checks that need it by value.
pub(super) fn variable_hash_of(t: &Tag) -> Option<u32> {
    match t.extra {
        TagUnion::VariableHash(h) if h != 0 => Some(h),
        _ => None,
    }
}
