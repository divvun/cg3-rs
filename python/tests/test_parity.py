"""The parity check: the package against vislcg3 on the golden corpus.

Every test/T_* grammar runs through the built package twice, loaded from its
textual source and from the `.cg3b` cg-comp writes for it, with the options its
args.txt gives vislcg3 read as a RunOptions. Each output must be the bytes
vislcg3 writes for the same grammar, or both must fail. A test whose options the
package cannot take is skipped with the reason, and the exempt set is checked.

Both run in a scratch directory holding the program the corpus's EXTERNAL rule
names as `./cg3-external-example`, so that rule runs as it does under vislcg3.
"""

from __future__ import annotations

import dataclasses
import os
import re
import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

import cg3
from conftest import CORPUS, Tools

EXEMPT = {
    "T_CG2Compat": "an option the package does not offer (--vislcg-compat)",
    "T_NRules": "an option the package does not offer (--nrules)",
}


def corpus_tests() -> list[Path]:
    return sorted(
        d
        for d in CORPUS.glob("T_*")
        if (d / "grammar.cg3").is_file() and (d / "input.txt").is_file()
    )


def option_kinds() -> dict[str, str]:
    """Each field of `RunOptions`, with its annotation."""
    return {f.name: str(f.type) for f in dataclasses.fields(cg3.RunOptions)}


@dataclass
class Options:
    """What a test's args.txt is to the package: the fields of a RunOptions,
    or why there are none."""

    kwargs: dict[str, Any]
    exempt: str | None = None


def short_options(tools: Tools) -> dict[str, str]:
    """vislcg3's short options and the long ones they stand for, from its help."""
    shown = subprocess.run(
        [tools.vislcg3, "--help"], capture_output=True, text=True, check=False
    )
    return dict(re.findall(r"^ -(\S), --(\S+)", shown.stdout + shown.stderr, re.M))


def apply_options(args: list[str], short: dict[str, str]) -> Options:
    kinds = option_kinds()
    kwargs: dict[str, Any] = {}
    words = iter(args)
    for word in words:
        if word.startswith("--"):
            name, _, attached = word[2:].partition("=")
        elif len(word) == 2 and word[0] == "-" and word[1] in short:
            name, attached = short[word[1]], ""
        else:
            return Options({}, f"an argument the check cannot read ({word})")
        if name in ("in-binary", "out-binary"):
            return Options({}, "a binary stream")
        if name.startswith("in-"):
            kwargs["input_format"] = name.removeprefix("in-")
            continue
        if name.startswith("out-"):
            kwargs["output_format"] = name.removeprefix("out-")
            continue
        key = name.replace("-", "_")
        kind = kinds.get(key)
        if kind is None:
            return Options({}, f"an option the package does not offer (--{name})")
        if kind == "bool":
            kwargs[key] = True
        elif kind == "bool | str":
            kwargs[key] = attached or True
        else:
            value = attached or next(words)
            kwargs[key] = int(value) if kind == "int | None" else value
    return Options(kwargs)


@pytest.fixture(scope="module")
def short(tools: Tools) -> dict[str, str]:
    return short_options(tools)


@pytest.fixture(scope="module")
def scratch(tools: Tools, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A directory to run in, where `./cg3-external-example` is the program."""
    directory = tmp_path_factory.mktemp("parity")
    os.symlink(tools.external_example, directory / tools.external_example.name)
    return directory


@dataclass
class Run:
    output: bytes | None
    error: str


def vislcg3(tools: Tools, args: list[str], grammar: Path, test: Path, cwd: Path) -> Run:
    out = cwd / f"{test.name}.{grammar.suffix.lstrip('.')}.out"
    out.unlink(missing_ok=True)
    ran = subprocess.run(
        [tools.vislcg3, *args, "-g", grammar, "-I", test / "input.txt", "-O", out],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    if ran.returncode != 0:
        return Run(None, ran.stderr)
    return Run(out.read_bytes(), "")


def package(load: Callable[[], cg3.Grammar], options: cg3.RunOptions, test: Path) -> Run:
    text = (test / "input.txt").read_bytes().decode("utf-8")
    try:
        return Run(load().apply(text, options).output.encode("utf-8"), "")
    except cg3.Cg3Error as error:
        return Run(None, str(error))


def difference(test: str, how: str, native: Run, ours: Run) -> str | None:
    if native.output is None and ours.output is None:
        return None
    if native.output is None:
        return f"{test} ({how}): vislcg3 failed and the package did not: {native.error}"
    if ours.output is None:
        return f"{test} ({how}): the package failed and vislcg3 did not: {ours.error}"
    if native.output == ours.output:
        return None
    i = next(
        (i for i, (a, b) in enumerate(zip(native.output, ours.output)) if a != b),
        min(len(native.output), len(ours.output)),
    )
    return (
        f"{test} ({how}): byte {i} of {len(ours.output)} differs from vislcg3's "
        f"{len(native.output)}: {ours.output[max(0, i - 30) : i + 50]!r}, "
        f"vislcg3 {native.output[max(0, i - 30) : i + 50]!r}"
    )


def args_of(test: Path) -> list[str]:
    args = test / "args.txt"
    return args.read_text(encoding="utf-8").split() if args.is_file() else []


# [spec:cg3:req:python.parity/test]
@pytest.mark.parametrize("test", corpus_tests(), ids=lambda d: d.name)
def test_the_package_gives_vislcg3s_bytes(
    test: Path,
    tools: Tools,
    short: dict[str, str],
    scratch: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    args = args_of(test)
    options = apply_options(args, short)
    if options.exempt is not None:
        pytest.skip(f"exempt: {options.exempt}")
    run_options = cg3.RunOptions(**options.kwargs)
    textual = test / "grammar.cg3"
    compiled = scratch / f"{test.name}.cg3b"
    tools.compile(textual, compiled)
    monkeypatch.chdir(scratch)
    runs = {
        "textual": (
            vislcg3(tools, args, textual, test, scratch),
            package(lambda: cg3.load_grammar(textual), run_options, test),
        ),
        ".cg3b": (
            vislcg3(tools, args, compiled, test, scratch),
            package(lambda: cg3.read_grammar(compiled.read_bytes()), run_options, test),
        ),
    }
    if test.name == "T_External":
        # Two failures would agree; the check is that the program ran.
        assert all(native.output for native, _ in runs.values()), runs
    differences = [difference(test.name, how, *run) for how, run in runs.items()]
    assert [d for d in differences if d] == []


# [spec:cg3:req:python.parity/test]
def test_the_exemptions_are_the_named_ones(short: dict[str, str]) -> None:
    tests = corpus_tests()
    exempt = {
        t.name: reason
        for t in tests
        if (reason := apply_options(args_of(t), short).exempt) is not None
    }
    assert exempt == EXEMPT
    assert len(tests) - len(exempt) > 60
    assert "T_External" in {t.name for t in tests}
