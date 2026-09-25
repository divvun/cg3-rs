//! The limit on how deeply grammar constructs may nest.
//!
//! `[spec:cg3:req:robustness.depth-bounded]` sorts what input can nest into
//! two kinds. Depth that is inherent in data — a sub-reading chain, a
//! dependency chain, a trie, a set built from sets, a `LINK` chain read from a
//! `.cg3b` — is walked without recursing, wherever it is walked. Depth that an
//! author writes — `LINK`s, inline templates, `WITH` blocks, variable tags
//! naming variable tags — is parsed and evaluated by recursion, as the C++
//! does, and bounded here.

// [spec:cg3:req:robustness.depth-bounded]
/// How many levels deep grammar constructs may nest, all kinds counted
/// together.
///
/// While a grammar is parsed, a level is each `LINK`, each item after the
/// first of a `[...]` template list, each inline template inside a test, each
/// `WITH` block inside another, and each variable tag whose name or value is
/// itself a variable tag. While a rule runs, it is each `LINK`ed test, each
/// template or `OR` alternative entered, each `WITH` sub-rule run, and each
/// variable tag a generated tag nests. A construct that would go deeper is
/// refused: at parse time with a
/// [`ParseErrorKind::NestingTooDeep`](crate::error::ParseErrorKind::NestingTooDeep)
/// where it begins, at run time with a
/// [`RunError::NestingTooDeep`](crate::error::RunError::NestingTooDeep) naming
/// the rule.
///
/// The kinds count together because they are recursions through the same
/// stack: a `LINK` chain inside a `WITH` block inside another uses the stack
/// of all three. Real grammars nest a handful of levels — the Giellatekno
/// grammars no more than eight — and a template that keeps recursing at run
/// time, through an `OR` alternative or a `LINK`, is the one construct that
/// goes deeper, which is what the limit is for. It is set well below the depth
/// at which a debug build overflows the 2 MiB stack a thread gets by default:
/// the costliest levels, `LINK`ed dependency tests and `WITH` sub-rules being
/// run, take about 18 KiB each there, so 64 of them fit in about 1.25 MiB.
pub const MAX_NESTING: usize = 64;
