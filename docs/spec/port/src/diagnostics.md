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
