#!/usr/bin/python3
from pathlib import Path
import os
import subprocess
import sys
import tempfile


root = Path(__file__).parent
tests = (root / "tests/framer.rs").read_text(encoding="utf-8")
required = {
    "buffers_split_lines_and_crlf",
    "buffers_split_multibyte_utf8_and_rejects_invalid_complete_lines",
}
if not all(name in tests for name in required):
    raise SystemExit(1)


def run(arguments: list[str], target: str) -> subprocess.CompletedProcess[str]:
    environment = dict(os.environ)
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_TARGET_DIR"] = target
    return subprocess.run(
        arguments,
        cwd=root,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=90,
        check=False,
    )


with tempfile.TemporaryDirectory(prefix="codewhale-m15-framer-") as target:
    checked = run(["cargo", "test", "--locked", "--offline", "--quiet"], target)
    if checked.returncode:
        sys.stderr.write(checked.stderr)
        raise SystemExit(1)
    hidden = run(
        ["cargo", "run", "--locked", "--offline", "--quiet", "--bin", "_eval_probe"],
        target,
    )
    expected = [
        'Ok(["alpha"])',
        'Ok(["beta"])',
        'Ok(["café"])',
        'Ok(["last"])',
        'Err("invalid utf-8")',
    ]
    if hidden.returncode or hidden.stdout.splitlines() != expected:
        raise SystemExit(1)
