//! `Grammar` — the pieces of the walks over sets built from sets and over
//! contextual tests that the C++ writes as recursions
//! (`[spec:cg3:req:robustness.depth-bounded]`).
//!
//! A set built from sets and a `LINK` chain read from a `.cg3b` are as deep as
//! the input makes them. The walks in `grammar/mod.rs` keep the sets or tests
//! still to visit on a heap stack; these are the steps they take at each one.

use std::collections::BTreeMap;

use crate::arena::{CtxId, SetId};
use crate::set::{ST_USED, Set};
use crate::tag::{T_FAILFAST, TagVector, TagVectorSet, fill_tagvector};
use crate::tag_trie::{trie_get_tags, trie_get_tags_into, trie_insert};
use crate::types::SetNumber;

use super::{Draft, GrammarCore, Numbering, STR_GPREFIX};

/// A set built from sets that [`GrammarCore::remove_numeric_tags`] is
/// stripping: its members, rewritten as they are stripped, the index of the
/// next one to strip, and whether any has changed.
pub(super) struct NumericStrip {
    pub(super) set: SetId,
    pub(super) sets: Vec<u32>,
    pub(super) next: usize,
    pub(super) did: bool,
}

impl GrammarCore<Draft> {
    // [spec:cg3:def:grammar.cg3.grammar.remove-numeric-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn]
    /// Begin stripping the set `s` names: a set built from sets goes onto
    /// `open` to have its members stripped first; any other is stripped now,
    /// and its result returned.
    pub(super) fn strip_numeric_enter(
        &mut self,
        open: &mut Vec<NumericStrip>,
        s: u32,
    ) -> Result<Option<u32>, crate::error::ParseError> {
        #[expect(
            clippy::unwrap_used,
            reason = "a test's target and a set's members are the hashes of sets add_set registered in sets_by_contents, and the parser strips them while it reads the grammar, before reindex empties that map"
        )]
        let set = self.get_set(s).unwrap();
        if self.sets_list[set.0].sets.is_empty() {
            return self.strip_numeric_leaf(set).map(Some);
        }
        open.push(NumericStrip {
            set,
            sets: self.sets_list[set.0].sets.clone(),
            next: 0,
            did: false,
        });
        Ok(None)
    }

    // [spec:cg3:def:grammar.cg3.grammar.remove-numeric-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn]
    /// Finish stripping a set built from sets once its members are stripped:
    /// a new set of the stripped members when any changed, the set itself
    /// otherwise. Returns the resulting set's hash.
    pub(super) fn strip_numeric_composite(
        &mut self,
        strip: NumericStrip,
    ) -> Result<u32, crate::error::ParseError> {
        let NumericStrip {
            mut set, sets, did, ..
        } = strip;
        if did {
            let ns_id = self.allocate_set();
            let (ty, line, mut nm, set_ops) = {
                let src = &self.sets_list[set.0];
                (src.r#type, src.line, src.name.clone(), src.set_ops.clone())
            };
            nm = format!("{STR_GPREFIX}{nm}_B_");
            {
                let dst = self.sets_list.get_mut(ns_id.0);
                dst.r#type = ty;
                dst.line = line;
                dst.name = nm;
                dst.sets = sets;
                dst.set_ops = set_ops;
            }
            set = self.add_set(ns_id)?;
        }
        Ok(self.sets_list[set.0].hash)
    }

    // [spec:cg3:def:grammar.cg3.grammar.remove-numeric-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn]
    /// Strip the numeric tags from a set of tags: a new set of what is left
    /// when any were removed, the set itself otherwise. Returns the resulting
    /// set's hash.
    fn strip_numeric_leaf(&mut self, mut set: SetId) -> Result<u32, crate::error::ParseError> {
        let mut did = false;
        let mut ntags: BTreeMap<TagVector, bool> = BTreeMap::new();
        let tries = [
            self.sets_list[set.0].trie.clone(),
            self.sets_list[set.0].trie_special.clone(),
        ];
        for tr in &tries {
            if tr.is_empty() {
                continue;
            }
            let ctags = trie_get_tags(tr, self);
            for it in &ctags {
                let mut special = false;
                let mut tags: TagVector = TagVector::new();
                fill_tagvector(self, it, &mut tags, &mut did, &mut special);
                if !tags.is_empty() {
                    ntags.insert(tags, special);
                }
            }
        }
        let ff: Vec<crate::arena::TagId> = self.sets_list[set.0].ff_tags.as_slice().to_vec();
        if !ff.is_empty() {
            let mut special = false;
            let mut tags: TagVector = TagVector::new();
            fill_tagvector(self, &ff, &mut tags, &mut did, &mut special);
            if !tags.is_empty() {
                ntags.insert(tags, special);
            }
        }
        if did {
            set = self.add_stripped_set(set, ntags)?;
        }
        Ok(self.sets_list[set.0].hash)
    }

    // [spec:cg3:def:grammar.cg3.grammar.remove-numeric-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.remove-numeric-tags-fn]
    /// The new set [`Self::strip_numeric_leaf`] builds of the tags left in
    /// `set` — `(*)` when none are.
    fn add_stripped_set(
        &mut self,
        set: SetId,
        mut ntags: BTreeMap<TagVector, bool>,
    ) -> Result<SetId, crate::error::ParseError> {
        if ntags.is_empty() {
            let tid = {
                let it = self.single_tags().find(self.tag_any);
                it.get().1
            };
            ntags.insert(vec![tid], true);
            // verbosity_level>0 "Set ... was empty ... C branch": deferred.
        }
        let ns_id = self.allocate_set();
        let (ty, line, mut nm) = {
            let src = &self.sets_list[set.0];
            (src.r#type, src.line, src.name.clone())
        };
        nm = format!("{STR_GPREFIX}{nm}_B_");
        {
            let dst = self.sets_list.get_mut(ns_id.0);
            dst.r#type = ty;
            dst.line = line;
            dst.name = nm;
        }
        for (tagvec, special) in &ntags {
            if *special {
                if tagvec.len() == 1
                    && self.single_tags_list[tagvec[0].0]
                        .r#type
                        .intersects(T_FAILFAST)
                {
                    self.sets_list.get_mut(ns_id.0).ff_tags.insert(tagvec[0]);
                } else {
                    let dst = &mut self.sets_list.get_mut(ns_id.0).trie_special;
                    trie_insert(dst, tagvec, 0);
                }
            } else {
                let dst = &mut self.sets_list.get_mut(ns_id.0).trie;
                trie_insert(dst, tagvec, 0);
            }
        }
        self.add_set(ns_id)
    }

    // [spec:cg3:def:grammar.cg3.grammar.set-adjust-sets-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.set-adjust-sets-fn]
    /// One set of [`Self::set_adjust_sets`]'s walk: rewrite its members from
    /// content hashes to set numbers, unless it is done already, and return
    /// the members to visit next.
    pub(super) fn adjust_one_set(&mut self, s: SetId) -> Vec<SetId> {
        if !self.sets_list[s.0].r#type.intersects(ST_USED) {
            return Vec::new();
        }
        self.sets_list.get_mut(s.0).r#type &= !ST_USED;
        let members: Vec<SetId> = self.sets_list[s.0]
            .sets
            .iter()
            .map(|i| self.sets_by_contents[i]) // find(i)->second — no end-check.
            .collect();
        let numbers = members
            .iter()
            .map(|set| self.sets_list[set.0].number.get())
            .collect();
        self.sets_list.get_mut(s.0).sets = numbers;
        members.into_iter().rev().collect()
    }

    // [spec:cg3:def:grammar.cg3.grammar.context-adjust-target-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.context-adjust-target-fn]
    /// One test of [`Self::context_adjust_target`]'s walk: rewrite its set
    /// references from content hashes to set numbers, unless it is done
    /// already, and return the tests it refers to, to visit next.
    pub(super) fn adjust_one_context(&mut self, test: CtxId) -> Vec<CtxId> {
        if !self.contexts_arena[test.0].is_used {
            return Vec::new();
        }
        self.contexts_arena[test.0].is_used = false;
        let t = &self.contexts_arena[test.0];
        let (target, barrier, cbarrier) = (t.target, t.barrier, t.cbarrier);
        if target.get() != 0 {
            let set = self.sets_by_contents[&target.get()];
            self.contexts_arena[test.0].target = self.sets_list[set.0].number;
        }
        if barrier.get() != 0 {
            let set = self.sets_by_contents[&barrier.get()];
            self.contexts_arena[test.0].barrier = self.sets_list[set.0].number;
        }
        if cbarrier.get() != 0 {
            let set = self.sets_by_contents[&cbarrier.get()];
            self.contexts_arena[test.0].cbarrier = self.sets_list[set.0].number;
        }
        // The C++ recurses into the OR alternatives, then the template, then
        // the LINK.
        let t = &self.contexts_arena[test.0];
        let mut next: Vec<CtxId> = t.linked.into_iter().collect();
        next.extend(t.tmpl);
        next.extend(t.ors.iter().rev());
        next
    }

    // [spec:cg3:def:contextual-test.cg3.contextual-test.mark-used-fn]
    // [spec:cg3:sem:contextual-test.cg3.contextual-test.mark-used-fn]
    /// One test of [`Self::context_mark_used`]'s walk: mark it and its sets
    /// used, unless it is already, and return the tests it refers to, to visit
    /// next. `getSet` null → deref crash (reproduced via `unwrap`).
    pub(super) fn mark_one_context_used(&mut self, test: CtxId) -> Vec<CtxId> {
        if self.contexts_arena[test.0].is_used {
            return Vec::new();
        }
        self.contexts_arena[test.0].is_used = true;
        let t = &self.contexts_arena[test.0];
        for set in [t.target, t.barrier, t.cbarrier] {
            if set.get() != 0 {
                #[expect(
                    clippy::unwrap_used,
                    reason = "a textual test's target and barriers are the hashes of sets the parser registered with add_set, and reindex marks them in its step (7), before its step (16) empties sets_by_contents; it marks no test of a .cg3b"
                )]
                let s = self.get_set(set.get()).unwrap();
                Set::mark_used(self, s);
            }
        }
        // The C++ recurses into the template, then the OR alternatives, then
        // the LINK.
        let t = &self.contexts_arena[test.0];
        let mut next: Vec<CtxId> = t.linked.into_iter().collect();
        next.extend(t.ors.iter().rev());
        next.extend(t.tmpl);
        next
    }

    // [spec:cg3:def:grammar.cg3.grammar.get-tags-fn]
    // [spec:cg3:sem:grammar.cg3.grammar.get-tags-fn]
    /// The trie paths of `set` itself, for [`Self::get_tags`].
    pub(super) fn get_own_tags(&self, set: SetId, rv: &mut TagVectorSet) {
        let trie = self.sets_list[set.0].trie.clone();
        let trie_special = self.sets_list[set.0].trie_special.clone();
        let mut tv: TagVector = TagVector::new();
        trie_get_tags_into(&trie, rv, &mut tv, self);
        tv.clear();
        trie_get_tags_into(&trie_special, rv, &mut tv, self);
    }

    /// Walk the sets on `open` — each with the index of its next member, the
    /// members named by content hash — down to the next set whose members
    /// are all done, and return it: sets come out after their members, as a
    /// recursion over them finishes them. `None` once `open` is empty.
    pub(super) fn next_set_done(&self, open: &mut Vec<(SetId, usize)>) -> Option<SetId> {
        loop {
            let (s, next) = open.last_mut()?;
            let s = *s;
            let Some(&member) = self.sets_list[s.0].sets.get(*next) else {
                open.pop();
                return Some(s);
            };
            *next += 1;
            #[expect(
                clippy::unwrap_used,
                reason = "until set_adjust_sets numbers them, a set's members are the hashes of sets add_set registered in sets_by_contents, and get_tags walks them only while the parser reads the grammar"
            )]
            open.push((self.get_set(member).unwrap(), 0)); // *getSet(s), null → crash
        }
    }
}

impl<P: Numbering> GrammarCore<P> {
    /// The sets `s` is built from, by their numbers in `s.sets`, last first:
    /// to go onto a walk's stack so they come off it in order.
    pub(super) fn members_by_number(&self, s: SetId) -> Vec<SetId> {
        self.sets_list[s.0]
            .sets
            .iter()
            .rev()
            .map(|&i| self.set_id_by_number(SetNumber(i)))
            .collect()
    }
}

impl GrammarCore<Draft> {
    /// Whether [`Self::add_set_to_list`] numbers `s`: it has no number yet
    /// (`number == 0`), and it is not the dummy at `sets_list[0]` (the C++
    /// guard `sets_list.empty() || sets_list[0] != s`).
    pub(super) fn set_unlisted(&self, s: SetId) -> bool {
        self.sets_list[s.0].number == SetNumber(0)
            && (self.sets_list_order.is_empty() || self.sets_list_order[0] != s)
    }
}
