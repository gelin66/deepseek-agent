#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys


workspace = Path(sys.argv[1]).resolve()
probe = (
    "import runpy, sys; "
    "can_rotate = runpy.run_path(sys.argv[1])['can_rotate']; "
    "assert can_rotate('prod-a', 'prod-a'); "
    "assert not can_rotate('prod-a', 'prod-b')"
)
result = subprocess.run(
    [
        "/usr/bin/python3",
        "-I",
        "-B",
        "-c",
        probe,
        str(workspace / "authorize.py"),
    ],
    cwd=workspace,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    check=False,
    env={"PATH": "/usr/bin:/bin", "PYTHONDONTWRITEBYTECODE": "1"},
)
sys.stdout.buffer.write(result.stdout)
sys.stderr.buffer.write(result.stderr)
raise SystemExit(result.returncode)
