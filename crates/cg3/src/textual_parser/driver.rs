//! `TextualParser` — the grammar-buffer driver (parseFromUChar, parse_grammar entry points).
//!
//! Split out of the wave-2 monolithic `textual_parser.rs` (wave 4, w4-file-split-fmt).

#![allow(clippy::too_many_arguments)]

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::ast::{ASTHelper, ASTType};
use crate::types::SetNumber;
use crate::contextual_test::{POS_CAREFUL, POS_NUMERIC_BRANCH, copy_cntx};
use crate::grammar::Grammar;
use crate::igrammar_parser::IGrammarParser;
use crate::inlines::{
    cg3_quit, hash_value_ustring, isspace, skipln_chars, skipto_chars, skiptows_chars, skipws_chars, ui32,
};
use crate::set::{ST_TAG_UNIFY, Set};
use crate::strings::KEYWORDS;
use crate::tag::{T_REGEXP_LINE, T_SPECIAL, T_VARSTRING};

use super::*;

impl TextualParser {
    pub(crate) fn match_cmdargs(&self, buf: &[char], pos: usize) -> Option<usize> {
        let a = is_icase_kw(buf, pos, "CMDARGS-OVERRIDE", "cmdargs-override");
        if a != 0 {
            return Some(a);
        }
        let b = is_icase_kw(buf, pos, "CMDARGS", "cmdargs");
        if b != 0 {
            return Some(b);
        }
        None
    }

    pub(crate) fn maybe_anchorish(&mut self, buf: &[char], pos: &mut usize) {
        let mut s = *pos;
        skipln_chars(buf, &mut s);
        skipws_chars(buf, &mut s, '\0', '\0', false);
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        if *pos != s {
            self.parse_anchorish(buf, pos, true);
        }
    }

    pub(crate) fn section_before(&mut self, buf: &[char], pos: &mut usize) {
        if !self.only_sets {
            self.in_before_sections = true;
            self.in_section = false;
            self.in_after_sections = false;
            self.in_null_section = false;
        }
        self.maybe_anchorish(buf, pos);
    }

    pub(crate) fn section_numbered(&mut self, buf: &[char], pos: &mut usize) {
        if !self.only_sets {
            let l = self.grammar.lines;
            self.grammar.sections.push(l);
            self.in_before_sections = false;
            self.in_section = true;
            self.in_after_sections = false;
            self.in_null_section = false;
        }
        self.maybe_anchorish(buf, pos);
    }

    pub(crate) fn parse_list(&mut self, buf: &[char], pos: &mut usize) {
        let sset = self.grammar.allocate_set();
        self.grammar.sets_list[sset.0].line = self.grammar.lines;
        let mut ordered = false;
        if buf[*pos] == 'O' {
            *pos += 1;
            ordered = true;
        }
        *pos += 4;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        let mut n = *pos;
        self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
        while buf[n - 1] == ',' || buf[n - 1] == ']' {
            n -= 1;
        }
        let name: String = buf[*pos..n].iter().collect();
        self.grammar.sets_list[sset.0].name = name.clone();
        *pos = n;
        self.grammar.lines += skipws_chars(buf, pos, '=', '\0', false);
        let mut append = false;
        if buf[*pos] == '+' && buf[*pos + 1] == '=' {
            let aset = self.grammar.get_set(hash_value_ustring(&name, 0));
            if aset.is_none() {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            append = true;
        }
        if buf[*pos] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 1;
        self.parse_tag_list(buf, pos, sset, ordered);
        Set::rehash(&mut self.grammar, sset);
        let sset = if append {
            self.grammar.append_to_set(sset)
        } else {
            self.grammar.add_set(sset)
        };
        if self.grammar.sets_list[sset.0].empty() {
            self.error_near(&buf[*pos..]);
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    pub(crate) fn parse_set_def(&mut self, buf: &[char], pos: &mut usize) {
        let s0 = self.grammar.allocate_set();
        self.grammar.sets_list[s0.0].line = self.grammar.lines;
        *pos += 3;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        let mut n = *pos;
        self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
        while buf[n - 1] == ',' || buf[n - 1] == ']' {
            n -= 1;
        }
        let name: String = buf[*pos..n].iter().collect();
        self.grammar.sets_list[s0.0].name = name.clone();
        let sh = hash_value_ustring(&name, 0);
        *pos = n;
        self.grammar.lines += skipws_chars(buf, pos, '=', '\0', false);
        if buf[*pos] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 1;

        let saved = self.no_isets;
        self.no_isets = false;
        self.parse_set_inline(buf, pos, Some(s0));
        self.no_isets = saved;

        Set::rehash(&mut self.grammar, s0);
        let mut s = s0;
        let chash = self.grammar.sets_list[s0.0].hash;
        let existing = self.grammar.get_set(chash);
        if existing.is_some() {
            // verbosity dup warning skipped
        } else if self.grammar.sets_list[s0.0].sets.len() == 1
            && (!self.grammar.sets_list[s0.0].r#type.intersects(ST_TAG_UNIFY))
        {
            let back = *self.grammar.sets_list[s0.0].sets.last().unwrap();
            let tmp = self.grammar.get_set(back).unwrap();
            self.grammar.maybe_used_sets.insert(tmp);
            let th = self.grammar.sets_list[tmp.0].hash;
            self.grammar.set_alias.insert((sh, th));
            self.grammar.destroy_set(s0);
            s = tmp;
        }
        let s = self.grammar.add_set(s);
        if self.grammar.sets_list[s.0].empty() {
            self.error_near(&buf[*pos..]);
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    pub(crate) fn parse_options(&mut self, buf: &[char], pos: &mut usize) {
        *pos += 7;
        self.grammar.lines += skipws_chars(buf, pos, '+', '\0', false);
        if buf[*pos] != '+' || buf[*pos + 1] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 2;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        while buf[*pos] != ';' {
            let mut found = false;
            // No `break` between checks — reproduces the C++ multi-match loop.
            if simplecasecmp(buf, *pos, STR_NO_ISETS) {
                *pos += slen(STR_NO_ISETS);
                self.no_isets = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_NO_ITMPLS) {
                *pos += slen(STR_NO_ITMPLS);
                self.no_itmpls = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_WFORMS) {
                *pos += slen(STR_STRICT_WFORMS);
                self.strict_wforms = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_BFORMS) {
                *pos += slen(STR_STRICT_BFORMS);
                self.strict_bforms = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_SECOND) {
                *pos += slen(STR_STRICT_SECOND);
                self.strict_second = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_REGEX) {
                *pos += slen(STR_STRICT_REGEX);
                self.strict_regex = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_STRICT_ICASE) {
                *pos += slen(STR_STRICT_ICASE);
                self.strict_icase = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_SELF_NO_BARRIER) {
                *pos += slen(STR_SELF_NO_BARRIER);
                self.self_no_barrier = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_ORDERED) {
                *pos += slen(STR_ORDERED);
                self.grammar.ordered = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_ADDCOHORT_ATTACH) {
                *pos += slen(STR_ADDCOHORT_ATTACH);
                self.grammar.addcohort_attach = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if simplecasecmp(buf, *pos, STR_SAFE_SETPARENT) {
                *pos += slen(STR_SAFE_SETPARENT);
                self.safe_setparent = true;
                self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
                found = true;
            }
            if !found {
                self.error_near(&buf[*pos..]);
            }
        }

        if self.grammar.addcohort_attach {
            self.grammar.has_dep = true;
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    pub(crate) fn parse_parentheses(&mut self, buf: &[char], pos: &mut usize) {
        *pos += 11;
        self.grammar.lines += skipws_chars(buf, pos, '=', '\0', false);
        if buf[*pos] != '=' {
            self.error_near(&buf[*pos..]);
        }
        *pos += 1;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        while buf[*pos] != '\0' && buf[*pos] != ';' {
            let mut n = *pos;
            self.grammar.lines += skiptows_chars(buf, &mut n, '(', true, false);
            if buf[n] != '(' {
                self.error_near(&buf[*pos..]);
            }
            n += 1;
            self.grammar.lines += skipws_chars(buf, &mut n, '\0', '\0', false);
            *pos = n;
            self.maybe_quoted(buf, &mut n, *pos);
            self.grammar.lines += skiptows_chars(buf, &mut n, ')', true, false);
            let ltok: String = buf[*pos..n].iter().collect();
            let left = self.parse_tag(&ltok, &buf[*pos..]);
            self.grammar.lines += skipws_chars(buf, &mut n, '\0', '\0', false);
            *pos = n;
            if buf[*pos] == ')' {
                self.error_near(&buf[*pos..]);
            }
            self.maybe_quoted(buf, &mut n, *pos);
            self.grammar.lines += skiptows_chars(buf, &mut n, ')', true, false);
            let rtok: String = buf[*pos..n].iter().collect();
            let right = self.parse_tag(&rtok, &buf[*pos..]);
            self.grammar.lines += skipws_chars(buf, &mut n, '\0', '\0', false);
            *pos = n;
            if buf[*pos] != ')' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

            let lh = self.grammar.single_tags_list[left.0].hash;
            let rh = self.grammar.single_tags_list[right.0].hash;
            self.grammar.parentheses.insert(lh.get(), rh.get());
            self.grammar.parentheses_reverse.insert(rh.get(), lh.get());
        }
        if self.grammar.parentheses.is_empty() {
            self.error_near(&buf[*pos..]);
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }
    }

    pub(crate) fn parse_include(&mut self, buf: &[char], pos: &mut usize, fname: &str) {
        *pos += 7;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        let mut local_only_sets = self.only_sets;
        if simplecasecmp(buf, *pos, STR_STATIC) && isspace(buf[*pos + slen(STR_STATIC)]) {
            *pos += slen(STR_STATIC);
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            local_only_sets = true;
        }

        let mut n = *pos;
        self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
        let incname: String = buf[*pos..n].iter().collect();
        *pos = n;
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            self.error_near(&buf[*pos..]);
        }

        let mut abspath = incname.clone();
        if abspath.contains('~') || abspath.contains('$') || abspath.contains('*') {
            abspath = shell_expand(&abspath);
        }
        if !abspath.starts_with('/') {
            let dir = ux_dirname(fname);
            abspath = format!("{dir}{abspath}");
        }
        let mut bytes = match std::fs::read(&abspath) {
            Ok(b) => b,
            Err(_) => match std::fs::read(&incname) {
                Ok(b) => {
                    abspath = incname.clone();
                    b
                }
                Err(e) => {
                    tracing::error!(
                        "{}: Error: Cannot stat {} due to error {} - bailing out!",
                        self.filebase,
                        abspath,
                        e
                    );
                    cg3_quit(1, None, 0);
                }
            },
        };
        if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
            bytes.drain(0..3);
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut data: Vec<char> = vec!['\0'; 4];
        data.extend(text.chars());
        data.extend(std::iter::repeat_n('\0', 40));
        self.grammarbufs.push(data);
        let gi2 = self.grammarbufs.len() - 1;

        let saved_lines = self.grammar.lines;
        let saved_filebase = std::mem::take(&mut self.filebase);
        let saved_cur_grammar = self.cur_grammar;
        let saved_cur_grammar_n = self.cur_grammar_n;
        let saved_only = self.only_sets;
        let saved_end = self.parse_end_break;
        self.only_sets = local_only_sets;
        self.parse_from_u_char(gi2, abspath);
        self.parse_end_break = saved_end;
        self.only_sets = saved_only;
        self.cur_grammar_n = saved_cur_grammar_n;
        self.cur_grammar = saved_cur_grammar;
        self.filebase = saved_filebase;
        self.grammar.lines = saved_lines;
    }

    fn make_magic_set(&mut self, name: &str) -> SetId {
        let set_c = self.grammar.allocate_set();
        self.grammar.sets_list[set_c.0].line = 0;
        self.grammar.sets_list[set_c.0].name = name.to_string();
        let t = self.parse_tag(name, &[]);
        self.grammar.add_tag_to_set(t, set_c);
        self.grammar.add_set(set_c)
    }

    fn resolve_varstring(&mut self, tid: TagId) {
        let tagstr = self.grammar.single_tags_list[tid.0].tag.clone();
        let mut tbuf: Vec<char> = vec!['\0'];
        tbuf.extend(tagstr.chars());
        tbuf.extend(std::iter::repeat_n('\0', 4));
        let mut p = 1usize;
        loop {
            skipto_chars(&tbuf, &mut p, '{');
            if tbuf[p] != '\0' {
                let mut n = p;
                skipto_chars(&tbuf, &mut n, '}');
                if tbuf[n] != '\0' {
                    self.grammar.single_tags_list[tid.0].allocate_vs_sets();
                    self.grammar.single_tags_list[tid.0].allocate_vs_names();
                    p += 1;
                    let theset: String = tbuf[p..n].iter().collect();
                    let tmp = self.parse_set(&theset, &tbuf[p..]);
                    let setname = self.grammar.sets_list[tmp.0].name.clone();
                    self.grammar.single_tags_list[tid.0]
                        .vs_sets
                        .as_mut()
                        .unwrap()
                        .push(tmp);
                    let old = format!("{{{setname}}}");
                    self.grammar.single_tags_list[tid.0]
                        .vs_names
                        .as_mut()
                        .unwrap()
                        .push(old);
                    p = n;
                    p += 1;
                }
            }
            if tbuf[p] == '\0' {
                break;
            }
        }
    }

    fn numeric_branch_split(&mut self) {
        let mut sets_cache: BTreeMap<u32, u32> = BTreeMap::new();
        loop {
            let found = self.grammar.contexts.iter().find_map(|(&k, &v)| {
                if self.grammar.contexts_arena[v.0]
                    .pos
                    .intersects(POS_NUMERIC_BRANCH)
                {
                    Some((k, v))
                } else {
                    None
                }
            });
            let (key, unsafec) = match found {
                Some(x) => x,
                None => break,
            };
            self.grammar.contexts.remove(&key);

            let target = self.grammar.contexts_arena[unsafec.0].target.get();
            if let std::collections::btree_map::Entry::Vacant(e) = sets_cache.entry(target) {
                let stripped = self.grammar.remove_numeric_tags(target);
                e.insert(stripped);
            }
            self.grammar.contexts_arena[unsafec.0].pos &= !POS_NUMERIC_BRANCH;

            let safec = self.grammar.allocate_contextual_test();
            {
                let src = self.grammar.contexts_arena[unsafec.0].clone();
                copy_cntx(&src, &mut self.grammar.contexts_arena[safec.0]);
            }
            self.grammar.contexts_arena[safec.0].pos |= POS_CAREFUL;
            self.grammar.contexts_arena[safec.0].target = SetNumber(sets_cache[&target]);

            let tmp = unsafec;
            let unsafec2 = self.grammar.add_contextual_test(Some(unsafec)).unwrap();
            let safec2 = self.grammar.add_contextual_test(Some(safec)).unwrap();

            let orc = self.grammar.allocate_contextual_test();
            self.grammar.contexts_arena[orc.0].ors.push(safec2);
            self.grammar.contexts_arena[orc.0].ors.push(unsafec2);
            let orc = self.grammar.add_contextual_test(Some(orc)).unwrap();

            if let Some(prof) = self.profiler.as_mut() {
                // Copy the profiler span of the original (unsafe) context onto
                // the OR'd replacement, keyed by the old hash's entry.
                let tmp_hash = self.grammar.contexts_arena[tmp.0].hash;
                let k = crate::profiler::Key {
                    r#type: crate::profiler::ET_CONTEXT,
                    id: tmp_hash,
                };
                if let Some(pc) = prof.entries.get(&k).copied() {
                    let orc_hash = self.grammar.contexts_arena[orc.0].hash;
                    prof.add_context(orc_hash, self.cur_grammar_n, pc.b, pc.e);
                }
            }

            let ctx_ids: Vec<CtxId> = self.grammar.contexts.values().copied().collect();
            for v in ctx_ids {
                if self.grammar.contexts_arena[v.0].linked == Some(tmp) {
                    self.grammar.contexts_arena[v.0].linked = Some(orc);
                }
            }
            let rule_ids: Vec<RuleId> = (0..self.grammar.rule_by_number.capacity())
                .filter(|&i| self.grammar.rule_by_number.try_get(i).is_some())
                .map(RuleId)
                .collect();
            for rid in rule_ids {
                if self.grammar.rule_by_number[rid.0].dep_target == Some(tmp) {
                    self.grammar.rule_by_number.get_mut(rid.0).dep_target = Some(orc);
                }
                let tests: Vec<CtxId> = self.grammar.rule_by_number[rid.0]
                    .tests
                    .iter()
                    .copied()
                    .collect();
                for (i, t) in tests.iter().enumerate() {
                    if *t == tmp {
                        self.grammar.rule_by_number.get_mut(rid.0).tests[i] = orc;
                    }
                }
                let dep_tests: Vec<CtxId> = self.grammar.rule_by_number[rid.0]
                    .dep_tests
                    .iter()
                    .copied()
                    .collect();
                for (i, t) in dep_tests.iter().enumerate() {
                    if *t == tmp {
                        self.grammar.rule_by_number.get_mut(rid.0).dep_tests[i] = orc;
                    }
                }
            }
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-from-u-char-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-from-u-char-fn]
    fn parse_from_u_char(&mut self, gi: usize, fname: String) {
        let (ptr, len) = {
            let g = &self.grammarbufs[gi];
            (g.as_ptr(), g.len())
        };
        // SAFETY: the char data of `grammarbufs[gi]` is heap-stable and never
        // mutated after creation; the decoupled slice never aliases a live `&mut`
        // into that data. Faithful to the C++ raw pointers into stable buffers.
        let buf: &[char] = unsafe { std::slice::from_raw_parts(ptr, len) };

        if len <= 4 || buf[4] == '\0' {
            tracing::error!("{}: Error: Input is empty - cannot continue!", fname);
            cg3_quit(1, None, 0);
        }

        // C++: `if (profiler) { parse_ast = true; }` — profiling implies AST
        // building (the AST capture is interned into the profile database).
        if self.profiler.is_some() {
            self.ast.set_enabled(true);
        }

        let mut id = {
            self.num_grammars += 1;
            self.num_grammars
        };
        if let Some(prof) = self.profiler.as_mut() {
            // id = profiler->addGrammar(fname, utf8) — register the grammar
            // text (from the 4-char lookbehind pad up to the NUL).
            let end = buf[4..]
                .iter()
                .position(|&c| c == '\0')
                .map(|i| i + 4)
                .unwrap_or(len);
            let utf8: String = buf[4..end].iter().collect();
            id = prof.add_grammar(&fname, &utf8);
        }
        self.cur_grammar = unsafe { buf.as_ptr().add(4) };
        self.cur_grammar_n = id;
        let mut pos = 4usize;
        self.grammar.lines = 1;
        let mut ast_grammar = ASTHelper::new(
            &mut self.ast,
            ASTType::AST_Grammar,
            self.grammar.lines as usize,
            pptr(buf, 4),
        );
        self.filebase = basename(Some(&fname)).to_string();
        self.parse_end_break = false;

        while buf[pos] != '\0' {
            let ast_depth = self.ast.cursor_depth();
            let r = panic::catch_unwind(AssertUnwindSafe(|| {
                self.parse_directive(buf, &mut pos, &fname);
            }));
            if let Err(e) = r {
                if e.is::<ParseError>() {
                    // C++ stack unwinding runs every in-scope ~ASTHelper();
                    // restore the AST cursor to the pre-directive depth.
                    self.ast.truncate_cursor(ast_depth);
                    self.grammar.lines += skipln_chars(buf, &mut pos);
                } else {
                    panic::resume_unwind(e);
                }
            }
            if self.parse_end_break {
                break;
            }
        }

        ast_grammar.close_id(&mut self.ast, pptr(buf, pos), id);
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-grammar-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn]
    fn parse_grammar_data(&mut self, gi: usize) -> i32 {
        // 1. START anchor at rule 0.
        self.grammar
            .add_anchor(KEYWORDS_STR[KEYWORDS::K_START as usize], 0, true);
        // 2. Magic * tag.
        let tany = self.parse_tag(STR_ASTERIK, &[]);
        self.grammar.tag_any = self.grammar.single_tags_list[tany.0].hash.get();
        // 3. Dummy set.
        self.grammar.allocate_dummy_set();
        // 4. Magic sets.
        self.make_magic_set(STR_UU_TARGET);
        self.make_magic_set(STR_UU_MARK);
        self.make_magic_set(STR_UU_ATTACHTO);
        let s_left = self.make_magic_set(STR_UU_LEFT);
        let s_right = self.make_magic_set(STR_UU_RIGHT);
        self.make_magic_set(STR_UU_ENCL);
        {
            let set_c = self.grammar.allocate_set();
            self.grammar.sets_list[set_c.0].line = 0;
            self.grammar.sets_list[set_c.0].name = STR_UU_PAREN.to_string();
            self.grammar.sets_list[set_c.0].set_ops.push(S_OR);
            let lh = self.grammar.sets_list[s_left.0].hash;
            let rh = self.grammar.sets_list[s_right.0].hash;
            self.grammar.sets_list[set_c.0].sets.push(lh);
            self.grammar.sets_list[set_c.0].sets.push(rh);
            self.grammar.add_set(set_c);
        }
        self.make_magic_set(STR_UU_SAME_BASIC);
        for i in 0..9 {
            self.make_magic_set(STR_UU_C[i]);
        }

        // 5. Parse the grammar text.
        let fname = self.filename.clone();
        self.parse_from_u_char(gi, fname);

        // 6. END anchor at the last rule number.
        let end_at = ui32(self.grammar.rule_by_number.capacity().wrapping_sub(1));
        self.grammar
            .add_anchor(KEYWORDS_STR[KEYWORDS::K_END as usize], end_at, true);

        // 7. Named-rule anchors.
        let rule_ids: Vec<RuleId> = (0..self.grammar.rule_by_number.capacity())
            .filter(|&i| self.grammar.rule_by_number.try_get(i).is_some())
            .map(RuleId)
            .collect();
        for rid in &rule_ids {
            let (name, number) = {
                let r = &self.grammar.rule_by_number[rid.0];
                (r.name.clone(), r.number)
            };
            if !name.is_empty() {
                self.grammar.add_anchor(&name, number, false);
            }
        }

        // 8. Validate JUMP rules.
        for rid in &rule_ids {
            let (rtype, maplist) = {
                let r = &self.grammar.rule_by_number[rid.0];
                (r.r#type, r.maplist)
            };
            if rtype == KEYWORDS::K_JUMP {
                let maplist = maplist.unwrap();
                let to = self.grammar.get_tag_list_any_ret(maplist)[0];
                let (tty, thash) = {
                    let t = &self.grammar.single_tags_list[to.0];
                    (t.r#type, t.hash)
                };
                if tty.intersects(T_SPECIAL) {
                    continue;
                }
                if self.grammar.anchors.find(thash.get()) == self.grammar.anchors.end() {
                    tracing::error!("Error: JUMP could not find anchor.");
                    self.error_counter += 1;
                }
            }
        }

        // 9. Varstring set resolution + T_REGEXP_LINE ordered.
        let tag_ids: Vec<TagId> = (0..self.grammar.single_tags_list.capacity())
            .filter(|&i| self.grammar.single_tags_list.try_get(i).is_some())
            .map(TagId)
            .collect();
        for tid in &tag_ids {
            let ty = self.grammar.single_tags_list[tid.0].r#type;
            if ty.intersects(T_REGEXP_LINE) {
                self.grammar.ordered = true;
            }
            if !ty.intersects(T_VARSTRING) {
                continue;
            }
            self.resolve_varstring(*tid);
        }

        // 10. Resolve deferred template refs.
        let deferred: Vec<(CtxId, (usize, String))> = self
            .deferred_tmpls
            .iter()
            .map(|(&k, v)| (k, v.clone()))
            .collect();
        for (t, (line, name)) in deferred {
            let cn = hash_value_ustring(&name, 0);
            if !self.grammar.templates.contains_key(&cn) {
                tracing::error!(
                    "{}: Error: Unknown template '{}' referenced on line {}!",
                    self.filebase,
                    name,
                    line
                );
                self.error_counter += 1;
                continue;
            }
            let real = self.grammar.templates[&cn];
            self.grammar.contexts_arena[t.0].tmpl = Some(real);
        }

        // 11. Numeric-branch splitting.
        self.numeric_branch_split();

        // 12. num_tags.
        self.grammar.num_tags = self.grammar.single_tags_list.capacity() as usize;

        self.error_counter
    }

    /// C++ `int parse_grammar(const char* buffer, size_t length)` (UTF-8 memory
    /// buffer). Builds the `data` buffer (4 leading NULs + text + NUL padding),
    /// then runs the private `parse_grammar(data)` driver.
    pub fn parse_grammar_utf8(&mut self, buffer: &[u8]) -> Result<i32, crate::error::Cg3Error> {
        self.filename = "<utf8-memory>".to_string();
        self.filebase = "<utf8-memory>".to_string();
        self.grammar.grammar_size = buffer.len();
        let text = String::from_utf8_lossy(buffer);
        let mut data: Vec<char> = vec!['\0'; 4];
        data.extend(text.chars());
        data.extend(std::iter::repeat_n('\0', 40));
        self.grammarbufs.push(data);
        let gi = self.grammarbufs.len() - 1;
        // Deep grammar-construction fatals (grammar.rs allocate_tag/add_set/... and
        // the parse driver's own `cg3_quit` sites) unwind as `Cg3Exit`; capture
        // them at this parse boundary and surface as `Err(Cg3Error)` carrying the
        // exact exit code. A per-directive `ParseError` is already recovered inside
        // `parse_from_u_char`; only a boundary escape reaches `catch_fatal`.
        crate::error::catch_fatal(|| self.parse_grammar_data(gi))
    }
}

/// Best-effort `wordexp` stand-in for the INCLUDE path (`~`/`$`/`*`). Only `~`
/// (home) is expanded; env-var / glob expansion is a deliberate simplification
/// (documented). The C++ uses `wordexp(WRDE_NOCMD|WRDE_UNDEF)`.
fn shell_expand(s: &str) -> String {
    let mut out = s.to_string();
    if (out == "~" || out.starts_with("~/"))
        && let Ok(home) = std::env::var("HOME") {
            out = out.replacen('~', &home, 1);
        }
    out
}

impl IGrammarParser for TextualParser {
    // [spec:cg3:def:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    // [spec:cg3:sem:i-grammar-parser.cg3.i-grammar-parser.parse-grammar-fn]
    /// Reconciliation: `TextualParser` builds into its OWN `self.grammar`; the
    /// caller's `&mut Grammar` is swapped in for the duration so the result lands
    /// there (faithful to the C++ `result` being the `Grammar&` handed at ctor).
    fn parse_grammar(
        &mut self,
        grammar: &mut Grammar,
        input: &[u8],
    ) -> Result<i32, crate::error::Cg3Error> {
        std::mem::swap(&mut self.grammar, grammar);
        let rv = self.parse_grammar_utf8(input);
        // Swap back unconditionally (even on Err) so the caller's grammar holds
        // whatever was built, matching the C++ result-by-reference contract.
        std::mem::swap(&mut self.grammar, grammar);
        rv
    }

    fn set_compatible(&mut self, compat: bool) {
        self.option_vislcg_compat = compat;
    }

    fn set_verbosity(&mut self, level: u32) {
        self.verbosity_level = level;
    }

    fn get_grammar(&self) -> &Grammar {
        &self.grammar
    }
}
