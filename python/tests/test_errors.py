"""What a failed load or run raises."""

from __future__ import annotations

from pathlib import Path

import pytest

import cg3
from conftest import corpus, read

COHORT = '"<word>"\n\t"word" n\n\t"word" v\n'


# [spec:cg3:req:python.errors/test]
def test_the_exceptions_are_one_family() -> None:
    assert issubclass(cg3.Cg3Error, Exception)
    assert issubclass(cg3.GrammarError, cg3.Cg3Error)
    assert issubclass(cg3.RunError, cg3.Cg3Error)
    assert not issubclass(cg3.RunError, cg3.GrammarError)
    assert cg3.GrammarError.__module__ == "cg3"


# [spec:cg3:req:python.errors/test]
def test_a_failed_parse_quotes_its_source() -> None:
    source = 'DELIMITERS = "<.>" ;\nSELECT NoSuchSet ;\nREMOVE AlsoMissing ;\n'
    with pytest.raises(cg3.GrammarError) as raised:
        cg3.parse_grammar(source, "broken.cg3")
    message = str(raised.value)
    assert "broken.cg3:2:" in message
    assert "SELECT NoSuchSet ;" in message
    assert "REMOVE AlsoMissing ;" in message
    assert message.rstrip().endswith("grammar could not be parsed: 2 error(s)")
    assert "\x1b[" not in message


# [spec:cg3:req:python.errors/test]
def test_foreign_bytes_raise_grammar_error() -> None:
    with pytest.raises(cg3.GrammarError, match="CG3B magic"):
        cg3.read_grammar(b"not a grammar")
    with pytest.raises(cg3.GrammarError):
        cg3.read_grammar(b"CG3B")


# [spec:cg3:req:python.errors/test]
def test_a_missing_file_raises_what_open_raises(tmp_path: Path) -> None:
    missing = tmp_path / "missing.cg3"
    with pytest.raises(FileNotFoundError) as raised:
        cg3.load_grammar(missing)
    assert raised.value.filename == str(missing)
    with pytest.raises(OSError):
        cg3.load_grammar(tmp_path)


# [spec:cg3:req:python.errors/test]
def test_a_failed_run_leaves_the_grammar_usable() -> None:
    grammar = cg3.parse_grammar(
        'DELIMITERS = "<.>" ;\nEXTERNAL ONCE "/no/such/program" (*) ;\n', "external.cg3"
    )
    with pytest.raises(cg3.RunError, match="EXTERNAL"):
        grammar.apply(COHORT)
    with pytest.raises(cg3.RunError, match="EXTERNAL"):
        grammar.apply(COHORT)

    barrier = cg3.load_grammar(corpus("T_Barrier") / "grammar.cg3")
    input_text = read(corpus("T_Barrier") / "input.txt")
    first = barrier.apply(input_text).output
    with pytest.raises(TypeError):
        barrier.apply(input_text, ordered="yes")  # type: ignore[arg-type]
    assert barrier.apply(input_text).output == first
