#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m20b-request-budget-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "test" / "hidden.test.ts"
    hidden.write_text(
        """
import assert from "node:assert/strict";
import test from "node:test";
import { parseRequestBudget } from "../src/request_budget.ts";

test("hidden exact grammar and boundaries", () => {
  assert.deepEqual(parseRequestBudget("requests=1;seconds=1"), { requests: 1, seconds: 1 });
  assert.deepEqual(parseRequestBudget("requests=32;seconds=600"), { requests: 32, seconds: 600 });
  assert.equal(parseRequestBudget(" requests=8;seconds=120"), null);
  assert.equal(parseRequestBudget("requests=8;seconds=120 "), null);
  assert.equal(parseRequestBudget("requests=+8;seconds=120"), null);
  assert.equal(parseRequestBudget("requests=8;seconds=1.2"), null);
  assert.equal(parseRequestBudget("requests=８;seconds=120"), null);
  assert.equal(parseRequestBudget(undefined), null);
});
""".lstrip(),
        encoding="utf-8",
    )
    tests = [
        str(path.relative_to(target))
        for path in sorted((target / "test").glob("*.test.ts"))
    ]
    result = subprocess.run(
        ["node", "--experimental-strip-types", "--test", *tests],
        cwd=target,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)
