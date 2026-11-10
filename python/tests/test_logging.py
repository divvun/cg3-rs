"""The library's events reach Python's logging module."""

from __future__ import annotations

import logging

import pytest

import cg3

GRAMMAR = 'DELIMITERS = "<.>" ; SELECT (n) ;'
STRAY_DOLLAR = "word$ ^word/word<n>$\n"
APERTIUM = cg3.RunOptions(input_format="apertium", output_format="apertium")


# [spec:cg3:req:python.logging/test]
def test_a_runs_events_reach_their_targets_logger(caplog: pytest.LogCaptureFixture) -> None:
    grammar = cg3.parse_grammar(GRAMMAR)
    with caplog.at_level(logging.WARNING, logger="cg3"):
        grammar.apply(STRAY_DOLLAR, APERTIUM)
    [record] = caplog.records
    assert record.name == "cg3.apertium_applicator"
    assert record.levelno == logging.ERROR
    assert "$ found without prior ^" in record.getMessage()


# [spec:cg3:req:python.logging/test]
def test_a_loads_events_reach_logging(caplog: pytest.LogCaptureFixture) -> None:
    with caplog.at_level(logging.DEBUG, logger="cg3"):
        with pytest.raises(cg3.GrammarError):
            cg3.parse_grammar("SELECT NoSuchSet ;\nREMOVE AlsoMissing ;\n", "two.cg3")
    debug = [r for r in caplog.records if r.levelno == logging.DEBUG]
    assert [r.name for r in debug] == ["cg3.textual_parser", "cg3.textual_parser"]
    assert "NoSuchSet" in debug[0].getMessage()
    assert "AlsoMissing" in debug[1].getMessage()


# [spec:cg3:req:python.logging/test]
def test_events_below_the_cg3_level_are_dropped(caplog: pytest.LogCaptureFixture) -> None:
    with caplog.at_level(logging.WARNING, logger="cg3"):
        with pytest.raises(cg3.GrammarError):
            cg3.parse_grammar("SELECT NoSuchSet ;\n")
    assert caplog.records == []

    caplog.set_level(5, logger="cg3")
    with pytest.raises(cg3.GrammarError):
        cg3.parse_grammar("SELECT NoSuchSet ;\n")
    assert [r.levelno for r in caplog.records] == [logging.DEBUG]


# [spec:cg3:req:python.logging/test]
def test_warnings_come_back_whatever_the_level(caplog: pytest.LogCaptureFixture) -> None:
    grammar = cg3.parse_grammar(GRAMMAR)
    with caplog.at_level(logging.CRITICAL, logger="cg3"):
        result = grammar.apply(STRAY_DOLLAR, APERTIUM)
    assert caplog.records == []
    assert len(result.warnings) == 1

    logging.disable(logging.CRITICAL)
    try:
        assert len(grammar.apply(STRAY_DOLLAR, APERTIUM).warnings) == 1
    finally:
        logging.disable(logging.NOTSET)
