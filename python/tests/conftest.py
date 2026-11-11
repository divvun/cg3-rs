"""What every test reads: the golden corpus, the cg3 tools, and the program
the corpus's EXTERNAL rule runs.

The programs come from the directory `CG3_BIN` names, or else are built with
cargo from the checkout the tests are in.
"""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
CORPUS = ROOT / "test"


def corpus(name: str) -> Path:
    return CORPUS / name


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def same_lines(expected: str, got: str) -> bool:
    """Compares as the corpus harness does with `diff -b`, ignoring blank lines."""

    def lines(text: str) -> list[list[str]]:
        return [line.split() for line in text.splitlines() if line.strip()]

    return lines(expected) == lines(got)


# The tools the tests hold the package to.
PROGRAMS = ("cg-comp", "vislcg3", "cg-conv", "cg-mwesplit", "cg-proc", "cg-relabel")


@dataclass(frozen=True)
class Tools:
    directory: Path
    cg_comp: Path
    vislcg3: Path
    cg_conv: Path
    cg_mwesplit: Path
    cg_proc: Path
    cg_relabel: Path
    external_example: Path

    def compile(self, grammar: Path, out: Path) -> bytes:
        """The `.cg3b` cg-comp writes for `grammar`."""
        subprocess.run(
            [self.cg_comp, grammar, out], check=True, capture_output=True
        )
        return out.read_bytes()

    def output(self, program: Path, args: list[str | Path], input: str) -> str:
        """What `program` writes for `input` on its standard input."""
        ran = subprocess.run(
            [program, *args], input=input.encode("utf-8"), capture_output=True, check=True
        )
        return ran.stdout.decode("utf-8")


def _built_tools() -> Path:
    bins = [arg for program in PROGRAMS for arg in ("--bin", program)]
    subprocess.run(
        ["cargo", "build", "--quiet", "-p", "cg3-cli", *bins],
        cwd=ROOT,
        check=True,
    )
    subprocess.run(
        ["cargo", "build", "--quiet", "-p", "cg3", "--example", "cg3-external-example"],
        cwd=ROOT,
        check=True,
    )
    metadata = subprocess.run(
        ["cargo", "metadata", "--format-version=1", "--no-deps"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return Path(json.loads(metadata.stdout)["target_directory"]) / "debug"


@pytest.fixture(scope="session")
def tools() -> Tools:
    given = os.environ.get("CG3_BIN")
    directory = Path(given) if given else _built_tools()
    suffix = ".exe" if os.name == "nt" else ""
    examples = directory if given else directory / "examples"
    return Tools(
        directory=directory,
        cg_comp=directory / f"cg-comp{suffix}",
        vislcg3=directory / f"vislcg3{suffix}",
        cg_conv=directory / f"cg-conv{suffix}",
        cg_mwesplit=directory / f"cg-mwesplit{suffix}",
        cg_proc=directory / f"cg-proc{suffix}",
        cg_relabel=directory / f"cg-relabel{suffix}",
        external_example=examples / f"cg3-external-example{suffix}",
    )
