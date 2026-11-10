"""The package as it installs: its name, version, licence, wheel and types."""

from __future__ import annotations

import ast
import inspect
import os
import re
import subprocess
import sys
import tomllib
import typing
import zipfile
from importlib import metadata
from pathlib import Path

import pytest

import cg3
import cg3._cg3
from conftest import ROOT, corpus

PACKAGE = Path(cg3.__file__).parent
STUB = ast.parse((PACKAGE / "_cg3.pyi").read_text(encoding="utf-8"))
OPTIONS = ast.parse((PACKAGE / "_options.py").read_text(encoding="utf-8"))


def toml(path: Path) -> dict[str, typing.Any]:
    with path.open("rb") as f:
        return tomllib.load(f)


def stub_names() -> dict[str, ast.stmt]:
    names: dict[str, ast.stmt] = {}
    for node in STUB.body:
        if isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            names[node.name] = node
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            names[node.target.id] = node
    return names


def stub_class(name: str) -> ast.ClassDef:
    node = stub_names()[name]
    assert isinstance(node, ast.ClassDef)
    return node


def stub_method(cls: str, name: str) -> ast.FunctionDef:
    for node in stub_class(cls).body:
        if isinstance(node, ast.FunctionDef) and node.name == name:
            return node
    raise AssertionError(f"{cls}.{name} is not in the stub")


def properties(cls: str) -> dict[str, str]:
    """Each property of `cls` in the stub, with its return annotation."""
    found: dict[str, str] = {}
    for node in stub_class(cls).body:
        if isinstance(node, ast.FunctionDef) and node.returns is not None:
            if any(isinstance(d, ast.Name) and d.id == "property" for d in node.decorator_list):
                found[node.name] = ast.unparse(node.returns)
    return found


# [spec:cg3:req:python.module+1/test]
def test_the_version_is_the_binding_crates() -> None:
    version = toml(ROOT / "crates/cg3-python/Cargo.toml")["package"]["version"]
    assert isinstance(version, str)
    assert cg3.__version__ == version
    assert metadata.version("divvun-cg3") == version


@pytest.fixture(scope="module")
def wheel(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """The wheel `CG3_WHEEL` names, which CI sets to the one it tests and
    publishes, or else one maturin builds from python/ as a release would."""
    given = os.environ.get("CG3_WHEEL")
    if given:
        named = Path(given)
        assert named.is_file(), f"CG3_WHEEL names {given}, not a file from {Path.cwd()}"
        return named
    out = tmp_path_factory.mktemp("wheel")
    subprocess.run(
        [sys.executable, "-m", "maturin", "build", "--quiet", "--out", out],
        cwd=ROOT / "python",
        check=True,
    )
    [built] = out.glob("*.whl")
    return built


# [spec:cg3:req:python.module+1/test]
def test_the_wheel_is_cg3_under_the_gpl(wheel: Path) -> None:
    meta = metadata.metadata("divvun-cg3")
    assert meta["Name"] == "divvun-cg3"
    assert meta["License-Expression"] == "GPL-3.0-or-later"
    assert meta["Requires-Python"] == ">=3.11"
    version = cg3.__version__
    with zipfile.ZipFile(wheel) as built:
        files = set(built.namelist())
        licence = built.read(f"divvun_cg3-{version}.dist-info/licenses/COPYING")
    assert {"cg3/__init__.py", "cg3/_cg3.pyi", "cg3/py.typed"} <= files
    assert {f.split("/")[0] for f in files} == {"cg3", f"divvun_cg3-{version}.dist-info"}
    assert licence.decode().strip() == (ROOT / "COPYING").read_text().strip()


# [spec:cg3:req:python.module+1/test]
def test_a_named_wheel_is_the_one_imported() -> None:
    if not os.environ.get("CG3_WHEEL"):
        pytest.skip("no CG3_WHEEL: the tests run against the checkout's build")
    assert PACKAGE.resolve() != (ROOT / "python" / "cg3").resolve()
    assert Path(cg3._cg3.__file__ or "").parent == PACKAGE


# [spec:cg3:req:python.module+1/test]
def test_one_wheel_serves_cpython_311_on(wheel: Path) -> None:
    pyo3 = toml(ROOT / "crates/cg3-python/Cargo.toml")["dependencies"]["pyo3"]
    assert pyo3["features"] == ["abi3-py311"]
    assert f"-{cg3.__version__}-cp311-abi3-" in wheel.name
    with zipfile.ZipFile(wheel) as built:
        [native] = [f for f in built.namelist() if f.startswith("cg3/_cg3.") and f != "cg3/_cg3.pyi"]
    assert native in ("cg3/_cg3.abi3.so", "cg3/_cg3.pyd")


# [spec:cg3:req:python.typed/test]
def test_the_stub_declares_the_whole_api() -> None:
    assert (PACKAGE / "py.typed").is_file()
    assert set(cg3.__all__) <= set(stub_names()) | {"RunOptions"}
    declared = {n for n in stub_names() if not n.startswith("_")} | {"__version__"}
    native = {n for n in dir(cg3._cg3) if not n.startswith("_")} | {"__version__"}
    assert declared == native
    assert all(hasattr(cg3._cg3, n) for n in stub_names())


# [spec:cg3:req:python.typed/test]
def test_no_public_signature_uses_any() -> None:
    for node in [*ast.walk(STUB), *ast.walk(OPTIONS)]:
        assert not (isinstance(node, ast.Name) and node.id == "Any")
        assert not (isinstance(node, ast.Attribute) and node.attr == "Any")


# [spec:cg3:req:python.typed/test]
def test_the_stubs_apply_is_the_runtimes() -> None:
    declared = stub_method("Grammar", "apply")
    assert [a.arg for a in declared.args.posonlyargs] == ["self", "input"]
    assert [a.arg for a in declared.args.args] == ["options"]
    assert [ast.unparse(a.annotation) for a in declared.args.args if a.annotation] == [
        "RunOptions | None"
    ]
    assert str(inspect.signature(cg3.Grammar.apply)) == "(self, input, /, options=None)"


# [spec:cg3:req:python.typed/test]
def test_format_is_the_runtime_literal() -> None:
    node = stub_names()["Format"]
    assert isinstance(node, ast.AnnAssign) and node.value is not None
    declared = ast.unparse(node.value)
    assert declared == f"Literal[{', '.join(repr(f) for f in typing.get_args(cg3.Format))}]"


# [spec:cg3:req:python.typed/test]
def test_results_carry_their_declared_types() -> None:
    kinds: dict[str, type | tuple[type, ...]] = {"int": int, "bool": bool, "str": str}
    grammar = cg3.load_grammar(corpus("T_Barrier") / "grammar.cg3")
    info = grammar.info()
    for name, annotation in properties("GrammarInfo").items():
        value = getattr(info, name)
        assert isinstance(value, kinds[annotation]), name
        assert annotation != "int" or not isinstance(value, bool), name
    result = grammar.apply("")
    assert properties("ApplyResult") == {"output": "str", "warnings": "list[str]"}
    assert isinstance(result.output, str)
    assert isinstance(result.warnings, list)


# [spec:cg3:req:python.typed/test]
def test_names_follow_python_conventions() -> None:
    snake = re.compile(r"^_?_?[a-z][a-z0-9_]*$")
    pascal = re.compile(r"^[A-Z][A-Za-z0-9]*$")
    for node in [*ast.walk(STUB), *ast.walk(OPTIONS)]:
        if isinstance(node, ast.ClassDef):
            assert pascal.match(node.name), node.name
            for field in node.body:
                if isinstance(field, ast.AnnAssign) and isinstance(field.target, ast.Name):
                    assert snake.match(field.target.id), field.target.id
        elif isinstance(node, ast.FunctionDef):
            assert snake.match(node.name), node.name
            for arg in [*node.args.posonlyargs, *node.args.args, *node.args.kwonlyargs]:
                assert snake.match(arg.arg), arg.arg
