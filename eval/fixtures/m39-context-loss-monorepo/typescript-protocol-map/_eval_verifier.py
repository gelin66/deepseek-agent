#!/usr/bin/python3
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parent
result = subprocess.run(
    ["node", "--experimental-strip-types", "test/protocol.test.ts"],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
raise SystemExit(result.returncode)
