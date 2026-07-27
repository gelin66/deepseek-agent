#!/usr/bin/python3
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


workspace = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="dse-m36a2-route-") as raw:
    target = Path(raw) / "workspace"
    shutil.copytree(workspace, target)
    hidden = target / "test" / "hidden.test.ts"
    hidden.write_text(
        r'''
import assert from "node:assert/strict";
import test from "node:test";
import { decodeRoute } from "../src/codec.ts";
import { encodeRoute } from "../src/route.ts";

test("canonical v2 and exact v1 read", () => {
  const input = ["tests", "src", "src"];
  const encoded = encodeRoute("deepseek-v4-pro", "high", input);
  assert.deepEqual(encoded, {
    version: 2,
    route: {model: "deepseek-v4-pro", effort: "high"},
    scopes: ["src", "tests"],
  });
  encoded.scopes.push("docs");
  assert.deepEqual(input, ["tests", "src", "src"]);
  assert.deepEqual(decodeRoute({
    version: 1,
    model: "deepseek-v4-flash",
    reasoning: "high",
    paths: ["src"],
  }), {model: "deepseek-v4-flash", effort: "high", scopes: ["src"]});
  assert.deepEqual(decodeRoute({
    version: 2,
    route: {model: "deepseek-v4-pro", effort: "max"},
    scopes: ["src", "tests"],
  }), {model: "deepseek-v4-pro", effort: "max", scopes: ["src", "tests"]});
});

test("rejects noncanonical records", () => {
  const invalid = [
    null,
    {version: true, model: "deepseek-v4-pro", reasoning: "high", paths: ["src"]},
    {version: 1, model: "deepseek-v4-pro", reasoning: "high", paths: ["src"], extra: 1},
    {version: 2, route: {model: "deepseek-v4-pro", effort: "high"}, scopes: ["tests", "src"]},
    {version: 2, route: {model: "deepseek-v4-pro", effort: "high"}, scopes: ["src", "src"]},
    {version: 2, route: {model: "other", effort: "high"}, scopes: ["src"]},
    {version: 2, route: {model: "deepseek-v4-pro", effort: "off"}, scopes: ["src"]},
    {version: 2, route: {model: "deepseek-v4-pro", effort: "high"}, scopes: ["../src"]},
    {version: 2, route: {model: "deepseek-v4-pro", effort: "high"}, scopes: ["/src"]},
    {version: 3, route: {}, scopes: []},
  ];
  for (const value of invalid) assert.throws(() => decodeRoute(value));
  assert.throws(() => encodeRoute("other", "high", ["src"]));
});
'''.lstrip(),
        encoding="utf-8",
    )
    tests = [str(path.relative_to(target)) for path in sorted((target / "test").glob("*.test.ts"))]
    result = subprocess.run(
        ["node", "--experimental-strip-types", "--test", *tests],
        cwd=target,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=60,
        check=False,
    )
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)
