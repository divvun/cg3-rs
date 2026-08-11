# Divvun command-line identity

> [spec:cg3:req:tools.divvun-version-banner+2]
> Every binary shipped by the `cg3` crate MUST accept `--version`, exit
> successfully, write nothing to stderr, and write exactly four lines to
> stdout: `Divvun CG-3 <product> v<crate-version> (<build-date>
> <short-git-hash>)`, `Copyright (C) 2026 UiT The Arctic University of Norway`,
> the retained GrammarSoft GPL copyright notice, and `Source:
> <repository-url>`. `<crate-version>` and `<repository-url>` MUST come from the
> Cargo package's `version` and `repository` metadata; `<build-date>` and
> `<short-git-hash>` MUST come from the build metadata required by
> `[spec:cg3:req:tools.build-provenance]`. The product names are `Disambiguator`
> for `vislcg3` and `cg-proc`, `Compiler` for `cg-comp`, `Format Converter` for
> `cg-conv`, `MWE Splitter` for `cg-mwesplit`, `Relabeller` for `cg-relabel`,
> `Profiler Annotator` for `cg-annotate`, and `Annotation Merger` for
> `cg-merge-annotations`. The established short aliases `vislcg3 -V` and
> `cg-proc -v` MUST emit the same complete banners as their long forms.

> [spec:cg3:req:tools.build-provenance]
> The `cg3` build script MUST export `CG3_BUILD_DATE` and `CG3_GIT_HASH` to every
> package target. `CG3_BUILD_DATE` MUST be an ISO `YYYY-MM-DD` UTC date selected,
> in order, from an explicit `CG3_BUILD_DATE`, `SOURCE_DATE_EPOCH`, or the current
> system time. `CG3_GIT_HASH` MUST be selected, in order, from a non-empty
> explicit `CG3_GIT_HASH`, `git rev-parse --short HEAD`, or the literal
> `unknown` when no Git identity is available. The build script MUST rerun when
> either override, `SOURCE_DATE_EPOCH`, its source inputs, or the current Git
> HEAD/ref changes.
