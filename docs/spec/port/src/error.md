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
