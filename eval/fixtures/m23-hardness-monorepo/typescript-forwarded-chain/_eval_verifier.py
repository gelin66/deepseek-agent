#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m19-forwarded-chain-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "test" / "hidden.test.ts"
    hidden.write_text(
        """
import assert from "node:assert/strict";
import test from "node:test";
import { parseForwardedChain } from "../src/forwarded_chain.ts";

test("hidden boundary matrix", () => {
  assert.deepEqual(parseForwardedChain("a, z9"), ["a", "z9"]);
  assert.equal(parseForwardedChain("a.,origin"), null);
  assert.equal(parseForwardedChain("a..b,origin"), null);
  assert.equal(parseForwardedChain("a b,origin"), null);
  assert.equal(parseForwardedChain(""), null);
  assert.equal(parseForwardedChain(undefined), null);
  assert.equal(parseForwardedChain("a".repeat(64)), null);
  assert.deepEqual(parseForwardedChain("a".repeat(63)), ["a".repeat(63)]);
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
