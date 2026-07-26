#!/usr/bin/python3
from pathlib import Path
import subprocess
import sys


workspace = Path(sys.argv[1]).resolve()
probe = (
    "from authorize import can_export; "
    "assert can_export('tenant-a', 'tenant-a'); "
    "assert not can_export('tenant-a', 'tenant-b')"
)
result = subprocess.run(
    ["/usr/bin/python3", "-I", "-B", "-c", probe],
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
