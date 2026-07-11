//! `TextualParser` — rule parsing and the directive dispatch loop.
//!
//! Split out of the wave-2 monolithic `textual_parser.rs` (wave 4, w4-file-split-fmt).

#![allow(clippy::too_many_arguments)]

use crate::arena::RuleId;
use crate::ast::{ASTHelper, ASTType};
use crate::contextual_test::{GSR_SPECIALS, POS_JUMP, POS_JUMP_POS};
use crate::inlines::{backtonl, isnl, isspace, skiptows, skipws};
use crate::rule::{
    FLAGS_COUNT, FLAGS_EXCLS, RF_AFTER, RF_BEFORE, RF_ITERATE, RF_KEEPORDER, RF_NOCHILD,
    RF_NOITERATE, RF_REMEMBERX, RF_REVERSE, RF_SAFE, RF_UNSAFE, RF_WITHCHILD, Rule,
};
use crate::set::Set;
use crate::sorted_vector::uint32SortedVector;
use crate::strings::KEYWORDS;
use crate::tag::T_REGEXP;

use super::*;

impl TextualParser {
    // [spec:cg3:def:textual-parser.cg3.textual-parser.add-rule-to-grammar-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.add-rule-to-grammar-fn]
    fn add_rule_to_grammar(&mut self, mut rule: Rule) -> RuleId {
        if self.in_nested_rule {
            rule.section = -3;
            let rid = self.grammar.add_rule(rule);
            self.nested_subrules.push(rid);
            rid
        } else if self.in_section {
            rule.section = self.grammar.sections.len() as i32 - 1;
            self.grammar.add_rule(rule)
        } else if self.in_after_sections {
            rule.section = -2;
            self.grammar.add_rule(rule)
        } else if self.in_null_section {
            rule.section = -3;
            self.grammar.add_rule(rule)
        } else {
            rule.section = -1;
            self.grammar.add_rule(rule)
        }
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-rule-fn]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-rule-fn]
    pub(crate) fn parse_rule(&mut self, buf: &[char], pos: &mut usize, key: KEYWORDS) {
        // C++ `AST_OPEN(Rule)` — also the profiler's rule span start.
        let ast_rule_b = *pos;
        let mut ast_rule = ASTHelper::new(
            &mut self.ast,
            ASTType::AST_Rule,
            self.grammar.lines as usize,
            pptr(buf, *pos),
        );
        let mut rule = self.grammar.allocate_rule();
        rule.line = self.grammar.lines;
        rule.r#type = key;

        // Leading wordform.
        let mut lp = *pos;
        backtonl(buf, &mut lp);
        self.grammar.lines += skipws(buf, &mut lp, '\0', '\0', false);
        if lp != *pos && lp < *pos {
            let mut n = lp;
            self.maybe_quoted(buf, &mut n, lp);
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            let token: String = buf[lp..n].iter().collect();
            let wform = self.parse_tag(&token, &buf[lp..]);
            rule.wordform = Some(wform);
        }

        *pos += slen(KEYWORDS_STR[key as usize]);
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        if buf[*pos] == ':' {
            *pos += 1;
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '(', false, false);
            let name: String = buf[*pos..n].iter().collect();
            if name.is_empty() {
                tracing::warn!("{}: Warning: Rule had : but no name.", self.filebase);
            } else {
                rule.set_name(Some(&name));
            }
            *pos = n;
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        if key == KEYWORDS::K_EXTERNAL {
            if simplecasecmp(buf, *pos, STR_ONCE) {
                *pos += slen(STR_ONCE);
                rule.r#type = KEYWORDS::K_EXTERNAL_ONCE;
            } else if simplecasecmp(buf, *pos, STR_ALWAYS) {
                *pos += slen(STR_ALWAYS);
                rule.r#type = KEYWORDS::K_EXTERNAL_ALWAYS;
            } else {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

            let mut n = *pos;
            if buf[n] == '"' {
                n += 1;
                crate::inlines::skipto_nospan(buf, &mut n, '"');
                if buf[n] != '"' {
                    self.error_near(&buf[*pos..]);
                }
            }
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            let cmd: String = if buf[*pos] == '"' {
                // strip surrounding quotes
                buf[*pos + 1..n - 1].iter().collect()
            } else {
                buf[*pos..n].iter().collect()
            };
            let ext = self.grammar.allocate_tag(&cmd);
            rule.varname = self.grammar.single_tags_list[ext.0].hash;
            *pos = n;
        }

        let flags = self.parse_rule_flags(buf, pos);
        rule.flags = flags.flags;
        rule.sub_reading = flags.sub_reading;

        if !self.section_flags.flags.is_empty() {
            for i in 0..FLAGS_COUNT {
                let f = crate::rule::RuleFlags::from_bits_retain(1u64 << i);
                if self.section_flags.flags.intersects(f) && !rule.flags.intersects(FLAGS_EXCLS[i])
                {
                    rule.flags |= f;
                }
            }
        }
        if self.section_flags.sub_reading != 0 && rule.sub_reading == 0 {
            rule.sub_reading = self.section_flags.sub_reading;
        }

        if !rule.flags.intersects(RF_ITERATE | RF_NOITERATE)
            && key != KEYWORDS::K_SELECT
            && key != KEYWORDS::K_REMOVE
            && key != KEYWORDS::K_IFF
            && key != KEYWORDS::K_DELIMIT
            && key != KEYWORDS::K_REMCOHORT
            && key != KEYWORDS::K_MOVE
            && key != KEYWORDS::K_SWITCH
        {
            rule.flags |= RF_NOITERATE;
        }
        if key == KEYWORDS::K_UNMAP && !rule.flags.intersects(RF_SAFE | RF_UNSAFE) {
            rule.flags |= RF_SAFE;
        }
        if key == KEYWORDS::K_SETPARENT && !rule.flags.intersects(RF_SAFE | RF_UNSAFE) {
            rule.flags |= if self.safe_setparent {
                RF_SAFE
            } else {
                RF_UNSAFE
            };
        }

        if rule.flags.intersects(RF_WITHCHILD) {
            self.grammar.has_dep = true;
            let s = self.parse_set_inline_wrapper(buf, pos);
            rule.childset1 = self.grammar.sets_list[s.0].hash;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        } else if rule.flags.intersects(RF_NOCHILD) {
            rule.childset1 = 0;
        }

        lp = *pos;
        if key == KEYWORDS::K_SUBSTITUTE || key == KEYWORDS::K_EXECUTE {
            let saved = self.no_isets;
            self.no_isets = false;
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.no_isets = saved;
            Set::reindex(&mut self.grammar, s);
            rule.sublist = Some(s);
            if self.grammar.sets_list[s.0].empty() {
                self.error_near(&buf[lp..]);
            }
            if !is_mapping_list(&self.grammar, s) {
                self.error_near(&buf[lp..]);
            }
        }

        if rule.sub_reading == GSR_SPECIALS::GSR_ANY as i32
            && (key == KEYWORDS::K_MAP
                || key == KEYWORDS::K_ADD
                || key == KEYWORDS::K_REPLACE
                || key == KEYWORDS::K_SUBSTITUTE
                || key == KEYWORDS::K_COPY
                || key == KEYWORDS::K_COPYCOHORT)
        {
            self.error_bare();
        }

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        lp = *pos;
        if matches!(
            key,
            KEYWORDS::K_MAP
                | KEYWORDS::K_ADD
                | KEYWORDS::K_REPLACE
                | KEYWORDS::K_APPEND
                | KEYWORDS::K_SUBSTITUTE
                | KEYWORDS::K_COPY
                | KEYWORDS::K_COPYCOHORT
                | KEYWORDS::K_ADDRELATIONS
                | KEYWORDS::K_ADDRELATION
                | KEYWORDS::K_SETRELATIONS
                | KEYWORDS::K_SETRELATION
                | KEYWORDS::K_REMRELATIONS
                | KEYWORDS::K_REMRELATION
                | KEYWORDS::K_SETVARIABLE
                | KEYWORDS::K_REMVARIABLE
                | KEYWORDS::K_ADDCOHORT
                | KEYWORDS::K_JUMP
                | KEYWORDS::K_SPLITCOHORT
                | KEYWORDS::K_MERGECOHORTS
                | KEYWORDS::K_RESTORE
        ) {
            let saved = self.no_isets;
            self.no_isets = false;
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.no_isets = saved;
            Set::reindex(&mut self.grammar, s);
            rule.maplist = Some(s);
            if self.grammar.sets_list[s.0].empty() {
                self.error_near(&buf[lp..]);
            }
            if !is_mapping_list(&self.grammar, s) {
                self.error_near(&buf[lp..]);
            }
        }

        let mut copy_except = false;
        if (key == KEYWORDS::K_COPY || key == KEYWORDS::K_COPYCOHORT || key == KEYWORDS::K_REPLACE)
            && simplecasecmp(buf, *pos, STR_EXCEPT)
        {
            *pos += slen(STR_EXCEPT);
            copy_except = true;
        }

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        lp = *pos;
        if key == KEYWORDS::K_ADDRELATIONS
            || key == KEYWORDS::K_SETRELATIONS
            || key == KEYWORDS::K_REMRELATIONS
            || key == KEYWORDS::K_SETVARIABLE
            || copy_except
        {
            let saved = self.no_isets;
            self.no_isets = false;
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.no_isets = saved;
            Set::reindex(&mut self.grammar, s);
            rule.sublist = Some(s);
            if self.grammar.sets_list[s.0].empty() {
                self.error_near(&buf[lp..]);
            }
            if !is_mapping_list(&self.grammar, s) {
                self.error_near(&buf[lp..]);
            }
        }

        if key == KEYWORDS::K_ADDCOHORT {
            if simplecasecmp(buf, *pos, STR_AFTER) {
                *pos += slen(STR_AFTER);
                rule.r#type = KEYWORDS::K_ADDCOHORT_AFTER;
            } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                *pos += slen(STR_BEFORE);
                rule.r#type = KEYWORDS::K_ADDCOHORT_BEFORE;
            } else {
                self.error_near(&buf[*pos..]);
            }
        }

        if key == KEYWORDS::K_ADD
            || key == KEYWORDS::K_MAP
            || key == KEYWORDS::K_SUBSTITUTE
            || key == KEYWORDS::K_COPY
            || key == KEYWORDS::K_COPYCOHORT
        {
            if simplecasecmp(buf, *pos, STR_AFTER) {
                *pos += slen(STR_AFTER);
                rule.flags |= RF_AFTER;
            } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                *pos += slen(STR_BEFORE);
                rule.flags |= RF_BEFORE;
            }
            if key != KEYWORDS::K_COPYCOHORT && (rule.flags.intersects(RF_BEFORE | RF_AFTER)) {
                let s = self.parse_set_inline_wrapper(buf, pos);
                rule.childset1 = self.grammar.sets_list[s.0].hash;
            }
        }

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_TARGET) {
            *pos += slen(STR_TARGET);
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        if simplecasecmp(buf, *pos, G_FLAGS[FL_WITHCHILD]) {
            *pos += slen(G_FLAGS[FL_WITHCHILD]);
            let s = self.parse_set_inline_wrapper(buf, pos);
            self.grammar.has_dep = true;
            rule.flags |= RF_WITHCHILD;
            rule.flags &= !RF_NOCHILD;
            rule.childset1 = self.grammar.sets_list[s.0].hash;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        } else if simplecasecmp(buf, *pos, G_FLAGS[FL_NOCHILD]) {
            *pos += slen(G_FLAGS[FL_NOCHILD]);
            rule.flags |= RF_NOCHILD;
            rule.flags &= !RF_WITHCHILD;
            rule.childset1 = 0;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        }

        let s = self.parse_set_inline_wrapper(buf, pos);
        rule.target = self.grammar.sets_list[s.0].hash;

        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        if simplecasecmp(buf, *pos, STR_IF) {
            *pos += slen(STR_IF);
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

        while buf[*pos] != '\0' && buf[*pos] == '(' {
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            self.parse_contextual_tests(buf, pos, &mut rule);
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if buf[*pos] != ')' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        }

        if matches!(
            key,
            KEYWORDS::K_SETPARENT
                | KEYWORDS::K_SETCHILD
                | KEYWORDS::K_ADDRELATIONS
                | KEYWORDS::K_ADDRELATION
                | KEYWORDS::K_SETRELATIONS
                | KEYWORDS::K_SETRELATION
                | KEYWORDS::K_REMRELATIONS
                | KEYWORDS::K_REMRELATION
                | KEYWORDS::K_MOVE
                | KEYWORDS::K_SWITCH
                | KEYWORDS::K_MERGECOHORTS
                | KEYWORDS::K_COPYCOHORT
        ) {
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if key == KEYWORDS::K_MOVE {
                if simplecasecmp(buf, *pos, STR_AFTER) {
                    *pos += slen(STR_AFTER);
                    rule.r#type = KEYWORDS::K_MOVE_AFTER;
                } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                    *pos += slen(STR_BEFORE);
                    rule.r#type = KEYWORDS::K_MOVE_BEFORE;
                } else {
                    self.error_near(&buf[*pos..]);
                }
            } else if key == KEYWORDS::K_SWITCH || key == KEYWORDS::K_MERGECOHORTS {
                if simplecasecmp(buf, *pos, STR_WITH) {
                    *pos += slen(STR_WITH);
                } else {
                    self.error_near(&buf[*pos..]);
                }
            } else if simplecasecmp(buf, *pos, STR_TO) {
                *pos += slen(STR_TO);
            } else if simplecasecmp(buf, *pos, STR_FROM) {
                *pos += slen(STR_FROM);
                rule.flags |= RF_REVERSE;
            } else {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);

            if key == KEYWORDS::K_COPYCOHORT && (!rule.flags.intersects(RF_REVERSE)) {
                if simplecasecmp(buf, *pos, STR_AFTER) {
                    *pos += slen(STR_AFTER);
                    rule.flags |= RF_AFTER;
                } else if simplecasecmp(buf, *pos, STR_BEFORE) {
                    *pos += slen(STR_BEFORE);
                    rule.flags |= RF_BEFORE;
                }
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }

            if key == KEYWORDS::K_MOVE || key == KEYWORDS::K_COPYCOHORT {
                if simplecasecmp(buf, *pos, G_FLAGS[FL_WITHCHILD]) {
                    *pos += slen(G_FLAGS[FL_WITHCHILD]);
                    self.grammar.has_dep = true;
                    let s = self.parse_set_inline_wrapper(buf, pos);
                    rule.childset2 = self.grammar.sets_list[s.0].hash;
                    self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                } else if simplecasecmp(buf, *pos, G_FLAGS[FL_NOCHILD]) {
                    *pos += slen(G_FLAGS[FL_NOCHILD]);
                    rule.childset2 = 0;
                    self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                }
            }

            lp = *pos;
            while buf[*pos] != '\0' && buf[*pos] == '(' {
                *pos += 1;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                self.parse_contextual_dependency_tests(buf, pos, &mut rule);
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                if buf[*pos] != ')' {
                    self.error_near(&buf[*pos..]);
                }
                *pos += 1;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if rule.dep_tests.is_empty() {
                self.error_near(&buf[lp..]);
            }
            if key != KEYWORDS::K_MERGECOHORTS {
                rule.dep_target = rule.dep_tests.back().copied();
                rule.dep_tests.pop_back();
            }
        }
        if key == KEYWORDS::K_SETPARENT
            || key == KEYWORDS::K_SETCHILD
            || key == KEYWORDS::K_SPLITCOHORT
            || key == KEYWORDS::K_MERGECOHORTS
        {
            self.grammar.has_dep = true;
        }
        if key == KEYWORDS::K_SETRELATION
            || key == KEYWORDS::K_SETRELATIONS
            || key == KEYWORDS::K_ADDRELATION
            || key == KEYWORDS::K_ADDRELATIONS
            || key == KEYWORDS::K_REMRELATION
            || key == KEYWORDS::K_REMRELATIONS
            || key == KEYWORDS::K_MERGECOHORTS
        {
            self.grammar.has_relations = true;
        }
        if key == KEYWORDS::K_COPYCOHORT && (!rule.flags.intersects(RF_BEFORE | RF_AFTER)) {
            rule.flags |= RF_AFTER;
        }

        if !rule.flags.intersects(RF_REMEMBERX) {
            let mut found = false;
            if let Some(dt) = rule.dep_target {
                let c = &self.grammar.contexts_arena[dt.0];
                if c.pos.intersects(POS_JUMP) && c.jump_pos == POS_JUMP_POS::JUMP_MARK as i8 {
                    found = true;
                }
            }
            if !found {
                for &it in rule.tests.iter() {
                    let c = &self.grammar.contexts_arena[it.0];
                    if c.pos.intersects(POS_JUMP) && c.jump_pos == POS_JUMP_POS::JUMP_MARK as i8 {
                        found = true;
                        break;
                    }
                }
                for &it in rule.dep_tests.iter() {
                    let c = &self.grammar.contexts_arena[it.0];
                    if c.pos.intersects(POS_JUMP) && c.jump_pos == POS_JUMP_POS::JUMP_MARK as i8 {
                        found = true;
                        break;
                    }
                }
            }
            if found {
                rule.flags |= RF_REMEMBERX | RF_KEEPORDER;
            }
        }

        if key == KEYWORDS::K_WITH {
            rule.flags |= RF_KEEPORDER;
            self.grammar.lines += skipws(buf, pos, '{', ';', false);
            if buf[*pos] == '{' {
                *pos += 1;
                let prev_in_nested = self.in_nested_rule;
                let prev_sub = std::mem::take(&mut self.nested_subrules);
                self.in_nested_rule = true;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                loop {
                    if !self.maybe_parse_rule(buf, pos) {
                        self.error_near(&buf[*pos..]);
                    }
                    self.grammar.lines += skipws(buf, pos, '}', ';', false);
                    if buf[*pos] == ';' {
                        *pos += 1;
                        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                    }
                    if buf[*pos] == '}' {
                        break;
                    }
                }
                *pos += 1;
                rule.sub_rules = std::mem::take(&mut self.nested_subrules);
                self.nested_subrules = prev_sub;
                self.in_nested_rule = prev_in_nested;
            }
        }

        rule.reverse_contextual_tests();

        let mut destroy = self.only_sets;
        if let Some(re) = &self.nrules {
            // UNANCHORED search over the rule name.
            if !re.is_match(&rule.name) {
                destroy = true;
            }
        }
        if let Some(re) = &self.nrules_inv {
            if re.is_match(&rule.name) {
                destroy = true;
            }
        }

        self.grammar.lines += skipws(buf, pos, ';', '\0', false);
        if buf[*pos] != ';' {
            tracing::warn!(
                "{}: Warning: Expected closing ; after previous rule!",
                self.filebase
            );
        }

        if destroy {
            // `destroyRule` on a heap Rule*; the port's local value is just dropped.
            // C++ `AST_CLOSE(p)`.
            ast_rule.close(&mut self.ast, pptr(buf, *pos));
        } else {
            let rid = self.add_rule_to_grammar(rule);
            if let Some(prof) = self.profiler.as_mut() {
                // profiler->addRule(rule->number + 1, cur_grammar_n,
                //                   cur_ast->b - cur_grammar, p - cur_grammar)
                // Offsets are relative to the grammar text (buffer index - 4).
                let rnum = self.grammar.rule_by_number.get(rid.0).number;
                prof.add_rule(rnum + 1, self.cur_grammar_n, ast_rule_b - 4, *pos - 4);
            }
            // C++ `AST_CLOSE_ID(p, rule->number + 1)`.
            let rnum = self.grammar.rule_by_number.get(rid.0).number;
            ast_rule.close_id(&mut self.ast, pptr(buf, *pos), rnum + 1);
        }
    }
}

impl TextualParser {
    /// One iteration of the `parseFromUChar` main loop (the C++ `try { ... }`
    /// body): progress print, leading `SKIPWS`, and the keyword dispatch chain.
    pub(crate) fn parse_directive(&mut self, buf: &[char], pos: &mut usize, fname: &str) {
        let p0 = *pos;
        if self.verbosity_level > 0 && self.grammar.lines % 500 == 0 {
            tracing::info!("Parsing line {}", self.grammar.lines);
        }
        self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
        let _ = p0;

        if is_icase_kw(buf, *pos, "DELIMITERS", "delimiters") != 0 {
            if self.grammar.delimiters.is_some() {
                self.error_near(&buf[*pos..]);
            }
            let d = self.grammar.allocate_set();
            self.grammar.sets_list[d.0].line = self.grammar.lines;
            self.grammar.sets_list[d.0].name = STR_DELIMITSET.to_string();
            self.grammar.delimiters = Some(d);
            *pos += 10;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.parse_tag_list(buf, pos, d, false);
            let d = self.grammar.add_set(d);
            self.grammar.delimiters = Some(d);
            if self.grammar.sets_list[d.0].empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "SOFT-DELIMITERS", "soft-delimiters") != 0 {
            if self.grammar.soft_delimiters.is_some() {
                self.error_near(&buf[*pos..]);
            }
            let d = self.grammar.allocate_set();
            self.grammar.sets_list[d.0].line = self.grammar.lines;
            self.grammar.sets_list[d.0].name = STR_SOFTDELIMITSET.to_string();
            self.grammar.soft_delimiters = Some(d);
            *pos += 15;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.parse_tag_list(buf, pos, d, false);
            let d = self.grammar.add_set(d);
            self.grammar.soft_delimiters = Some(d);
            if self.grammar.sets_list[d.0].empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "TEXT-DELIMITERS", "text-delimiters") != 0 {
            if self.grammar.text_delimiters.is_some() {
                self.error_near(&buf[*pos..]);
            }
            let d = self.grammar.allocate_set();
            self.grammar.sets_list[d.0].line = self.grammar.lines;
            self.grammar.sets_list[d.0].name = STR_TEXTDELIMITSET.to_string();
            self.grammar.text_delimiters = Some(d);
            *pos += 15;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.parse_tag_list(buf, pos, d, false);
            let d = self.grammar.add_set(d);
            self.grammar.text_delimiters = Some(d);
            if self.grammar.sets_list[d.0].empty() {
                self.error_near(&buf[*pos..]);
            }
            let mut the_tags = crate::tag::TagList::new();
            let trie = self.grammar.sets_list[d.0].trie.clone();
            let trie_sp = self.grammar.sets_list[d.0].trie_special.clone();
            crate::tag_trie::trie_get_tag_list_append(&trie, &mut the_tags, &self.grammar);
            crate::tag_trie::trie_get_tag_list_append(&trie_sp, &mut the_tags, &self.grammar);
            for tag in the_tags {
                if !self.grammar.single_tags_list[tag.0]
                    .r#type
                    .intersects(T_REGEXP)
                {
                    self.error_bare();
                }
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "MAPPING-PREFIX", "mapping-prefix") != 0 {
            if self.seen_mapping_prefix != 0 {
                self.inc_error_count();
            }
            self.seen_mapping_prefix = self.grammar.lines;
            *pos += 14;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, ';', false, false);
            let token: String = buf[*pos..n].iter().collect();
            *pos = n;
            self.grammar.mapping_prefix = token.chars().next().unwrap_or('\0');
            if self.grammar.mapping_prefix == '\0' {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "PREFERRED-TARGETS", "preferred-targets") != 0 {
            *pos += 17;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                let h = self.grammar.single_tags_list[t.0].hash;
                self.grammar.preferred_targets.push(h);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if self.grammar.preferred_targets.is_empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "REOPEN-MAPPINGS", "reopen-mappings") != 0 {
            *pos += 15;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                let h = self.grammar.single_tags_list[t.0].hash;
                self.grammar.reopen_mappings.insert(h);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if self.grammar.reopen_mappings.empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "STATIC-SETS", "static-sets") != 0 {
            *pos += 11;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let name: String = buf[*pos..n].iter().collect();
                self.grammar.static_sets.push(name);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if self.grammar.static_sets.is_empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if let Some(icn) = self.match_cmdargs(buf, *pos) {
            *pos += icn;
            self.grammar.lines += skipws(buf, pos, '+', '\0', false);
            if buf[*pos] != '+' || buf[*pos + 1] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 2;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let s = *pos;
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            let args: String = buf[s..*pos].iter().collect();
            if icn == 16 {
                self.grammar.cmdargs_override = args;
            } else {
                self.grammar.cmdargs = args;
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "UNDEF-SETS", "undef-sets") != 0 {
            *pos += 10;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut did = false;
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let name: String = buf[*pos..n].iter().collect();
                if self.grammar.undef_set(&name).is_none() {
                    tracing::warn!("{}: Warning: Set {} wasn't defined.", self.filebase, name);
                }
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
                did = true;
            }
            if !did {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "SETS", "sets") != 0 {
            *pos += 4;
        } else if is_icase_kw(buf, *pos, "LIST-TAGS", "list-tags") != 0 {
            *pos += 9;
            self.grammar.lines += skipws(buf, pos, '+', '\0', false);
            if buf[*pos] != '+' || buf[*pos + 1] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 2;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut tmp = uint32SortedVector::new();
            self.list_tags.swap(&mut tmp);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                tmp.insert(self.grammar.single_tags_list[t.0].hash);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if tmp.empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
            self.list_tags.swap(&mut tmp);
        } else if is_icase_kw(buf, *pos, "LIST", "list") != 0
            || is_icase_kw(buf, *pos, "OLIST", "olist") != 0
        {
            self.parse_list(buf, pos);
        } else if is_icase_kw(buf, *pos, "SET", "set") != 0 {
            self.parse_set_def(buf, pos);
        } else if is_icase_kw(buf, *pos, "MAPPINGS", "mappings") != 0 {
            *pos += 8;
            self.section_before(buf, pos);
        } else if is_icase_kw(buf, *pos, "CORRECTIONS", "corrections") != 0 {
            *pos += 11;
            self.section_before(buf, pos);
        } else if is_icase_kw(buf, *pos, "BEFORE-SECTIONS", "before-sections") != 0 {
            *pos += 15;
            self.section_before(buf, pos);
        } else if is_icase_kw(buf, *pos, "SECTION", "section") != 0 {
            *pos += 7;
            self.section_numbered(buf, pos);
        } else if is_icase_kw(buf, *pos, "CONSTRAINTS", "constraints") != 0 {
            *pos += 11;
            self.section_numbered(buf, pos);
        } else if is_icase_kw(buf, *pos, "AFTER-SECTIONS", "after-sections") != 0 {
            *pos += 14;
            if !self.only_sets {
                self.in_before_sections = false;
                self.in_section = false;
                self.in_after_sections = true;
                self.in_null_section = false;
            }
            self.maybe_anchorish(buf, pos);
        } else if is_icase_kw(buf, *pos, "NULL-SECTION", "null-section") != 0 {
            *pos += 12;
            if !self.only_sets {
                self.in_before_sections = false;
                self.in_section = false;
                self.in_after_sections = false;
                self.in_null_section = true;
            }
            self.maybe_anchorish(buf, pos);
        } else if is_icase_kw(buf, *pos, "SUBREADINGS", "subreadings") != 0 {
            *pos += 11;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            if buf[*pos] == 'L' || buf[*pos] == 'l' {
                self.grammar.sub_readings_ltr = true;
            } else if buf[*pos] == 'R' || buf[*pos] == 'r' {
                self.grammar.sub_readings_ltr = false;
            } else {
                self.error_near(&buf[*pos..]);
            }
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            *pos = n;
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "OPTIONS", "options") != 0 {
            self.parse_options(buf, pos);
        } else if is_icase_kw(buf, *pos, "STRICT-TAGS", "strict-tags") != 0 {
            *pos += 11;
            self.grammar.lines += skipws(buf, pos, '+', '\0', false);
            if buf[*pos] != '+' || buf[*pos + 1] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 2;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut tmp = uint32SortedVector::new();
            self.strict_tags.swap(&mut tmp);
            while buf[*pos] != '\0' && buf[*pos] != ';' {
                let mut n = *pos;
                self.maybe_quoted(buf, &mut n, *pos);
                self.grammar.lines += skiptows(buf, &mut n, ';', true, false);
                let token: String = buf[*pos..n].iter().collect();
                let t = self.parse_tag(&token, &buf[*pos..]);
                tmp.insert(self.grammar.single_tags_list[t.0].hash);
                *pos = n;
                self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            }
            if tmp.empty() {
                self.error_near(&buf[*pos..]);
            }
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
            self.strict_tags.swap(&mut tmp);
        } else if is_icase_kw(buf, *pos, "ANCHOR", "anchor") != 0 {
            *pos += 6;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            self.parse_anchorish(buf, pos, false);
        } else if is_icase_kw(buf, *pos, "INCLUDE", "include") != 0 {
            self.parse_include(buf, pos, fname);
        } else if is_icase_kw(buf, *pos, "TEMPLATE", "template") != 0 {
            let line = self.grammar.lines;
            *pos += 8;
            self.grammar.lines += skipws(buf, pos, '\0', '\0', false);
            let mut n = *pos;
            self.grammar.lines += skiptows(buf, &mut n, '\0', true, false);
            let name: String = buf[*pos..n].iter().collect();
            *pos = n;
            self.grammar.lines += skipws(buf, pos, '=', '\0', false);
            if buf[*pos] != '=' {
                self.error_near(&buf[*pos..]);
            }
            *pos += 1;
            let saved = self.no_itmpls;
            self.no_itmpls = false;
            let t = self.parse_contextual_test_list(buf, pos, None, true);
            self.no_itmpls = saved;
            self.grammar.contexts_arena[t.0].line = line;
            self.grammar.add_template(t, &name);
            self.grammar.lines += skipws(buf, pos, ';', '\0', false);
            if buf[*pos] != ';' {
                self.error_near(&buf[*pos..]);
            }
        } else if is_icase_kw(buf, *pos, "PARENTHESES", "parentheses") != 0 {
            self.parse_parentheses(buf, pos);
        } else if is_icase_kw(buf, *pos, "END", "end") != 0 {
            if (isnl(buf[*pos - 1]) || isspace(buf[*pos - 1]))
                && (buf[*pos + 3] == '\0' || isnl(buf[*pos + 3]) || isspace(buf[*pos + 3]))
            {
                // break the whole loop: signalled by leaving pos at the NUL.
                self.parse_end_break = true;
                return;
            }
            *pos += 1;
        } else if self.maybe_parse_rule(buf, pos) {
            // Has to happen last (so MAPPINGS is not parsed as MAP PINGS).
        } else {
            let n = *pos;
            if buf[*pos] == ';' || buf[*pos] == '"' {
                if buf[*pos] == '"' {
                    *pos += 1;
                    crate::inlines::skipto_nospan(buf, pos, '"');
                    if buf[*pos] != '"' {
                        self.error_near(&buf[n..]);
                    }
                }
                self.grammar.lines += skiptows(buf, pos, '\0', false, false);
            }
            if buf[*pos] != '\0'
                && buf[*pos] != ';'
                && buf[*pos] != '"'
                && !isnl(buf[*pos])
                && !isspace(buf[*pos])
            {
                self.error_near(&buf[*pos..]);
            }
            if isnl(buf[*pos]) {
                self.grammar.lines += 1;
            }
            *pos += 1;
        }
    }
}
