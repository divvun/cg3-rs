//! `Grammar::reindex` — the pass that turns a loaded grammar into one a run
//! and the writers can use — as its two jobs, each step its own helper.
//!
//! *Resolving* turns a grammar the textual parser built, whose sets, rules and
//! tests refer to sets by content hash, into one that refers to them by set
//! number. It runs once, on a textual grammar only. *Indexing* builds the maps
//! and flags a run reads from a grammar whose references are numbers, and runs
//! on every grammar, after resolving when there is any. The step numbers are
//! those of `[spec:cg3:sem:grammar.cg3.grammar.reindex-fn]`; the rule's port
//! divergence lists which step belongs to which job.
//!
//! Neither job reads what the other writes out of this order: resolving never
//! reads an index or `T_TEXTUAL`, and the `T_USED` marks both write are ORs.
//! After resolving, `sets_list_order[rule.target]` is the set the target's
//! content hash named, because `add_set_to_list` numbers a set by its position
//! there, so indexing a rule's target by number reaches the set the C++ reaches
//! by hash.

use std::collections::BTreeSet;

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::error::{Cg3Error, GrammarError};
use crate::inlines::{hash_value_str, is_textual};
use crate::rule::{RF_CAPTURE_UNIF, RF_KEEPORDER};
use crate::set::{MASK_ST_UNIFY, ST_CHILD_UNIFY, ST_STATIC, ST_USED, Set};
use crate::sorted_vector::Uint32SortedVector;
use crate::strings::Keywords;
use crate::tag::{T_CASE_INSENSITIVE, T_MAPPING, T_TEXTUAL, T_VARSTRING};
use crate::tag_trie::trie_has_type;
use crate::types::SetNumber;
use crate::uextras::eq_ignore_case;

use super::{Contexts, GrammarCore, Reindexed};

impl GrammarCore {
    // [spec:cg3:def:grammar.cg3.grammar.reindex-fn+1]
    // [spec:cg3:sem:grammar.cg3.grammar.reindex-fn+1]
    // [spec:cg3:req:grammar-phases.same-output]
    /// Core finalization pass. Resolves a textual grammar's references from
    /// content hashes to set numbers, then indexes the grammar. A `.cg3b` is
    /// stored numbered, so it is only indexed.
    ///
    /// Neither diagnostic flag prints: the unused-set report and the tag dump
    /// are not ported. `used_tags` asks for the dump, after which the C++
    /// `exit(0)`s. That is a successful stop, not a failure, so it comes back
    /// as [`Reindexed::DumpedTags`] rather than an error carrying exit code 0.
    pub fn reindex(&mut self, unused_sets: bool, used_tags: bool) -> Result<Reindexed, Cg3Error> {
        // (9) The unused-set report reads state resolving throws away; it is
        // not ported, and prints nothing.
        let _ = unused_sets;
        if !self.is_binary {
            self.resolve()?;
        }
        self.index()?;
        // (21) The tag dump is not ported; the caller stops, successfully.
        if used_tags {
            return Ok(Reindexed::DumpedTags);
        }
        Ok(Reindexed::Done)
    }

    /// Every live tag, in number order (arena order is number order).
    fn all_tag_ids(&self) -> Vec<TagId> {
        (0..self.single_tags_list.capacity())
            .filter(|&i| self.single_tags_list.try_get(i).is_some())
            .map(TagId)
            .collect()
    }

    /// Every live rule, in number order.
    fn all_rule_ids(&self) -> Vec<RuleId> {
        (0..self.rule_by_number.capacity())
            .filter(|&i| self.rule_by_number.try_get(i).is_some())
            .map(RuleId)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Resolving: a textual grammar's references, content hash to set number.
    // -----------------------------------------------------------------------

    // [spec:cg3:req:grammar-phases.same-output]
    /// Resolves a grammar the textual parser built: marks what its rules use,
    /// numbers the used sets, and rewrites every reference to a set from its
    /// content hash to its number. Not idempotent: it ends by dropping the
    /// content-hash map it resolves through.
    fn resolve(&mut self) -> Result<(), Cg3Error> {
        self.reset_set_state();
        self.make_static_sets()?;
        self.cut_to_numbering();
        self.mark_varstring_sets();
        self.mark_rule_sets();
        self.mark_delimiters();
        self.keep_used_contexts();
        self.number_used_sets();
        // (11) Set::reindex reads T_MAPPING, so it is set before step (12).
        self.set_mapping_flags();
        self.resolve_sets();
        self.resolve_rules();
        // (16) Sets are henceforth referenced by number.
        self.sets_by_contents.clear();
        Ok(())
    }

    /// (1) Every set starts unused and unnumbered, except the dummy, which is
    /// always used, and a static set, which stays used.
    fn reset_set_state(&mut self) {
        let all_content_sets: Vec<SetId> = self.sets_by_contents.values().copied().collect();
        for sid in &all_content_sets {
            let s = self.sets_list.get_mut(sid.0);
            if s.number == SetNumber(u32::MAX) {
                s.r#type |= ST_USED;
                continue;
            }
            if !s.r#type.intersects(ST_STATIC) {
                s.r#type &= !ST_USED;
            }
            s.number = SetNumber(0);
        }
    }

    /// (2) Marks each `STATIC-SETS` set used and static, under the name it was
    /// declared by.
    fn make_static_sets(&mut self) -> Result<(), Cg3Error> {
        let static_sets = self.static_sets.clone();
        for sset in &static_sets {
            let sh = hash_value_str(sset, 0);
            if self.set_alias.contains(sh) {
                return Err(GrammarError::StaticSetAlias {
                    name: sset.clone(),
                    line: self.lines,
                }
                .into());
            }
            let s = match self.get_set(sh) {
                Some(s) => s,
                None => continue, // verbosity warn deferred
            };
            if &self.sets_list[s.0].name != sset {
                self.sets_list.get_mut(s.0).name = sset.clone();
            }
            Set::mark_used(self, s);
            self.sets_list.get_mut(s.0).r#type |= ST_STATIC;
        }
        Ok(())
    }

    /// (3), resolving's part: drop the name maps, so every lookup from here on
    /// is by content hash, and cut `sets_list` back to the dummy for step (10)
    /// to number from.
    fn cut_to_numbering(&mut self) {
        self.set_alias.clear(0);
        self.sets_by_name.clear(0);
        self.set_name_seeds.clear();
        // sets_list.resize(1); sets_list[0]->number = 0 — keep only the dummy
        // in the numbered order and reset its number. Guarded so a dummy-less
        // grammar does not panic (C++ would UB).
        self.sets_list_order.truncate(1);
        if let Some(&d0) = self.sets_list_order.first() {
            self.sets_list.get_mut(d0.0).number = SetNumber(0);
        }
    }

    /// (4), resolving's part: the sets a varstring tag reads are used.
    fn mark_varstring_sets(&mut self) {
        for tid in self.all_tag_ids() {
            let Some(vs) = self.single_tags_list[tid.0].vs_sets.clone() else {
                continue;
            };
            for sit in *vs {
                Set::mark_used(self, sit);
            }
        }
    }

    /// (7), resolving's part: every set and test a rule refers to is used.
    fn mark_rule_sets(&mut self) {
        for rid in self.all_rule_ids() {
            let r = &self.rule_by_number[rid.0];
            let target = r.target.get();
            let child_sets = [r.childset1.get(), r.childset2.get()];
            let lists = [r.maplist, r.sublist];
            let mut tests: Vec<CtxId> = r.dep_target.into_iter().collect();
            tests.extend(r.tests.iter().copied());
            tests.extend(r.dep_tests.iter().copied());
            {
                #[expect(
                    clippy::unwrap_used,
                    reason = "a textual rule's target and child sets are the hashes of sets the parser registered with add_set, and sets_by_contents keeps them until resolving ends"
                )]
                let s = self.get_set(target).unwrap();
                Set::mark_used(self, s);
            }
            for child in child_sets.into_iter().filter(|&c| c != 0) {
                #[expect(
                    clippy::unwrap_used,
                    reason = "a textual rule's target and child sets are the hashes of sets the parser registered with add_set, and sets_by_contents keeps them until resolving ends"
                )]
                let s = self.get_set(child).unwrap();
                Set::mark_used(self, s);
            }
            for list in lists.into_iter().flatten() {
                Set::mark_used(self, list);
            }
            for test in tests {
                self.context_mark_used(test);
            }
        }
    }

    /// (8) The delimiter sets are used.
    fn mark_delimiters(&mut self) {
        let delimiters = [self.delimiters, self.soft_delimiters, self.text_delimiters];
        for d in delimiters.into_iter().flatten() {
            Set::mark_used(self, d);
        }
    }

    /// (8) Keep the templates and contexts a rule reaches; free the other
    /// contexts.
    fn keep_used_contexts(&mut self) {
        let templates = std::mem::take(&mut self.templates);
        self.templates = templates
            .into_iter()
            .filter(|&(_, v)| self.contexts_arena[v.0].is_used)
            .collect();

        let contexts = std::mem::take(&mut self.contexts);
        let mut kept = Contexts::default();
        for (k, v) in contexts {
            if self.contexts_arena[v.0].is_used {
                kept.insert(k, v);
            } else {
                self.contexts_arena.free_slot(v.0); // delete cntx.second
            }
        }
        self.contexts = kept;
    }

    /// (10) Numbers the used sets, depth first, after the dummy.
    fn number_used_sets(&mut self) {
        let content_sets: Vec<SetId> = self.sets_by_contents.values().copied().collect();
        for sid in content_sets {
            if self.sets_list[sid.0].r#type.intersects(ST_USED) {
                self.add_set_to_list(sid);
            }
        }
    }

    /// (12), resolving's part: recompute each listed set's derived flags, then
    /// rewrite its members from content hashes to numbers.
    fn resolve_sets(&mut self) {
        let listed = self.used_set_ids();
        for &sid in &listed {
            Set::reindex(self, sid);
        }
        for &sid in &listed {
            self.set_adjust_sets(sid);
        }
    }

    /// (13), resolving's part: rewrite each rule's target, child sets and
    /// tests from content hashes to numbers.
    fn resolve_rules(&mut self) {
        for rid in self.all_rule_ids() {
            let r = &self.rule_by_number[rid.0];
            let (target, childset1, childset2) = (r.target, r.childset1, r.childset2);
            let mut tests: Vec<CtxId> = r.dep_target.into_iter().collect();
            tests.extend(r.tests.iter().copied());
            tests.extend(r.dep_tests.iter().copied());
            if target.get() != 0 {
                let n = self.number_of_hash(target.get());
                self.rule_by_number.get_mut(rid.0).target = n;
            }
            if childset1.get() != 0 {
                let n = self.number_of_hash(childset1.get());
                self.rule_by_number.get_mut(rid.0).childset1 = n;
            }
            if childset2.get() != 0 {
                let n = self.number_of_hash(childset2.get());
                self.rule_by_number.get_mut(rid.0).childset2 = n;
            }
            for test in tests {
                self.context_adjust_target(test);
            }
        }
    }

    /// The number of the set with content hash `hash`. No presence check on
    /// the lookup (C++ `sets_by_contents[hash]`, UB when absent).
    fn number_of_hash(&self, hash: u32) -> SetNumber {
        let s = self.sets_by_contents[&hash];
        self.sets_list[s.0].number
    }

    // -----------------------------------------------------------------------
    // Indexing: the maps and flags a run reads, from a numbered grammar.
    // -----------------------------------------------------------------------

    // [spec:cg3:req:grammar-phases.same-output]
    /// Indexes a numbered grammar: builds the section lists, the maps from tags
    /// and sets to the rules and sets that use them, and the flags a run and
    /// the `.cg3b` writer read. Everything it builds it first clears, so
    /// indexing a grammar again leaves no stale or duplicated entry.
    fn index(&mut self) -> Result<(), Cg3Error> {
        self.clear_indexes();
        let all_tag_ids = self.all_tag_ids();
        self.collect_pattern_tags(&all_tag_ids);
        self.mark_textual_tags(&all_tag_ids);
        self.mark_bracket_tags_used();
        self.index_rule_kinds();
        // (11) again: a .cg3b's tags carry the flag as written.
        self.set_mapping_flags();
        // (12), indexing's part.
        for sid in self.used_set_ids() {
            let num = self.sets_list[sid.0].number.get();
            self.index_sets(num, sid);
        }
        self.index_rules();
        self.cache_any_indexes();
        self.name_static_sets()?;
        let sets_vstr = self.varstring_sets();
        let nk = self.unifying_contexts(&sets_vstr);
        self.keep_order_where_needed(&sets_vstr, &nk);
        Ok(())
    }

    /// (3), indexing's part, and every other index this pass builds: empty,
    /// so each is built from nothing.
    fn clear_indexes(&mut self) {
        self.set_alias.clear(0);
        self.sets_by_name.clear(0);
        self.set_name_seeds.clear();
        self.rules.clear();
        self.before_sections.clear();
        self.after_sections.clear();
        self.null_section.clear();
        self.sections.clear();
        self.sets_any = None;
        self.rules_any = None;
        self.wf_rules.clear();
        self.has_protect = false;
        self.sets_by_tag.clear();
        self.rules_by_tag.clear();
        self.rules_by_set.clear();
        self.regex_tags.clear();
        self.icase_tags.clear();
    }

    /// (4) The regex and case-insensitive tags that are not literals.
    fn collect_pattern_tags(&mut self, all_tag_ids: &[TagId]) {
        for &tid in all_tag_ids {
            let t = &self.single_tags_list[tid.0];
            if is_textual(&*t.tag) {
                continue;
            }
            let (has_regexp, is_icase) =
                (t.regexp.is_some(), t.r#type.intersects(T_CASE_INSENSITIVE));
            if has_regexp {
                self.regex_tags.insert(tid);
            }
            if is_icase {
                self.icase_tags.insert(tid);
            }
        }
    }

    /// (5) A tag a regex tag finds (unanchored, as the C++'s `uregex_find`) or
    /// a case-insensitive tag equals is textual.
    fn mark_textual_tags(&mut self, all_tag_ids: &[TagId]) {
        let regex_tag_ids: Vec<TagId> = self.regex_tags.iter().copied().collect();
        let icase_tag_ids: Vec<TagId> = self.icase_tags.iter().copied().collect();
        for &tid in all_tag_ids {
            if self.single_tags_list[tid.0].r#type.intersects(T_TEXTUAL) {
                continue;
            }
            let ttext = &self.single_tags_list[tid.0].tag;
            let by_regex = regex_tag_ids.iter().any(|rid| {
                self.single_tags_list[rid.0]
                    .regexp
                    .as_ref()
                    .is_some_and(|re| re.is_match(ttext))
            });
            let by_icase = icase_tag_ids
                .iter()
                .any(|iid| eq_ignore_case(ttext, &self.single_tags_list[iid.0].tag));
            if by_regex || by_icase {
                self.single_tags_list.get_mut(tid.0).r#type |= T_TEXTUAL;
            }
        }
    }

    /// (6) The parenthesis and preferred-target tags are used.
    fn mark_bracket_tags_used(&mut self) {
        let mut hashes: Vec<u32> = Vec::new();
        for (&a, &b) in &self.parentheses {
            hashes.push(a);
            hashes.push(b);
        }
        hashes.extend(self.preferred_targets.iter().copied());
        for h in hashes {
            let t = self.single_tags().find(h).get().1;
            self.single_tags_list.get_mut(t.0).mark_used();
        }
    }

    /// (7), indexing's part: the wordform rules, and whether any rule is a
    /// PROTECT.
    fn index_rule_kinds(&mut self) {
        for rid in self.all_rule_ids() {
            let r = &self.rule_by_number[rid.0];
            let (wordform, rtype) = (r.wordform.is_some(), r.r#type);
            if wordform {
                self.wf_rules.push(rid);
            }
            if rtype == Keywords::KProtect {
                self.has_protect = true;
            }
        }
    }

    /// (11) A tag is a mapping tag when it starts with the mapping prefix.
    fn set_mapping_flags(&mut self) {
        let mp = self.mapping_prefix;
        for tid in self.all_tag_ids() {
            let t = self.single_tags_list.get_mut(tid.0);
            if t.tag.chars().next().unwrap_or('\0') == mp {
                t.r#type |= T_MAPPING;
            } else {
                t.r#type &= !T_MAPPING;
            }
        }
    }

    /// (13), indexing's part, and (14): file each rule under its section,
    /// index the tags of its target, and flag a rule whose lists unify; then
    /// number the sections without gaps.
    fn index_rules(&mut self) {
        let mut sects = Uint32SortedVector::new();
        for rid in self.all_rule_ids() {
            let r = &self.rule_by_number[rid.0];
            let (section, number, target) = (r.section, r.number, r.target);
            let (maplist, sublist) = (r.maplist, r.sublist);
            match section {
                -1 => self.before_sections.push(rid),
                -2 => self.after_sections.push(rid),
                -3 => self.null_section.push(rid),
                _ => {
                    sects.insert(section as u32);
                    self.rules.push(rid);
                }
            }
            if target.get() != 0 {
                let set = self.set_id_by_number(target);
                self.index_set_to_rule(number, set);
                self.rules_by_set
                    .entry(target.get())
                    .or_default()
                    .insert(number);
            } // else: "Warning: Rule on line ... had no target": deferred I/O.

            let unifies = [maplist, sublist]
                .into_iter()
                .flatten()
                .any(|s| self.sets_list[s.0].r#type.intersects(ST_CHILD_UNIFY));
            if unifies {
                self.rule_by_number.get_mut(rid.0).flags |= RF_CAPTURE_UNIF;
            }
        }
        // (14) Fill sections contiguously 0..=sects.back().
        if !sects.empty() {
            self.sections.extend(0..=sects.back());
        }
    }

    /// (15) Cache the any-tag indexes (a copy: the port fields own one, not a
    /// pointer into the map — see the `sets_any`/`rules_any` field docs).
    fn cache_any_indexes(&mut self) {
        let ta = self.tag_any;
        if let Some(bs) = self.sets_by_tag.get(&ta).cloned() {
            self.sets_any = Some(bs);
        }
        if let Some(iv) = self.rules_by_tag.get(&ta).cloned() {
            self.rules_any = Some(iv);
        }
    }

    /// (17) Register each static set's name by its NUMBER, seeding past a
    /// name hash another set holds.
    fn name_static_sets(&mut self) -> Result<(), Cg3Error> {
        for to in self.used_set_ids() {
            if self.sets_list[to.0].r#type.intersects(ST_STATIC) {
                self.name_static_set(to)?;
            }
        }
        Ok(())
    }

    /// Registers static set `to` under its name. Another static set already
    /// registered under the same name is an error; a different name that
    /// hashes the same is seeded past.
    fn name_static_set(&mut self, to: SetId) -> Result<(), Cg3Error> {
        let nm = self.sets_list[to.0].name.clone();
        let nhash = hash_value_str(&nm, 0);
        let cnum = self.sets_list[to.0].number;
        if !self.sets_by_name.contains(nhash) {
            self.sets_by_name.insert((nhash, cnum.get()));
            return Ok(());
        }
        let existing_num = self.sets_by_name.find(nhash).get().1;
        let a_sid = self.sets_list_order[existing_num as usize];
        let a_num = self.sets_list[a_sid.0].number;
        if cnum == a_num {
            return Ok(());
        }
        if self.sets_list[a_sid.0].name == nm {
            return Err(GrammarError::StaticSetRedefined {
                name: nm,
                existing: a_num.get(),
                line: self.lines,
            }
            .into());
        }
        if let Some(seed) =
            (0..1000u32).find(|&seed| !self.sets_by_name.contains(nhash.wrapping_add(seed)))
        {
            self.set_name_seeds.insert(nm, seed);
            self.sets_by_name
                .insert((nhash.wrapping_add(seed), cnum.get()));
        }
        Ok(())
    }

    /// (18) `sets_vstr`, by set number: the sets that hold a varstring tag,
    /// directly or through a member (to a fixpoint).
    fn varstring_sets(&self) -> Vec<bool> {
        let listed = self.used_set_ids();
        let mut sets_vstr: Vec<bool> = vec![false; self.sets_list_order.len()];
        let mut did = true;
        while did {
            did = false;
            for &set in &listed {
                let s = &self.sets_list[set.0];
                let num = s.number.get() as usize;
                if sets_vstr[num] {
                    continue;
                }
                if s.sets.iter().any(|&m| sets_vstr[m as usize]) {
                    sets_vstr[num] = true;
                    did = true;
                }
                if trie_has_type(&s.trie, T_VARSTRING, self)
                    || trie_has_type(&s.trie_special, T_VARSTRING, self)
                {
                    sets_vstr[num] = true;
                    did = true;
                }
            }
        }
        sets_vstr
    }

    /// (19) `nk`: the contexts that use unification or varstrings, directly or
    /// through their template or LINK (to a fixpoint).
    fn unifying_contexts(&self, sets_vstr: &[bool]) -> BTreeSet<CtxId> {
        let context_ids: Vec<CtxId> = self.contexts.values().copied().collect();
        let mut nk: BTreeSet<CtxId> = BTreeSet::new();
        let mut did = true;
        while did {
            did = false;
            for &t in &context_ids {
                if !nk.contains(&t) && self.context_unifies(t, &nk, sets_vstr) && nk.insert(t) {
                    did = true;
                }
            }
        }
        nk
    }

    /// One test of [`Self::unifying_contexts`]: its template or LINK is in
    /// `nk`, or its target or a barrier unifies or holds a varstring.
    fn context_unifies(&self, t: CtxId, nk: &BTreeSet<CtxId>, sets_vstr: &[bool]) -> bool {
        let ct = &self.contexts_arena[t.0];
        if ct.tmpl.is_some_and(|tm| nk.contains(&tm)) || ct.linked.is_some_and(|l| nk.contains(&l))
        {
            return true;
        }
        [ct.target, ct.barrier, ct.cbarrier]
            .into_iter()
            .filter(|s| s.get() != 0)
            .any(|s| {
                self.set_by_number(s).r#type.intersects(MASK_ST_UNIFY)
                    || sets_vstr[s.get() as usize]
            })
    }

    /// (20) A rule whose lists hold a varstring, or whose tests unify, keeps
    /// its order.
    fn keep_order_where_needed(&mut self, sets_vstr: &[bool], nk: &BTreeSet<CtxId>) {
        for rid in self.all_rule_ids() {
            let r = &self.rule_by_number[rid.0];
            if r.flags.intersects(RF_KEEPORDER) {
                continue;
            }
            let by_list = [r.sublist, r.maplist]
                .into_iter()
                .flatten()
                .any(|s| sets_vstr[self.sets_list[s.0].number.get() as usize]);
            let by_test = r
                .dep_target
                .iter()
                .chain(r.tests.iter())
                .chain(r.dep_tests.iter())
                .any(|c| nk.contains(c));
            if by_list || by_test {
                self.rule_by_number.get_mut(rid.0).flags |= RF_KEEPORDER;
            }
        }
    }
}
