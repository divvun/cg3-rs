"""Applying a grammar, and the keyword arguments that are its run options."""

from __future__ import annotations

import inspect
from typing import Any

import pytest

import cg3
from conftest import corpus, read, same_lines

SELECT_N = 'DELIMITERS = "<.>" ; SELECT (n) ;'
COHORT = '"<word>"\n\t"word" n\n\t"word" v\n'

# A value each option that is not a flag accepts. A flag takes True.
VALUES: dict[str, Any] = {
    "input_format": "cg",
    "output_format": "cg",
    "sections": "1",
    "rules": "1",
    "rule": "1",
    "debug_rules": "1",
    "verbose": True,
    "trace": True,
    "text_delimit": True,
    "dep_delimit": True,
    "max_runs": 3,
    "num_windows": 2,
    "soft_limit": 300,
    "hard_limit": 500,
}


def options() -> list[inspect.Parameter]:
    signature = inspect.signature(cg3.Grammar.apply)
    return [p for p in signature.parameters.values() if p.kind is p.KEYWORD_ONLY]


# [spec:cg3:req:python.apply/test]
def test_apply_returns_output_and_warnings() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    result = grammar.apply(COHORT)
    assert isinstance(result, cg3.ApplyResult)
    assert result.output == '"<word>"\n\t"word" n\n'
    assert result.warnings == []
    assert result == grammar.apply(COHORT)
    assert repr(result) == "ApplyResult(output='\"<word>\"\\n\\t\"word\" n\\n', warnings=[])"

    warned = grammar.apply(
        "word$ ^word/word<n>$\n", input_format="apertium", output_format="apertium"
    )
    assert len(warned.warnings) == 1
    assert "$ found without prior ^" in warned.warnings[0]


# [spec:cg3:req:python.apply/test]
def test_options_act_as_their_long_options() -> None:
    sections = cg3.load_grammar(corpus("T_Sections") / "grammar.cg3")
    output = sections.apply(read(corpus("T_Sections") / "input.txt"), sections="4").output
    assert same_lines(read(corpus("T_Sections") / "expected.txt"), output)

    grammar = cg3.parse_grammar(SELECT_N)
    traced = grammar.apply(COHORT, trace=True).output
    assert "SELECT" in traced and ";" in traced
    assert grammar.apply(COHORT, trace=False).output == grammar.apply(COHORT).output
    assert grammar.apply(COHORT, output_format="plain").output.strip() == "word"


# [spec:cg3:req:python.apply/test]
def test_the_signature_names_every_option() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    names = [p.name for p in options()]
    assert len(names) == 40
    assert len(set(names)) == len(names)
    for parameter in options():
        assert parameter.default in (None, False), parameter.name
        left_off: dict[str, Any] = {parameter.name: None}
        grammar.apply(COHORT, **left_off)
        given: dict[str, Any] = {parameter.name: VALUES.get(parameter.name, True)}
        grammar.apply(COHORT, **given)
    assert set(VALUES) <= set(names)


# [spec:cg3:req:python.apply/test]
def test_an_unknown_option_is_a_type_error() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    with pytest.raises(TypeError, match="unexpected keyword argument 'nmu_windows'"):
        grammar.apply(COHORT, nmu_windows=4)  # type: ignore[call-arg]
    with pytest.raises(TypeError):
        grammar.apply(input=COHORT)  # type: ignore[call-arg]


# [spec:cg3:req:python.apply/test]
def test_formats_leave_out_the_binary_one() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    for name in ("cg", "niceline", "apertium", "fst", "plain", "jsonl"):
        grammar.apply("", input_format=name, output_format=name)
    with pytest.raises(ValueError, match="'binary'"):
        grammar.apply(COHORT, output_format="binary")  # type: ignore[arg-type]


# [spec:cg3:req:python.errors/test]
def test_a_wrong_value_raises_pythons_error() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    with pytest.raises(TypeError, match="'ordered' must be bool, not int"):
        grammar.apply(COHORT, ordered=1)  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="'num_windows' must be int, not bool"):
        grammar.apply(COHORT, num_windows=True)
    with pytest.raises(TypeError, match="'num_windows' must be int, not str"):
        grammar.apply(COHORT, num_windows="4")  # type: ignore[arg-type]
    with pytest.raises(OverflowError, match="'max_runs' must be from 0 to 4294967295"):
        grammar.apply(COHORT, max_runs=-1)
    with pytest.raises(OverflowError):
        grammar.apply(COHORT, soft_limit=2**32)
    with pytest.raises(TypeError, match="'trace' must be bool or str, not int"):
        grammar.apply(COHORT, trace=1)  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        grammar.apply(b"bytes")  # type: ignore[arg-type]
