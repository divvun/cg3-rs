# Grammar diagnostics

`docs/spec/port/src/error.md` governs what an error VALUE carries. These rules
govern what a person sees when a grammar will not compile.

A grammar author's failure is a syntax error in a text file they wrote, and the
only useful answer to one is the offending line with the offending part of it
marked. The C++ answered with a line number and twenty characters of context on
stderr, because it printed at the raise site and terminated; the port inherited
the shape without inheriting the reason. These rules replace it.

> [spec:cg3:req:diagnostics.span]
> A grammar parse failure MUST carry where in the source it happened, not only
> which line. The parser walks a cursor and every failure site already holds the
> offset it failed at, so throwing that away and re-deriving a position from a
> line number and a twenty-character quotation is a loss with no compensating
> benefit. The offset MUST be expressed against the text as the author wrote it,
> free of any padding the parser adds to its working buffer, so a consumer can
> index the source with it directly. A failure with no place in any source — an
> empty input, a reference resolved after the buffer walk has ended, a tag the
> running stream asked for — MUST say so rather than inventing a position.

> [spec:cg3:req:diagnostics.source-identity]
> A span MUST identify its source unambiguously. `#include` means one parse
> covers several files at once, and a file name does not distinguish them: two
> included files may share a base name, and the name a failure carries today is
> the base name alone. The identity MUST therefore be one the parse assigns,
> not one derived from the text of a path.

> [spec:cg3:req:diagnostics.source-retained]
> A failed parse MUST hand its sources over with its errors. The parser's
> buffers are its own working state and every caller drops the parser as soon as
> it has the grammar, so a span with nothing to index is a position into text
> that no longer exists. Re-reading the files at render time is not a substitute:
> it doubles the I/O, cannot work for a parse that was handed bytes, and quotes
> whatever the file says now rather than what was parsed.

> [spec:cg3:req:diagnostics.errors-carried]
> A grammar that failed to parse MUST report the errors themselves, not a tally
> of them. `[spec:cg3:req:errors.parse-reports-all]` requires the parse to find
> every recoverable error in one pass; handing back only how many there were
> spends that work and then discards it, leaving a consumer that wants to show
> them no option but to scrape the diagnostic log — the thing
> `[spec:cg3:req:errors.context]` exists to make unnecessary.

> [spec:cg3:req:diagnostics.source-named]
> A caller that knows the path its grammar came from MUST be able to say so. The
> C++ has a filename entry point; without one every caller reads the file itself
> and hands over anonymous bytes, so the parse can only call the grammar
> `<utf8-memory>` and head every diagnostic with a placeholder. Naming the source
> MUST NOT take the reading away from the caller, which has already sniffed the
> binary-grammar magic off the front and owns the message when the read fails.

> [spec:cg3:req:diagnostics.rendered]
> A grammar parse failure MUST be rendered to a CLI user as the offending source
> quoted with the failure marked in it, with each recoverable error shown in
> full. This is the DEFAULT for a failed grammar load, not an opt-in: a grammar
> author is exactly who a syntax error is for, and a diagnostic they have to
> pass a flag to get is one they will not have when they need it. The rendering
> MAY bypass the diagnostic log, because it is a formatted block for a terminal
> rather than an event for a subscriber, and the log MUST NOT then repeat what
> was rendered.

> [spec:cg3:req:diagnostics.colour-at-tty]
> Rendered diagnostics MUST use colour only when the stream they are written to
> is a terminal. A CLI's stderr is redirected into log files and test harnesses
> as a matter of course, and escape sequences in one are noise at best.

## Runtime diagnostics

The rules above are about a grammar that will not compile. These are about a
grammar that compiled and then failed while it was running: the applicator
builds tags from the stream, and a tag a rule asks for can be one no tag parser
will accept. The C++ labelled that `RT RULE <line>` or `RT INPUT <line>` and the
port inherited the label. A line number with no file is not a location — the
grammar it counts lines in may be a `.cg3b` compiled on another machine, or one
of a dozen `INCLUDE`d files.

> [spec:cg3:req:diagnostics.rule-provenance]
> A textual grammar parse MUST record, for every rule it keeps, which of its
> sources the rule was written in and the char span that rule's text occupies
> there. The parser already computes both — the span is what the profiler
> records for the same rule — so a runtime failure reduced to a bare line number
> is a position the parse had and threw away. The offsets MUST be char offsets
> against the author's text, free of the parser's buffer padding, matching
> `[spec:cg3:req:diagnostics.span]`, because the renderer indexes by character.

> [spec:cg3:req:diagnostics.wire-frozen]
> Rule provenance MUST NOT reach the `.cg3b`. The binary format is byte-compatible
> with the C++ at revision 13898, and neither the grammar's section list nor the
> rule record is length-prefixed, so a field added mid-stream does not degrade for
> a C++ reader — it desynchronises it and every record after it. Provenance is
> therefore in-memory state of a textual parse, and anything a binary load needs
> MUST arrive beside the `.cg3b` rather than inside it.

> [spec:cg3:req:diagnostics.sidecar]
> Compiling a grammar MUST write the sources and their per-rule provenance to a
> companion file beside the `.cg3b`, named by suffixing the binary's own path.
> This is unconditional, not an option: the port tracks the C++ `Options` table,
> and a flag that does not exist upstream is a larger divergence than a file that
> does not. A consumer that finds no companion file MUST degrade to the
> location-free report it would have given anyway — a grammar compiled by the
> C++, or by an older build of this port, still runs.

> [spec:cg3:req:diagnostics.sidecar-identity]
> A companion source file MUST carry a stamp identifying the exact `.cg3b` it
> describes, and a reader MUST verify that stamp before quoting anything from it.
> A stale companion left by an earlier compile is the failure mode this exists to
> prevent: it would quote confidently, from the wrong lines of the wrong grammar,
> and nothing downstream could tell. A stamp that does not match MUST leave the
> reader with no sources rather than with these.

> [spec:cg3:req:diagnostics.source-lazy]
> A loaded grammar MUST retain the PATH of the source it came from, never the
> source text. Grammar sources run to megabytes and the overwhelming majority of
> runs never fail, so materialising them at load time charges every run for a
> diagnostic almost none of them will print. The text MUST be read on the failure
> path and nowhere else.

> [spec:cg3:req:diagnostics.runtime-placed]
> A runtime failure attributed to a rule MUST report the file that rule was
> written in and quote it, by the same rendering
> `[spec:cg3:req:diagnostics.rendered]` gives a parse failure. Both grammar loads
> MUST reach that: a textual load has the sources' names and the rule's span in
> hand, and a binary load has whatever the companion file
> (`[spec:cg3:req:diagnostics.sidecar]`) supplies. Where the sources cannot be
> resolved — no companion file, a stale one, a source that has since been
> deleted — the report MUST fall back to the label and line it always gave, not
> to a guess.

> [spec:cg3:req:diagnostics.runtime-input-named]
> A runtime failure attributed to the input stream rather than to a rule MUST
> name the input it was reading. The line count is the stream's, not the
> grammar's, and a count on its own says nothing about which of several input
> files produced it. The name is the CLI's to supply, since it is the CLI that
> opened the stream; a stream with no file behind it MUST say that rather than
> claim a name.
