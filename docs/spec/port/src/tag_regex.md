# Tag regex compatibility

The C++ compiles every tag pattern with ICU's `uregex_open`. Grammars in the
wild are authored against ICU, so the port must accept ICU syntax or silently
stop matching. These rules govern the seam between ICU-authored patterns and the
Rust engine that actually runs them.

> [spec:cg3:req:tag-regex.single-seam+1]
> Every user-authored pattern the C++ compiled with ICU MUST be compiled through
> a single helper: tag patterns, text-delimiter patterns, and the `--nrules` /
> `--nrules-v` rule-name filters. No call site may construct one directly. A
> rule-name filter is not a tag, but it is written by the same author against the
> same engine and the C++ handed it to the same `uregex_open`; compiling it
> anywhere else means one spelling has two meanings depending on whether it was
> typed on the command line or into a grammar. Case-insensitivity MUST be
> applied as an engine builder flag and MUST NOT be injected into the pattern
> text, because the binary grammar writer serialises the compiled pattern back
> out and an injected flag would leak into every emitted `.cg3b`, diverging from
> the C++ `uregex_pattern` round-trip. The case-insensitive bit survives a
> `.cg3b` round-trip in the tag's `T_CASE_INSENSITIVE` type flag, which the
> reader re-derives.

> [spec:cg3:req:tag-regex.engine]
> Tag patterns MUST be compiled with a backtracking-capable engine, so that
> ICU's lookahead, lookbehind, backreferences, atomic groups and possessive
> quantifiers are supported rather than rejected or — worse — silently
> reinterpreted. A finite-automaton-only engine is insufficient: it reads `a*+`
> as `(?:a*)+` and matches where ICU does not. Because patterns are
> user-authored grammar content, compilation MUST impose an explicit backtrack
> limit, and a match attempt that exceeds it MUST surface as a bounded failure
> rather than a hang. Match sites that have no error channel MUST treat such a
> failure as "no match" and MUST log it, since the C++ `uregex_find` error path
> terminated the process and degrading to "no match" is a deliberate divergence.

> [spec:cg3:req:tag-regex.icu-translation+1]
> Patterns MUST be translated from ICU spelling before compilation, for every
> construct where ICU and the engine differ but an exact equivalent exists:
> `\Q...\E` literal quoting, ICU's fixed-width `\uXXXX` / `\UXXXXXXXX` escapes,
> `\Z`, and `$`. ICU's `$` IS `\Z` — it matches before a single final line
> terminator, where both Rust engines mean end of haystack. This is invisible
> for a tag, whose haystack is one tag's text, but the text-delimiter haystack
> is the raw input line with its terminator still attached, so an anchored
> delimiter would otherwise silently never fire. `$` MUST be left untranslated
> under a multi-line flag, where ICU's `$` means end of line instead.
> `\Q...\E` MUST follow ICU's scanner: the span ends at the FIRST
> `\E`, an unterminated `\Q` runs to the end of the pattern, `\\Q` is an escaped
> backslash followed by a literal `Q` and does NOT open a span, and a bare `\E`
> with no open span is an escaped literal `E` rather than a no-op. Quoted text
> MUST remain literal under every flag combination, including free-spacing mode.
> `\Z` MUST match end of input or exactly one final line terminator drawn from
> ICU's terminator set, which is wider than the engine's native `\Z`.

> [spec:cg3:req:tag-regex.source-fidelity]
> The translated pattern MUST NOT be the pattern that gets serialised. A
> compiled tag regex MUST retain the pattern as authored, in ICU spelling, and
> the binary grammar writer MUST emit that. Writing the translation instead
> would make every `.cg3b` the port emits diverge from what the C++ writes and
> stop being readable as ICU — the translation is an implementation detail of
> how this engine matches, and must not reach the wire. This is the same
> invariant that forbids injecting a case-insensitivity flag into the pattern
> text, generalised to every rewrite the compatibility seam performs.

> [spec:cg3:req:tag-regex.silent-divergence]
> A construct the engine would accept but interpret differently from ICU MUST be
> rejected at compile time, naming both the offending construct and the tag it
> came from. Silently compiling to different semantics is the failure mode these
> rules exist to prevent: a grammar that quietly stops firing is worse than one
> that refuses to load. This covers at least ICU's `[:name=value:]` in-set
> properties, `\N{NAME}` named characters, and `\0ooo` octal escapes.
