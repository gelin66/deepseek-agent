#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m19-retry-window-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "test" / "hidden.test.ts"
    hidden.write_text(
        """
import assert from "node:assert/strict";
import test from "node:test";
import { parseRetryWindow } from "../src/retry_window.ts";

test("hidden syntax and integer matrix", () => {
  assert.equal(parseRetryWindow(undefined, 100), null);
  assert.equal(parseRetryWindow("", 100), null);
  assert.equal(parseRetryWindow(" 000 ", 100), null);
  assert.equal(parseRetryWindow("@3700", 100), 3600);
  assert.equal(parseRetryWindow("@3701", 100), null);
  assert.equal(parseRetryWindow("@100.0", 100), null);
  assert.equal(parseRetryWindow("１２", 100), null);
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
