# src/GrammarApplicator_context.cpp

> [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.check-unif-tags-fn]
> bool GrammarApplicator::check_unif_tags(uint32_t set, const void* val)

> [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.check-unif-tags-fn]
> Unification helper keyed on the current (top) rule context. `context_stack`
> is a stack (vector) of `Rule_Context`; the "current" context is its back().
> If `context_stack` is empty, immediately return false. Otherwise take a
> reference to the map obtained by dereferencing `context_stack.back().unif_tags`
> (an ordered flat_map from uint32_t set-number to `const void*`; the void* is
> actually a tag- or trie-pointer only ever compared by identity, never
> dereferenced). Look up key `set`. If an entry already exists, return whether
> its stored pointer equals `val` (true iff the same value is being unified
> again). If no entry exists, insert `unif_tags[set] = val` and return true.
> Net effect: the first time a given set number is unified it records `val` and
> succeeds; every later attempt for the same set succeeds only if it presents
> the identical pointer, enforcing "same tag across all uses" unification.

> [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.get-apply-to-fn]
> ReadingSpec GrammarApplicator::get_apply_to()

> [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.get-apply-to-fn]
> Returns the ReadingSpec (a {cohort, reading, subreading} triple, all pointers)
> that the current rule action should be applied to, preferring an explicit
> attach target over the matched target. If `context_stack` is empty, return a
> default-constructed ReadingSpec (all three pointers null). Otherwise inspect
> the top context (`context_stack.back()`): if its `attach_to.cohort` is
> non-null, return the whole `attach_to` spec by value; else return the `target`
> spec by value. This is the accessor used pervasively (e.g. the TRACE macro and
> every rule action) to reach "the reading to act on".

> [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.get-attach-to-fn]
> ReadingSpec GrammarApplicator::get_attach_to()

> [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.get-attach-to-fn]
> Returns the current context's explicit attach target. If `context_stack` is
> empty, return a default-constructed ReadingSpec (all pointers null). Otherwise
> return `context_stack.back().attach_to` by value (its {cohort, reading,
> subreading}). Unlike get_apply_to, it does not fall back to `target`; a null
> `attach_to.cohort` here signals "no attach target was set".

> [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.get-mark-fn]
> Cohort* GrammarApplicator::get_mark()

> [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.get-mark-fn]
> Returns the current context's mark cohort (the cohort designated by the `X`
> position / MARK, used as the jump-to point for `jM` and as the reference for
> `X`-relative tests). If `context_stack` is empty, return nullptr; otherwise
> return `context_stack.back().mark` (which may itself be nullptr).

> [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.set-attach-to-fn]
> void GrammarApplicator::set_attach_to(Reading* reading, Reading* subreading)

> [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.set-attach-to-fn]
> Records an attach target on the current context. If `context_stack` is empty,
> do nothing (silent no-op). Otherwise take a reference to
> `context_stack.back().attach_to` and set its three fields: `cohort =
> reading->parent`, `reading = reading`, `subreading = subreading`. Note the
> cohort is derived from the passed `reading`'s parent, not passed separately.
> After this, get_attach_to / get_apply_to will report this cohort as the attach
> target.

> [spec:cg3:def:grammar-applicator-context.cg3.grammar-applicator.set-mark-fn]
> void GrammarApplicator::set_mark(Cohort* cohort)

> [spec:cg3:sem:grammar-applicator-context.cg3.grammar-applicator.set-mark-fn]
> Sets the current context's mark cohort. If `context_stack` is empty, do
> nothing (silent no-op). Otherwise assign `context_stack.back().mark = cohort`.
> Callers use this to (re)point the `X`/MARK reference, e.g. runSingleRule marks
> the current cohort at the start of each reading and each test resets it to the
> cohort unless RF_REMEMBERX/RF_RESETX say otherwise.

