# Library error handling

> [spec:cg3:req:errors.result-primary]
> `Result` MUST be the mechanism by which this crate reports failure. Unwinding
> MUST NOT be used as control flow: no library function may raise a panic to
> signal a condition another frame is expected to catch, and no library function
> may be `-> !` except where the process genuinely cannot continue. A caught
> panic is invisible to the type system, forces every boundary to install a
> catch, and — because the default hook prints before the catch — leaks what
> looks like a crash to an embedder. Panics remain reserved for bugs in this
> crate.

> [spec:cg3:req:errors.layered]
> Error types MUST be layered by boundary rather than collected into a single
> crate-wide enum. Each layer MUST name only the failures it can actually
> produce, and compose into its caller's type by conversion. A consumer matching
> on a parse failure MUST NOT have to consider variants that only a stream
> failure can produce.

> [spec:cg3:req:errors.context]
> An error value MUST carry what a consumer needs to locate and explain the
> failure without reading the diagnostic log: the offending input, where it came
> from, and the underlying cause where one exists. A failure MUST NOT be
> represented by an exit code alone, by a bare `&'static str`, or by `Option`'s
> `None`. Where the crate already emits a human-readable diagnostic for parity,
> that MUST be in addition to the error value, never instead of it.

> [spec:cg3:req:errors.exit-codes-at-cli]
> Process exit codes MUST be derived at the CLI boundary, not carried through
> library error values. The exit code a failure maps to is a property of the
> command-line contract, not of the failure.

> [spec:cg3:req:errors.parse-reports-all]
> A grammar parse MUST report every recoverable error it encountered, in one
> pass, with each error's own line and context. Recoverable parse errors are
> resumable — the parser skips to the next line and continues — so the error
> channel MUST accumulate rather than short-circuit on the first failure. This
> behaviour predates the error-handling rework and MUST survive it.

# Library error detail

The C++ prints a diagnostic at the fatal site and then terminates the process,
so the error value never needed to carry anything but an exit code. A library
cannot do that: its host owns the log stream, and an embedder that must scrape
`tracing` output to find out what went wrong has no supported API at all. These
rules govern what an error value carries.

> [spec:cg3:req:errors.tag-regex-diagnostic]
> A tag pattern that fails to compile MUST surface as an error value carrying
> the diagnostic: the tag text, the offending pattern, the underlying cause, and
> the grammar line where the call site knows it. An embedder MUST be able to
> render the failure without reading the diagnostic log. All tag patterns that
> fail in one grammar load MUST be reported together, so a grammar author fixes
> them in one pass instead of recompiling once per bad tag. The C++-shaped
> `uregex_open` message MAY be retained for parity but MUST NOT be the only
> channel carrying the information, and MUST NOT be emitted at a level that
> forces an embedder to show its users the name of an ICU C function.

> [spec:cg3:req:errors.control-flow-quiet]
> Unwinds this crate raises as control flow MUST NOT produce Rust panic output,
> at every boundary that can observe them — not only the CLI entry points. The
> parser ports the C++ `throw int` parse-error recovery as a panic caught once
> per directive, so the default hook would otherwise print
> `thread '...' panicked ... Box<dyn Any>` for every recoverable parse error,
> several frames before the catch. A library consumer would see that alongside
> the perfectly good error value it also gets, and it reads as a crash. The
> suppression MUST be limited to the payload types this crate raises and always
> catches, and MUST chain to any hook already installed, so a genuine bug —
> here or in the host — still reports normally.

> [spec:cg3:req:errors.parse-result]
> Grammar parse and binary-grammar entry points MUST report success as `Ok(())`
> and nothing else. The C++ convention of returning a recoverable-error COUNT in
> the success channel MUST NOT be reproduced: it makes a single spelling the
> only correct success check and every other spelling silently accept a broken
> grammar. The count MUST be carried in the error value instead. A rejected
> legacy binary revision is a load failure and MUST be an error, not a success
> value. Grammar reindexing failures MUST carry enough detail to identify the
> offending set, rather than an exit code alone.
