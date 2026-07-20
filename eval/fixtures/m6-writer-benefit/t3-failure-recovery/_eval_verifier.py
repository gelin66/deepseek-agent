#!/usr/bin/env python3
"""Exact verifier for the M6-B1 failure-recovery fixture."""

from __future__ import annotations

import sys
from pathlib import Path


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    target = root / "retry_window.py"
    tests = root / "test_retry_window.py"
    if any(not path.is_file() or path.is_symlink() for path in (target, tests)):
        return 1
    namespace: dict[str, object] = {}
    sys.path.insert(0, str(root))
    try:
        exec(compile(tests.read_text(encoding="utf-8"), str(tests), "exec"), namespace)
        for name, value in namespace.items():
            if name.startswith("test_") and callable(value):
                value()
    except Exception:
        return 1
    finally:
        sys.path.pop(0)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
