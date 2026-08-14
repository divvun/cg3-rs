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
        "Whether the old fall-through was ever load-bearing is not yet known. If characterisation finds a golden or Apertium case that depends on a garbage tag being built, this decision flips to the rejected alternative and the recovery signal is implemented instead."
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

So this decision is `tentative` rather than `decided`, and it is gated: the
characterisation phase pins the current applicator behaviour with tests first.
If nothing depends on the fall-through, the decision moves to `decided` and the
conversion proceeds. If something does, the rejected alternative — a typed
recovery signal that lets each implementation state its own semantics — is
already specified and takes its place.

The state of this record is therefore the project's gate, not decoration. It
must not reach `decided` on the strength of the argument alone.
