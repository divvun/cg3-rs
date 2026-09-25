---
id [dec:cg3:grammar-load-phases]
epitome "A grammar's load phases are types: a draft or a numbered grammar is finished, once, into an indexed grammar, and only an indexed grammar is written or run."
state @decided
category @property
scope {
    elements ([arch:cg3:grammar-model] [arch:cg3:binary-format] [arch:cg3:textual-parser] [arch:cg3:cli-boundary])
    rules (
        [spec:cg3:req:grammar-phases.loaders]
        [spec:cg3:req:grammar-phases.finish]
        [spec:cg3:req:grammar-phases.indexed-only]
        [spec:cg3:req:grammar-phases.index-rebuilds]
        [spec:cg3:req:grammar-phases.same-output]
        [spec:cg3:req:robustness.panic-sites-justified]
    )
}
author "brendan@necessary.nu"
decided_at "2026-09-25T00:00:00Z"
alternatives (
    {
        option "Keep one `reindex(&mut self)` and make it idempotent: a private flag skips resolving the second time, and indexing clears what it builds first."
        rejected_because "The order would still be a runtime fact. Every place that relies on a grammar having been resolved exactly once would cite a flag in its justification, which `[spec:cg3:req:robustness.panic-sites-justified]` asks us to replace with a type wherever a type can carry it, and nothing would stop a caller writing or running a grammar that was never indexed, which today silently writes no sections and fires no rules."
    }
    {
        option "Two phases: a draft that `finish` turns into the finished grammar, with the `.cg3b` reader indexing as it reads and the relabeller calling an idempotent in-place `reindex`."
        rejected_because "It leaves an indexed-but-stale state — the relabeller between its edits and its reindex — that the types cannot see. And indexing inside the reader moves the static-set errors of step 17 ahead of the checks the tools make after loading, so two failures of a crafted `.cg3b` would report differently."
    }
    {
        option "Two separate structs for the draft and the finished grammar, with the shared fields in a sub-struct."
        rejected_because "Only three fields are draft-only and fourteen are built by indexing; about thirty-five are shared. Splitting them rewrites some 1,550 field accesses, or, through `Deref`, breaks the places that borrow two fields at once. A phase parameter on one struct puts the guarantee where it belongs — in which operations each phase has — and leaves every field access as it is."
    }
)
consequences {
    accepted (
        "`GrammarCore` takes a phase parameter that defaults to `Indexed`, so everything after loading — the writers, the run view, the relabels grammar — keeps its signature. The parser holds a `GrammarCore<Draft>`, the reader a `GrammarCore<Numbered>`."
        "`reindex` and its `Reindexed` result go. The tools call `finish` where they called `reindex`, so a failure is reported where it was. `--show-tags` stops after finishing, in the tool."
        "The relabeller takes the grammar it relabels by value, gives up its indexes, and finishes it again at the end, as the C++ reindexes at the end of `relabel`. A grammar that came from text can then be relabelled too."
        "A grammar can still be built in any phase from the operations every phase shares, and its fields are public. The types order the load; they do not seal the struct."
    )
    deferred (
        "`--show-unused-sets` still prints nothing. Its report needs the state resolving throws away, so when it is ported, finishing a draft returns it."
    )
}
codifies (
    [spec:cg3:req:grammar-phases.loaders]
    [spec:cg3:req:grammar-phases.finish]
    [spec:cg3:req:grammar-phases.indexed-only]
    [spec:cg3:req:grammar-phases.index-rebuilds]
    [spec:cg3:req:grammar-phases.same-output]
)
establishes ([arch:cg3:grammar-model])
---

## Rationale

The C++ `Grammar::reindex` is where a loaded grammar becomes a usable one, and
it is two passes woven together. One resolves a grammar the textual parser
built: sets there are known by the hash of their contents, and resolving numbers
them and rewrites every reference from hash to number, then throws the hash map
away. The other indexes: it builds the maps from tags and sets to the rules and
sets that use them, the section lists, and the flags a run and the `.cg3b`
writer read. A `.cg3b` is stored already numbered, so loading one needs only the
second pass; the C++ says so with an `is_binary` test at every step that
belongs to the first.

That makes the phase of a grammar a fact about its value, not its history. A
parsed grammar has hash references and no indexes; a read one has numbers and no
indexes; a finished one has numbers and indexes. The C++ keeps this in one
mutable object and a flag, and the port copied it, so the order was a matter of
every caller getting it right. Calling `reindex` twice on a parsed grammar
panicked, because the second pass looked hashes up in a map the first had
emptied; writing or running a grammar never reindexed produced empty output
without a word; and the relabeller's second reindex worked only because
`cg-relabel` refuses text, and even then left duplicated and stale index entries
behind, which it had to clear one of by hand.

With the phases as types, each of those is something the compiler refuses.
`finish` consumes the grammar it finishes, so there is no second resolve to get
wrong; the lookups by content hash exist only on a draft, so the justifications
that said "reindex runs once" become facts of the type; and only an indexed
grammar converts into a run's view or reaches a writer. The phase is a
parameter on the one struct because the guarantee lives in which operations a
phase has, and the fields — shared almost entirely between phases — did not
need to move for that.

Nothing a user can observe changes. Resolving and indexing, run one after the
other instead of interleaved, still build the same grammar, because neither
reads what the other writes out of order; the golden corpus and the `.cg3b`
bytes are the check. This is the same move the run view made earlier — the core
a run shares is immutable because nothing can name it mutably — taken back to
the start of a grammar's life.
