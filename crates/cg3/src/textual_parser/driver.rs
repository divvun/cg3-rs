//! `TextualParser` — the grammar-buffer driver (`parse_source`, parse_grammar
//! entry points).
//!
//! Split out of the wave-2 monolithic `textual_parser.rs` (wave 4, w4-file-split-fmt).

use std::collections::BTreeMap;

use crate::arena::{CtxId, RuleId, SetId, TagId};
use crate::ast::{ASTHelper, ASTType};
use crate::contextual_test::{POS_CAREFUL, POS_NUMERIC_BRANCH, copy_cntx};
use crate::grammar::{Draft, GrammarDraft};
use crate::igrammar_parser::IGrammarParser;
use crate::inlines::{
    hash_value_str, isspace, skipln_chars, skipto_chars, skiptows_chars, skipws_chars, ui32,
};
use crate::set::{ST_TAG_UNIFY, Set};
use crate::strings::Keywords;
use crate::tag::{T_REGEXP_LINE, T_VARSTRING};
use crate::types::SetNumber;

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

    pub(crate) fn maybe_anchorish(&mut self, buf: &[char], pos: &mut usize) -> ParseResult {
        let mut s = *pos;
        skipln_chars(buf, &mut s);
        skipws_chars(buf, &mut s, '\0', '\0', false);
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        if *pos != s {
            self.parse_anchorish(buf, pos, true)?;
        }
        Ok(())
    }

    pub(crate) fn section_before(&mut self, buf: &[char], pos: &mut usize) -> ParseResult {
        if !self.only_sets {
            self.in_before_sections = true;
            self.in_section = false;
            self.in_after_sections = false;
            self.in_null_section = false;
        }
        self.maybe_anchorish(buf, pos)?;
        Ok(())
    }

    pub(crate) fn section_numbered(&mut self, buf: &[char], pos: &mut usize) -> ParseResult {
        if !self.only_sets {
            let l = self.grammar.lines;
            self.grammar.sections.push(l);
            self.in_before_sections = false;
            self.in_section = true;
            self.in_after_sections = false;
            self.in_null_section = false;
        }
        self.maybe_anchorish(buf, pos)?;
        Ok(())
    }

    pub(crate) fn parse_list(&mut self, buf: &[char], pos: &mut usize) -> ParseResult {
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
            let aset = self.grammar.get_set(hash_value_str(&name, 0));
            if aset.is_none() {
                return Err(self.error_near(*pos));
            }
            *pos += 1;
            append = true;
        }
        if buf[*pos] != '=' {
            return Err(self.error_near(*pos));
        }
        *pos += 1;
        self.parse_tag_list(buf, pos, sset, ordered)?;
        Set::rehash(&mut self.grammar, sset);
        let sset = if append {
            self.grammar.append_to_set(sset)?
        } else {
            self.grammar.add_set(sset)?
        };
        if self.grammar.sets_list[sset.0].empty() {
            return Err(self.error_near(*pos));
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            return Err(self.error_near(*pos));
        }
        Ok(())
    }

    pub(crate) fn parse_set_def(&mut self, buf: &[char], pos: &mut usize) -> ParseResult {
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
        let sh = hash_value_str(&name, 0);
        *pos = n;
        self.grammar.lines += skipws_chars(buf, pos, '=', '\0', false);
        if buf[*pos] != '=' {
            return Err(self.error_near(*pos));
        }
        *pos += 1;

        let saved = self.no_isets;
        self.no_isets = false;
        self.parse_set_inline(buf, pos, Some(s0))?;
        self.no_isets = saved;

        Set::rehash(&mut self.grammar, s0);
        let mut s = s0;
        let chash = self.grammar.sets_list[s0.0].hash;
        let existing = self.grammar.get_set(chash);
        if existing.is_some() {
            // verbosity dup warning skipped
        } else if let &[back] = self.grammar.sets_list[s0.0].sets.as_slice()
            && (!self.grammar.sets_list[s0.0].r#type.intersects(ST_TAG_UNIFY))
        {
            #[expect(
                clippy::unwrap_used,
                reason = "get_set exists only on a draft, and a draft's set members are the hashes of sets add_set registered in sets_by_contents: only resolving numbers them and empties that map, and resolving runs in finish, which consumes the draft"
            )]
            let tmp = self.grammar.get_set(back).unwrap();
            self.grammar.maybe_used_sets.insert(tmp);
            let th = self.grammar.sets_list[tmp.0].hash;
            self.grammar.set_alias.insert((sh, th));
            self.grammar.destroy_set(s0);
            s = tmp;
        }
        let s = self.grammar.add_set(s)?;
        if self.grammar.sets_list[s.0].empty() {
            return Err(self.error_near(*pos));
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            return Err(self.error_near(*pos));
        }
        Ok(())
    }

    pub(crate) fn parse_options(&mut self, buf: &[char], pos: &mut usize) -> ParseResult {
        *pos += 7;
        self.grammar.lines += skipws_chars(buf, pos, '+', '\0', false);
        if buf[*pos] != '+' || buf[*pos + 1] != '=' {
            return Err(self.error_near(*pos));
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
                return Err(self.error_near(*pos));
            }
        }

        if self.grammar.addcohort_attach {
            self.grammar.has_dep = true;
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            return Err(self.error_near(*pos));
        }
        Ok(())
    }

    pub(crate) fn parse_parentheses(&mut self, buf: &[char], pos: &mut usize) -> ParseResult {
        *pos += 11;
        self.grammar.lines += skipws_chars(buf, pos, '=', '\0', false);
        if buf[*pos] != '=' {
            return Err(self.error_near(*pos));
        }
        *pos += 1;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        while buf[*pos] != '\0' && buf[*pos] != ';' {
            let mut n = *pos;
            self.grammar.lines += skiptows_chars(buf, &mut n, '(', true, false);
            if buf[n] != '(' {
                return Err(self.error_near(*pos));
            }
            n += 1;
            self.grammar.lines += skipws_chars(buf, &mut n, '\0', '\0', false);
            *pos = n;
            self.maybe_quoted(buf, &mut n, *pos)?;
            self.grammar.lines += skiptows_chars(buf, &mut n, ')', true, false);
            let ltok: String = buf[*pos..n].iter().collect();
            let left = self.parse_tag(&ltok, Near::At(*pos))?;
            self.grammar.lines += skipws_chars(buf, &mut n, '\0', '\0', false);
            *pos = n;
            if buf[*pos] == ')' {
                return Err(self.error_near(*pos));
            }
            self.maybe_quoted(buf, &mut n, *pos)?;
            self.grammar.lines += skiptows_chars(buf, &mut n, ')', true, false);
            let rtok: String = buf[*pos..n].iter().collect();
            let right = self.parse_tag(&rtok, Near::At(*pos))?;
            self.grammar.lines += skipws_chars(buf, &mut n, '\0', '\0', false);
            *pos = n;
            if buf[*pos] != ')' {
                return Err(self.error_near(*pos));
            }
            *pos += 1;
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

            let lh = self.grammar.single_tags_list[left.0].hash;
            let rh = self.grammar.single_tags_list[right.0].hash;
            self.grammar.parentheses.insert(lh.get(), rh.get());
            self.grammar.parentheses_reverse.insert(rh.get(), lh.get());
        }
        if self.grammar.parentheses.is_empty() {
            return Err(self.error_near(*pos));
        }
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            return Err(self.error_near(*pos));
        }
        Ok(())
    }

    pub(crate) fn parse_include(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        fname: &str,
    ) -> ParseResult {
        *pos += 7;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);

        let mut local_only_sets = self.only_sets;
        if simplecasecmp(buf, *pos, STR_STATIC) && isspace(buf[*pos + slen(STR_STATIC)]) {
            *pos += slen(STR_STATIC);
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            local_only_sets = true;
        }

        let name_at = *pos;
        let mut n = *pos;
        self.grammar.lines += skiptows_chars(buf, &mut n, '\0', true, false);
        let incname: String = buf[*pos..n].iter().collect();
        *pos = n;
        self.grammar.lines += skipws_chars(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            return Err(self.error_near(*pos));
        }

        let mut expanded = incname.clone();
        if expanded.contains('~') || expanded.contains('$') || expanded.contains('*') {
            expanded = shell_expand(&expanded);
        }
        let dir = dir_prefix(fname);
        let mut abspath = if expanded.starts_with('/') {
            expanded.clone()
        } else {
            format!("{dir}{expanded}")
        };

        // PORT WIDENING, not C++ parity: the C++ stats the including file's
        // directory once and bails. The second chance against the process CWD is
        // kept because it is the only thing that reaches a relative include
        // nested under an absolute- or `~`-rooted parent. It retries the
        // SHELL-EXPANDED name, never the raw token — retrying the token would
        // re-try a literal `~/…` that cannot exist. `dir == "./"` means the two
        // spellings name one file, so there is nothing to retry.
        let cwd_retry = (dir != "./" && !expanded.starts_with('/')).then(|| expanded.clone());

        let mut bytes = match std::fs::read(&abspath) {
            Ok(b) => b,
            Err(primary) => match cwd_retry.and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((p, b)) => {
                    abspath = p;
                    b
                }
                None => {
                    return Err(self.parse_error_at(
                        String::new(),
                        crate::error::ParseErrorKind::IncludeUnreadable {
                            path: abspath,
                            source: primary,
                        },
                    ));
                }
            },
        };
        let entry = self.include_entry(&abspath, name_at)?;
        if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
            bytes.drain(0..3);
        }
        let text = String::from_utf8_lossy(&bytes);
        self.grammarbufs
            .push(SourceBuf::new(abspath, text.as_ref()));
        let gi2 = self.grammarbufs.len() - 1;

        let saved_lines = self.grammar.lines;
        let saved_filebase = std::mem::take(&mut self.filebase);
        let saved_cur_grammar = self.cur_grammar_buf.clone();
        let saved_cur_source = self.cur_source;
        let saved_cur_grammar_n = self.cur_grammar_n;
        let saved_only = self.only_sets;
        let saved_end = self.parse_end_break;
        self.only_sets = local_only_sets;
        // Restore unconditionally, including on failure. The C++ threw straight
        // past these assignments, which was harmless while the only state they
        // guarded was cosmetic; `cur_source` is not — leaving it pointing at the
        // included buffer would give the outer parse's next error a span into
        // the wrong file.
        self.include_chain.push(entry);
        let rv = self.parse_source(gi2);
        self.include_chain.pop();
        self.parse_end_break = saved_end;
        self.only_sets = saved_only;
        self.cur_grammar_n = saved_cur_grammar_n;
        self.cur_source = saved_cur_source;
        self.cur_grammar_buf = saved_cur_grammar;
        self.filebase = saved_filebase;
        self.grammar.lines = saved_lines;
        rv
    }

    // [spec:cg3:req:robustness.cycles+1]
    /// The include-chain entry for `path`, or the cycle it would close.
    ///
    /// DIVERGENCE: the C++ follows an `INCLUDE` wherever it leads, so a file
    /// that includes itself, directly or through others, recurses until the
    /// stack runs out. A file already being included is refused, naming the
    /// chain from it back to itself. Files are compared by canonical path, so
    /// two spellings of one file are one file; the names shown are the paths as
    /// the grammar gave them.
    fn include_entry(
        &mut self,
        path: &str,
        name_at: usize,
    ) -> ParseResult<(std::path::PathBuf, String)> {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
        let Some(from) = self.include_chain.iter().position(|(c, _)| *c == canon) else {
            return Ok((canon, path.to_string()));
        };
        let mut cycle: Vec<String> = self.include_chain[from..]
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        cycle.push(path.to_string());
        let mut err = self.error_near(name_at);
        err.kind = crate::error::ParseErrorKind::IncludeCycle { cycle };
        Err(err)
    }

    fn make_magic_set(&mut self, name: &str) -> ParseResult<SetId> {
        let set_c = self.grammar.allocate_set();
        self.grammar.sets_list[set_c.0].line = 0;
        self.grammar.sets_list[set_c.0].name = name.to_string();
        let t = self.parse_tag(name, Near::Text(&[]))?;
        self.grammar.add_tag_to_set(t, set_c);
        self.grammar.add_set(set_c)
    }

    // [spec:cg3:req:robustness.grammar-text-errors]
    /// Resolve a varstring tag's `{set}` groups into its `vs_sets`/`vs_names`.
    ///
    /// DIVERGENCE: the C++ loop only advances past a `{` once it has found the
    /// `}` closing it, so a `{` with none spins forever. That is refused here,
    /// placed where the tag was written.
    fn resolve_varstring(&mut self, tid: TagId) -> ParseResult {
        let tagstr = self.grammar.single_tags_list[tid.0].tag.clone();
        let mut tbuf: Vec<char> = vec!['\0'];
        tbuf.extend(tagstr.chars());
        tbuf.extend(std::iter::repeat_n('\0', 4));
        let Some(groups) = varstring_groups(&tbuf) else {
            let raw = self.grammar.single_tags_list[tid.0].to_text(false);
            let span = self.locate_text(&raw);
            let kind = crate::error::ParseErrorKind::UnclosedVarstringBrace { tag: raw };
            return Err(self.placed_error(span, kind));
        };
        for (open, close) in groups {
            let tag = self.grammar.single_tags_list.get_mut(tid.0);
            tag.allocate_vs_sets();
            tag.allocate_vs_names();
            let theset: String = tbuf[open + 1..close].iter().collect();
            let tmp = self.parse_set(&theset, Near::Text(&tbuf[open + 1..]))?;
            let setname = self.grammar.sets_list[tmp.0].name.clone();
            let tag = self.grammar.single_tags_list.get_mut(tid.0);
            tag.vs_sets.get_or_insert_default().push(tmp);
            tag.vs_names
                .get_or_insert_default()
                .push(format!("{{{setname}}}"));
        }
        Ok(())
    }

    /// The span of the first place `text` is written in any source this parse
    /// read — for an error about something the parser only checks once every
    /// source has been read, and so has no cursor for.
    fn locate_text(&self, text: &str) -> Option<crate::error::ParseSpan> {
        let needle: Vec<char> = text.chars().collect();
        if needle.is_empty() {
            return None;
        }
        self.grammarbufs
            .iter()
            .enumerate()
            .find_map(|(source, buf)| {
                let hay = &buf.buf[BUF_TEXT_START..];
                let at = hay.windows(needle.len()).position(|w| w == needle)?;
                Some(crate::error::ParseSpan {
                    source,
                    range: at..at + needle.len(),
                })
            })
    }

    fn numeric_branch_split(&mut self) -> ParseResult {
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
                let stripped = self.grammar.remove_numeric_tags(target)?;
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
            let unsafec2 = self.grammar.intern_contextual_test(unsafec);
            let safec2 = self.grammar.intern_contextual_test(safec);

            let orc = self.grammar.allocate_contextual_test();
            self.grammar.contexts_arena[orc.0].ors.push(safec2);
            self.grammar.contexts_arena[orc.0].ors.push(unsafec2);
            let orc = self.grammar.intern_contextual_test(orc);

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
        Ok(())
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-from-u-char-fn+1]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-from-u-char-fn+1]
    fn parse_source(&mut self, gi: usize) -> ParseResult {
        // Clone the shared handle (a refcount bump) so `buf` is owned and does
        // NOT borrow `self`; the char data is immutable, so `#include` may push
        // new `grammarbufs` entries while this parse is in flight without
        // invalidating the slice. Replaces the C++ raw pointers into the stable
        // buffer.
        let fname = self.grammarbufs[gi].name.clone();
        let buf_handle: crate::ast::SrcBuf = self.grammarbufs[gi].buf.clone();
        let buf: &[char] = &buf_handle;
        let len = buf.len();

        if len <= 4 || buf[4] == '\0' {
            return Err(crate::error::ParseError {
                file: basename(Some(&fname)).to_string(),
                line: self.grammar.lines,
                near: String::new(),
                span: None,
                kind: crate::error::ParseErrorKind::EmptyInput,
            });
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
        // C++ `cur_grammar = &buf[4]`: record the buffer handle whose spans the
        // AST nodes opened during this parse belong to (the profiler offsets are
        // computed from `pos` directly, not from this handle), and the index
        // that names it to a diagnostic.
        self.cur_grammar_buf = buf_handle.clone();
        self.cur_source = gi;
        self.cur_grammar_n = id;
        let mut pos = 4usize;
        self.grammar.lines = 1;
        let mut ast_grammar = ASTHelper::new(
            &mut self.ast,
            ASTType::AstGrammar,
            self.grammar.lines as usize,
            4,
            self.cur_grammar_buf.clone(),
        );
        self.filebase = basename(Some(&fname)).to_string();
        self.parse_end_break = false;

        // [spec:cg3:req:errors.parse-reports-all]
        // A recoverable parse error is RESUMABLE, so this loop records it and
        // carries on rather than propagating — the one frame in the parser that
        // cannot use `?`. It is what makes a bad grammar report all of its
        // errors instead of only the first
        // (`[spec:cg3:req:errors.parse-reports-all]`).
        while buf[pos] != '\0' {
            let ast_depth = self.ast.cursor_depth();
            let directive_start = pos;
            if let Err(e) = self.parse_directive(buf, &mut pos, &fname) {
                // The C++ unwound here, which ran every in-scope ~ASTHelper();
                // restore the AST cursor to the pre-directive depth by hand.
                self.ast.truncate_cursor(ast_depth);
                // A failure with no position of its own came from grammar
                // construction rather than from the scanner — a redefined set, a
                // tag that would not build. The offending thing is the whole
                // directive, so it gets the directive's first token; the cursor
                // by now sits at whatever the scanner happened to reach, which
                // for a redefinition is the closing `;`. The loop's own cursor
                // is still on the whitespace before the keyword, so skip it the
                // way `parse_directive` was about to.
                let mut at = directive_start;
                skipws_chars(buf, &mut at, '\0', '\0', false);
                let e = self.locate_unplaced(e, at);
                // Not every parse error is resumable — see
                // `ParseErrorKind::is_fatal`. The C++ terminated at those; this
                // loop reports them and stops reading.
                let fatal = e.kind.is_fatal();
                self.record(e);
                if fatal {
                    break;
                }
                if self.error_count() >= MAX_PARSE_ERRORS {
                    tracing::error!("{}: Too many errors - giving up...", self.filebase);
                    break;
                }
                self.grammar.lines += skipln_chars(buf, &mut pos);
            }
            if self.parse_end_break {
                break;
            }
        }

        ast_grammar.close_id(&mut self.ast, pos, id);
        Ok(())
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-grammar-fn+1]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-grammar-fn+1]
    fn parse_grammar_data(&mut self, gi: usize) -> ParseResult {
        // 1. START anchor at rule 0.
        self.grammar
            .add_anchor(KEYWORDS_STR[Keywords::KStart as usize], 0, true)?;
        // 2. Magic * tag.
        let tany = self.parse_tag(STR_ASTERIK, Near::Text(&[]))?;
        self.grammar.tag_any = self.grammar.single_tags_list[tany.0].hash.get();
        // 3. Dummy set.
        self.grammar.allocate_dummy_set();
        // 4. Magic sets.
        self.make_magic_set(STR_UU_TARGET)?;
        self.make_magic_set(STR_UU_MARK)?;
        self.make_magic_set(STR_UU_ATTACHTO)?;
        let s_left = self.make_magic_set(STR_UU_LEFT)?;
        let s_right = self.make_magic_set(STR_UU_RIGHT)?;
        self.make_magic_set(STR_UU_ENCL)?;
        {
            let set_c = self.grammar.allocate_set();
            self.grammar.sets_list[set_c.0].line = 0;
            self.grammar.sets_list[set_c.0].name = STR_UU_PAREN.to_string();
            self.grammar.sets_list[set_c.0].set_ops.push(S_OR);
            let lh = self.grammar.sets_list[s_left.0].hash;
            let rh = self.grammar.sets_list[s_right.0].hash;
            self.grammar.sets_list[set_c.0].sets.push(lh);
            self.grammar.sets_list[set_c.0].sets.push(rh);
            self.grammar.add_set(set_c)?;
        }
        self.make_magic_set(STR_UU_SAME_BASIC)?;
        for name in STR_UU_C {
            self.make_magic_set(name)?;
        }

        // 5. Parse the grammar text.
        self.parse_source(gi)?;

        // 6. END anchor at the last rule number.
        let end_at = ui32(self.grammar.rule_by_number.capacity().wrapping_sub(1));
        self.grammar
            .add_anchor(KEYWORDS_STR[Keywords::KEnd as usize], end_at, true)?;

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
                self.grammar.add_anchor(&name, number, false)?;
            }
        }

        // 8. Validate JUMP rules.
        self.validate_jumps(&rule_ids);

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
            self.resolve_varstring(*tid)?;
        }

        // 10. Resolve deferred template refs, then check the graph of tests
        // they complete — which only exists once every reference resolved.
        if self.resolve_deferred_templates() {
            self.check_template_cycles();
            self.check_unknown_positions();
        }

        // 11. Numeric-branch splitting.
        self.numeric_branch_split()?;

        // 12. num_tags.
        self.grammar.num_tags = self.grammar.single_tags_list.capacity() as usize;

        Ok(())
    }

    /// C++ `int parse_grammar(const char* buffer, size_t length)` (UTF-8 memory
    /// buffer), for a caller that has bytes and no file behind them.
    ///
    /// Reports head `<utf8-memory>`, because that is the truth about a buffer
    /// with no path. A caller that DOES have one wants
    /// [`parse_grammar_named`](Self::parse_grammar_named).
    // [spec:cg3:req:errors.parse-result]
    pub fn parse_grammar_utf8(&mut self, buffer: &[u8]) -> Result<(), crate::error::Cg3Error> {
        self.parse_grammar_named(buffer, MEMORY_SOURCE_NAME)
    }

    // [spec:cg3:req:diagnostics.source-named]
    /// C++ `int parse_grammar(const char* filename)`, minus the reading: the
    /// bytes plus the path they came from.
    ///
    /// The C++ has a filename entry point and the port did not, so every CLI
    /// read its own file and handed over bytes — leaving the parse to call the
    /// grammar `<utf8-memory>` and head every diagnostic with it. The bytes stay
    /// the caller's to read (the CLIs sniff the `.cg3b` magic off the front
    /// first, and own the message when the read fails); only the name is new.
    ///
    /// A relative `#include` resolves against the directory of the file that
    /// includes it, so naming the top-level source also gives the top-level
    /// file's includes the base directory they always should have had. See
    /// `[dec:cg3:parse-sources-carry-their-name]`.
    pub fn parse_grammar_named(
        &mut self,
        buffer: &[u8],
        filename: &str,
    ) -> Result<(), crate::error::Cg3Error> {
        self.filename = filename.to_string();
        self.filebase = basename(Some(filename)).to_string();
        self.grammar.grammar_size = buffer.len();
        // The top-level file is the first link of the include chain, when it is
        // a file at all.
        let top = std::fs::canonicalize(filename).ok();
        self.include_chain
            .extend(top.map(|c| (c, filename.to_string())));
        let text = String::from_utf8_lossy(buffer);
        self.grammarbufs
            .push(SourceBuf::new(self.filename.clone(), text.as_ref()));
        let gi = self.grammarbufs.len() - 1;
        // A recoverable error stops only its own directive: the loop inside
        // `parse_source` records it and continues, so `Ok` here still means
        // "found errors" if any were accumulated. What reaches this frame is an
        // error the parser could not resume from, and it joins the same list.
        if let Err(hard) = self.parse_grammar_data(gi) {
            self.record(hard);
        }
        // [spec:cg3:req:diagnostics.source-lazy]
        // The names only. A rule's `provenance` indexes this list, so a runtime
        // failure can find the file it was written in without the grammar
        // holding that file's text for the life of the run.
        self.grammar.source_names = self.grammarbufs.iter().map(|b| b.name.clone()).collect();
        let errors = self.take_errors();
        if errors.is_empty() {
            Ok(())
        } else {
            // The sources travel with the errors: the spans index them, and the
            // parser's buffers do not outlive the parse.
            Err(crate::error::GrammarError::Parse {
                errors,
                sources: self.sources(),
            }
            .into())
        }
    }
}

/// The `{`...`}` groups of a varstring, as `(open, close)` indices into
/// `tbuf` (its text behind one leading NUL, with trailing NULs), found the way
/// the C++ loop finds them: the next unescaped `{`, then the next unescaped `}`
/// after it. `None` when a `{` has no `}` after it.
fn varstring_groups(tbuf: &[char]) -> Option<Vec<(usize, usize)>> {
    let mut groups = Vec::new();
    let mut p = 1usize;
    loop {
        skipto_chars(tbuf, &mut p, '{');
        if tbuf[p] == '\0' {
            return Some(groups);
        }
        let mut n = p;
        skipto_chars(tbuf, &mut n, '}');
        if tbuf[n] == '\0' {
            return None;
        }
        groups.push((p, n));
        p = n + 1;
    }
}

/// Best-effort `wordexp` stand-in for the INCLUDE path (`~`/`$`/`*`). Only `~`
/// (home) is expanded; env-var / glob expansion is a deliberate simplification
/// (documented). The C++ uses `wordexp(WRDE_NOCMD|WRDE_UNDEF)`.
fn shell_expand(s: &str) -> String {
    let mut out = s.to_string();
    if (out == "~" || out.starts_with("~/"))
        && let Ok(home) = std::env::var("HOME")
    {
        out = out.replacen('~', &home, 1);
    }
    out
}

impl IGrammarParser for TextualParser {
    type Phase = Draft;

    /// Parses `input` as grammar text with no file behind it; see
    /// [`parse_grammar_utf8`](TextualParser::parse_grammar_utf8).
    fn parse_grammar(&mut self, input: &[u8]) -> Result<(), crate::error::Cg3Error> {
        self.parse_grammar_utf8(input)
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.set-compatible-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.set-compatible-fn]
    fn set_compatible(&mut self, compat: bool) {
        self.option_vislcg_compat = compat;
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.set-verbosity-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.set-verbosity-fn]
    fn set_verbosity(&mut self, level: u32) {
        self.verbosity_level = level;
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.get-grammar-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.get-grammar-fn]
    fn get_grammar(&self) -> &GrammarDraft {
        &self.grammar
    }
}
