"""cg-conv, cg-mwesplit, cg-proc and cg-relabel through the package, and the
ConvertOptions and ProcOptions they read."""

from __future__ import annotations

import dataclasses
import inspect
from pathlib import Path
from typing import Any

import pytest

import cg3
from conftest import Tools, corpus, read, same_lines
from test_threads import python_runs_during

# A value each option that is not a flag accepts. A flag takes True.
CONVERT_VALUES: dict[str, Any] = {
    "input_format": "cg",
    "output_format": "cg",
    "prefix": "%",
    "wfactor": 2.5,
    "wtag": "X",
    "sub_delim": "#",
    "dep_delimit": 3,
}
PROC_VALUES: dict[str, Any] = {"stream_format": "cg", "sections": 1, "rule": "r1"}

# A dependency number the hash tables reserve, which a stream may not give.
RESERVED = '"<a>"\n\t"a" N #4294967295->1\n'


# [spec:cg3:req:python.tools/test]
def test_convert_writes_what_cg_conv_writes(tools: Tools) -> None:
    jsonl = read(corpus("T_Jsonl") / "input.txt")
    dependency = read(corpus("T_Dependency") / "input.txt")
    subreadings = "^w/a<n>+b<v>$\n"
    cases: list[tuple[list[str | Path], cg3.ConvertOptions, str]] = [
        ([], cg3.ConvertOptions(), jsonl),
        (["--out-apertium"], cg3.ConvertOptions(output_format="apertium"), jsonl),
        (
            ["--out-fst", "-W", "2", "--wtag", "X", "-S", "#"],
            cg3.ConvertOptions(output_format="fst", wfactor=2, wtag="X", sub_delim="#"),
            jsonl,
        ),
        (["-a"], cg3.ConvertOptions(input_format="apertium"), subreadings),
        (["-a", "-l"], cg3.ConvertOptions(input_format="apertium", ltr=True), subreadings),
        (["-a", "-p", "%"], cg3.ConvertOptions(input_format="apertium", prefix="%"), "^w/a<n><%X>$\n"),
        (["-D", "-J"], cg3.ConvertOptions(parse_dep=True, output_format="jsonl"), dependency),
        (["--dep-delimit"], cg3.ConvertOptions(dep_delimit=True), dependency),
        (["--dep-delimit", "2"], cg3.ConvertOptions(dep_delimit=2), dependency),
        (["--add-tags", "-o"], cg3.ConvertOptions(add_tags=True, ordered=True), "one two\n"),
    ]
    for args, options, text in cases:
        assert cg3.convert(text, options) == tools.output(tools.cg_conv, args, text), args


# [spec:cg3:req:python.tools/test]
def test_mwesplit_writes_what_cg_mwesplit_writes(tools: Tools) -> None:
    text = read(corpus("T_MweSplit") / "input.txt")
    split = cg3.mwesplit(text)
    assert split == tools.output(tools.cg_mwesplit, [], text)
    assert same_lines(read(corpus("T_MweSplit") / "expected.txt"), split)
    assert str(inspect.signature(cg3.mwesplit)) == "(input, /)"


# [spec:cg3:req:python.tools/test]
def test_proc_writes_what_cg_proc_writes(tools: Tools, tmp_path: Path) -> None:
    cases: list[tuple[str, list[str | Path], cg3.ProcOptions]] = [
        ("Apertium/T_Select", [], cg3.ProcOptions()),
        ("Apertium/T_Select", ["-t", "-w"], cg3.ProcOptions(trace=True, wordform_case=True)),
        ("Apertium/T_Select", ["-n", "-1"], cg3.ProcOptions(no_word_forms=True, first=True)),
        ("Apertium/T_Select", ["--generation"], cg3.ProcOptions(generation=True)),
        ("Apertium/T_Sections", ["-s", "1"], cg3.ProcOptions(sections=1)),
        ("Apertium/T_MultiWords", ["-f", "2"], cg3.ProcOptions(stream_format="matxin")),
        ("T_Sections", ["-f", "0", "-t", "-s", "2"], cg3.ProcOptions(stream_format="cg", trace=True, sections=2)),
    ]
    for name, args, options in cases:
        compiled = tmp_path / f"{name.replace('/', '_')}.cg3b"
        grammar = cg3.read_grammar(tools.compile(corpus(name) / "grammar.cg3", compiled))
        text = read(corpus(name) / "input.txt")
        assert grammar.proc(text, options) == tools.output(tools.cg_proc, [*args, compiled], text), name
    select = cg3.load_grammar(corpus("Apertium/T_Select") / "grammar.cg3")
    text = read(corpus("Apertium/T_Select") / "input.txt")
    assert same_lines(read(corpus("Apertium/T_Select") / "expected.txt"), select.proc(text))
    assert str(inspect.signature(cg3.Grammar.proc)) == "(self, input, /, options=None)"


# [spec:cg3:req:python.tools/test]
def test_relabel_makes_what_cg_relabel_writes(tools: Tools, tmp_path: Path) -> None:
    test = corpus("T_RelabelList")
    compiled = tmp_path / "grammar.cg3b"
    tools.compile(test / "grammar.cg3", compiled)
    written = tmp_path / "relabelled.cg3b"
    tools.output(tools.cg_relabel, [compiled, test / "relabel.cg3r", written], "")

    grammar = cg3.read_grammar(compiled.read_bytes())
    rules = cg3.load_grammar(test / "relabel.cg3r")
    before = (grammar.to_cg3b(), rules.to_cg3b())
    relabelled = grammar.relabel(rules)
    assert isinstance(relabelled, cg3.Grammar)
    assert relabelled.to_cg3b() == written.read_bytes()
    assert (grammar.to_cg3b(), rules.to_cg3b()) == before
    output = relabelled.apply(read(test / "input.txt")).output
    assert same_lines(read(test / "expected.txt"), output)
    from_text = cg3.load_grammar(test / "grammar.cg3").relabel(rules)
    assert from_text.to_cg3b() == written.read_bytes()


# [spec:cg3:req:python.tools/test]
def test_convert_options_has_a_field_per_option() -> None:
    fields = dataclasses.fields(cg3.ConvertOptions)
    assert [f.name for f in fields] == [
        "input_format",
        "in_auto",
        "add_tags",
        "output_format",
        "prefix",
        "wfactor",
        "wtag",
        "sub_delim",
        "rtl",
        "ltr",
        "ordered",
        "parse_dep",
        "dep_delimit",
        "unicode_tags",
        "deleted",
        "no_break",
    ]
    assert all(f.kw_only for f in fields)
    for field in fields:
        assert field.default in (None, False), field.name
        given: dict[str, Any] = {field.name: CONVERT_VALUES.get(field.name, True)}
        options = cg3.ConvertOptions(**given)
        assert getattr(options, field.name) == given[field.name]
        cg3.convert('"<a>"\n\t"a" N\n', options)
    assert str(inspect.signature(cg3.convert)) == "(input, /, options=None)"


# [spec:cg3:req:python.tools/test]
def test_proc_options_has_a_field_per_option() -> None:
    fields = dataclasses.fields(cg3.ProcOptions)
    assert [f.name for f in fields] == [
        "stream_format",
        "sections",
        "rule",
        "trace",
        "wordform_case",
        "no_word_forms",
        "generation",
        "first",
        "disambiguation",
        "null_flush",
    ]
    assert all(f.kw_only for f in fields)
    grammar = cg3.load_grammar(corpus("Apertium/T_Select") / "grammar.cg3")
    for field in fields:
        assert field.default in ("apertium", None, False), field.name
        given: dict[str, Any] = {field.name: PROC_VALUES.get(field.name, True)}
        options = cg3.ProcOptions(**given)
        assert getattr(options, field.name) == given[field.name]
        grammar.proc("^word/word<n>$\n", options)


# [spec:cg3:req:python.tools/test]
def test_options_the_tools_ignore_change_nothing() -> None:
    text = read(corpus("Apertium/T_MultiWords") / "input.txt")
    assert cg3.convert(text, cg3.ConvertOptions(rtl=True)) == cg3.convert(text)
    grammar = cg3.load_grammar(corpus("Apertium/T_Select") / "grammar.cg3")
    text = read(corpus("Apertium/T_Select") / "input.txt")
    ignored = cg3.ProcOptions(disambiguation=True, null_flush=True)
    assert grammar.proc(text, ignored) == grammar.proc(text)


# [spec:cg3:req:python.tools/test]
def test_tool_options_are_checked_when_made() -> None:
    with pytest.raises(ValueError, match="'binary'"):
        cg3.ConvertOptions(input_format="binary")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="'binary'"):
        cg3.ConvertOptions(output_format="binary")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="'binary'"):
        cg3.ProcOptions(stream_format="binary")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="one character"):
        cg3.ConvertOptions(prefix="")
    with pytest.raises(TypeError, match="'wfactor' must be float, not str"):
        cg3.ConvertOptions(wfactor="2")  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="'wfactor' must be float, not bool"):
        cg3.ConvertOptions(wfactor=True)
    with pytest.raises(TypeError, match="'dep_delimit' must be bool or int, not str"):
        cg3.ConvertOptions(dep_delimit="3")  # type: ignore[arg-type]
    with pytest.raises(OverflowError, match="'dep_delimit' must be from 0 to 4294967295"):
        cg3.ConvertOptions(dep_delimit=2**32)
    with pytest.raises(OverflowError, match="'sections' must be from 0 to 4294967295"):
        cg3.ProcOptions(sections=-1)
    with pytest.raises(TypeError, match="'trace' must be bool, not str"):
        cg3.ProcOptions(trace="yes")  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="unexpected keyword argument 'in_cg'"):
        cg3.ConvertOptions(in_cg=True)  # type: ignore[call-arg]
    with pytest.raises(TypeError, match="unexpected keyword argument 'stream'"):
        cg3.ProcOptions(stream="cg")  # type: ignore[call-arg]
    with pytest.raises(TypeError, match="'options' must be ConvertOptions, not RunOptions"):
        cg3.convert("", cg3.RunOptions())  # type: ignore[arg-type]
    grammar = cg3.load_grammar(corpus("Apertium/T_Select") / "grammar.cg3")
    with pytest.raises(TypeError, match="'options' must be ProcOptions, not dict"):
        grammar.proc("", {"trace": True})  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        grammar.relabel(b"MAP (A) (a) ;")  # type: ignore[arg-type]


# [spec:cg3:req:python.tools/test]
def test_tool_options_are_immutable_values() -> None:
    convert = cg3.ConvertOptions(output_format="apertium", dep_delimit=0)
    assert repr(convert) == "ConvertOptions(output_format='apertium', dep_delimit=0)"
    assert repr(cg3.ConvertOptions()) == "ConvertOptions()"
    assert hash(convert) == hash(cg3.ConvertOptions(dep_delimit=0, output_format="apertium"))
    proc = cg3.ProcOptions(stream_format="cg", trace=True)
    assert repr(proc) == "ProcOptions(stream_format='cg', trace=True)"
    with pytest.raises(dataclasses.FrozenInstanceError):
        proc.trace = False  # type: ignore[misc]
    assert dataclasses.replace(proc, trace=False) == cg3.ProcOptions(stream_format="cg")
    with pytest.raises(TypeError):
        dataclasses.replace(convert, wfactor="x")  # type: ignore[arg-type]


# [spec:cg3:req:python.tools/test]
# [spec:cg3:req:python.errors/test]
def test_a_failed_tool_raises_the_named_error() -> None:
    with pytest.raises(cg3.RunError, match="out of range"):
        cg3.convert(RESERVED, cg3.ConvertOptions(parse_dep=True))
    with pytest.raises(cg3.RunError, match="out of range"):
        cg3.mwesplit(RESERVED)
    external = cg3.parse_grammar('DELIMITERS = "<.>" ;\nEXTERNAL ONCE "/no/such/program" (*) ;\n')
    with pytest.raises(cg3.RunError, match="EXTERNAL"):
        external.proc('"<a>"\n\t"a" N\n', cg3.ProcOptions(stream_format="cg"))
    assert external.info().rules == 1
    grammar = cg3.load_grammar(corpus("T_RelabelList") / "grammar.cg3")
    with pytest.raises(cg3.GrammarError, match="SELECT rule"):
        grammar.relabel(cg3.parse_grammar("SELECT (x) ;"))
    with pytest.raises(TypeError):
        cg3.convert(b"bytes")  # type: ignore[arg-type]


# [spec:cg3:req:python.tools/test]
def test_the_tools_release_the_gil() -> None:
    text = read(corpus("T_Jsonl") / "input.txt") * 2000
    assert python_runs_during(lambda: cg3.convert(text)) > 1000
    split = read(corpus("T_MweSplit") / "input.txt") * 2000
    assert python_runs_during(lambda: cg3.mwesplit(split)) > 1000
    grammar = cg3.load_grammar(corpus("Apertium/T_Select") / "grammar.cg3")
    stream = read(corpus("Apertium/T_Select") / "input.txt") * 3000
    assert python_runs_during(lambda: grammar.proc(stream)) > 1000
    rules = "".join(f"MAP (T{i}) (t{i}) ;\n" for i in range(5000))
    many = cg3.parse_grammar(
        'DELIMITERS = "<.>" ;\n' + "".join(f"SELECT (t{i}) ;\n" for i in range(5000))
    )
    relabels = cg3.parse_grammar(rules)
    assert python_runs_during(lambda: many.relabel(relabels)) > 1000
