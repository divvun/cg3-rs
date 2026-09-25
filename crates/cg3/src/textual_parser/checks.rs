//! `TextualParser` — the checks that need the whole grammar: JUMP targets, and
//! the shape of the graph contextual tests form once template references are
//! resolved.
//!
//! These run after the buffer walk, when there is no cursor left to point
//! with, so their errors are placed by the spans recorded as each test was
//! parsed (`ctx_spans`).

use std::collections::{HashMap, HashSet};

use crate::arena::{CtxId, RuleId, SetId};
use crate::contextual_test::{POS_TMPL_OVERRIDE, POS_UNKNOWN};
use crate::error::{ParseError, ParseErrorKind, ParseSpan};
use crate::grammar::GrammarCore;
use crate::inlines::{hash_value_str, isspace, ui32};
use crate::set::{ST_SET_UNIFY, ST_TAG_UNIFY};
use crate::strings::Keywords;
use crate::tag::{T_SPECIAL, TagList};
use crate::tag_trie::trie_get_tag_list_append;
use crate::uextras::basename;

use super::{BUF_TEXT_START, NEAR_CONTEXT_CHARS, ParseResult, TextualParser};

/// Where a depth-first walk has got to with one test.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Walk {
    /// On the path from the root being walked.
    OnPath,
    /// Every test reachable from it has been walked.
    Done,
}

impl TextualParser {
    // [spec:cg3:req:diagnostics.span]
    /// The span of the text from `begin` to `end` in the buffer being parsed,
    /// with surrounding whitespace left out — what an error about a construct
    /// found after the buffer walk points at.
    pub(super) fn trimmed_span(&self, buf: &[char], begin: usize, end: usize) -> ParseSpan {
        let mut b = begin.min(end);
        let mut e = end;
        while b < e && (isspace(buf[b]) || buf[b] == '\0') {
            b += 1;
        }
        while e > b && (isspace(buf[e - 1]) || buf[e - 1] == '\0') {
            e -= 1;
        }
        ParseSpan {
            source: self.cur_source,
            range: b.saturating_sub(BUF_TEXT_START)..e.saturating_sub(BUF_TEXT_START),
        }
    }

    /// An error found once the whole grammar has been read, placed at `span`
    /// when there is one: the file and line are the ones the span points
    /// into, which for an `INCLUDE`d file are not the file the parse started
    /// from or the line it finished on.
    pub(super) fn placed_error(&self, span: Option<ParseSpan>, kind: ParseErrorKind) -> ParseError {
        let Some(sp) = span else {
            return self.parse_error_at(String::new(), kind);
        };
        let src = &self.grammarbufs[sp.source];
        let before = src.buf.get(BUF_TEXT_START..sp.range.start + BUF_TEXT_START);
        let line = before
            .unwrap_or_default()
            .iter()
            .filter(|&&c| c == '\n')
            .count()
            + 1;
        let near = src
            .buf
            .get(sp.range.start + BUF_TEXT_START..)
            .unwrap_or_default()
            .iter()
            .take(NEAR_CONTEXT_CHARS)
            .take_while(|&&c| c != '\0' && c != '\n' && c != '\r')
            .collect();
        ParseError {
            file: basename(Some(&src.name)).to_string(),
            line: ui32(line),
            near,
            span: Some(sp),
            kind,
        }
    }

    /// Register a `TEMPLATE`, remembering where its test was written: the
    /// place `parse_contextual_test_list` recorded last.
    pub(super) fn add_template_def(&mut self, t: CtxId, name: &str) -> ParseResult {
        self.grammar.add_template(t, name)?;
        let def = self.ctx_spans.get(&t).and_then(|at| at.last()).cloned();
        self.template_defs.extend(def.map(|d| (t, d)));
        Ok(())
    }

    // [spec:cg3:req:robustness.grammar-text-errors]
    /// Step 8 of `parse_grammar`: every JUMP names an anchor, unless its target
    /// relies on unification or a varstring and so cannot be checked here.
    ///
    /// DIVERGENCE: the C++ reads the target through `getTagList_Any`, which
    /// takes a composite set's members as set NUMBERS; before `reindex` they
    /// are content hashes, so a JUMP whose maplist is built from other sets
    /// indexes far past the set list. Members are resolved by hash here.
    pub(super) fn validate_jumps(&mut self, rule_ids: &[RuleId]) {
        for rid in rule_ids {
            let rule = &self.grammar.rule_by_number[rid.0];
            let (Keywords::KJump, Some(maplist)) = (rule.r#type, rule.maplist) else {
                continue;
            };
            let mut the_tags = TagList::new();
            tag_list_any_by_hash(&self.grammar, maplist, &mut the_tags);
            let Some(&to) = the_tags.first() else {
                continue;
            };
            let tag = &self.grammar.single_tags_list[to.0];
            if tag.r#type.intersects(T_SPECIAL) {
                continue;
            }
            if self.grammar.anchors.find(tag.hash.get()) == self.grammar.anchors.end() {
                self.record(self.parse_error_at(String::new(), ParseErrorKind::Syntax));
            }
        }
    }

    /// Step 10 of `parse_grammar`: point each deferred template reference at
    /// its template. Returns whether every one resolved — a reference that did
    /// not still holds the name hash in place of a test, so nothing may walk
    /// the graph of tests through it.
    pub(super) fn resolve_deferred_templates(&mut self) -> bool {
        let mut deferred: Vec<(CtxId, (usize, String))> = self
            .deferred_tmpls
            .iter()
            .map(|(&k, v)| (k, v.clone()))
            .collect();
        deferred.sort_by_key(|(t, (line, _))| (*line, *t));
        let mut resolved = true;
        for (t, (line, name)) in deferred {
            let cn = hash_value_str(&name, 0);
            let Some(&real) = self.grammar.templates.get(&cn) else {
                // The line is the deferred reference's own, not `grammar.lines`:
                // resolution happens after the whole buffer has been walked, so
                // the running line counter points at the end of the grammar. The
                // C++ printed the correct line in a separate message beside
                // an error value that carried the wrong one; there is one
                // diagnostic now, and it is the right one.
                let mut e =
                    self.parse_error_at(String::new(), ParseErrorKind::UnknownTemplate { name });
                e.line = ui32(line);
                self.record(e);
                resolved = false;
                continue;
            };
            self.grammar.contexts_arena[t.0].tmpl = Some(real);
        }
        resolved
    }

    /// The tests that evaluating `c` runs next at the same position, before any
    /// test of its own can decide: its template, and the first alternative of
    /// an inline OR group. Later alternatives only run once earlier ones have
    /// failed, and a LINK only runs from the cohort its test found.
    fn runs_first(&self, c: CtxId) -> Vec<CtxId> {
        let ctx = &self.grammar.contexts_arena[c.0];
        ctx.tmpl
            .into_iter()
            .chain(ctx.ors.first().copied())
            .collect()
    }

    // [spec:cg3:req:robustness.cycles+1]
    /// Refuse every template that must refer to itself again, at the same
    /// position, before any test can decide: `TEMPLATE a = T:a ;`, or `a` and
    /// `b` naming each other, or the same through the first alternative of an
    /// inline OR. Evaluating one never returns — the C++ recurses until the
    /// stack runs out.
    ///
    /// A reference behind a LINK, or behind an alternative that only runs once
    /// the earlier ones have failed, is left alone: whether it ever runs
    /// depends on the input, and the C++ test corpus has one
    /// (`TEMPLATE alts = ... OR (T:alts)`, `test/T_Templates`).
    ///
    /// The walk is iterative: the depth of the graph is the grammar's to
    /// choose.
    pub(super) fn check_template_cycles(&mut self) {
        let mut roots: Vec<CtxId> = self.grammar.contexts.values().copied().collect();
        roots.sort_by_key(|c| (self.grammar.contexts_arena[c.0].line, *c));
        let mut marks: HashMap<CtxId, Walk> = HashMap::new();
        let mut cycles: Vec<Vec<CtxId>> = Vec::new();
        for root in roots {
            if marks.contains_key(&root) {
                continue;
            }
            marks.insert(root, Walk::OnPath);
            let mut path: Vec<(CtxId, Vec<CtxId>)> = vec![(root, self.runs_first(root))];
            while let Some((c, next)) = path.last_mut() {
                let Some(d) = next.pop() else {
                    marks.insert(*c, Walk::Done);
                    path.pop();
                    continue;
                };
                match marks.get(&d) {
                    None => {
                        marks.insert(d, Walk::OnPath);
                        let after = self.runs_first(d);
                        path.push((d, after));
                    }
                    Some(Walk::OnPath) => {
                        let from = path.iter().position(|(p, _)| *p == d).unwrap_or(0);
                        cycles.push(path[from..].iter().map(|(p, _)| *p).collect());
                    }
                    Some(Walk::Done) => {}
                }
            }
        }
        for cycle in cycles {
            self.report_template_cycle(&cycle);
        }
    }

    /// Report one cycle, at the test the walk came back to, naming the
    /// templates it runs through in the order it runs through them.
    fn report_template_cycle(&mut self, cycle: &[CtxId]) {
        // Each edge of the cycle leaves one of its tests; the edge that closes
        // it leaves the last one and enters the first.
        let order = cycle.iter().rev().take(1).chain(&cycle[..cycle.len() - 1]);
        let mut names: Vec<String> = order
            .filter(|c| self.grammar.contexts_arena[c.0].tmpl.is_some())
            .filter_map(|c| self.deferred_tmpls.get(c).map(|(_, name)| name.clone()))
            .collect();
        if let Some(first) = names.first().cloned() {
            names.push(first);
        }
        // The template the cycle came back to is where it is defined, when it is
        // a template's own test; any other test is where it was first written.
        let at = cycle[0];
        let first = self.ctx_spans.get(&at).and_then(|spans| spans.first());
        let span = self.template_defs.get(&at).or(first).cloned();
        let e = self.placed_error(span, ParseErrorKind::TemplateCycle { cycle: names });
        self.record(e);
    }

    /// Where to point at test `c` for a failure the rule `rid` runs into: where
    /// the rule itself writes it, when it does, and otherwise the first place it
    /// was written (the template it came from).
    fn span_in_rule(&self, c: CtxId, rid: RuleId) -> Option<ParseSpan> {
        let spans = self.ctx_spans.get(&c)?;
        let prov = self.grammar.rule_by_number[rid.0].provenance.as_ref();
        let inside = |sp: &&ParseSpan| {
            prov.is_some_and(|p| {
                ui32(sp.source) == p.source && (p.begin..p.end).contains(&ui32(sp.range.start))
            })
        };
        spans.iter().find(inside).or(spans.first()).cloned()
    }

    // [spec:cg3:req:robustness.grammar-text-errors]
    /// Refuse every test with position `?` that a rule runs without an override
    /// position to replace it.
    ///
    /// `?` stands for "the position the reference supplies", so it is only
    /// meaningful in a template reached through a reference that has one,
    /// like `(-1 T:t)`. The override replaces the position of the template's
    /// own test, and carries on into the templates and OR alternatives that
    /// test runs; a LINKed test runs with its own position. The C++ checks
    /// this only when the test runs, and quits.
    ///
    /// DIVERGENCE: refused when the grammar is parsed, whether or not a rule
    /// ever gets as far as running the test.
    pub(super) fn check_unknown_positions(&mut self) {
        let mut seen: HashSet<(CtxId, bool)> = HashSet::new();
        let mut reported: HashSet<CtxId> = HashSet::new();
        let rule_ids: Vec<RuleId> = (0..self.grammar.rule_by_number.capacity())
            .filter(|&i| self.grammar.rule_by_number.try_get(i).is_some())
            .map(RuleId)
            .collect();
        for rid in rule_ids {
            let rule = &self.grammar.rule_by_number[rid.0];
            let rule_line = rule.line;
            let mut todo: Vec<(CtxId, bool)> = rule
                .tests
                .iter()
                .chain(rule.dep_tests.iter())
                .chain(rule.dep_target.iter())
                .map(|&c| (c, false))
                .collect();
            while let Some((c, overridden)) = todo.pop() {
                if !seen.insert((c, overridden)) {
                    continue;
                }
                let ctx = &self.grammar.contexts_arena[c.0];
                let unknown = !overridden && ctx.pos.intersects(POS_UNKNOWN);
                let passes = overridden || ctx.pos.intersects(POS_TMPL_OVERRIDE);
                todo.extend(ctx.tmpl.iter().chain(&ctx.ors).map(|&t| (t, passes)));
                todo.extend(ctx.linked.map(|l| (l, false)));
                if unknown && reported.insert(c) {
                    let span = self.span_in_rule(c, rid);
                    let kind = ParseErrorKind::PositionWithoutOverride { rule_line };
                    let e = self.placed_error(span, kind);
                    self.record(e);
                }
            }
        }
    }
}

/// C++ `Grammar::getTagList_Any` over a set that has not been through
/// `reindex`: a composite set's members are the content hashes the parser
/// stored, so they are looked up by hash rather than taken as set numbers.
fn tag_list_any_by_hash(grammar: &GrammarCore, set: SetId, the_tags: &mut TagList) {
    let s = &grammar.sets_list[set.0];
    if s.r#type.intersects(ST_SET_UNIFY | ST_TAG_UNIFY) {
        the_tags.clear();
        the_tags.extend(grammar.single_tags().find(grammar.tag_any).tag());
    } else if !s.sets.is_empty() {
        for &member in &s.sets {
            if let Some(child) = grammar.get_set(member) {
                tag_list_any_by_hash(grammar, child, the_tags);
            }
        }
    } else {
        trie_get_tag_list_append(&s.trie, the_tags, grammar);
        trie_get_tag_list_append(&s.trie_special, the_tags, grammar);
    }
}
