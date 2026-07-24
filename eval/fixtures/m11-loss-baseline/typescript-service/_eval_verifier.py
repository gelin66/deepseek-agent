#!/usr/bin/python3
from pathlib import Path
import json
import subprocess
import sys


root = Path(__file__).parent
tests = (root / "test/service.test.ts").read_text(encoding="utf-8")
required = {
    "ignores query and fragment",
    "decodes route parameters safely",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

checked = subprocess.run(
    [
        "node",
        "--experimental-strip-types",
        "--test",
        "test/service.test.ts",
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
import { matchRoute } from "./src/router.ts";
const cases = [
  ["/users/:id", "/users/a%20b?debug=1#x", {params:{id:"a b"}}],
  ["/users/:id", "/users/%E0%A4%A", null],
  ["/users/:id", "/teams/alice", null],
  ["/", "/?ok=1", {params:{}}],
];
console.log(JSON.stringify(cases.map(([p,u]) => matchRoute(p,u))));
"""
hidden = subprocess.run(
    ["node", "--experimental-strip-types", "--input-type=module", "--eval", program],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if hidden.returncode:
    raise SystemExit(1)
actual = json.loads(hidden.stdout)
expected = [
    {"params": {"id": "a b"}},
    None,
    None,
    {"params": {}},
]
if actual != expected:
    raise SystemExit(1)
