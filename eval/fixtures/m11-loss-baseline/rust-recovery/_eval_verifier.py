#!/usr/bin/python3
from pathlib import Path
import os
import subprocess
import sys
import tempfile


environment = dict(os.environ)
environment["CARGO_INCREMENTAL"] = "0"
environment["CARGO_NET_OFFLINE"] = "true"
with tempfile.TemporaryDirectory(
    prefix="codewhale-m11-rust-recovery-"
) as target:
    environment["CARGO_TARGET_DIR"] = target
    result = subprocess.run(
        ["cargo", "test", "--locked", "--offline", "--quiet"],
        cwd=Path(__file__).parent,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=90,
        check=False,
    )
if result.returncode:
    sys.stderr.write(result.stderr)
    raise SystemExit(1)
