"""VISL CG-3 for Python.

Loads Constraint Grammars, compiled (``.cg3b``) or textual, and runs them on
text, giving the output vislcg3 gives for the same grammar, input and options.
It also does what the other tools do: it writes a grammar out as cg-comp and
vislcg3 --grammar-out do, relabels one as cg-relabel does, runs one as cg-proc
does, and converts and splits streams as cg-conv and cg-mwesplit do.
"""

# [spec:cg3:req:python.module+1]
# The package is imported as cg3, and its version is the one crates/cg3-python
# carries, apart from the library's.
from cg3._cg3 import (
    ApplyResult,
    Cg3Error,
    Format,
    Grammar,
    GrammarError,
    GrammarInfo,
    ProcFormat,
    RunError,
    __version__,
    convert,
    load_grammar,
    mwesplit,
    parse_grammar,
    read_grammar,
)
from cg3._options import ConvertOptions, LoadOptions, ProcOptions, RunOptions

__all__ = [
    "ApplyResult",
    "Cg3Error",
    "ConvertOptions",
    "Format",
    "Grammar",
    "GrammarError",
    "GrammarInfo",
    "LoadOptions",
    "ProcFormat",
    "ProcOptions",
    "RunError",
    "RunOptions",
    "__version__",
    "convert",
    "load_grammar",
    "mwesplit",
    "parse_grammar",
    "read_grammar",
]
