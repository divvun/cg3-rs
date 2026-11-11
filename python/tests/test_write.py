"""Writing a grammar out: its `.cg3b` and companion file, as cg-comp writes
them, and its text, as vislcg3 --grammar-out writes it."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

import cg3
from conftest import Tools, corpus, read
from test_threads import python_runs_during

# T_Include's grammar INCLUDEs two files, so its companion carries three sources.
INCLUDE = corpus("T_Include") / "grammar.cg3"


def grammar_out(tools: Tools, grammar: Path, out: Path) -> str:
    """What vislcg3 --grammar-only --grammar-out writes for `grammar`."""
    subprocess.run(
        [tools.vislcg3, "--grammar-only", "-g", grammar, "--grammar-out", out],
        check=True,
        capture_output=True,
    )
    return out.read_text(encoding="utf-8")


# [spec:cg3:req:python.write/test]
def test_to_cg3b_is_what_cg_comp_writes(tools: Tools, tmp_path: Path) -> None:
    expected = tools.compile(INCLUDE, tmp_path / "comp.cg3b")
    cg3b = cg3.load_grammar(INCLUDE).to_cg3b()
    assert isinstance(cg3b, bytes)
    assert cg3b == expected
    assert cg3.parse_grammar(read(INCLUDE), str(INCLUDE)).to_cg3b() == expected
    assert cg3.read_grammar(expected).to_cg3b() == expected


# [spec:cg3:req:python.write/test]
def test_to_text_is_vislcg3s_grammar_out(tools: Tools, tmp_path: Path) -> None:
    text = cg3.load_grammar(INCLUDE).to_text()
    assert isinstance(text, str)
    assert text == grammar_out(tools, INCLUDE, tmp_path / "text.cg3")
    compiled = tmp_path / "comp.cg3b"
    tools.compile(INCLUDE, compiled)
    from_cg3b = cg3.load_grammar(compiled).to_text()
    assert from_cg3b == grammar_out(tools, compiled, tmp_path / "binary.cg3")


# [spec:cg3:req:python.write/test]
def test_write_cg3b_writes_cg_comps_companion(tools: Tools, tmp_path: Path) -> None:
    compiled = tmp_path / "comp.cg3b"
    tools.compile(INCLUDE, compiled)
    written = tmp_path / "written.cg3b"
    cg3.load_grammar(INCLUDE).write_cg3b(written)
    assert written.read_bytes() == compiled.read_bytes()
    companion = Path(f"{written}.cg3src")
    assert companion.read_bytes() == Path(f"{compiled}.cg3src").read_bytes()

    again = tmp_path / "again.cg3b"
    cg3.read_grammar(compiled.read_bytes()).write_cg3b(str(again))
    assert again.read_bytes() == compiled.read_bytes()
    assert not Path(f"{again}.cg3src").exists()


# [spec:cg3:req:python.write/test]
def test_writing_leaves_the_grammar_as_it_was(tmp_path: Path) -> None:
    grammar = cg3.load_grammar(INCLUDE)
    text = read(corpus("T_Include") / "input.txt")
    before = (grammar.apply(text), grammar.info(), grammar.to_cg3b(), grammar.to_text())
    grammar.write_cg3b(tmp_path / "one.cg3b")
    grammar.write_cg3b(tmp_path / "two.cg3b")
    after = (grammar.apply(text), grammar.info(), grammar.to_cg3b(), grammar.to_text())
    assert after == before
    assert (tmp_path / "one.cg3b").read_bytes() == (tmp_path / "two.cg3b").read_bytes()


# [spec:cg3:req:python.write/test]
# [spec:cg3:req:python.errors/test]
def test_an_unwritable_file_raises_what_open_raises(tmp_path: Path) -> None:
    grammar = cg3.load_grammar(INCLUDE)
    missing = tmp_path / "no" / "such" / "dir.cg3b"
    with pytest.raises(FileNotFoundError) as raised:
        grammar.write_cg3b(missing)
    assert raised.value.filename == str(missing)
    with pytest.raises(OSError):
        grammar.write_cg3b(tmp_path)
    with pytest.raises(TypeError):
        grammar.write_cg3b(1)  # type: ignore[arg-type]


# [spec:cg3:req:python.write/test]
def test_writing_releases_the_gil(tmp_path: Path) -> None:
    rules = "".join(f'LIST L{i} = "w{i}" ;\nSELECT (t{i}) IF (0 L{i}) ;\n' for i in range(20000))
    grammar = cg3.parse_grammar('DELIMITERS = "<.>" ;\n' + rules)
    assert python_runs_during(grammar.to_cg3b) > 1000
    assert python_runs_during(grammar.to_text) > 1000
    assert python_runs_during(lambda: grammar.write_cg3b(tmp_path / "big.cg3b")) > 1000
