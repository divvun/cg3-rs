"""Loads and runs release the GIL, and threads share one grammar."""

from __future__ import annotations

import threading
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor

import cg3
from conftest import corpus, read


def python_runs_during(work: Callable[[], object]) -> int:
    """How many times the calling thread gets round a Python loop while `work`
    runs in another thread. Work that holds the GIL from start to finish leaves
    the loop next to no turns."""
    started = threading.Event()

    def run() -> None:
        started.set()
        work()

    worker = threading.Thread(target=run)
    worker.start()
    started.wait()
    turns = 0
    while worker.is_alive():
        turns += 1
    worker.join()
    return turns


# [spec:cg3:req:python.threads/test]
def test_a_run_releases_the_gil() -> None:
    grammar = cg3.load_grammar(corpus("T_Barrier") / "grammar.cg3")
    long_input = read(corpus("T_Barrier") / "input.txt") * 3000
    assert python_runs_during(lambda: grammar.apply(long_input)) > 1000


# [spec:cg3:req:python.threads/test]
def test_a_load_releases_the_gil() -> None:
    rules = "".join(f'LIST L{i} = "w{i}" ;\nSELECT (t{i}) IF (0 L{i}) ;\n' for i in range(20000))
    source = 'DELIMITERS = "<.>" ;\n' + rules
    assert python_runs_during(lambda: cg3.parse_grammar(source)) > 1000


# [spec:cg3:req:python.threads/test]
def test_threads_apply_one_grammar_at_once() -> None:
    grammar = cg3.load_grammar(corpus("T_Variables") / "grammar.cg3")
    inputs = [
        read(corpus("T_Variables") / "input.txt"),
        read(corpus("T_Barrier") / "input.txt"),
    ]
    alone = [grammar.apply(text).output for text in inputs]
    with ThreadPoolExecutor(max_workers=8) as pool:
        outputs = list(pool.map(lambda i: grammar.apply(inputs[i % 2]).output, range(64)))
    assert outputs == [alone[i % 2] for i in range(64)]
