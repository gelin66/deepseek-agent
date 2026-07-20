#!/usr/bin/env python3
"""Deterministic verifier for both frozen M5-B long-context tasks.

The verifier lives outside the copied Agent workspace. It executes public and
hidden behavior checks, verifies the exact changed-file boundary, and reads
the required context proof through Python's AST without executing the target
module. Output contains only typed booleans.
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import json
import os
import stat
import subprocess
import sys
from pathlib import Path


TIMEOUT_SECONDS = 20
SCENARIOS = {
    "task-a": {
        "target": "ranges.py",
        "files": {"README.md", "ranges.py", "test_ranges.py"},
        "hidden": r"""
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
""",
    },
    "task-b": {
        "target": "settings.py",
        "files": {"README.md", "settings.py", "test_settings.py"},
        "hidden": r"""
import copy
import sys
sys.path.insert(0, sys.argv[1])
from settings import merge_settings

base = {
    "agent": {"model": "flash", "limits": {"turns": 8, "tools": 12}},
    "cache": {"enabled": True},
}
override = {
    "agent": {"limits": {"turns": 16}, "reasoning": "high"},
    "new": {"value": 1},
}
before_base = copy.deepcopy(base)
before_override = copy.deepcopy(override)
expected = {
    "agent": {
        "model": "flash",
        "limits": {"turns": 16, "tools": 12},
        "reasoning": "high",
    },
    "cache": {"enabled": True},
    "new": {"value": 1},
}
if merge_settings(base, override) != expected:
    raise SystemExit(1)
if base != before_base or override != before_override:
    raise SystemExit(1)
""",
    },
}


def marker_values(scenario: str) -> list[str]:
    return [
        hashlib.sha256(
            f"codewhale-m5b-v1|{scenario}|{index:02d}".encode("utf-8")
        ).hexdigest()[:10]
        for index in range(1, 10)
    ]


def detect_scenario(workspace: Path) -> str | None:
    files = {
        path.relative_to(workspace).as_posix()
        for path in workspace.iterdir()
        if path.is_file() or path.is_symlink()
    }
    matches = [
        name
        for name, definition in SCENARIOS.items()
        if files == definition["files"]
    ]
    return matches[0] if len(matches) == 1 else None


def run(command: list[str], workspace: Path) -> tuple[bool, bool]:
    environment = {
        "PATH": os.environ.get("PATH", ""),
        "PYTHONDONTWRITEBYTECODE": "1",
        "GIT_CONFIG_NOSYSTEM": "1",
    }
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
    return (
        (bool(relative.parts) and relative.parts[0] == ".git")
        or "__pycache__" in relative.parts
        or relative.parts[:2] == (".codewhale", "state")
    )


def changed_files(workspace: Path) -> set[str] | None:
    try:
        completed = subprocess.run(
            [
                "git",
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
            ],
            cwd=workspace,
            env={
                "PATH": os.environ.get("PATH", ""),
                "GIT_CONFIG_NOSYSTEM": "1",
            },
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return None
    if completed.returncode != 0:
        return None
    entries = completed.stdout.split(b"\0")
    paths: set[str] = set()
    index = 0
    while index < len(entries):
        entry = entries[index]
        index += 1
        if not entry:
            continue
        if len(entry) < 4:
            return None
        status = entry[:2]
        raw_path = entry[3:]
        if b"R" in status or b"C" in status:
            if index >= len(entries):
                return None
            raw_path = entries[index]
            index += 1
        try:
            paths.add(raw_path.decode("utf-8"))
        except UnicodeDecodeError:
            return None
    return paths


def proof_matches(path: Path, expected: str) -> bool:
    try:
        module = ast.parse(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, SyntaxError):
        return False
    values = []
    for node in module.body:
        if (
            isinstance(node, (ast.Assign, ast.AnnAssign))
            and isinstance(node.value, ast.Constant)
            and isinstance(node.value.value, str)
        ):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            if any(isinstance(target, ast.Name) and target.id == "CONTEXT_PROOF" for target in targets):
                values.append(node.value.value)
    return values == [expected]


def workspace_contract(
    workspace: Path, scenario: dict[str, object]
) -> tuple[bool, bool, bool, bool]:
    visible = [
        path
        for path in workspace.rglob("*")
        if not ignored(path.relative_to(workspace))
        and (path.is_file() or path.is_symlink())
    ]
    files = {path.relative_to(workspace).as_posix() for path in visible}
    regular = all(path.is_file() and not path.is_symlink() for path in visible)
    modes = all(
        stat.S_IMODE(path.lstat().st_mode) & 0o111 == 0
        for path in visible
        if not path.is_symlink()
    )
    changed = changed_files(workspace)
    return (
        files == scenario["files"],
        regular,
        modes,
        changed == {scenario["target"]},
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("workspace", type=Path)
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    scenario_name = detect_scenario(workspace)
    if scenario_name is None:
        return 2
    scenario = SCENARIOS[scenario_name]
    target = workspace / str(scenario["target"])

    public_ok, public_timeout = run(
        [sys.executable, "-B", "-m", "unittest", "-q"], workspace
    )
    hidden_ok, hidden_timeout = run(
        [
            sys.executable,
            "-I",
            "-B",
            "-c",
            str(scenario["hidden"]),
            str(workspace),
        ],
        workspace,
    )
    files_ok, regular_ok, modes_ok, changed_ok = workspace_contract(
        workspace, scenario
    )
    checks = {
        "public_tests": public_ok,
        "hidden_cases": hidden_ok
        and proof_matches(target, "".join(marker_values(scenario_name))),
        "immutable_files": changed_ok,
        "workspace_file_set": files_ok,
        "regular_files": regular_ok,
        "file_modes": modes_ok,
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
