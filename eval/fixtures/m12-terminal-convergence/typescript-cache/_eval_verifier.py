#!/usr/bin/python3
from pathlib import Path
import json
import subprocess
import sys


root = Path(__file__).parent
tests = (root / "test/cache_policy.test.ts").read_text(encoding="utf-8")
required = {
    "prefers shared max-age and ignores case",
    "rejects invalid and negative lifetimes",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

checked = subprocess.run(
    [
        "node",
        "--experimental-strip-types",
        "--test",
        "test/cache_policy.test.ts",
    ],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=60,
    check=False,
)
if checked.returncode:
    sys.stderr.write(checked.stderr)
    raise SystemExit(1)

program = """
import { parseCacheControl } from "./src/cache_policy.ts";
const values = [
  "PUBLIC, S-MAXAGE=120, MAX-AGE=60",
  "no-cache, max-age=0",
  "max-age=-1",
  "private, max-age=abc",
  "public",
];
console.log(JSON.stringify(values.map(parseCacheControl)));
"""
hidden = subprocess.run(
    [
        "node",
        "--experimental-strip-types",
        "--input-type=module",
        "--eval",
        program,
    ],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if hidden.returncode:
    sys.stderr.write(hidden.stderr)
    raise SystemExit(1)
actual = json.loads(hidden.stdout)
expected = [
    {"cacheable": True, "ttlSeconds": 120},
    {"cacheable": False, "ttlSeconds": None},
    {"cacheable": False, "ttlSeconds": None},
    {"cacheable": False, "ttlSeconds": None},
    {"cacheable": True, "ttlSeconds": None},
]
if actual != expected:
    raise SystemExit(1)
