"""The tool parity check: the package against cg-conv, cg-mwesplit, cg-proc
and cg-relabel.

The cases are the JavaScript tool parity check's. js/test/tool-fixtures.mjs
lists every input the repository's tests give each tool, reads each tool's
command line into the package's options as the tool's own parser reads it, and
writes what the tool wrote for it. This check runs that module to write them,
or reads the directory `CG3_TOOL_FIXTURES` names, where CI writes them once
for every agent, and gives every case to the built package. The options are
the JavaScript package's, in camelCase: each is the field of the same name in
snake_case, but cg-conv's `input` and `output`, which are `input_format` and
`output_format` as they are in RunOptions.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

import cg3
from conftest import ROOT, Tools

FIXTURES = "CG3_TOOL_FIXTURES"
TOOLS = ("cg-conv", "cg-mwesplit", "cg-proc", "cg-relabel")

# Why tool-fixtures.mjs leaves a case out: a string carries neither.
EXEMPTIONS = {
    "a binary stream, which a string cannot carry",
    "an input that is not UTF-8, which a string cannot carry",
}


@pytest.fixture(scope="module")
def fixtures(request: pytest.FixtureRequest, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """The directory of fixtures, with tools.json listing them."""
    given = os.environ.get(FIXTURES)
    if given:
        directory = Path(given)
        assert (directory / "tools.json").is_file(), f"{FIXTURES} names {given}, which has no tools.json"
        return directory
    node = shutil.which("node")
    if node is None:
        pytest.fail(
            f"the tool parity check writes its fixtures with node, which is not on PATH; "
            f"or name a directory `node js/test/tool-fixtures.mjs <directory>` wrote in {FIXTURES}"
        )
    tools: Tools = request.getfixturevalue("tools")
    out = tmp_path_factory.mktemp("tool-fixtures")
    subprocess.run(
        [node, ROOT / "js" / "test" / "tool-fixtures.mjs", out],
        env={**os.environ, "CG3_BIN": str(tools.directory)},
        check=True,
        capture_output=True,
    )
    return out


@pytest.fixture(scope="module")
def cases(fixtures: Path) -> list[dict[str, Any]]:
    manifest = json.loads((fixtures / "tools.json").read_text(encoding="utf-8"))
    tools: list[dict[str, Any]] = manifest["tools"]
    return tools


def snake(options: dict[str, Any], renamed: dict[str, str]) -> dict[str, Any]:
    """The JavaScript package's options as the fields of the package's."""
    case = re.compile(r"[A-Z]")
    return {
        renamed.get(name, case.sub(lambda m: f"_{m[0].lower()}", name)): value
        for name, value in options.items()
    }


@dataclass
class Outcome:
    output: bytes | None
    error: str


def attempt(f: Callable[[], str | bytes]) -> Outcome:
    """What the package gave, or the failure it raised. Any other exception is
    the package's fault, so it ends the check."""
    try:
        given = f()
    except cg3.Cg3Error as error:
        return Outcome(None, f"{type(error).__name__}: {error}")
    return Outcome(given.encode("utf-8") if isinstance(given, str) else given, "")


class Package:
    """The package's side of each case, with each `.cg3b` loaded once."""

    def __init__(self, fixtures: Path) -> None:
        self.fixtures = fixtures
        self.grammars: dict[str, cg3.Grammar] = {}

    def grammar(self, path: str) -> cg3.Grammar:
        if path not in self.grammars:
            self.grammars[path] = cg3.read_grammar((self.fixtures / path).read_bytes())
        return self.grammars[path]

    def input(self, case: dict[str, Any]) -> str:
        path: str = case["input"]
        return (self.fixtures / path).read_bytes().decode("utf-8")

    def run(self, case: dict[str, Any]) -> Outcome:
        tool = case["tool"]
        if tool == "cg-conv":
            options = cg3.ConvertOptions(
                **snake(case["options"], {"input": "input_format", "output": "output_format"})
            )
            return attempt(lambda: cg3.convert(self.input(case), options))
        if tool == "cg-mwesplit":
            return attempt(lambda: cg3.mwesplit(self.input(case)))
        if tool == "cg-proc":
            proc = cg3.ProcOptions(**snake(case["options"], {}))
            return attempt(lambda: self.grammar(case["grammar"]).proc(self.input(case), proc))
        assert tool == "cg-relabel", tool
        rules = cg3.load_grammar(self.fixtures / case["rules"])
        return attempt(lambda: self.grammar(case["grammar"]).relabel(rules).to_cg3b())


def difference(fixtures: Path, case: dict[str, Any], ours: Outcome) -> str | None:
    label = case["label"]
    if case["failed"]:
        return None if ours.output is None else f"{label}: the tool failed, and the package did not"
    if ours.output is None:
        return f"{label}: the tool succeeded, and the package raised {ours.error}"
    expected = (fixtures / case["expected"]).read_bytes()
    if ours.output == expected:
        return None
    i = next(
        (i for i, (a, b) in enumerate(zip(expected, ours.output)) if a != b),
        min(len(expected), len(ours.output)),
    )
    return (
        f"{label}: byte {i} of {len(ours.output)} differs from the tool's {len(expected)}: "
        f"{ours.output[max(0, i - 30) : i + 50]!r}, the tool {expected[max(0, i - 30) : i + 50]!r}"
    )


# [spec:cg3:req:python.tools-parity/test]
@pytest.mark.parametrize("tool", TOOLS)
def test_the_package_gives_each_tools_bytes(
    tool: str, fixtures: Path, cases: list[dict[str, Any]]
) -> None:
    package = Package(fixtures)
    checked = [c for c in cases if c.get("tool") == tool and "exempt" not in c]
    assert checked, f"tools.json has no case for {tool}"
    differences = [difference(fixtures, case, package.run(case)) for case in checked]
    assert [d for d in differences if d] == []


# [spec:cg3:req:python.tools-parity/test]
def test_the_exemptions_are_the_named_ones(cases: list[dict[str, Any]]) -> None:
    exempt = {c["label"]: c["exempt"] for c in cases if "exempt" in c}
    assert set(exempt.values()) <= EXEMPTIONS, exempt
    assert {c.get("tool") for c in cases} == set(TOOLS)
    assert len(cases) - len(exempt) > 200
