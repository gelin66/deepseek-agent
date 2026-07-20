#!/usr/bin/env python3
"""Exact verifier for the M6-B1 single-file control fixture."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path


def load_module(path: Path):
    spec = importlib.util.spec_from_file_location("m6b_format_bytes", path)
    if spec is None or spec.loader is None:
        raise AssertionError("module loader unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    target = root / "format_bytes.py"
    if not target.is_file() or target.is_symlink():
        return 1
    module = load_module(target)
    expected = {
        0: "0 B",
        1023: "1023 B",
        1024: "1.0 KiB",
        1536: "1.5 KiB",
        1_048_576: "1.0 MiB",
        1_073_741_824: "1.0 GiB",
    }
    if any(module.format_bytes(value) != result for value, result in expected.items()):
        return 1
    try:
        module.format_bytes(-1)
    except ValueError:
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
