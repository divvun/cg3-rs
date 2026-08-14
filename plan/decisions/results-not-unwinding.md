---
id [dec:cg3:results-not-unwinding]
epitome "Result is the error mechanism; unwinding is not control flow."
state @decided
category @ban
scope {
    elements ([arch:cg3:error-model] [arch:cg3:textual-parser] [arch:cg3:cli-boundary])
    rules (
        [spec:cg3:req:errors.result-primary]
        [spec:cg3:req:errors.exit-codes-at-cli]
        [spec:cg3:req:errors.parse-reports-all]
    )
}
author "brendan@necessary.nu"
decided_at "2026-08-14T00:00:00Z"
alternatives (
    {
        option "Keep panic-as-control-flow, suppress its output with a global hook."
        rejected_because "This is the status quo and it already failed in the field: an embedder saw `thread 'main' panicked ... Box<dyn Any>` once per recoverable parse error. The hook that hides it is process-global state a library installs to conceal its own design, and a host that sets its own hook afterwards gets the noise back."
    }
    {
        option "Convert the boundaries only, leaving the parser on unwinding internally."
        rejected_because "The parser is where 123 of the 189 divergent sites live. Converting everything except the parser keeps `catch_fatal`, `ParseError`, the panic hook and the whole apparatus alive to serve one module, for none of the benefit."
    }
    {
        option "Return an error-carrying sentinel value instead of Result."
        rejected_because "That is the C-ism being removed. It is how `parse_grammar_data` returns an error COUNT today, and the codebase already documented why that is wrong one layer up: it makes a single spelling the only correct success check."
    }
)
consequences {
    accepted (
        "About 30 function signatures inside the recursive-descent parser change in one atomic step, because `parse_include` -> `parse_from_u_char` -> `parse_directive` -> `parse_include` is a cycle and `parse_contextual_test_list` self-recurses. This phase cannot be split."
        "`parse_from_u_char`'s directive loop cannot use `?`. Recoverable parse errors are RESUMABLE: the loop matches, records the error, restores the AST cursor, skips to the next line and continues."
        "`Cg3Exit`, `ParseError`, `catch_fatal`, `install_panic_filter` and the nine `-> !` functions are all deleted at the end. Their existence is the cost being paid."
        "`[spec:cg3:req:errors.control-flow-quiet]` becomes vacuous once no control-flow unwind exists. It is obsolesced rather than deleted, and `embedder-panic-noise`'s `implements` is edited before the rule goes."
    )
    deferred (
        "The `.unwrap()` residue (10 in textual_parser/mod.rs, 8 in driver.rs, 18 in grammar.rs) is out of scope. Those are documented infallible-lookup asserts, not control flow."
        "Panics remain reachable through arena indexing and slice bounds. This decision bans unwinding as a SIGNAL, not every possible panic."
    )
}
edges {
    enables ([dec:cg3:layered-error-types] [dec:cg3:parse-tag-aborts-on-invalid])
}
codifies (
    [spec:cg3:req:errors.result-primary]
    [spec:cg3:req:errors.exit-codes-at-cli]
    [spec:cg3:req:errors.parse-reports-all]
)
establishes ([arch:cg3:error-model])
---

## Rationale

The port reproduced the C++ `throw`/`catch` structure literally, because during
the port fidelity was the point: `TextualParser::error(...)` threw an `int` that
`parseFromUChar` caught once per directive to recover and continue, and
`CG3Quit` terminated from deep inside library code. Rust has no `throw`, so both
became `panic_any` with a matching `catch_unwind`.

That was the right call while the port was being proven against the reference
implementation. It is the wrong shape to keep. The port is finished, and what
remains is a Rust library whose primary error mechanism is invisible to the type
system: a function that can fail is indistinguishable from one that cannot, the
compiler cannot tell a caller to handle anything, and every public boundary has
to remember to install a net.

The cost stopped being theoretical. Recoverable parse errors printed Rust panic
messages to an embedder's stderr — several frames before the catch that made
them recoverable — reading as a crash next to the perfectly good `Err` the same
call returned. The fix was a process-global panic hook that suppresses this
crate's own payload types. A library installing a global hook to hide its own
control flow is the clearest possible statement that the control flow is wrong.

Unwinding also costs what it was supposed to buy. The `catch_unwind` in
`parse_from_u_char` is the *only* reason multi-error reporting works, so the
mechanism that makes the parser user-friendly is the same one that makes it
opaque. `Result` gives that back explicitly: the directive loop accumulates
rather than short-circuits, which is what
`[spec:cg3:req:errors.parse-reports-all]` now requires in its own right rather
than as an accident of where the catch happens to sit.

The distribution makes this tractable. `grammar_applicator/` is 15,946 lines but
holds 8 divergent sites in 4 functions; `textual_parser/` plus `parser_helpers`
is 4,440 lines and holds 123. Everything outside the parser converts
incrementally and independently. The parser converts once, atomically, and that
is the whole risk of the project.
