---
id [dec:cg3:parse-sources-carry-their-name]
epitome "A parse holds its sources — name and text — instead of naming one buffer `<utf8-memory>` and dropping the rest."
state @decided
category @existence
scope {
    elements ([arch:cg3:textual-parser] [arch:cg3:error-model] [arch:cg3:cli-boundary])
    rules (
        [spec:cg3:req:diagnostics.source-named]
        [spec:cg3:req:diagnostics.source-identity]
        [spec:cg3:req:diagnostics.source-retained]
    )
}
author "brendan@necessary.nu"
decided_at "2026-08-15T00:00:00Z"
alternatives (
    {
        option "Keep `<utf8-memory>` and let a renderer re-read the grammar file at report time."
        rejected_because "It cannot work for the entry point that exists: a caller that handed over bytes has no file to re-read. It also doubles the I/O, quotes whatever the file says at render time rather than what was parsed, and still leaves `#include` unresolvable — the included buffers were never on the caller's side of the API at all."
    }
    {
        option "Add a filename-carrying entry point that reads the file itself, matching the C++ `parse_grammar(const char*)` exactly."
        rejected_because "Every CLI has already opened the grammar to sniff the `.cg3b` magic off the front, and each owns a specific `Error opening X for reading!` message on failure. Taking the read away would either duplicate it or change four CLIs' error text for no gain. Only the NAME was missing."
    }
    {
        option "Identify a source by its file name rather than by an index the parse assigns."
        rejected_because "`#include` can pull in two files with the same base name, and the base name is all a `ParseError` has ever carried. An index is assigned by the parse, so it cannot collide."
    }
)
consequences {
    accepted (
        "A relative `INCLUDE` in the TOP-LEVEL grammar now resolves against that grammar's directory before falling back to the process working directory. Nested includes always did — `parse_include` passes the included file's own path down — and only the top level was broken, because its path was the `<utf8-memory>` placeholder and `ux_dirname` of that is `./`. This is a consequence of naming the source, not a separate change: the name IS the include base directory."
        "The lookup is strictly wider than it was: the directory-relative path is tried first, and the old working-directory-relative path remains as the fallback `parse_include` always had. A grammar that loaded before still loads."
        "`GrammarError::Parse` grows the sources it retains, so a failed parse holds the grammar text until the error is dropped. A successful parse retains nothing extra — the sources are materialised on the failure path only."
        "`parse_include` now restores the parser's saved buffer state on failure as well as on success. The C++ threw straight past those assignments, which was harmless while the state they guarded was cosmetic; the current-source index is not, and leaving it pointing into an included buffer would give the outer parse's next error a span into the wrong file."
    )
    deferred (
        "`tests/golden.rs` still `current_dir`s into each fixture directory before running, which is what made the top-level include work despite the placeholder. That workaround is now unnecessary for includes but is load-bearing for the fixtures' other relative paths, so it stays."
        "Whether the working-directory fallback in `parse_include` should exist at all is a separate question — it is C++ behaviour, it is now a second-chance lookup rather than the only one, and removing it would be a real behaviour change rather than a widening. Split out as `grammar-diagnostics.include-resolution`."
    )
}
edges {
    requires ([dec:cg3:layered-error-types])
}
codifies (
    [spec:cg3:req:diagnostics.source-named]
    [spec:cg3:req:diagnostics.source-identity]
    [spec:cg3:req:diagnostics.source-retained]
)
establishes ([arch:cg3:textual-parser])
---

## Rationale

A diagnostic that quotes the offending line needs two things the port did not
have: a position in a source, and the source. Neither was withheld on purpose —
both fell out of a port that reproduced the C++ structure faithfully enough to
inherit its constraints without inheriting its context.

The C++ prints at the raise site and terminates, so it never needs to keep a
buffer past the failure, and it never needs to tell a caller which file the
failure was in — the message is already on stderr with the file name in it. The
port kept the message shape and lost the printing-at-the-raise-site part, so the
file name survived as a base-name string on an error value and the buffers went
out of scope with the parser.

`<utf8-memory>` is the visible edge of the same gap. The C++ has both a
filename entry point and a buffer entry point; the port has only the buffer one,
so every CLI reads its own grammar and hands over anonymous bytes. The parse then
has nothing to call the grammar, and every diagnostic — including the ones a user
sees today, before any of this renders — is headed with a placeholder. Passing
the path the caller already has costs one argument.

The name is also the include base directory, because that is what a file name
means to a parser that has to resolve a relative `INCLUDE`. The C++ resolves
against the including file's directory; so does this port, at every level except
the top one, where the directory of `<utf8-memory>` is the working directory.
Naming the top-level source fixes that by construction rather than by design,
and the fix is a widening — the directory-relative path is tried first, and the
working-directory path stays as the fallback it always was. No grammar that
loaded stops loading.
