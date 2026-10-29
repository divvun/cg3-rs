# Loading and running a grammar from a program

The C++ offers embedders `libcg3`, a C API over the same classes the tools use.
The port offers one API in the library instead: load a compiled grammar from
bytes, then run it on bytes. It is
what the wasm package binds, and vislcg3 runs every grammar through it, so the
golden corpus tests it.

A run is vislcg3 without a process. Its options are typed fields where vislcg3
has a command line, the grammar's own options are merged with them as vislcg3
merges them, and its output is what vislcg3 writes.

> [spec:cg3:req:embed.load]
> `CompiledGrammar::from_cg3b` MUST load a `.cg3b` from a byte slice, with no
> file, path or environment behind it, and finish it into an indexed grammar
> that every run of it shares without copying. Bytes vislcg3 refuses MUST fail
> with the error vislcg3 reports for them. A grammar a tool loads or compiles
> some other way and finishes MAY become a `CompiledGrammar` too.

> [spec:cg3:req:embed.info]
> `CompiledGrammar::info` MUST report what vislcg3's verbose summary of a loaded
> grammar reports, with the same values: the numbers of sections, templates,
> rules, sets and tags, the number of rules no index can skip, and whether the
> grammar has dependency rules and relation rules.

> [spec:cg3:req:embed.run-options]
> `RunOptions` MUST express as typed fields the input and output stream formats
> and every option a run reads, which are the options
> `GrammarApplicator::set_options` reads. A field left unset MUST behave as that
> option left off vislcg3's command line.

> [spec:cg3:req:embed.grammar-options]
> A run MUST merge the options its grammar carries as vislcg3 merges them with
> its command line: `CMDARGS` give a value only to an option the caller left
> unset, and `CMDARGS-OVERRIDE` replace the caller's value.

> [spec:cg3:req:embed.run]
> `CompiledGrammar::run` MUST read a whole input from bytes and return the whole
> output as bytes. For the same grammar, input and options it MUST produce,
> byte for byte, the output vislcg3 writes, and when it fails it MUST fail with
> the error vislcg3 reports.

> [spec:cg3:req:embed.runs-independent]
> Each run MUST start from the grammar alone. The state of one run — a window, a
> variable, an external process — MUST NOT reach another, so runs of one grammar
> MAY happen in any order, and at once on several threads.

> [spec:cg3:req:embed.cli-runs-through]
> vislcg3 MUST run every grammar through `CompiledGrammar::run`, whichever way it
> loaded the grammar.
