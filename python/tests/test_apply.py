"""Applying a grammar, and the RunOptions a run reads."""

from __future__ import annotations

import dataclasses
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
    "verbose": "1",
    "trace": "1",
    "text_delimit": "/x/",
    "dep_delimit": "10",
    "max_runs": 3,
    "num_windows": 2,
    "soft_limit": 300,
    "hard_limit": 500,
}


# [spec:cg3:req:python.apply+1/test]
def test_apply_returns_output_and_warnings() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    result = grammar.apply(COHORT)
    assert isinstance(result, cg3.ApplyResult)
    assert result.output == '"<word>"\n\t"word" n\n'
    assert result.warnings == []
    assert result == grammar.apply(COHORT, cg3.RunOptions())
    assert repr(result) == "ApplyResult(output='\"<word>\"\\n\\t\"word\" n\\n', warnings=[])"

    apertium = cg3.RunOptions(input_format="apertium", output_format="apertium")
    warned = grammar.apply("word$ ^word/word<n>$\n", apertium)
    assert len(warned.warnings) == 1
    assert "$ found without prior ^" in warned.warnings[0]


# [spec:cg3:req:python.apply+1/test]
def test_options_act_as_their_long_options() -> None:
    sections = cg3.load_grammar(corpus("T_Sections") / "grammar.cg3")
    text = read(corpus("T_Sections") / "input.txt")
    output = sections.apply(text, cg3.RunOptions(sections="4")).output
    assert same_lines(read(corpus("T_Sections") / "expected.txt"), output)

    grammar = cg3.parse_grammar(SELECT_N)
    traced = grammar.apply(COHORT, cg3.RunOptions(trace=True)).output
    assert "SELECT" in traced and ";" in traced
    assert grammar.apply(COHORT, cg3.RunOptions(trace=False)) == grammar.apply(COHORT)
    plain = grammar.apply(COHORT, cg3.RunOptions(output_format="plain"))
    assert plain.output.strip() == "word"


# [spec:cg3:req:python.apply+1/test]
def test_run_options_has_a_field_per_option() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    fields = dataclasses.fields(cg3.RunOptions)
    assert len(fields) == 40
    assert all(f.kw_only for f in fields)
    assert set(VALUES) <= {f.name for f in fields}
    for field in fields:
        assert field.default in (None, False), field.name
        given: dict[str, Any] = {field.name: VALUES.get(field.name, True)}
        options = cg3.RunOptions(**given)
        assert getattr(options, field.name) == given[field.name]
        grammar.apply(COHORT, options)
    assert str(inspect.signature(cg3.Grammar.apply)) == "(self, input, /, options=None)"


# [spec:cg3:req:python.apply+1/test]
def test_run_options_are_one_immutable_value() -> None:
    options = cg3.RunOptions(trace=True, num_windows=4)
    assert repr(options) == "RunOptions(trace=True, num_windows=4)"
    assert repr(cg3.RunOptions()) == "RunOptions()"
    assert options == cg3.RunOptions(num_windows=4, trace=True)
    assert hash(options) == hash(cg3.RunOptions(num_windows=4, trace=True))
    with pytest.raises(dataclasses.FrozenInstanceError):
        options.trace = False  # type: ignore[misc]
    assert dataclasses.replace(options, trace=False) == cg3.RunOptions(num_windows=4)
    with pytest.raises(TypeError):
        dataclasses.replace(options, max_runs="many")  # type: ignore[arg-type]


# [spec:cg3:req:python.apply+1/test]
def test_an_unknown_option_is_a_type_error() -> None:
    with pytest.raises(TypeError, match="unexpected keyword argument 'nmu_windows'"):
        cg3.RunOptions(nmu_windows=4)  # type: ignore[call-arg]
    with pytest.raises(TypeError):
        cg3.RunOptions(True)  # type: ignore[call-arg, arg-type]
    grammar = cg3.parse_grammar(SELECT_N)
    with pytest.raises(TypeError, match="'options' must be RunOptions, not dict"):
        grammar.apply(COHORT, {"trace": True})  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        grammar.apply(COHORT, trace=True)  # type: ignore[call-arg]


# [spec:cg3:req:python.apply+1/test]
def test_formats_leave_out_the_binary_one() -> None:
    grammar = cg3.parse_grammar(SELECT_N)
    formats: tuple[cg3.Format, ...] = ("cg", "niceline", "apertium", "fst", "plain", "jsonl")
    for name in formats:
        grammar.apply("", cg3.RunOptions(input_format=name, output_format=name))
    with pytest.raises(ValueError, match="'binary'"):
        cg3.RunOptions(output_format="binary")  # type: ignore[arg-type]


# [spec:cg3:req:python.errors/test]
def test_a_wrong_value_raises_pythons_error() -> None:
    with pytest.raises(TypeError, match="'ordered' must be bool, not int"):
        cg3.RunOptions(ordered=1)  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="'num_windows' must be int, not bool"):
        cg3.RunOptions(num_windows=True)
    with pytest.raises(TypeError, match="'num_windows' must be int, not str"):
        cg3.RunOptions(num_windows="4")  # type: ignore[arg-type]
    with pytest.raises(OverflowError, match="'max_runs' must be from 0 to 4294967295"):
        cg3.RunOptions(max_runs=-1)
    with pytest.raises(OverflowError):
        cg3.RunOptions(soft_limit=2**32)
    with pytest.raises(TypeError, match="'trace' must be bool or str, not int"):
        cg3.RunOptions(trace=1)  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        cg3.parse_grammar(SELECT_N).apply(b"bytes")  # type: ignore[arg-type]
