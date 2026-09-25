//! Keeping the engine's recursion off the depth of its input
//! (`[spec:cg3:req:robustness.depth-bounded]`).
//!
//! Contextual tests, templates and `WITH` sub-rules evaluate one another by
//! recursion, as in the C++. Each such recursion goes one level deeper through
//! here, and one that would pass [`MAX_NESTING`] ends the run with an error
//! naming the rule, rather than running the stack out. Everything else the
//! engine walks — sub-reading chains, dependency chains, set chains, tries —
//! is walked without recursing, where it is walked.

use crate::arena::{CohortId, CtxId, ReadingId, RuleId, SwId, TagId};
use crate::contextual_test::TestRef;
use crate::error::{Nesting, RunError};
use crate::nesting::MAX_NESTING;
use crate::set::{ST_SET_UNIFY, ST_TAG_UNIFY, Set};
use crate::tag::{T_FAILFAST, T_SET, T_SPECIAL, T_VARSTRING, Tag, TagList, TagType};
use crate::types::SetNumber;

use super::run_rules::RRState;
use super::{Engine, Matcher};

/// How many times a varstring that the tag matcher meets is re-expanded while
/// its expansion is another varstring — the bound
/// `Engine::expand_varstring` stops at when a rule adds one.
const MAX_VARSTRING_EXPANSIONS: usize = 16;

impl Matcher<'_> {
    // [spec:cg3:req:robustness.depth-bounded]
    /// Go a level deeper for `what`, or stop the run with the rule in flight
    /// named if that would pass [`MAX_NESTING`].
    ///
    /// DIVERGENCE: the C++ nests contextual tests, templates and `WITH`
    /// sub-rules until its stack runs out.
    pub(super) fn enter_nesting(&mut self, what: Nesting) -> Result<(), RunError> {
        if self.scratch.nesting >= MAX_NESTING {
            let line = self
                .scratch
                .current_rule
                .map_or(0, |r| self.grammar.rule_by_number[r.0].line);
            return Err(RunError::NestingTooDeep {
                what,
                line,
                limit: MAX_NESTING,
            });
        }
        self.scratch.nesting += 1;
        Ok(())
    }

    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-set-match-cohort-test-linked-fn+1]
    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-fn+2]
    // [spec:cg3:req:robustness.depth-bounded]
    /// `runContextualTest` for a test reached from the one being evaluated —
    /// its `LINK` — one level deeper.
    pub(super) fn run_linked_test(
        &mut self,
        sw: Option<SwId>,
        position: u32,
        test: TestRef,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<Option<CohortId>, RunError> {
        self.enter_nesting(Nesting::Link)?;
        let found = self.run_contextual_test(sw, position, test, deep, origin);
        self.scratch.nesting -= 1;
        found
    }

    // [spec:cg3:def:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    // [spec:cg3:sem:grammar-applicator-run-contextual-test.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-contextual-test-tmpl-fn+1]
    // [spec:cg3:req:robustness.depth-bounded]
    /// `runContextualTest` for `test`, the template the test `outer` names or
    /// one of its `OR` alternatives, one level deeper. A template that
    /// recurses through a later alternative or a `LINK` is legitimate
    /// (`[spec:cg3:req:robustness.cycles+1]`), and it is here that one which
    /// never stops is stopped.
    pub(super) fn run_template_test(
        &mut self,
        outer: CtxId,
        sw: Option<SwId>,
        position: u32,
        test: TestRef,
        deep: Option<&mut Option<CohortId>>,
        origin: Option<CohortId>,
    ) -> Result<Option<CohortId>, RunError> {
        let what = if self.grammar.contexts_arena[outer.0].tmpl == Some(test.id) {
            Nesting::Template
        } else {
            Nesting::InlineTemplate
        };
        self.enter_nesting(what)?;
        let found = self.run_contextual_test(sw, position, test, deep, origin);
        self.scratch.nesting -= 1;
        found
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:req:robustness.depth-bounded]
    /// `doesSetMatchReading` for the set a `SET:` tag names, one level deeper:
    /// the named set can hold a `SET:` tag in turn, and those references,
    /// like templates, are followed by recursion.
    pub(super) fn does_named_set_match(
        &mut self,
        reading: crate::arena::ReadingId,
        set: u32,
        bypass_index: bool,
        unif_mode: bool,
    ) -> Result<bool, RunError> {
        self.enter_nesting(Nesting::SetTag)?;
        let matched = self.does_set_match_reading(reading, set, bypass_index, unif_mode);
        self.scratch.nesting -= 1;
        matched
    }

    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:def:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:sem:grammar-applicator-match-set.cg3.grammar-applicator.does-tag-match-reading-fn+2]
    // [spec:cg3:req:robustness.depth-bounded]
    /// The tag a varstring stands for when it is matched against a reading:
    /// its expansion, expanded again while that is a varstring the tag matcher
    /// would expand in turn.
    ///
    /// DIVERGENCE: the C++ recurses into the tag matcher for each expansion,
    /// and captured text reading `VSTR:$1` makes that recursion endless. This
    /// expands in a loop, and a varstring still expanding into another after
    /// [`MAX_VARSTRING_EXPANSIONS`] is the same run error a rule adding one
    /// gets.
    pub(super) fn expand_matched_varstring(
        &mut self,
        tag_id: TagId,
        tag: &Tag,
    ) -> Result<TagId, RunError> {
        let mut expanded = self.generate_varstring_tag(tag_id, tag)?;
        for _ in 0..MAX_VARSTRING_EXPANSIONS {
            if !expands_when_matched(self.grammar.tag_type(expanded)) {
                return Ok(expanded);
            }
            let again = self.grammar.single_tags_list[expanded.0].clone();
            expanded = self.generate_varstring_tag(expanded, &again)?;
        }
        if !expands_when_matched(self.grammar.tag_type(expanded)) {
            return Ok(expanded);
        }
        let line = self
            .scratch
            .current_rule
            .map_or(0, |r| self.grammar.rule_by_number[r.0].line);
        Err(RunError::VarstringLoop {
            tag: self.grammar.single_tags_list[tag_id.0].to_text(false),
            line,
        })
    }
}

/// `getTagList`'s last step: remove each run of equal tags but its first —
/// consecutive repeats only; repeats apart are kept, for `ADDCOHORT` and
/// `APPEND` repeating tags across readings.
pub(crate) fn collapse_repeated_tags(the_tags: &mut TagList) {
    let mut oti = 0usize;
    while the_tags.len() > 1 && oti < the_tags.len() {
        let mut it = oti + 1;
        while it < the_tags.len() && it - oti == 1 {
            if the_tags[oti] == the_tags[it] {
                the_tags.remove(it);
            } else {
                it += 1;
            }
        }
        oti += 1;
    }
}

/// Whether `does_tag_match_reading` takes a tag of type `ty` down its
/// varstring branch: not a plain or fail-fast tag, and not a `SET:` tag,
/// which the branches before it claim.
fn expands_when_matched(ty: TagType) -> bool {
    ty.intersects(T_SPECIAL)
        && !ty.intersects(T_FAILFAST)
        && !ty.intersects(T_SET)
        && ty.intersects(T_VARSTRING)
}

impl Engine<'_> {
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.print-reading-fn]
    // [spec:cg3:req:robustness.depth-bounded]
    /// C++ `printReading`: `reading` and, a tab deeper each, its sub-readings.
    /// The C++ recurses once per sub-reading; this prints them in a loop, each
    /// through [`Self::print_reading`], which also says where the chain stops.
    pub fn print_reading_chain<W: std::io::Write>(
        &mut self,
        reading: ReadingId,
        output: &mut W,
        sub: usize,
        trace: bool,
    ) {
        let mut next = Some(reading);
        let mut sub = sub;
        while let Some(r) = next {
            next = self.print_reading(r, output, sub, trace);
            sub += 1;
        }
    }

    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn+2]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.run-rules-on-single-window-fn+2]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn+2]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.run-rules-on-single-window-fn+2]
    // [spec:cg3:req:robustness.depth-bounded]
    /// `runSingleRule` for a sub-rule of the `WITH` rule being run, one level
    /// deeper.
    pub(super) fn run_nested_rule(
        &mut self,
        current: SwId,
        rule: RuleId,
        st: &mut RRState,
    ) -> Result<bool, RunError> {
        self.matcher().enter_nesting(Nesting::With)?;
        let applied = self.run_single_rule(current, rule, st);
        self.scratch.nesting -= 1;
        applied
    }
}

impl Matcher<'_> {
    // [spec:cg3:def:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:sem:grammar-applicator-run-rules.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:def:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
    // [spec:cg3:sem:grammar-applicator.cg3.grammar-applicator.get-tag-list-fn]
    /// One set of `getTagList`'s walk: append a set of tags' tags to
    /// `the_tags`, or push the member sets of a set built from sets onto
    /// `todo`, in order, each with the unification mode it is taken in.
    pub(super) fn tag_list_step<'s>(
        &'s self,
        the_set: &'s Set,
        the_tags: &mut TagList,
        unif_mode: bool,
        todo: &mut Vec<(&'s Set, bool)>,
    ) {
        if the_set.r#type.intersects(ST_SET_UNIFY) {
            // usets = (*context_stack.back().unif_sets)[theSet.number]
            #[expect(
                clippy::unwrap_used,
                reason = "tag lists are expanded only inside a rule (its actions, or a varstring tag it matches or adds), and run_single_rule_body gives the frame it pushes unif indices before matching a reading or running a tag-list action on it"
            )]
            let unif_sets = self
                .scratch
                .context_stack
                .last()
                .unwrap()
                .unif_sets
                .unwrap();
            let usets = self.scratch.unif_sets_store[unif_sets].get(&the_set.number.get());
            let p_set = self.grammar.set_by_number(SetNumber(the_set.sets[0]));
            for &iter in &p_set.sets {
                let present = usets.map(|s| s.count(iter) != 0).unwrap_or(false);
                if present {
                    todo.push((self.grammar.set_by_number(SetNumber(iter)), false));
                }
            }
        } else if the_set.r#type.intersects(ST_TAG_UNIFY) {
            for &iter in &the_set.sets {
                todo.push((self.grammar.set_by_number(SetNumber(iter)), true));
            }
        } else if !the_set.sets.is_empty() {
            for &iter in &the_set.sets {
                todo.push((self.grammar.set_by_number(SetNumber(iter)), unif_mode));
            }
        } else if unif_mode {
            #[expect(
                clippy::unwrap_used,
                reason = "tag lists are expanded only inside a rule (its actions, or a varstring tag it matches or adds), and run_single_rule_body gives the frame it pushes unif indices before matching a reading or running a tag-list action on it"
            )]
            let unif_tags = self
                .scratch
                .context_stack
                .last()
                .unwrap()
                .unif_tags
                .unwrap();
            let val = self.scratch.unif_tags_store[unif_tags]
                .get(&the_set.number.get())
                .cloned();
            // C++ `trie_getTagList(trie, theTags, node)` / `(trie_special, ...)`
            // walk both tries by the recorded node ADDRESS, appending the
            // reconstructed root-to-node tag path. The address-free `UnifKey`
            // ALREADY carries that path (in root-to-node order — exactly what the
            // successful DFS branch pushes) plus which trie it lives in, so the
            // walk collapses to appending `key.path` to `the_tags`.
            if let Some(key) = val {
                for tid in key.path {
                    the_tags.push(tid);
                }
            }
        } else {
            crate::tag_trie::trie_get_tag_list_append(&the_set.trie, the_tags, self.grammar);
            crate::tag_trie::trie_get_tag_list_append(
                &the_set.trie_special,
                the_tags,
                self.grammar,
            );
        }
    }
}
