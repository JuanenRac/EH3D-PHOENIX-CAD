#!/usr/bin/env python3
# =============================================================================
# EH3D-PHOENIX-CAD - tools/check_test_count.py
# Copyright (C) 2026 JuanenRac (Electro Hobby 3D) <electrohobby3d@gmail.com>
# GPL-3.0-or-later - see LICENSE
# =============================================================================
"""Checks the number of Rust tests that the README files state against what `cargo test` ran.

    cargo test --all-targets 2>&1 | tee test.log
    cargo test --doc         2>&1 | tee -a test.log
    python tools/check_test_count.py test.log

The figure is the sum of the tests that passed in every `test result:` line of the log: the unit
tests, the integration tests and the examples of the documentation. `cargo test --all-targets`
leaves the examples of the documentation out, which is why both commands are needed. Each README
states the figure on the line that names `cargo test` and repeats it twice more.
Prints TEST_COUNT=PASS or TEST_COUNT=FAIL with the reasons.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
README_FILES = ("README.md", "README_spa.md", "README_fra.md", "README_ita.md", "README_deu.md", "README_zho.md", "README_jpn.md", "README_por.md")
PLACES = 3
RESULT = re.compile(r"test result: (\w+)\. (\d+) passed; (\d+) failed")


def measured(log: str) -> int:
    """The tests that passed. A log without results or with a failure is not a measurement."""
    found = RESULT.findall(log)
    if not found:
        raise ValueError("the log has no `test result:` line")
    if any(word != "ok" or int(failed) for word, _, failed in found):
        raise ValueError("the log has tests that failed")
    if "Doc-tests " not in log:
        raise ValueError("the log has no examples of the documentation (run `cargo test --doc` too)")
    return sum(int(passed) for _, passed, _ in found)


def problems(total: int, readmes: dict[str, str]) -> list[str]:
    """What is wrong with the README files for a measured total."""
    out = []
    figure = re.compile(rf"(?<!\d){total}(?!\d)")
    for name, text in readmes.items():
        stated = [line for line in text.splitlines() if "`cargo test`" in line and figure.search(line)]
        if not stated:
            out.append(f"{name}: the line of `cargo test` does not state {total}")
        times = len(figure.findall(text))
        if times < PLACES:
            out.append(f"{name}: states {total} {times} times, not {PLACES}")
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print("usage: check_test_count.py LOG", file=sys.stderr)
        return 0 if argv in (["-h"], ["--help"]) else 2
    try:
        total = measured(Path(argv[0]).read_text(encoding="utf-8", errors="replace"))
    except (OSError, ValueError) as error:
        print(f"TEST_COUNT=FAIL {error}")
        return 1
    found = problems(total, {name: (ROOT / name).read_text(encoding="utf-8") for name in README_FILES})
    for problem in found:
        print(problem)
    print(f"TEST_COUNT={'FAIL' if found else 'PASS'} measured={total}")
    return 1 if found else 0


if __name__ == "__main__":
    if sys.argv[1:] in (["-h"], ["--help"]):
        print(__doc__)
        raise SystemExit(0)
    raise SystemExit(main(sys.argv[1:]))
