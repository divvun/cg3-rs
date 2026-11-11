# Loading and running a grammar from a program

The C++ offers embedders `libcg3`, a C API over the same classes the tools use.
The port offers one API in the library instead: load a compiled grammar from
bytes, then run it on bytes. A textual grammar loads from bytes too, for a
host with files to resolve its includes. It is what the wasm and Python
packages bind, and vislcg3 runs every grammar through it, so the golden corpus
tests it.

A run is vislcg3 without a process. Its options are typed fields where vislcg3
has a command line, the grammar's own options are merged with them as vislcg3
merges them, and its output is what vislcg3 writes.

> [spec:cg3:req:embed.load]
> `CompiledGrammar::from_cg3b` MUST load a `.cg3b` from a byte slice, with no
> file, path or environment behind it, and finish it into an indexed grammar
> that every run of it shares without copying. Bytes vislcg3 refuses MUST fail
> with the error vislcg3 reports for them. A grammar a tool loads or compiles
> some other way and finishes MAY become a `CompiledGrammar` too.

> [spec:cg3:req:embed.load-text]
> `CompiledGrammar::from_text` MUST parse a textual grammar from bytes, under
> the name of the source they came from, and finish it as cg-comp finishes the
> grammar it compiles, so that a run of it gives what a run of the `.cg3b`
> cg-comp writes for it gives. A relative `INCLUDE` MUST resolve as it does for
> vislcg3 reading the file that name names. A grammar that does not parse MUST
> fail with every error the parse found and the sources they point into.

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

> [spec:cg3:req:embed.cli-runs-through+1]
> vislcg3 MUST run every grammar, whichever way it loaded it, through
> `CompiledGrammar::prepare` and `PreparedRun::run`, the two steps
> `CompiledGrammar::run` is made of, with the option table it merged itself.
> Preparing MUST report a bad option before any input is read.

## Load options

vislcg3 sets some options on a grammar while it loads it, not on the run. A
run cannot apply them afterwards: `--nrules` decides which rules a textual
grammar keeps, and `--prefix` decides which tags `finish` marks as mappings.

> [spec:cg3:req:embed.load-options]
> `LoadOptions` MUST express as typed fields every option vislcg3 applies to a
> grammar while it loads it: the `--nrules` and `--nrules-v` rule-name
> filters, `--vislcg-compat` and `--prefix`. `CompiledGrammar::from_cg3b` and
> `CompiledGrammar::from_text` MUST take them and apply each to a grammar of
> that kind as vislcg3 applies it, so that a run of the loaded grammar gives
> what vislcg3 gives with the same options. A `--prefix` the grammar's
> `CMDARGS` or `CMDARGS-OVERRIDE` carry MUST apply as vislcg3 applies it. A
> field left unset MUST behave as that option left off vislcg3's command line.

## Includes

> [spec:cg3:req:embed.include-resolver]
> `CompiledGrammar::from_text` MUST read every file an `INCLUDE` names through
> a resolver the caller supplies, given the name as the grammar writes it and
> the name of the source that includes it. The default resolver MUST find and
> read the file as vislcg3 does, so `embed.load-text` holds. The library MUST
> also offer a resolver over named files held in memory, for a host with no
> filesystem, which resolves a relative name against the including source's
> name as vislcg3 resolves it on disk. A name the resolver cannot supply MUST
> fail the parse with the error vislcg3 reports for an include it cannot read.
> A grammar loaded from text MUST keep the text of every source it parsed, so
> an error that quotes the grammar never reads a file again.

## Writing a grammar

The writers in the C++ change the grammar they write: the textual writer names
unnamed sets, and the binary writer reverses each rule's contextual tests in
place. A `CompiledGrammar` is shared, by runs and by threads, so the port's
writers leave it as it was.

> [spec:cg3:req:embed.write-binary]
> `CompiledGrammar::to_cg3b` MUST return the bytes of a `.cg3b` holding the
> grammar. For a grammar `from_text` loaded, they MUST be byte for byte what
> cg-comp writes for the same source under the same name; for one
> `from_cg3b` loaded, what vislcg3 `--grammar-only --grammar-bin` writes for
> it. `CompiledGrammar::companion` MUST return the bytes of the companion file
> cg-comp writes beside that `.cg3b`, or nothing when the grammar has no parse
> sources. Neither may change the grammar: writing it twice MUST give the same
> bytes, and a run after a write MUST give what it gave before.

> [spec:cg3:req:embed.write-text]
> `CompiledGrammar::to_text` MUST return, byte for byte, the textual grammar
> vislcg3 `--grammar-only --grammar-out` writes for the same grammar, and MUST
> NOT change the grammar.

> [spec:cg3:req:embed.cli-compiles-through]
> cg-comp MUST compile through `CompiledGrammar::from_text`, `to_cg3b` and
> `companion`, and vislcg3 MUST write `--grammar-out` and `--grammar-bin`
> through `to_text` and `to_cg3b`, so the golden corpus tests the writers the
> packages bind.

## The other tools

Each of these is a function over bytes in memory, and the tool is a process
around it: it reads its arguments and files, calls the function and writes
what it returns.

> [spec:cg3:req:embed.convert]
> `ConvertOptions` MUST express as typed fields every option cg-conv takes
> that changes its output, and `cg3::convert(input, &ConvertOptions)` MUST
> return, byte for byte, what cg-conv writes for the same input and options,
> or fail with the error cg-conv reports. cg-conv MUST convert through it.

> [spec:cg3:req:embed.mwesplit]
> `cg3::mwesplit(input)` MUST return, byte for byte, what cg-mwesplit writes
> for the same input, or fail with the error it reports. cg-mwesplit MUST
> split through it.

> [spec:cg3:req:embed.relabel]
> `CompiledGrammar::relabel(relabels)` MUST return the grammar cg-relabel
> makes from the grammar and the relabel rules `relabels` holds, so that its
> `.cg3b` is byte for byte the file cg-relabel writes, or fail with the error
> cg-relabel reports. It MUST NOT change either grammar. cg-relabel MUST
> relabel through it.

> [spec:cg3:req:embed.proc]
> `ProcOptions` MUST express as typed fields every cg-proc option that changes
> its output, and `CompiledGrammar::proc(input, &ProcOptions)` MUST return,
> byte for byte, what cg-proc writes for the same grammar, input and options,
> or fail with the error cg-proc reports. cg-proc MUST run through it.
