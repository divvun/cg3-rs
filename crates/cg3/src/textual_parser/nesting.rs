//! `TextualParser` — keeping the constructs the parser recurses on within
//! [`MAX_NESTING`] (`[spec:cg3:req:robustness.depth-bounded]`).
//!
//! The parser recurses once per `LINK`, per inline template and per `WITH`
//! block, as the C++ does. Every such recursion goes through here, one level
//! deeper, and a construct that would take the grammar past the limit is
//! refused where it begins.

use crate::arena::CtxId;
use crate::error::{Nesting, ParseErrorKind};
use crate::inlines::skipws_chars;
use crate::nesting::MAX_NESTING;
use crate::types::SetNumber;

use super::{ParseResult, TextualParser};

impl TextualParser {
    // [spec:cg3:req:robustness.depth-bounded]
    /// Go one level deeper for the construct `what` beginning at `at`, or
    /// refuse it there if that passes [`MAX_NESTING`].
    pub(super) fn enter_nesting(&mut self, at: usize, what: Nesting) -> ParseResult {
        if self.nesting >= MAX_NESTING {
            let mut err = self.error_near(at);
            err.kind = ParseErrorKind::NestingTooDeep {
                what,
                limit: MAX_NESTING,
            };
            return Err(err);
        }
        self.nesting += 1;
        Ok(())
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+2]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+2]
    // [spec:cg3:req:robustness.depth-bounded]
    /// `parseContextualTestList`, one level deeper than the test holding it
    /// when `what` names the construct that nests it — a `LINK` or an inline
    /// template — and at the same level for a rule's or template's own test.
    /// Every level entered while it is parsed, the items of a `[...]` list
    /// included, is left again when it returns, error or not.
    pub(super) fn parse_nested_list(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        rule_flags: Option<crate::rule::RuleFlags>,
        in_tmpl: bool,
        what: Option<Nesting>,
    ) -> ParseResult<CtxId> {
        let saved = self.nesting;
        let entered = match what {
            Some(what) => self.enter_nesting(*pos, what),
            None => Ok(()),
        };
        let parsed =
            entered.and_then(|()| self.parse_contextual_test_list(buf, pos, rule_flags, in_tmpl));
        self.nesting = saved;
        parsed
    }

    // [spec:cg3:def:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+2]
    // [spec:cg3:sem:textual-parser.cg3.textual-parser.parse-contextual-test-list-fn+2]
    // [spec:cg3:req:robustness.depth-bounded]
    /// The `[set, set, ...]` template shorthand of `parseContextualTestList`,
    /// from its `[` to past its `]`: the first set is the target of `t_cur`,
    /// and each one after it the target of a new test `LINK`ed from the one
    /// before. Returns the last test of the chain.
    ///
    /// DIVERGENCE: each item after the first is a `LINK`, and counts as a
    /// level of nesting like any other; the C++ builds the chain however long
    /// it is written.
    pub(super) fn parse_template_list(
        &mut self,
        buf: &[char],
        pos: &mut usize,
        mut t_cur: CtxId,
    ) -> ParseResult<CtxId> {
        *pos += 1;
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        let s = self.parse_set_inline_wrapper(buf, pos)?;
        self.grammar.contexts_arena[t_cur.0].offset = 1;
        self.grammar.contexts_arena[t_cur.0].target = SetNumber(self.grammar.sets_list[s.0].hash);
        self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        while buf[*pos] == ',' {
            *pos += 1;
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
            self.enter_nesting(*pos, Nesting::Link)?;
            let lnk = self.grammar.allocate_contextual_test();
            let s2 = self.parse_set_inline_wrapper(buf, pos)?;
            self.grammar.contexts_arena[lnk.0].offset = 1;
            self.grammar.contexts_arena[lnk.0].target =
                SetNumber(self.grammar.sets_list[s2.0].hash);
            self.grammar.contexts_arena[t_cur.0].linked = Some(lnk);
            t_cur = lnk;
            self.grammar.lines += skipws_chars(buf, pos, '\0', '\0', false);
        }
        if buf[*pos] != ']' {
            return Err(self.error_near(*pos));
        }
        *pos += 1;
        Ok(t_cur)
    }
}
