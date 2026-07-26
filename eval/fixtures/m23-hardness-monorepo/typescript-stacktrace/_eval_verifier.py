#!/usr/bin/python3
from pathlib import Path
import json
import subprocess
import sys


root = Path(__file__).parent
tests = (root / "test/dispatch.test.ts").read_text(encoding="utf-8")
required = {
    "strips query fragment and trailing slash",
    "decodes parameters and rejects malformed escapes",
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
import { dispatchRoute } from "./src/web/dispatch.ts";
const cases = [
  ["/teams/:team/members/:id", "/teams/core/members/a%20b/?trace=1#top"],
  ["/teams/:team", "/teams/%E0%A4%A"],
  ["/teams/:team", "/projects/core"],
  ["/", "/?ready=1"],
];
console.log(JSON.stringify(cases.map(([pattern, target]) => dispatchRoute(pattern, target))));
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
    {"params": {"team": "core", "id": "a b"}},
    None,
    None,
    {"params": {}},
]
if actual != expected:
    raise SystemExit(1)

for untouched in (
    "src/legacy/route_matcher.ts",
    "src/experiments/route_matcher.ts",
    "src/mobile/dispatch.ts",
):
    if not (root / untouched).is_file():
        raise SystemExit(1)
