"""VISL CG-3 for Python.

Loads Constraint Grammars, compiled (``.cg3b``) or textual, and runs them on
text, giving the output vislcg3 gives for the same grammar, input and options.
"""

from cg3._cg3 import (
    ApplyResult,
    Cg3Error,
    Grammar,
    GrammarError,
    GrammarInfo,
    RunError,
    __version__,
    load_grammar,
    parse_grammar,
    read_grammar,
)

__all__ = [
    "ApplyResult",
    "Cg3Error",
    "Grammar",
    "GrammarError",
    "GrammarInfo",
    "RunError",
    "__version__",
    "load_grammar",
    "parse_grammar",
    "read_grammar",
]
