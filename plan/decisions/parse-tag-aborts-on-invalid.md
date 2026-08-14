---
id [dec:cg3:parse-tag-aborts-on-invalid]
epitome "parse_tag aborts on invalid input everywhere, dropping the applicator's inherited fall-through."
state @tentative
category @property
scope {
    elements ([arch:cg3:tag-parsing])
    rules ([spec:cg3:req:errors.result-primary] [spec:cg3:req:errors.context])
}
author "brendan@necessary.nu"
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
        "Characterisation found the empty-tag path does not fall through benignly: it reaches `inlines::is_textual`, which indexes an empty slice and panics. That case is a clear fix. The two paths that DO intern today — a `(`-leading tag and one whose regex will not compile — change from producing a tag to producing an error, and whether any real grammar depends on that is still open. The evidence that closes it is the golden and Apertium suites passing after the parser conversion; until then this stays tentative."
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

So this decision is `tentative` rather than `decided`, and it is gated on
characterisation. That phase has now run, and it strengthens the case without
closing it.

Three of the six sites are reachable from the applicator. The empty-tag guard
turns out not to fall through benignly at all: execution continues into
`inlines::is_textual`, which indexes `s[0]` on an empty slice and panics. Its
own doc comment admits the provenance — "Panics on empty `s` (C++ front()/back()
on empty is UB)". So on that path the fall-through reproduces undefined
behaviour as a crash, and converting the site to `?` removes it outright.

The other two — a `(`-leading tag and one whose regex will not compile — do
intern today, and under the conversion would stop. Nothing in the ported corpus
exercises them, which is weak evidence rather than none: the suites cannot fail
on behaviour they never reach. The evidence that closes this is those suites
still passing once the parser conversion lands, at which point the decision
moves to `decided`. If they break, the rejected alternative — a typed recovery
signal letting each implementation state its own semantics — is already
specified and takes its place.

The state of this record is therefore the project's gate, not decoration. It
must not reach `decided` on the strength of the argument alone.
