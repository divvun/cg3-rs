---
id [dec:cg3:grammar-source-sidecar]
epitome "A compiled grammar's source travels in a companion file beside the .cg3b, never inside it, and is read only when something fails."
state @decided
category @existence
scope {
    elements ([arch:cg3:binary-format] [arch:cg3:grammar-sources] [arch:cg3:textual-parser] [arch:cg3:diagnostics])
    rules (
        [spec:cg3:req:diagnostics.rule-provenance]
        [spec:cg3:req:diagnostics.wire-frozen]
        [spec:cg3:req:diagnostics.sidecar]
        [spec:cg3:req:diagnostics.sidecar-identity]
        [spec:cg3:req:diagnostics.source-lazy]
        [spec:cg3:req:diagnostics.runtime-placed]
    )
}
author "brendan@necessary.nu"
decided_at "2026-08-15T00:00:00Z"
alternatives (
    {
        option "Carry the sources and per-rule spans inside the .cg3b, behind a new `fields` bit and a new rule `rfields` bit."
        rejected_because "The format is byte-compatible with the C++ at revision 13898 and nothing in it is length-prefixed — not the grammar's section list, not the rule record. A reader that does not know the new bit cannot skip the new bytes; it reads them as the next record and every record after that is garbage. `tests/binary_serial.rs` and `tests/golden.rs` assert the byte compatibility, and it is the whole point of the port."
    }
    {
        option "Append the sources as a trailer after the last record, where a C++ reader would simply stop before reaching it."
        rejected_because "It would be format-safe, but it defeats the reason for keeping the source at all. A trailer is part of the file the loader opens, so either the loader reads past the grammar to find it — paying for a multi-megabyte source on every run — or it seeks, which makes the .cg3b no longer readable from a pipe. A separate file costs nothing until it is opened."
    }
    {
        option "Give cg-comp a `--with-sources` option so writing the companion file is opt-in."
        rejected_because "The port tracks the C++ `Options` table; an option that exists in one and not the other is a divergence every future comparison has to account for. A companion file is invisible to the C++ tools. And an opt-in diagnostic is one nobody has when they need it — the same reasoning as `[spec:cg3:req:diagnostics.rendered]`."
    }
    {
        option "Re-read the grammar's source at failure time using the paths alone, with no companion file, for binary loads too."
        rejected_because "A .cg3b is routinely shipped without its source, compiled on another machine, at a path that means nothing where it runs. Worse, a path that still resolves may hold a different grammar, so quoting from it would be confidently wrong. Provenance a binary load can trust has to travel with the binary."
    }
)
consequences {
    accepted (
        "Compiling a grammar writes two files where it wrote one. Anything that copies a `.cg3b` around without its companion still works — the reader finds nothing and reports what it reported before."
        "A rule carries an in-memory `provenance` a binary load cannot populate, so `Rule::provenance` is `Option`, and the binary path resolves the same information out of the companion file instead. The two loads meet at one resolver rather than at two renderers."
        "The companion file's identity stamp means reading it costs a read of the `.cg3b` as well, to check the stamp. That is paid on the failure path, where a process is about to end anyway."
        "The stamp makes a stale companion file inert rather than wrong. A grammar recompiled without its companion being rewritten reports what it reported before the companion existed."
    )
    deferred (
        "The companion format is this port's own and is not read by anything else. It carries a magic and a version so a future change can be detected, but there is no compatibility obligation and no reader outside this crate."
        "`cg-relabel` rewrites a `.cg3b` and does not carry the companion file across, so a relabelled grammar reports runtime failures without a location. Relabelling rewrites tags rather than rules, so the provenance would still be accurate; wiring it through is separate work."
    )
}
edges {
    requires ([dec:cg3:parse-sources-carry-their-name])
}
codifies (
    [spec:cg3:req:diagnostics.rule-provenance]
    [spec:cg3:req:diagnostics.wire-frozen]
    [spec:cg3:req:diagnostics.sidecar]
    [spec:cg3:req:diagnostics.sidecar-identity]
    [spec:cg3:req:diagnostics.source-lazy]
)
establishes ([arch:cg3:grammar-sources])
---

## Rationale

`Error: parseTag failed at RT RULE 3467` is a real production failure report, and
it is very nearly useless. It names a line in a file it does not name, in a
grammar that may be a `.cg3b` compiled on a machine the reader has never seen,
whose 3467th line is one of a dozen `INCLUDE`d files if it is a line of the
grammar at all.

Everything needed to fix that exists at parse time. The parser knows every source
it read and what it was called; it computes each rule's char span already, for
the profiler. What it does not have is a way to get either past the compile step,
because the artefact between compiling and running is a `.cg3b`, and the `.cg3b`
cannot grow.

That constraint is absolute rather than conservative. The format is unversioned
in the sense that matters: the section list and the rule record have no length
prefix, so an unknown field is not skippable, and a C++ reader at revision 13898
does not fail on one — it silently misreads everything after it. The port asserts
byte-for-byte identity against that reader. So provenance stays in memory for a
textual load, and reaches a binary load through a file that sits beside the
`.cg3b` and is nothing to do with it.

Two properties make the companion file honest rather than merely convenient.

It is checked, not assumed. It stamps the exact `.cg3b` it was written for, and a
reader that cannot match the stamp behaves as though the file were not there.
Quoting the wrong lines of an older grammar with full confidence is a worse
failure than quoting nothing, because nothing downstream can detect it.

It is lazy. The path is retained; the text is not. A grammar source runs to
megabytes and the overwhelming majority of runs never fail, so loading sources at
grammar-load time would charge every run for a report almost none of them print.
This is the property that makes a separate file better than a trailer, and it is
easy to undermine by accident — resolution belongs on the failure path and
nowhere else.
