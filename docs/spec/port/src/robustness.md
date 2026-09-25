# Robustness against input

`[spec:cg3:req:errors.result-primary]` reserves panics for bugs in this crate.
An assessment of every panic site reachable from input (2026-09-25) found well
over a hundred that input can reach: a stray byte in a stream, a typo in a
grammar, a truncated `.cg3b`, a closed pipe. Most are the C++'s own behaviour
carried over faithfully — it throws from its decoder, trusts the binary files
it wrote, recurses as deep as the data goes — and the rest are indexing the
port added. These rules say what the crate does instead, one failure class at
a time.

They cover every input the crate does not produce in the same process: grammar
source and the files it includes, compiled grammars, the input stream in every
format, the command line and the environment, and replies from an `EXTERNAL`
process. Having written a file does not make it trusted when it is read back.

> [spec:cg3:req:robustness.no-input-panics+1]
> No input MAY cause this crate to panic, abort, overflow its stack, exhaust
> memory by an amount the input chose, or loop without end. Each input is
> either processed or refused with an error value that says what was wrong and
> where (`[spec:cg3:req:errors.context]`). This holds in every build profile:
> an overflow that only a debug build traps is still input the crate failed to
> handle, and in a release build it is silent corruption instead. The one
> exception is a loop the grammar itself asks for, which
> `[spec:cg3:req:robustness.terminates]` leaves to the grammar together with
> whatever it allocates: a `JUMP` back, a `REPEAT`, or a rule whose target
> matches what it produces, such as an `ADDCOHORT` after the last cohort that
> does not exclude the cohort it adds.

## The input stream

> [spec:cg3:req:robustness.stream-invalid-utf8]
> Invalid UTF-8 in the input stream — a stray byte, a truncated or overlong
> sequence, an encoded surrogate — MUST be reported as a run error naming the
> input and the line it occurred on, in every stream format. The C++ throws
> from its decoder and the process terminates; the port keeps the refusal and
> drops the crash. Bytes the author may not know are there MUST NOT be
> silently replaced with U+FFFD. Format detection only sniffs and MUST NOT
> fail on invalid UTF-8 itself; the reader it selects reports it.

> [spec:cg3:req:robustness.stream-text]
> A stream reader MUST read every field of every line in full, whatever its
> length and whatever characters it holds. No field may be cut, split, padded
> or misread because it is long, because it contains non-ASCII text, or because
> it is on the last line and that line has no trailing newline. Readers MUST
> move through text by character, never by byte offsets that can land inside a
> UTF-8 sequence. U+FFFF in the input is text, not end of stream.

> [spec:cg3:req:robustness.empty-tag]
> No tag with empty text MAY be interned. Input that would produce one — an
> empty `<>` in Apertium, an empty `<STREAMCMD:SETVAR:>`, an empty string in a
> binary stream or an `EXTERNAL` reply — is either skipped, where the C++
> produces nothing for it, or reported as a run error.

## Grammars

> [spec:cg3:req:robustness.grammar-text-errors]
> Every textual grammar MUST either load or be refused with a `ParseError`
> placed at the offending source (`[spec:cg3:req:diagnostics.span]`). A
> malformed construct is an authoring mistake and MUST be reported as one,
> including: an empty `()` tag list, a regex or case-insensitive tag with no
> body (`/r`, `/i`), an empty item in a context list (`[A,]`), a variable
> outside `A`–`Z` in a numeric-math tag, a template flag that leaves a
> contextual test without a target, a `?` position used where no override
> supplies one, and a template, varstring brace or parenthesis still open at
> end of input.

> [spec:cg3:req:robustness.cycles+1]
> A cycle in a grammar's structure that can never be decided MUST be refused
> with an error naming it: an `INCLUDE` that reaches itself directly or through
> other files, a template that is its own first step (directly, through other
> templates, or through the first alternative of an `OR`), and a set that
> contains itself. In grammar source these are parse errors; in a `.cg3b`, load
> errors. A template that recurses through a LATER `OR` alternative or through
> a `LINK` is repetition, not a cycle — the recursion ends when an earlier
> alternative or the linked test decides, and `test/T_Templates` relies on it —
> so it loads, and how deep it may recurse at run time is
> `[spec:cg3:req:robustness.depth-bounded]`'s to bound.

> [spec:cg3:req:robustness.binary-grammar-validated]
> A `.cg3b` is untrusted bytes. The reader MUST detect truncation — a read past
> the end is an error, never a silent zero — and MUST check every number it
> reads against what that number indexes before storing it: tag, set, rule and
> context numbers against their counts; hashes against the tables they key;
> set operators, keyword ids and section numbers against their ranges; a tag's
> role bits against its type; each tag number against the others, for
> duplicates. A `.cg3b` that loads MUST be one that `reindex`, both writers
> and a run can process without panicking.

> [spec:cg3:req:robustness.accepted-grammars-run]
> Every grammar that loads MUST run on every input without panicking. A
> construct the engine cannot apply in some situation MUST either be refused
> when the grammar loads or be a run error naming the rule — never a panic.
> Among the cases found: `ADDCOHORT` or `MERGECOHORTS` whose tag list carries
> no wordform (the C++ reports and quits), `SET:` tags, variable tags carrying
> a value, `SWITCHPARENT` on a cohort with no parent, a `CAREFUL` context that
> attaches, a jump position checked against the window it left, and a case
> marker at the end of captured text.

> [spec:cg3:req:robustness.cross-window-actions]
> A rule that acts on a cohort in a window other than the current one —
> reached through a spanning context, a dependency or an attachment — MUST act
> on that cohort's own window: inserts, removals, merges, splits, moves and
> delimits index and renumber the window the cohort is in, and no position
> from one window indexes another. A cohort or window an earlier action
> removed MUST NOT be dereferenced afterwards.

> [spec:cg3:req:robustness.enclosures]
> `PARENTHESES` bookkeeping MUST stay consistent when rules remove, ignore or
> merge cohorts beside or inside an enclosure: an enclosure count never goes
> below zero, a removed or ignored cohort is never unpacked, and a window whose
> cohorts are all enclosed — including by a pair that opens on the window's
> `>>>` — still has its `>>>` cohort.

## Bounds

> [spec:cg3:req:robustness.depth-bounded]
> The depth of anything input can nest MUST NOT be able to exhaust the stack.
> Where the depth is inherent in data — a sub-reading chain, a dependency
> chain, a trie, a set built from sets — the traversal MUST NOT recurse on it.
> Where it is an authoring construct — nested `LINK`s, inline templates, `WITH`
> blocks, chained variable tags — the implementation MAY recurse up to a stated
> limit and MUST refuse input past it with an error.

> [spec:cg3:req:robustness.terminates]
> Every loop in the implementation MUST terminate on every input. A loop whose
> progress depends on the input having some shape — a tag list consumed only
> once a baseform turns up, a reader that stops only at a delimiter, a varstring
> expanded until it is no longer a varstring — MUST also stop at end of input
> or at a bound. Loops the GRAMMAR asks for (`REPEAT`, a `JUMP` back to an
> earlier anchor) are the grammar's to terminate, as they are in the C++, and
> are outside this rule.

> [spec:cg3:req:robustness.hash-probe-bounded]
> The flat hash containers' insert and erase MUST terminate however many
> deleted slots the table holds, as lookup already does, and deleted slots MUST
> be reclaimed so a long-lived table does not degrade. A stream that sets and
> removes variables for as long as it runs MUST NOT be able to hang the
> process.

> [spec:cg3:req:robustness.reserved-keys]
> A number taken from input that the flat hash containers reserve as a
> sentinel (`u32::MAX` and `u32::MAX - 1`) — a dependency or relation number, a
> cohort id, a hash stored in a `.cg3b` or binary stream — MUST be refused
> where it is parsed, as an error. It MUST NOT reach a container, where a
> release build silently corrupts the table.

> [spec:cg3:req:robustness.checked-arithmetic]
> Arithmetic on numbers taken from input — context offsets, sub-reading
> positions, option values such as `--num-windows` and `--rules`, counts read
> from a stream — MUST NOT overflow: it is checked and the input refused, or
> clamped where the C++ semantics define a limit. A count a format stores in a
> fixed width MUST be checked against that width before it is written: the
> binary stream writer refuses a window it cannot represent, with an error,
> rather than wrapping and emitting a corrupt stream.

> [spec:cg3:req:robustness.allocation-bounded]
> No allocation MAY be sized by a count or length taken from input until that
> number has been checked against what the input can actually supply — the
> bytes remaining in a `.cg3b` or binary stream, what an `EXTERNAL` reply
> delivers — or against a stated limit. A range option such as
> `--rules 0-4000000000` MUST NOT be expanded element by element.

> [spec:cg3:req:robustness.binary-stream-validated]
> The binary input stream (`CGBF`) MUST be read with the same discipline as a
> `.cg3b`: a truncated packet, a string running past its packet, or an index
> past the window's tag table is a run error naming the window, never an
> out-of-bounds read.

> [spec:cg3:req:robustness.external-validated]
> A reply from an `EXTERNAL` process is untrusted input. Its counts, lengths
> and indices MUST be checked against the window that was sent before they are
> used, and a mismatch is the external-protocol run error — never an index past
> the window, nor an allocation of whatever size the process declared.

## The command line

> [spec:cg3:req:robustness.cli-output+1]
> A standard output or standard error that closes under a tool — `--help`
> piped into `head`, a reader that exits early — MUST NOT make the tool panic.
> On Unix the tool is ended by `SIGPIPE`, as the C++ tools are: each restores
> the signal's default disposition, which the Rust runtime sets to ignored,
> before it does anything else. Where there is no `SIGPIPE`, the tool stops
> writing to the closed stream, reports nothing, and returns the exit code it
> already had.

> [spec:cg3:req:robustness.cli-arguments]
> Every tool MUST refuse a malformed command line with a message and a nonzero
> exit, never a panic: an argument that is not valid UTF-8, a missing
> positional argument, an option value that does not parse as the number it
> names (`-W abc`, `--dep-delimit abc`) or is out of range, and a relabel file
> holding a rule the relabeller cannot apply. The same holds for options read
> from the environment and from a grammar's `CMDARGS`, and, for the profile
> tools, for an output folder that cannot be created and a profile database
> that is malformed or belongs to another grammar.

## Keeping it that way

> [spec:cg3:req:robustness.panic-sites-justified]
> Once the classes above are handled, the crate MUST deny `clippy::unwrap_used`,
> `clippy::expect_used`, `clippy::panic`, `clippy::unreachable`, `clippy::todo`
> and `clippy::unimplemented` outside tests. Each panic site that remains MUST
> be allowed locally, with a comment stating the invariant that makes it
> unreachable and where that invariant is established. An invariant
> established in another module is the weakest kind; where the type system can
> carry it instead, it SHOULD.
