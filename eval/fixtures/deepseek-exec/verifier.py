#!/usr/bin/env python3
"""Deterministic verifier for the DeepSeek exec coding fixture.

This file is deliberately outside the workspace copied for the Agent. It emits
only check names and booleans; test output and imported values are discarded.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import subprocess
import sys
from pathlib import Path


TIMEOUT_SECONDS = 20
EXPECTED_FILES = {"README.md", "ranges.py", "test_ranges.py"}
IMMUTABLE_SHA256 = {
    "README.md": "b81288d6d2b41c36a0d4bb1eeba7624429086882161292a0f63112078ccb7fab",
    "test_ranges.py": "5a7e98c4a7fb7afe20a465c6a2485e73d4d95f65ad4e9cc40c76662300268b6e",
}
HIDDEN_CASES = r"""
import sys
sys.path.insert(0, sys.argv[1])
from ranges import coalesce_ranges

cases = [
    ([8, 1, 2, 5, 6, 10], [(1, 2), (5, 6), (8, 8), (10, 10)]),
    ([-3, -2, 0, 2, 3, 4], [(-3, -2), (0, 0), (2, 4)]),
    ([7, 6, 5, 7, 5], [(5, 7)]),
]
if any(coalesce_ranges(values) != expected for values, expected in cases):
    raise SystemExit(1)
"""


def run(command: list[str], workspace: Path) -> tuple[bool, bool]:
    environment = os.environ.copy()
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    try:
        completed = subprocess.run(
            command,
            cwd=workspace,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return False, True
    return completed.returncode == 0, False


def ignored(relative: Path) -> bool:
    parts = relative.parts
    return (
        (bool(parts) and parts[0] == ".git")
        or "__pycache__" in parts
        or parts[:2] == (".codewhale", "state")
    )


def workspace_contract(workspace: Path) -> tuple[bool, bool, bool, bool]:
    visible = [
        path
        for path in workspace.rglob("*")
        if not ignored(path.relative_to(workspace))
        and (path.is_file() or path.is_symlink())
    ]
    files = {path.relative_to(workspace).as_posix() for path in visible}
    regular_files = all(path.is_file() and not path.is_symlink() for path in visible)
    file_modes = all(
        stat.S_IMODE(path.lstat().st_mode) & 0o111 == 0
        for path in visible
        if not path.is_symlink()
    )
    immutable_ok = all(
        (workspace / relative).is_file()
        and not (workspace / relative).is_symlink()
        and hashlib.sha256((workspace / relative).read_bytes()).hexdigest() == digest
        for relative, digest in IMMUTABLE_SHA256.items()
    )
    return immutable_ok, files == EXPECTED_FILES, regular_files, file_modes


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("workspace", type=Path)
    args = parser.parse_args()
    workspace = args.workspace.resolve()

    public_ok, public_timeout = run(
        [sys.executable, "-m", "unittest", "-q"], workspace
    )
    hidden_ok, hidden_timeout = run(
        [sys.executable, "-I", "-c", HIDDEN_CASES, str(workspace)], workspace
    )
    immutable_ok, files_ok, regular_files_ok, file_modes_ok = workspace_contract(workspace)
    checks = {
        "public_tests": public_ok,
        "hidden_cases": hidden_ok,
        "immutable_files": immutable_ok,
        "workspace_file_set": files_ok,
        "regular_files": regular_files_ok,
        "file_modes": file_modes_ok,
    }
    result = {
        "schema": "codewhale.eval.deepseek-exec-verifier.v1",
        "passed": all(checks.values()),
        "timed_out": public_timeout or hidden_timeout,
        "checks": checks,
    }
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
