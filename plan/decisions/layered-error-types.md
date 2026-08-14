---
id [dec:cg3:layered-error-types]
epitome "Error types are layered per boundary and derived with thiserror, not collected into one enum."
state @decided
category @existence
scope {
    elements ([arch:cg3:error-model])
    rules ([spec:cg3:req:errors.layered] [spec:cg3:req:errors.context])
}
author "brendan@necessary.nu"
decided_at "2026-08-14T00:00:00Z"
alternatives (
    {
        option "Keep one crate-wide `Cg3Error` and add variants."
        rejected_because "It is already drifting that way and the drift is visible: `exit_code()` hardcodes 1 for two of three variants, and `tag_regex_errors()` returns an empty slice for the variants that can never carry them. Every caller would match variants its layer cannot produce."
    }
    {
        option "Hand-roll Display/Error/From as the crate does today."
        rejected_because "The crate already hand-rolls these on four types, and layering multiplies that by the number of layers plus every conversion between them. The boilerplate is the reason single-enum drift looks attractive."
    }
    {
        option "Use `anyhow` / a boxed dynamic error."
        rejected_because "Wrong tool for a library. It erases exactly the structure an embedder needs to match on, which is the problem being fixed — the reason the tag-regex diagnostic had to be scraped out of a `tracing` subscriber in the first place."
    }
)
consequences {
    accepted (
        "`thiserror` joins the dependency list. It is proc-macro only, with no runtime component."
        "`Cg3Error` stops being the type most functions return; it becomes the outermost composition. This is a breaking API change for embedders, on top of the ones already made this cycle."
        "`Cg3Error::Fatal { code }` is reshaped: the exit code moves to a CLI-side mapping per `[spec:cg3:req:errors.exit-codes-at-cli]`, and the variant keeps only real context."
        "`TagRegexError` is the template — a struct with the offending input, its provenance, an optional line, and a typed cause, plus `.with_tag()` / `.with_line()` builders. Existing shape; new types follow it."
    )
    deferred (
        "`MathError`'s consumers currently discard the payload entirely (`tag.rs:504`, `match_set.rs:230`). Giving it position information is worthwhile on its own, but nothing consumes it until those two sites are revisited."
    )
}
edges {
    requires ([dec:cg3:results-not-unwinding])
}
codifies ([spec:cg3:req:errors.layered] [spec:cg3:req:errors.context])
establishes ([arch:cg3:error-model])
---

## Rationale

Once failure travels by value rather than by unwinding, the shape of the value
is the API. A single crate-wide enum makes that API worse the more it is used:
every consumer matches variants its layer cannot produce, and every new failure
mode widens the type for everyone.

The evidence that this drift is already underway is in the current type.
`Cg3Error::exit_code()` hardcodes `1` for two of its three variants, because the
exit code was only ever meaningful for the third. `tag_regex_errors()` returns
an empty slice for `Fatal` and `Parse`, because only one variant can carry them.
Both are the enum apologising for holding unrelated things.

Layering by boundary — parse, grammar load, run — lets each layer name only what
it can produce and compose upward by conversion. It also matches how the crate
is actually consumed: an embedder loading a grammar wants to know which tag on
which line failed, and has no interest in stream errors.

`TagRegexError` already demonstrates the target shape, and it exists because the
alternative was a consumer installing a thread-local `tracing` subscriber to
string-match `uregex_open returned {} trying to parse tag {}` out of the log.
That is the standard the rest of the crate's errors are held to.

`thiserror` is the ordinary way to write this in Rust, and the boilerplate it
removes is precisely what makes the single-enum shortcut tempting. Declining it
would mean paying for the layering in hand-written `Display`, `Error` and `From`
impls at every layer and every conversion between them.
