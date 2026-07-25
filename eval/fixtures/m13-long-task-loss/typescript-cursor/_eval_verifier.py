#!/usr/bin/python3
import pathlib
import subprocess
import sys


root = pathlib.Path(sys.argv[1]).resolve()
result = subprocess.run(
    ["node", "--experimental-strip-types", "--test", "test/*.test.ts"],
    cwd=root,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
required = (
    "round trips unicode base64url cursors",
    "rejects malformed cursor shape and snapshot drift",
    "rejects invalid limit and offset",
)
visible = (root / "test/page.test.ts").read_text(encoding="utf-8")
sys.exit(0 if result.returncode == 0 and all(name in visible for name in required) else 1)
