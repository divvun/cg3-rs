# [spec:cg3:req:python.typed]
"""The types of the extension module crates/cg3-python builds, which the cg3
package re-exports. Every public name is declared here, and nothing uses Any.
"""

from collections.abc import Mapping
from os import PathLike
from typing import ClassVar, Final, Literal, TypeAlias, final

from cg3._options import ConvertOptions, LoadOptions, ProcOptions, RunOptions

__version__: Final[str]

Format: TypeAlias = Literal["cg", "niceline", "apertium", "fst", "plain", "jsonl"]

ProcFormat: TypeAlias = Literal["cg", "apertium", "matxin"]

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
    def apply(self, input: str, /, options: RunOptions | None = None) -> ApplyResult:
        """Runs the grammar on `input`, as vislcg3 runs it on one standard
        input, with the options a RunOptions gives, and returns the output with
        every warning and non-fatal error the run logged. A run that fails
        raises RunError, and the grammar stays usable."""
    def proc(self, input: str, /, options: ProcOptions | None = None) -> str:
        """Runs the grammar on `input` as cg-proc does, with the options a
        ProcOptions gives, and returns what cg-proc writes. The options the
        grammar sets with CMDARGS apply as they do in cg-proc. A run that fails
        raises RunError, and the grammar stays usable."""
    def relabel(self, relabels: Grammar) -> Grammar:
        """The grammar cg-relabel makes of this one and the relabel rules in
        `relabels`, as a new grammar; both are left as they were. cg-relabel
        relabels a `.cg3b`, so a grammar loaded from text is relabelled as the
        `.cg3b` to_cg3b returns for it. Load the rules with load_grammar to
        read them as cg-relabel reads its rule file, compiled or textual. A
        relabel that fails raises GrammarError."""
    def to_cg3b(self) -> bytes:
        """The `.cg3b` holding the grammar: what cg-comp writes for a grammar
        loaded from text, and vislcg3 --grammar-bin for one loaded from a
        `.cg3b`. The grammar is left as it was."""
    def to_text(self) -> str:
        """The grammar as text, as vislcg3 --grammar-out writes it. The
        grammar is left as it was."""
    def write_cg3b(self, path: str | PathLike[str]) -> None:
        """Writes the `.cg3b` to_cg3b returns to the file at `path`, and, as
        cg-comp does, the companion file beside it, at `path` with `.cg3src`
        added, which holds the grammar's sources so that a runtime error can
        quote them. A grammar loaded from text has one; a grammar loaded from
        a `.cg3b` has none, and nothing is written beside it. The grammar is
        left as it was. A file that cannot be written raises what `open()`
        would."""

def read_grammar(data: bytes | bytearray, *, options: LoadOptions | None = None) -> Grammar:
    """Loads a compiled grammar from the bytes of a `.cg3b`, as cg-comp writes
    it, under the load options a LoadOptions gives. Bytes that are not one
    raise GrammarError."""

def parse_grammar(
    source: str,
    name: str | None = None,
    *,
    options: LoadOptions | None = None,
    files: Mapping[str, str | bytes] | None = None,
) -> Grammar:
    """Parses a textual grammar from `source`, calling it `name` in its
    diagnostics, under the load options a LoadOptions gives. Given `files`, a
    mapping from names to text or bytes, every INCLUDE reads from those files
    and no others: a relative name is found beside the source that includes
    it, else at the top of the names, as on disk. Without it, a relative
    INCLUDE resolves against the directory of the file `name` names, or the
    current directory without one, as vislcg3 resolves it. A grammar that does
    not parse raises GrammarError, quoting every error."""

def load_grammar(path: str | PathLike[str], *, options: LoadOptions | None = None) -> Grammar:
    """Loads the grammar in the file at `path`: compiled when it begins with
    the `.cg3b` magic, as vislcg3 decides, and textual otherwise, with relative
    INCLUDEs resolved against its directory, under the load options a
    LoadOptions gives."""

def convert(input: str, /, options: ConvertOptions | None = None) -> str:
    """Converts `input` from one stream format to another, as cg-conv does,
    with the options a ConvertOptions gives, and returns what cg-conv writes. A
    conversion that fails raises RunError."""

def mwesplit(input: str, /) -> str:
    """Splits each cohort whose wordform tags give its words into one cohort
    per word, as cg-mwesplit does, and returns what cg-mwesplit writes. A split
    that fails raises RunError."""

def _check_load_options(options: LoadOptions) -> None:
    """Checks a LoadOptions as it is made, raising what a load reading its
    fields would raise."""

def _check_run_options(options: RunOptions) -> None:
    """Checks a RunOptions as it is made, raising what a run reading its
    fields would raise."""

def _check_convert_options(options: ConvertOptions) -> None:
    """Checks a ConvertOptions as it is made, raising what a conversion
    reading its fields would raise."""

def _check_proc_options(options: ProcOptions) -> None:
    """Checks a ProcOptions as it is made, raising what a cg-proc run reading
    its fields would raise."""
