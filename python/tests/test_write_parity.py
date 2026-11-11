"""The write parity check: the package against cg-comp and vislcg3
--grammar-out on the golden corpus.

Every test/T_* grammar is compiled by cg-comp from its directory as
`grammar.cg3`, and the built package loads the same source under the same
name twice: from disk, through the default resolver, and from memory, with
every other `.cg3` file of the directory as `files`. Each must write
cg-comp's `.cg3b` and companion file byte for byte, and its `to_text` must be
what vislcg3 --grammar-only --grammar-out writes for the source. The grammar
loaded from cg-comp's `.cg3b` must give vislcg3's text for that `.cg3b` too.
A grammar cg-comp cannot compile must fail to load.
"""

from __future__ import annotations

import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

import cg3
from conftest import CORPUS, Tools, read


def corpus_grammars() -> list[Path]:
    return sorted(d for d in CORPUS.glob("T_*") if (d / "grammar.cg3").is_file())


def companion(cg3b: Path) -> bytes | None:
    """The companion file beside `cg3b`, if one was written."""
    beside = Path(f"{cg3b}.cg3src")
    return beside.read_bytes() if beside.is_file() else None


def grammar_out(tools: Tools, grammar: str | Path, out: Path) -> bytes:
    """What vislcg3 --grammar-only --grammar-out writes for `grammar`."""
    subprocess.run(
        [tools.vislcg3, "--grammar-only", "-g", grammar, "--grammar-out", out],
        check=True,
        capture_output=True,
    )
    return out.read_bytes()


def siblings(test: Path) -> dict[str, str | bytes]:
    """Every `.cg3` file beside the grammar, by the name an INCLUDE gives it."""
    return {f.name: f.read_bytes() for f in test.glob("*.cg3") if f.name != "grammar.cg3"}


# [spec:cg3:req:python.write-parity/test]
@pytest.mark.parametrize("test", corpus_grammars(), ids=lambda d: d.name)
def test_the_package_writes_what_cg_comp_writes(
    test: Path, tools: Tools, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(test)
    compiled = tmp_path / "comp.cg3b"
    comp = subprocess.run([tools.cg_comp, "grammar.cg3", compiled], capture_output=True)
    loads: dict[str, Callable[[], cg3.Grammar]] = {
        "disk": lambda: cg3.load_grammar("grammar.cg3"),
        "memory": lambda: cg3.parse_grammar(
            read(test / "grammar.cg3"), "grammar.cg3", files=siblings(test)
        ),
    }
    if comp.returncode != 0:
        for load in loads.values():
            with pytest.raises(cg3.GrammarError):
                load()
        return
    cg3b = compiled.read_bytes()
    assert companion(compiled) is not None
    text = grammar_out(tools, "grammar.cg3", tmp_path / "text.cg3")
    for how, load in loads.items():
        grammar = load()
        written = tmp_path / f"{how}.cg3b"
        grammar.write_cg3b(written)
        assert written.read_bytes() == cg3b, how
        assert companion(written) == companion(compiled), how
        assert grammar.to_cg3b() == cg3b, how
        assert grammar.to_text().encode("utf-8") == text, how
    from_cg3b = cg3.read_grammar(cg3b).to_text().encode("utf-8")
    assert from_cg3b == grammar_out(tools, compiled, tmp_path / "binary.cg3")


# [spec:cg3:req:python.write-parity/test]
def test_the_check_covers_the_corpus() -> None:
    names = {d.name for d in corpus_grammars()}
    assert len(names) > 60
    assert {"T_Include", "T_External", "T_RelabelList"} <= names
