# [spec:cg3:req:python.typed]
"""The types of the extension module crates/cg3-python builds, which the cg3
package re-exports. Every public name is declared here, and nothing uses Any.
"""

from os import PathLike
from typing import ClassVar, Final, Literal, TypeAlias, final

__version__: Final[str]

Format: TypeAlias = Literal["cg", "niceline", "apertium", "fst", "plain", "jsonl"]

class Cg3Error(Exception):
    """A failure the cg3 library reports. GrammarError and RunError say which kind."""

class GrammarError(Cg3Error):
    """A grammar that does not load or parse. The message quotes the source of
    a parse failure as vislcg3 does."""

class RunError(Cg3Error):
    """A run that fails, such as one whose EXTERNAL process cannot start. The
    grammar stays usable."""

@final
class GrammarInfo:
    """What vislcg3's verbose summary reports about a loaded grammar."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def sections(self) -> int: ...
    @property
    def templates(self) -> int: ...
    @property
    def rules(self) -> int: ...
    @property
    def sets(self) -> int: ...
    @property
    def tags(self) -> int: ...
    @property
    def rules_any(self) -> int:
        """Rules no tag index can skip."""
    @property
    def has_dependency_rules(self) -> bool: ...
    @property
    def has_relation_rules(self) -> bool: ...

@final
class ApplyResult:
    """A run's output, and every warning and non-fatal error it logged, in order."""

    __hash__: ClassVar[None]  # type: ignore[assignment]
    @property
    def output(self) -> str: ...
    @property
    def warnings(self) -> list[str]: ...

@final
class Grammar:
    """A loaded grammar, applied any number of times. It is immutable, so
    threads may apply it at the same time: each apply is a run of its own over
    the one grammar, without the GIL. Make one with read_grammar, parse_grammar
    or load_grammar."""

    def info(self) -> GrammarInfo:
        """What vislcg3's verbose summary reports about the grammar."""
    def apply(
        self,
        input: str,
        /,
        *,
        input_format: Format | None = None,
        output_format: Format | None = None,
        ordered: bool = False,
        unsafe: bool = False,
        sections: str | None = None,
        rules: str | None = None,
        rule: str | None = None,
        debug_rules: str | None = None,
        verbose: bool | str | None = None,
        no_mappings: bool = False,
        no_corrections: bool = False,
        no_before_sections: bool = False,
        no_sections: bool = False,
        no_after_sections: bool = False,
        trace: bool | str | None = None,
        trace_name_only: bool = False,
        trace_no_removed: bool = False,
        trace_encl: bool = False,
        deleted: bool = False,
        single_run: bool = False,
        max_runs: int | None = None,
        unicode_tags: bool = False,
        unique_tags: bool = False,
        print_ids: bool = False,
        print_dep: bool = False,
        num_windows: int | None = None,
        always_span: bool = False,
        soft_limit: int | None = None,
        hard_limit: int | None = None,
        text_delimit: bool | str | None = None,
        dep_delimit: bool | str | None = None,
        dep_absolute: bool = False,
        dep_original: bool = False,
        dep_allow_loops: bool = False,
        dep_no_crossing: bool = False,
        no_magic_readings: bool = False,
        no_pass_origin: bool = False,
        split_mappings: bool = False,
        show_end_tags: bool = False,
        no_break: bool = False,
    ) -> ApplyResult:
        """Runs the grammar on `input`, as vislcg3 runs it on one standard
        input, and returns the output with every warning and non-fatal error
        the run logged. Each keyword argument is a vislcg3 long option in
        snake_case; `input_format` and `output_format` stand for its `--in-*`
        and `--out-*` options. A run that fails raises RunError, and the
        grammar stays usable."""

def read_grammar(data: bytes | bytearray) -> Grammar:
    """Loads a compiled grammar from the bytes of a `.cg3b`, as cg-comp writes
    it. Bytes that are not one raise GrammarError."""

def parse_grammar(source: str, name: str | None = None) -> Grammar:
    """Parses a textual grammar from `source`, calling it `name` in its
    diagnostics. A relative INCLUDE resolves against the directory of the file
    `name` names, or the current directory without one. A grammar that does
    not parse raises GrammarError, quoting every error."""

def load_grammar(path: str | PathLike[str]) -> Grammar:
    """Loads the grammar in the file at `path`: compiled when it begins with
    the `.cg3b` magic, as vislcg3 decides, and textual otherwise, with relative
    INCLUDEs resolved against its directory."""
