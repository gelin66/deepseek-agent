#!/usr/bin/python3
from pathlib import Path
import json
import subprocess
import sys


root = Path(__file__).parent
tests = (root / "test_cli.py").read_text(encoding="utf-8")
required = {
    "test_value_may_contain_equals",
    "test_invalid_assignment_exits_two",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

checked = subprocess.run(
    [sys.executable, "-I", "-B", "-m", "unittest", "-q"],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    timeout=30,
    check=False,
)
if checked.returncode:
    sys.stderr.write(checked.stderr)
    raise SystemExit(1)


def run(*arguments: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-I", "-B", "cli.py", *arguments],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
        check=False,
    )


valid = run("--set=z=last", "--set=token=a=b=c", "--set=z=final")
if valid.returncode or json.loads(valid.stdout) != {
    "token": "a=b=c",
    "z": "final",
}:
    raise SystemExit(1)
for arguments in (
    ("--set=missing",),
    ("--set==value",),
    ("--unknown",),
):
    invalid = run(*arguments)
    if invalid.returncode != 2 or invalid.stdout:
        raise SystemExit(1)
