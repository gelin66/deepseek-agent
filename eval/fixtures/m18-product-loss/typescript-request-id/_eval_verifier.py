#!/usr/bin/python3
from pathlib import Path
import json
import subprocess
import sys


root = Path(__file__).parent
tests = (root / "test/dispatch.test.ts").read_text(encoding="utf-8")
required = {
    "normalizes bounded request ids",
    "rejects malformed segments and missing headers",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

checked = subprocess.run(
    ["node", "--experimental-strip-types", "--test", "test/dispatch.test.ts"],
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

program = r'''
import { dispatchRequest } from "./src/http/dispatch.ts";
const cases = [
  {"x-dse-request-id": "  RUN-42  "},
  {"x-dse-request-id": "release-2026-a"},
  {"x-dse-request-id": "-run"},
  {"x-dse-request-id": "run--42"},
  {"x-dse-request-id": "run_42"},
  {"x-dse-request-id": "请求-42"},
  {"x-dse-request-id": "a".repeat(65)},
  {},
];
console.log(JSON.stringify(cases.map((headers) => dispatchRequest(headers))));
'''
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
    {"requestId": "run-42"},
    {"requestId": "release-2026-a"},
    None,
    None,
    None,
    None,
    None,
    None,
]
if actual != expected:
    raise SystemExit(1)

for untouched in (
    "src/legacy/request_id.ts",
    "src/experiments/request_id.ts",
    "src/mobile/dispatch.ts",
):
    if not (root / untouched).is_file():
        raise SystemExit(1)
