#!/usr/bin/python3
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parent
result = subprocess.run(
    ["node", "--experimental-strip-types", "test/frames.test.ts"],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
contract = subprocess.run(
    ["node", "--experimental-strip-types", "test/hidden.test.ts"],
    cwd=root,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
)
raise SystemExit(0 if result.returncode == 0 and contract.returncode == 0 else 1)
