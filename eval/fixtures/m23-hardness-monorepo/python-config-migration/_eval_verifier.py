#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys

root = Path(__file__).parent
sys.path.insert(0, str(root))

from serializer import dump_settings
from settings import load_settings
tests = (root / "test_settings.py").read_text(encoding="utf-8")
required = {
    "test_accepts_legacy_timeout_without_reemitting_it",
    "test_rejects_conflicts_bool_and_out_of_range_values",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

checked = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-q"],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if checked.returncode:
    sys.stderr.write(checked.stderr)
    raise SystemExit(1)

legacy = {"timeout_ms": 2500, "mode": "safe"}
legacy_before = dict(legacy)
loaded = load_settings(legacy)
if legacy != legacy_before:
    raise SystemExit(1)
if loaded != {"request_timeout_ms": 2500, "mode": "safe"}:
    raise SystemExit(1)
if dump_settings(loaded) != {"mode": "safe", "request_timeout_ms": 2500}:
    raise SystemExit(1)

canonical = {"request_timeout_ms": 120_000, "mode": "safe"}
canonical_before = dict(canonical)
if load_settings(canonical) != canonical or canonical != canonical_before:
    raise SystemExit(1)

for invalid in (
    {"timeout_ms": 1, "request_timeout_ms": 1},
    {"request_timeout_ms": True},
    {"request_timeout_ms": 0},
    {"request_timeout_ms": 120_001},
    {"timeout_ms": "1000"},
):
    try:
        load_settings(invalid)
    except (TypeError, ValueError):
        pass
    else:
        raise SystemExit(1)

try:
    dump_settings({"timeout_ms": 1000})
except ValueError:
    pass
else:
    raise SystemExit(1)
