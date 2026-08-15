---
id [dec:cg3:parse-tag-aborts-on-invalid]
epitome "parse_tag aborts on invalid input everywhere, dropping the applicator's inherited fall-through."
state @decided
category @property
scope {
    elements ([arch:cg3:tag-parsing])
    rules ([spec:cg3:req:errors.result-primary] [spec:cg3:req:errors.context])
}
author "brendan@necessary.nu"
decided_at "2026-08-14T00:00:00Z"
alternatives (
    {
        option "Preserve the asymmetry with a typed recovery signal (`ControlFlow`, or an associated `Recovery` type on the trait)."
        rejected_because "It keeps behaviour byte-identical and makes the conversion purely mechanical, but it encodes a C++ accident into the type system permanently and costs six explicit match arms in a function that would otherwise read as ordinary Rust. Chosen against deliberately, accepting a behaviour change instead."
    }
    {
        option "Split `ParseTagState` into separate parser and applicator traits."
        rejected_because "Removes the asymmetry by duplicating `parse_tag`'s body, or by making it generic over a second axis. The shared implementation is the reason the trait exists."
    }
)
consequences {
    accepted (
        "Runtime behaviour changes at six sites in `parser_helpers::parse_tag`. On the applicator path a malformed runtime varstring tag now stops tag construction instead of continuing with the input that just failed validation."
        "The blast radius is one applicator function — `GrammarApplicator::add_tag` (core.rs:1890), on the `T_VARSTRING` branch — but that function is called throughout `reflow.rs` and `match_set.rs`."
        "This is the only intentional divergence from C++ behaviour in the project, so it is gated on characterisation tests that pin today's behaviour BEFORE the parser converts, not after."
    )
    deferred (
        "The corpus exercises none of the three affected paths directly, so the suites passing is evidence that nothing depended on the old behaviour rather than proof that nothing could. A grammar in the wild that relies on a malformed varstring tag being interned would now fail to build that tag."
    )
}
edges {
    requires ([dec:cg3:results-not-unwinding])
    alternative_to ()
}
codifies ()
establishes ([arch:cg3:tag-parsing])
---

## Rationale

`parser_helpers::parse_tag` is generic over `ParseTagState`, which the textual
parser and the runtime applicator both implement. Its `error_near` returns `()`,
and the two implementations disagree about what that means: the parser's
inherent `error_near` is `-> !`, so the code after each call is unreachable; the
applicator's prints a diagnostic and returns, so the code after each call runs —
with the exact malformed input that just failed validation, producing a tag from
it.

That split is inherited from the C++, where the same template was instantiated
with a throwing state and a non-throwing one. In Rust it is encoded nowhere
except the two impl bodies, and the `()` return type that makes it possible
reads as an oversight rather than a decision.

Converting `error_near` to return `Result` and using `?` at all six sites makes
`parse_tag` ordinary, at the cost of changing what the applicator does with
invalid input. That change is very likely a fix — building a tag out of input
that just failed validation is hard to defend, and the diagnostic printed
alongside it says as much — but it is a behaviour change to a shipped engine,
and "very likely a fix" is not evidence.

This decision was gated on characterisation, and both halves have now run.

Three of the six sites are reachable from the applicator. The empty-tag guard
turns out not to fall through benignly at all: execution continues into
`inlines::is_textual`, which indexes `s[0]` on an empty slice and panics. Its
own doc comment admits the provenance — "Panics on empty `s` (C++ front()/back()
on empty is UB)". So on that path the fall-through reproduces undefined
behaviour as a crash, and converting the site to `?` removes it outright.

Since then (`textual-sniff-bounds`) `is_textual` has been made total: an empty
`s` answers false rather than panicking. That does not weaken this decision, but
it does change what would happen if it were ever reverted — the fall-through
would intern a tag from empty text instead of crashing on it, which is quieter
and worse. The guard converted here is now the only thing standing between
invalid input and a tag built from it.

The other two — a `(`-leading tag and one whose regex will not compile — did
intern, and now stop. The second is the more telling: it produced a tag with no
compiled regex, one that could never match the thing it named. The golden and
Apertium suites pass unchanged either side of the conversion, which is what
moved this to `decided`.

That evidence is real but bounded: the corpus never reaches these paths, so it
shows nothing depended on the old behaviour rather than proving nothing could. A
grammar in the wild that relies on a malformed varstring tag being interned will
now not get that tag. Given the alternative was inherited UB, that trade is
worth making explicitly rather than by default.
