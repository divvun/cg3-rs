//! `doesSetMatchReading` over sets built from sets, without recursion
//! (`[spec:cg3:req:robustness.depth-bounded]`).
//!
//! A set built from sets — `SET S2 = S1 - Z`, a `&&`-unified set — is as deep
//! as the grammar makes it, and the C++ tests a reading against one by
//! recursing into its members. Here each such set part way through its members
//! is a [`SetFrame`] on a heap stack, driven by
//! [`Matcher::does_set_match_reading`], and each frame steps through its
//! members in exactly the order, and with exactly the short-circuits, of the
//! C++ loop it replaces.

use crate::arena::ReadingId;
use crate::error::RunError;
use crate::set::{ST_ANY, ST_SET_UNIFY, ST_TAG_UNIFY, SetType};
use crate::types::SetNumber;

use super::match_set::{S_FAILFAST, S_MINUS, S_OR, S_PLUS};
use super::{Matcher, UnifKey};

/// A set built from sets that a reading is being tested against, part way
/// through its members: what the C++ recursion keeps on the stack.
pub(super) struct SetFrame {
    /// The set's number.
    set: u32,
    stype: SetType,
    /// The `unif_mode` the set itself is tested in.
    unif_mode: bool,
    kind: FrameKind,
}

enum FrameKind {
    Operators(OperatorWalk),
    Unified(UnifiedWalk),
}

/// What a [`SetFrame`] needs next.
pub(super) enum SetStep {
    /// A member set's result, tested in the given `unif_mode`.
    Test(u32, bool),
    /// Nothing more: the set's own result.
    Done(bool),
}

/// Which operand an [`OperatorWalk`] is waiting for the result of.
#[derive(Clone, Copy)]
enum Awaiting {
    Nothing,
    /// The first member of a run of non-`OR` operators.
    First,
    /// The right operand of the operator at `i`.
    Plus,
    FailFast,
    Minus,
}

/// Case (d) of `doesSetMatchReading`, a SET: its members are combined left
/// to right, with every operator but `OR` binding tighter than `OR`.
struct OperatorWalk {
    sets: Vec<u32>,
    ops: Vec<u32>,
    /// The `unif_mode` the members are tested in: the set's own, or set by a
    /// tag-unified set.
    members_unif: bool,
    i: usize,
    m: bool,
    failfast: bool,
    /// Past the first member of the current run of operators.
    in_group: bool,
    awaiting: Awaiting,
}

impl OperatorWalk {
    /// Take the result of the member [`Self::step`] asked for.
    fn take(&mut self, matched: bool) {
        match std::mem::replace(&mut self.awaiting, Awaiting::Nothing) {
            Awaiting::First => {
                self.m = matched;
                self.failfast = false;
                self.in_group = true;
            }
            Awaiting::Plus => {
                self.m = matched;
                self.i += 1;
            }
            Awaiting::FailFast => {
                if matched {
                    self.m = false;
                    self.failfast = true;
                }
                self.i += 1;
            }
            Awaiting::Minus => {
                if matched {
                    self.m = false;
                }
                self.i += 1;
            }
            Awaiting::Nothing => {}
        }
    }

    /// The C++ loop, from where it left off to the next member it tests or
    /// to its end: `m` for each member not joined by `OR`, `+` testing its
    /// right operand only while `m` holds, `^` always, `-` only while `m`
    /// holds.
    fn step(&mut self) -> SetStep {
        let size = self.sets.len();
        loop {
            if !self.in_group {
                if self.i >= size {
                    return SetStep::Done(false);
                }
                self.awaiting = Awaiting::First;
                return SetStep::Test(self.sets[self.i], self.members_unif);
            }
            if self.i < size - 1 && self.ops[self.i] != S_OR {
                #[expect(
                    clippy::panic,
                    reason = "a set's operators between its members are only OR, +, - and ^: binary_grammar's set_shape refuses a .cg3b set with any other, the textual parser applies \\, ∩ and ∆ as soon as their right operand is read, and the relabeller joins with OR and +"
                )]
                let wants = match self.ops[self.i] {
                    x if x == S_PLUS => self.m.then_some(Awaiting::Plus),
                    x if x == S_FAILFAST => Some(Awaiting::FailFast),
                    x if x == S_MINUS => self.m.then_some(Awaiting::Minus),
                    _ => panic!("Set operator not implemented!"),
                };
                match wants {
                    Some(awaiting) => {
                        self.awaiting = awaiting;
                        return SetStep::Test(self.sets[self.i + 1], self.members_unif);
                    }
                    None => self.i += 1,
                }
                continue;
            }
            if self.m {
                return SetStep::Done(true);
            }
            if self.failfast {
                return SetStep::Done(false);
            }
            self.i += 1;
            self.in_group = false;
        }
    }
}

/// Case (c) of `doesSetMatchReading`, a `&&`-unified set: every one of its
/// sub-sets is tested, and it matches when any does.
struct UnifiedWalk {
    snumber: u32,
    /// Where the rule records which sub-sets matched; `None` with no rule in
    /// flight.
    usets_idx: Option<usize>,
    /// The sub-sets to test, by number.
    members: Vec<u32>,
    next: usize,
    members_unif: bool,
    /// Record each sub-set that matches: this is the set's first evaluation
    /// in the rule.
    record: bool,
    any: bool,
}

impl Matcher<'_> {
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn+1]
    /// The start of `doesSetMatchReading` for `set`: the yes/no memo, then the
    /// set itself when it is `(*)` or a LIST. A set built from sets goes onto
    /// `open` instead, to have its members tested first. Returns the result
    /// when it is known now.
    pub(super) fn set_match_begin(
        &mut self,
        open: &mut Vec<SetFrame>,
        reading: ReadingId,
        set: u32,
        bypass_index: bool,
        unif_mode: bool,
    ) -> Result<Option<bool>, RunError> {
        if !bypass_index && !unif_mode {
            let rhash = self.readings.get(reading.0).hash;
            if self.scratch.index_reading_set_no[set as usize].contains(rhash) {
                return Ok(Some(false));
            }
            if self.scratch.index_reading_set_yes[set as usize].contains(rhash) {
                return Ok(Some(true));
            }
        }

        let (stype, snumber, ssets_empty) = {
            let s = self.grammar.set_by_number(SetNumber(set)); // grammar->sets_list[set]
            (s.r#type, s.number.get(), s.sets.is_empty())
        };
        let tagunif = stype.intersects(ST_TAG_UNIFY);

        let retval = if stype.intersects(ST_ANY) {
            // (a) the (*) set
            true
        } else if ssets_empty {
            // (b) LIST set. `does_set_match_reading_tags` navigates the set's
            // `ff_tags`/`trie`/`trie_special` fresh from `self.grammar` at each
            // step (short borrows), so no grammar borrow aliases the `&mut self`
            // re-entry — the C++ `&kv` node identity is carried as an address-free
            // `UnifKey` (`(special, TagId path)`), leaving this case plain safe code.
            self.does_set_match_reading_tags(reading, snumber, tagunif || unif_mode)?
        } else {
            // (c) &&-unified set, or (d) SET set.
            let kind = if stype.intersects(ST_SET_UNIFY) {
                FrameKind::Unified(self.unified_walk(set, unif_mode, tagunif || unif_mode))
            } else {
                let s = self.grammar.set_by_number(SetNumber(set));
                FrameKind::Operators(OperatorWalk {
                    sets: s.sets.clone(),
                    ops: s.set_ops.clone(),
                    members_unif: tagunif || unif_mode,
                    i: 0,
                    m: false,
                    failfast: false,
                    in_group: false,
                    awaiting: Awaiting::Nothing,
                })
            };
            open.push(SetFrame {
                set,
                stype,
                unif_mode,
                kind,
            });
            return Ok(None);
        };
        self.set_match_remember(reading, set, stype, unif_mode, retval);
        Ok(Some(retval))
    }

    // [spec:cg3:req:robustness.accepted-grammars-run]
    /// Case (c) of [`Self::does_set_match_reading`], a `&&`-unified set: its
    /// first evaluation in a rule records each sub-set of `sets[0]` the reading
    /// matches (tested with `first_unif`), and later ones match only against the
    /// recorded sub-sets.
    ///
    /// DIVERGENCE: with no rule in flight there is no unification frame to
    /// record in — a `SET:` tag in DELIMITERS reaches one while the stream is
    /// read — and the set matches when any of its sub-sets does, recording
    /// nothing. The C++ read the back of an empty context stack.
    fn unified_walk(&self, set: u32, unif_mode: bool, first_unif: bool) -> UnifiedWalk {
        let snumber = self.grammar.set_by_number(SetNumber(set)).number.get();
        let usets_idx = self.scratch.context_stack.last().and_then(|f| f.unif_sets);
        let recorded: Vec<u32> = usets_idx
            .and_then(|i| self.scratch.unif_sets_store[i].get(&snumber))
            .map(|v| v.as_slice().to_vec())
            .unwrap_or_default();
        let (members, members_unif, record) = if !recorded.is_empty() {
            // Subsequent evaluations: test the previously-stored sets.
            (recorded, unif_mode, false)
        } else {
            // First evaluation: gather all matching sub-sets of sets[0].
            let sets0 = self.grammar.set_by_number(SetNumber(set)).sets[0];
            let uset_sets = &self.grammar.set_by_number(SetNumber(sets0)).sets;
            let tnums = uset_sets
                .iter()
                .map(|&t| self.grammar.set_by_number(SetNumber(t)).number.get())
                .collect();
            (tnums, first_unif, true)
        };
        UnifiedWalk {
            snumber,
            usets_idx,
            members,
            next: 0,
            members_unif,
            record,
            any: false,
        }
    }

    /// Hand a member's result to the set frame that asked for it.
    pub(super) fn set_frame_take(&mut self, frame: &mut SetFrame, matched: bool) {
        match &mut frame.kind {
            FrameKind::Operators(walk) => walk.take(matched),
            FrameKind::Unified(walk) => {
                if !matched {
                    return;
                }
                walk.any = true;
                if let (true, Some(i)) = (walk.record, walk.usets_idx) {
                    let tnum = walk.members[walk.next - 1];
                    self.scratch.unif_sets_store[i]
                        .entry(walk.snumber)
                        .or_default()
                        .insert(tnum);
                }
            }
        }
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn+1]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-reading-fn+1]
    /// The end of `doesSetMatchReading` for a set built from sets, once its
    /// members are tested: a SET spreads a unified tag across its members,
    /// and the result is remembered. Returns the result.
    pub(super) fn set_match_finish(
        &mut self,
        reading: ReadingId,
        frame: SetFrame,
        retval: bool,
    ) -> bool {
        // Propagate a unified tag across the set's members.
        if let FrameKind::Operators(walk) = &frame.kind
            && walk.members_unif
            && let Some(top) = self.scratch.context_stack.last()
        {
            #[expect(
                clippy::unwrap_used,
                reason = "a set is matched under a context frame only while run_single_rule_body matches a reading, after giving the frame its unif_tags and unif_sets indices (fresh, or from the plain-signature cache), or while an action runs under a saved copy of such a frame"
            )]
            let ut_idx = top.unif_tags.unwrap();
            let ut = &mut self.scratch.unif_tags_store[ut_idx];
            let tag: Option<UnifKey> = walk.sets.iter().find_map(|s| ut.get(s).cloned());
            if let Some(t) = tag {
                for &s in &walk.sets {
                    ut.insert(s, t.clone());
                }
            }
        }
        self.set_match_remember(reading, frame.set, frame.stype, frame.unif_mode, retval);
        retval
    }

    /// Cache a set's result for the reading: a match always, a mismatch
    /// only when neither the set nor the test unifies.
    fn set_match_remember(
        &mut self,
        reading: ReadingId,
        set: u32,
        stype: SetType,
        unif_mode: bool,
        retval: bool,
    ) {
        if retval {
            let rhash = self.readings.get(reading.0).hash;
            self.scratch.index_reading_set_yes[set as usize].insert(rhash);
        } else if !stype.intersects(ST_TAG_UNIFY) && !unif_mode {
            let rhash = self.readings.get(reading.0).hash;
            self.scratch.index_reading_set_no[set as usize].insert(rhash);
        }
    }
}

impl SetFrame {
    /// What the frame needs next: a member's result, or nothing more.
    pub(super) fn step(&mut self) -> SetStep {
        match &mut self.kind {
            FrameKind::Operators(walk) => walk.step(),
            FrameKind::Unified(walk) => match walk.members.get(walk.next) {
                Some(&member) => {
                    walk.next += 1;
                    SetStep::Test(member, walk.members_unif)
                }
                None => SetStep::Done(walk.any),
            },
        }
    }
}
