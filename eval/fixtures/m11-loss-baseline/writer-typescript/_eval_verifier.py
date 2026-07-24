#!/usr/bin/python3
from pathlib import Path
import json
import subprocess
import sys


root = Path(__file__).parent
tests = (root / "test/headers.test.ts").read_text(encoding="utf-8")
required = {
    "override wins case-insensitively",
    "rejects newline header values",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

checked = subprocess.run(
    ["node", "--experimental-strip-types", "--test", "test/headers.test.ts"],
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
import { mergeHeaders } from "./src/headers.ts";
const defaults = {"Content-Type":"application/json","X-Trace":"base"};
const overrides = {"content-type":"text/plain","x-new":"yes"};
let newlineRejected = false;
try { mergeHeaders({}, {"x-bad":"ok\\nInjected: yes"}); } catch { newlineRejected = true; }
console.log(JSON.stringify({
  result: mergeHeaders(defaults, overrides),
  defaults,
  overrides,
  newlineRejected,
}));
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
expected = {
    "result": {
        "content-type": "text/plain",
        "x-trace": "base",
        "x-new": "yes",
    },
    "defaults": {
        "Content-Type": "application/json",
        "X-Trace": "base",
    },
    "overrides": {
        "content-type": "text/plain",
        "x-new": "yes",
    },
    "newlineRejected": True,
}
if actual != expected:
    raise SystemExit(1)
