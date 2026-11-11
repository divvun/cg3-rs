"""Loading a grammar: from `.cg3b` bytes, from a textual source, or from a file."""

from __future__ import annotations

from pathlib import Path

import pytest

import cg3
from conftest import Tools, corpus, read, same_lines


def runs_as_expected(grammar: cg3.Grammar, name: str) -> bool:
    output = grammar.apply(read(corpus(name) / "input.txt")).output
    return same_lines(read(corpus(name) / "expected.txt"), output)


# [spec:cg3:req:python.grammar-load/test]
def test_read_grammar_loads_cg3b_bytes(tools: Tools, tmp_path: Path) -> None:
    data = tools.compile(corpus("T_Barrier") / "grammar.cg3", tmp_path / "g.cg3b")
    assert runs_as_expected(cg3.read_grammar(data), "T_Barrier")
    assert runs_as_expected(cg3.read_grammar(bytearray(data)), "T_Barrier")


# [spec:cg3:req:python.grammar-load/test]
def test_read_grammar_takes_bytes_not_text() -> None:
    with pytest.raises(TypeError):
        cg3.read_grammar("CG3B")  # type: ignore[arg-type]


# [spec:cg3:req:python.grammar-load/test]
def test_parse_grammar_names_the_source() -> None:
    grammar = cg3.parse_grammar('DELIMITERS = "<.>" ; SELECT (n) ;')
    result = grammar.apply('"<word>"\n\t"word" n\n\t"word" v\n')
    assert result.output == '"<word>"\n\t"word" n\n'
    with pytest.raises(cg3.GrammarError, match="mine.cg3"):
        cg3.parse_grammar("SELECT NoSuchSet ;", "mine.cg3")


# [spec:cg3:req:python.grammar-load/test]
def test_parse_grammar_includes_beside_its_name() -> None:
    path = corpus("T_Include") / "grammar.cg3"
    assert runs_as_expected(cg3.parse_grammar(read(path), str(path)), "T_Include")
    with pytest.raises(cg3.GrammarError, match="sets.cg3"):
        cg3.parse_grammar(read(path), str(corpus("T_Active") / "grammar.cg3"))


# [spec:cg3:req:python.grammar-load/test]
def test_load_grammar_sniffs_the_cg3b_magic(tools: Tools, tmp_path: Path) -> None:
    textual = corpus("T_Include") / "grammar.cg3"
    assert runs_as_expected(cg3.load_grammar(textual), "T_Include")
    assert runs_as_expected(cg3.load_grammar(str(textual)), "T_Include")
    compiled = tmp_path / "compiled.grammar"
    tools.compile(textual, compiled)
    assert runs_as_expected(cg3.load_grammar(compiled), "T_Include")


# [spec:cg3:req:python.grammar-load/test]
def test_info_reports_the_grammars_summary() -> None:
    info = cg3.load_grammar(corpus("T_Include") / "grammar.cg3").info()
    assert isinstance(info, cg3.GrammarInfo)
    assert (info.sections, info.rules, info.templates) == (1, 3, 0)
    assert info.rules_any == 0
    assert info.has_dependency_rules is False
    assert info.has_relation_rules is False
    assert info == cg3.load_grammar(corpus("T_Include") / "grammar.cg3").info()
    assert repr(info).startswith("GrammarInfo(sections=1, templates=0, rules=3,")
    dep = cg3.load_grammar(corpus("T_Dependency_Loops") / "grammar.cg3").info()
    assert dep.has_dependency_rules is True


# [spec:cg3:req:python.grammar-load/test]
def test_a_grammar_comes_only_from_a_loader() -> None:
    with pytest.raises(TypeError):
        cg3.Grammar()


# [spec:cg3:req:python.load-options/test]
def test_load_options_reach_every_loader(tools: Tools, tmp_path: Path) -> None:
    textual = corpus("T_NRules") / "grammar.cg3"
    compiled = tmp_path / "nrules.cg3b"
    data = tools.compile(textual, compiled)
    options = cg3.LoadOptions(nrules="pick", nrules_v="X")
    for grammar in (
        cg3.read_grammar(data, options=options),
        cg3.parse_grammar(read(textual), str(textual), options=options),
        cg3.load_grammar(textual, options=options),
        cg3.load_grammar(compiled, options=options),
    ):
        assert runs_as_expected(grammar, "T_NRules")
    assert not runs_as_expected(cg3.load_grammar(textual), "T_NRules")


# [spec:cg3:req:python.load-options/test]
def test_load_options_are_checked_when_made(tools: Tools, tmp_path: Path) -> None:
    with pytest.raises(TypeError, match="LoadOptions"):
        cg3.LoadOptions(nrules=1)  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="vislcg_compat"):
        cg3.LoadOptions(vislcg_compat="yes")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="one character"):
        cg3.LoadOptions(prefix="ab")
    with pytest.raises(ValueError, match="--nrules-v"):
        cg3.LoadOptions(nrules_v="(")
    with pytest.raises(TypeError, match="must be LoadOptions"):
        cg3.parse_grammar("SELECT (x) ;", options=cg3.RunOptions())  # type: ignore[arg-type]
    assert repr(cg3.LoadOptions(prefix="%")) == "LoadOptions(prefix='%')"
    data = tools.compile(corpus("T_Barrier") / "grammar.cg3", tmp_path / "g.cg3b")
    with pytest.raises(cg3.GrammarError, match="Mapping prefix must match"):
        cg3.read_grammar(data, options=cg3.LoadOptions(prefix="%"))
