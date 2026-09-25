//! The `.cg3b` checks that need a whole table, or more than one record:
//! cycles among sets, contextual tests and rules, `?` positions a run would
//! reach bare, tag hashes that are not the tag's own, variable values that
//! name no tag, and runs of hashes the tag interner could not probe past.
//!
//! A textual grammar cannot express most of these — a `WITH` block cannot
//! contain itself — so the C++ never checks them, and the reindexing, writing
//! and matching that recurse over these structures would loop without end.

use std::collections::{HashMap, HashSet};

use super::Load;
use super::cursor::{is_reserved_hash, malformed};
use super::read::variable_hash_of;
use crate::arena::CtxId;
use crate::contextual_test::{POS_TMPL_OVERRIDE, POS_UNKNOWN};
use crate::error::{BinaryFault, GrammarError};
use crate::grammar::GrammarNumbered;
use crate::tag::Tag;

/// A node on a cycle in the graph over `0..n` whose edges `succ` gives, or
/// `None` if there is no cycle. Walks with an explicit stack: the graphs come
/// from the file, and a long acyclic chain must not exhaust the call stack.
fn find_cycle(n: usize, succ: impl Fn(usize) -> Vec<usize>) -> Option<usize> {
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![0u8; n];
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        state[root] = OPEN;
        let mut stack = vec![(root, succ(root), 0usize)];
        while let Some((node, next, i)) = stack.last_mut() {
            let Some(&child) = next.get(*i) else {
                state[*node] = DONE;
                stack.pop();
                continue;
            };
            *i += 1;
            match state[child] {
                OPEN => return Some(child),
                DONE => {}
                _ => {
                    state[child] = OPEN;
                    let edges = succ(child);
                    stack.push((child, edges, 0));
                }
            }
        }
    }
    None
}

// [spec:cg3:req:robustness.binary-grammar-validated]
/// A set that contains itself, directly or through its member sets.
pub(super) fn set_cycles(grammar: &GrammarNumbered, load: &Load) -> Result<(), GrammarError> {
    let members = |i: usize| {
        grammar.sets_list[i as u32]
            .sets
            .iter()
            .map(|&s| s as usize)
            .collect()
    };
    match find_cycle(load.num_sets as usize, members) {
        Some(i) => Err(malformed(
            load.set_at[i],
            BinaryFault::SetCycle { set: i as u32 },
        )),
        None => Ok(()),
    }
}

// [spec:cg3:req:robustness.binary-grammar-validated]
/// A contextual test that reaches itself through its OR'd tests and `LINK`s
/// alone. Grammar source writes both inline, so they only ever nest; a cycle
/// through a template reference is another matter — the `T_Templates` fixture
/// has an `OR` alternative naming its own template, which the C++ compiles
/// and runs — so template edges are not followed here.
pub(super) fn context_cycles(grammar: &GrammarNumbered, load: &Load) -> Result<(), GrammarError> {
    let ids: Vec<CtxId> = grammar.contexts.values().copied().collect();
    let index: HashMap<CtxId, usize> = ids.iter().enumerate().map(|(i, &c)| (c, i)).collect();
    let links = |i: usize| {
        let ct = &grammar.contexts_arena[ids[i].0];
        let refs = ct.ors.iter().chain(ct.linked.iter());
        refs.filter_map(|c| index.get(c).copied()).collect()
    };
    let Some(i) = find_cycle(ids.len(), links) else {
        return Ok(());
    };
    let ct = &grammar.contexts_arena[ids[i].0];
    let fault = BinaryFault::ContextCycle {
        hash: ct.hash,
        line: ct.line,
    };
    Err(malformed(load.ctx_offset(ids[i]), fault))
}

// [spec:cg3:req:robustness.binary-grammar-validated]
/// A `?` position a run would reach with no template override to stand in
/// for it, which the run refuses when it gets there (the C++ quits). Grammar
/// source writes `?` only where an override will supply the position. A test
/// is reached without one from a rule or through a `LINK`, and through the
/// template or OR'd tests of a test so reached that carries no override
/// itself; everything else a run reaches, it reaches overridden.
pub(super) fn unknown_positions(
    grammar: &GrammarNumbered,
    load: &Load,
) -> Result<(), GrammarError> {
    let mut pending: Vec<CtxId> = Vec::new();
    for number in 0..load.num_rules {
        let r = &grammar.rule_by_number[number];
        pending.extend(r.tests.iter().chain(&r.dep_tests).chain(&r.dep_target));
    }
    let links = grammar
        .contexts
        .values()
        .filter_map(|c| grammar.contexts_arena[c.0].linked);
    pending.extend(links);
    let mut plain: HashSet<CtxId> = HashSet::new();
    while let Some(c) = pending.pop() {
        if !plain.insert(c) {
            continue;
        }
        let ct = &grammar.contexts_arena[c.0];
        if ct.pos.intersects(POS_UNKNOWN) {
            let fault = BinaryFault::UnknownPosition {
                hash: ct.hash,
                line: ct.line,
            };
            return Err(malformed(load.ctx_offset(c), fault));
        }
        if !ct.pos.intersects(POS_TMPL_OVERRIDE) {
            pending.extend(ct.tmpl.iter().chain(&ct.ors));
        }
    }
    Ok(())
}

// [spec:cg3:req:robustness.binary-grammar-validated]
/// A rule that is among its own `WITH` sub-rules, directly or further down.
pub(super) fn rule_cycles(grammar: &GrammarNumbered, load: &Load) -> Result<(), GrammarError> {
    let subs = |i: usize| {
        let rule = &grammar.rule_by_number[i as u32];
        rule.sub_rules.iter().map(|r| r.0 as usize).collect()
    };
    match find_cycle(load.num_rules as usize, subs) {
        Some(i) => Err(malformed(
            load.rule_at[i],
            BinaryFault::RuleCycle { rule: i as u32 },
        )),
        None => Ok(()),
    }
}

// [spec:cg3:req:robustness.binary-grammar-validated]
/// The tag table as a whole: every tag stores the hashes its text, type and
/// seed give it, and every variable value names a tag.
pub(super) fn tag_hashes(grammar: &GrammarNumbered, load: &Load) -> Result<(), GrammarError> {
    for number in 0..load.num_tags {
        let t = &grammar.single_tags_list[number];
        let at = load.tag_at[number as usize];
        stored_hashes(t).map_err(|fault| malformed(at, fault))?;
        if let Some(hash) = variable_hash_of(t)
            && (is_reserved_hash(hash) || !grammar.tags_by_hash.contains(hash))
        {
            let what = "tag variable value";
            return Err(malformed(at, BinaryFault::UnknownTag { what, hash }));
        }
    }
    Ok(())
}

/// A tag record's hashes against the ones [`Tag::rehash`] gives its text,
/// type and seed. A run finds tags by recomputing them, so a stored hash that
/// differs splits one tag in two: a varstring that expands to itself, a tag
/// no reading can carry.
fn stored_hashes(t: &Tag) -> Result<(), BinaryFault> {
    let mut probe = Tag {
        r#type: t.r#type,
        tag: t.tag.clone(),
        seed: t.seed,
        ..Tag::default()
    };
    let hash = probe.rehash();
    let (what, stored, computed) = if hash != t.hash {
        ("hash", t.hash, hash)
    } else if probe.plain_hash != t.plain_hash {
        ("plain hash", t.plain_hash, probe.plain_hash)
    } else {
        return Ok(());
    };
    let (stored, computed) = (stored.get(), computed.get());
    Err(BinaryFault::TagHash {
        tag: t.number,
        what,
        stored,
        computed,
    })
}
