#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys


root = Path(__file__).parent
sys.path.insert(0, str(root))

from codec import decode_policy
from policy import build_policy

tests = (root / "test_policy.py").read_text(encoding="utf-8")
required = {
    "test_emits_v3_with_canonical_scopes_and_copy",
    "test_decodes_v2_v3_and_rejects_invalid_contracts",
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

scopes = ["tests", "src", "src"]
encoded = build_policy("deepseek-v4-pro", "high", scopes)
if encoded != {
    "version": 3,
    "route": {"model": "deepseek-v4-pro", "effort": "high"},
    "scopes": ["src", "tests"],
}:
    raise SystemExit(1)
encoded["scopes"].append("docs")
if scopes != ["tests", "src", "src"]:
    raise SystemExit(1)

v2 = {
    "version": 2,
    "model": "deepseek-v4-flash",
    "reasoning": "high",
    "paths": ["src"],
}
v3 = {
    "version": 3,
    "route": {"model": "deepseek-v4-pro", "effort": "max"},
    "scopes": ["src", "tests"],
}
if decode_policy(v2) != ("deepseek-v4-flash", "high", ["src"]):
    raise SystemExit(1)
if decode_policy(v3) != ("deepseek-v4-pro", "max", ["src", "tests"]):
    raise SystemExit(1)

for invalid in (
    {"version": 4, "route": {}, "scopes": []},
    {"version": True, "model": "deepseek-v4-pro", "reasoning": "high", "paths": []},
    {"version": 3, "route": {"model": "other", "effort": "high"}, "scopes": ["src"]},
    {"version": 3, "route": {"model": "deepseek-v4-pro", "effort": "off"}, "scopes": ["src"]},
    {"version": 3, "route": {"model": "deepseek-v4-pro", "effort": "high"}, "scopes": ["../src"]},
    {"version": 3, "route": {"model": "deepseek-v4-pro", "effort": "high"}, "scopes": ["src", "src"]},
    {"version": 2, "model": "deepseek-v4-pro", "reasoning": "high", "paths": ["src"], "extra": 1},
):
    try:
        decode_policy(invalid)
    except (TypeError, ValueError, KeyError):
        pass
    else:
        raise SystemExit(1)
