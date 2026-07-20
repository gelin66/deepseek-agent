#!/usr/bin/env python3
"""Exact verifier for the M6-B1 cross-file implementation fixture."""

from __future__ import annotations

import ast
import copy
import importlib.util
import sys
from pathlib import Path


def load_module(path: Path):
    spec = importlib.util.spec_from_file_location("m6b_agent_profile", path)
    if spec is None or spec.loader is None:
        raise AssertionError("module loader unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def has_required_tests(path: Path) -> bool:
    tree = ast.parse(path.read_text(encoding="utf-8"))
    functions = {
        node.name: node
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
    }
    required = {
        "test_nested_limits_are_merged",
        "test_inputs_are_not_mutated",
    }
    return required.issubset(functions) and all(
        any(isinstance(node, ast.Assert) for node in ast.walk(functions[name]))
        and any(
            isinstance(node, ast.Call)
            and isinstance(node.func, ast.Name)
            and node.func.id == "merge_profile"
            for node in ast.walk(functions[name])
        )
        for name in required
    )


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    production = root / "agent_profile.py"
    tests = root / "test_agent_profile.py"
    if any(not path.is_file() or path.is_symlink() for path in (production, tests)):
        return 1
    if not has_required_tests(tests):
        return 1

    module = load_module(production)
    base = {
        "role": "writer",
        "limits": {"requests": 10, "wall_seconds": 240},
        "tools": ["read_file"],
    }
    override = {
        "limits": {"requests": 6},
        "tools": ["read_file", "edit_file"],
    }
    frozen_base, frozen_override = copy.deepcopy(base), copy.deepcopy(override)
    result = module.merge_profile(base, override)
    if result != {
        "role": "writer",
        "limits": {"requests": 6, "wall_seconds": 240},
        "tools": ["read_file", "edit_file"],
    }:
        return 1
    if base != frozen_base or override != frozen_override:
        return 1

    deeper = module.merge_profile(
        {"a": {"b": {"left": 1, "keep": 2}}},
        {"a": {"b": {"left": 9}}},
    )
    if deeper != {"a": {"b": {"left": 9, "keep": 2}}}:
        return 1

    namespace: dict[str, object] = {}
    sys.path.insert(0, str(root))
    try:
        exec(compile(tests.read_text(encoding="utf-8"), str(tests), "exec"), namespace)
        for name, value in namespace.items():
            if name.startswith("test_") and callable(value):
                value()
    finally:
        sys.path.pop(0)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
