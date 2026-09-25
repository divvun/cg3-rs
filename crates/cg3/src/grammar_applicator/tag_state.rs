//! The [`Matcher`] as the state a run parses its generated tags with.

use crate::arena::TagId;
use crate::grammar::{Grammar, TagSpace};
use crate::tag::Tag;

use super::Matcher;

/// The applicator instantiation of the C++ `parser_helpers.hpp`
/// `template<typename State> parseTag(...)` — used by
/// [`GrammarApplicator::add_tag`](super::GrammarApplicator::add_tag)'s
/// `T_VARSTRING` branch so runtime-generated tags go through the full parser
/// (regex compile, prefixes, suffixes, numerics) instead of the raw path.
/// Implemented on the [`Matcher`] sub-view: the varstring branch is reached
/// from the contextual matcher knot, so `parse_tag(..., self, ...)` threads a
/// `Matcher`.
impl crate::parser_helpers::ParseTagState for Matcher<'_> {
    type Tags = Grammar;
    fn grammar(&self) -> &Grammar {
        &*self.grammar
    }

    /// C++ `GrammarApplicator::filebase` is `nullptr` (never set) — the
    /// warnings that print it only fire on malformed tags.
    fn filebase(&self) -> &str {
        ""
    }

    // [spec:cg3:req:diagnostics.runtime-input-named]
    /// C++ `GrammarApplicator::error(str, p)` labelled the failure `RT RULE`
    /// with the current rule's line, or `RT INPUT` with the input line count.
    /// That label/line pair is the only position a runtime tag failure has, so
    /// it becomes the error's `file` and `line`. The C++ printed here and
    /// returned; the caller now decides.
    ///
    /// A failure with no rule in flight belongs to the INPUT, and the line
    /// counts that stream's lines rather than the grammar's — so it is headed
    /// with the input's own name
    /// ([`EngineConfig::input_name`](crate::grammar_applicator::EngineConfig::input_name))
    /// rather than with `RT INPUT`, which said which counter the number came
    /// from and nothing about which of several files produced it. `RT RULE`
    /// stays as the fallback for a rule that cannot be placed in a source;
    /// `place_in_grammar` replaces it when it can.
    ///
    /// No span: the offending text came off the input stream, not out of a
    /// grammar buffer. `near` is ignored for the same reason the C++ passed
    /// `p = 0` here.
    fn error_at(&mut self, _near: crate::parser_helpers::Near<'_>) -> crate::error::ParseError {
        let (label, line) = if let Some(rid) = self.scratch.current_rule
            && self.grammar.rule_by_number[rid.0].line != 0
        {
            ("RT RULE", self.grammar.rule_by_number[rid.0].line)
        } else {
            (self.cfg.input_name.as_str(), *self.num_lines)
        };
        crate::error::ParseError {
            file: label.to_string(),
            line,
            near: String::new(),
            span: None,
            kind: crate::error::ParseErrorKind::Syntax,
        }
    }

    /// C++ `state.addTag(tag)` → `GrammarApplicator::addTag(Tag*)` — the
    /// seed-probing interner, NOT `Grammar::addTag`.
    fn add_tag(&mut self, tag: Tag) -> TagId {
        self.grammar.add_tag(tag)
    }

    // [spec:cg3:req:robustness.depth-bounded]
    /// The run's own nesting count: a variable tag nested in a generated tag
    /// is a level on top of the contextual tests that generated it.
    fn nesting(&mut self) -> &mut usize {
        &mut self.scratch.nesting
    }
}
