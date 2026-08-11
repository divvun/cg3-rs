# Divvun command-line identity

> [spec:cg3:req:tools.divvun-version-banner]
> Every binary shipped by the `cg3` crate MUST accept `--version`, exit
> successfully, write nothing to stderr, and write exactly four lines to
> stdout: `Divvun CG-3 <product> version <crate-version>`, `Copyright (C) 2026
> UiT The Arctic University of Norway`, the retained GrammarSoft GPL copyright
> notice, and `Source: <repository-url>`. `<crate-version>` and
> `<repository-url>` MUST come from the Cargo package's `version` and
> `repository` metadata. The product names are `Disambiguator` for `vislcg3`
> and `cg-proc`, `Compiler` for `cg-comp`, `Format Converter` for `cg-conv`,
> `MWE Splitter` for `cg-mwesplit`, `Relabeller` for `cg-relabel`, `Profiler
> Annotator` for `cg-annotate`, and `Annotation Merger` for
> `cg-merge-annotations`. The established short aliases `vislcg3 -V` and
> `cg-proc -v` MUST emit the same complete banners as their long forms.
