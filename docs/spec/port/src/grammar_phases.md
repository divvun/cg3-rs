# Grammar load phases

The C++ `Grammar` is one mutable object for its whole life. A loader fills it,
`Grammar::reindex` turns what was loaded into what a run and the writers use,
and nothing stops either step being skipped or repeated: `reindex` can be called
twice, and a grammar that was never reindexed can be written or applied.
Neither is safe. The port gives each phase of a grammar's load its own type, so
the order is kept by the compiler.

`reindex` does two jobs that need different input. It *resolves* a grammar the
textual parser built, whose sets, rules and tests refer to sets by content
hash, into one that refers to them by set number; that can happen only once.
And it *indexes* a grammar whose references are numbers, building the maps and
flags a run reads; a `.cg3b` arrives already numbered, and needs only that. The
phases follow those two jobs.

> [spec:cg3:req:grammar-phases.loaders]
> A loader MUST return the phase its output is in. The textual parser builds a
> *draft*, whose references are content hashes. The `.cg3b` reader builds a
> *numbered* grammar, whose references are set numbers but which has none of
> the indexes a run reads. A grammar built by hand from the grammar's own
> operations starts as a draft.

> [spec:cg3:req:grammar-phases.finish]
> A draft or a numbered grammar MUST become an *indexed* grammar only through
> `finish`, which consumes it and returns the indexed grammar or an error.
> Finishing a draft resolves it and then indexes it; finishing a numbered
> grammar indexes it. An indexed grammar has no `finish`, so nothing is
> resolved twice, and the operations that look a set up by content hash exist
> only on a draft.

> [spec:cg3:req:grammar-phases.indexed-only]
> Only an indexed grammar MAY be written, by either writer, or made into a
> run's view of a grammar and applied. Writing or running a draft or a
> numbered grammar MUST NOT compile.

> [spec:cg3:req:grammar-phases.index-rebuilds]
> Indexing MUST build every index and derived flag it owns from nothing, so
> indexing a grammar a second time leaves no stale or duplicated entry. An
> indexed grammar that is edited in a way its indexes depend on — the
> relabeller adding sets, tags and set members — MUST first give its indexes
> up with `into_numbered`, and be finished again after the edit.

> [spec:cg3:req:grammar-phases.same-output]
> Finishing MUST produce the grammar `Grammar::reindex` produces: the same set
> and rule numbers, flags and indexes, so every `.cg3b` written and every run's
> output are byte-identical, and a grammar that fails to load fails at the same
> point with the same error.
