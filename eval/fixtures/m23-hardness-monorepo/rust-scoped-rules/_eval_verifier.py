#!/usr/bin/python3
from pathlib import Path
import os
import subprocess
import sys
import tempfile


root = Path(__file__).parent
tests = (root / "tests/wire_version.rs").read_text(encoding="utf-8")
required = {
    "normalizes_supported_separators",
    "rejects_empty_segments_and_non_ascii",
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


with tempfile.TemporaryDirectory(prefix="dse-m23b-wire-") as target:
    checked = run(["cargo", "test", "--locked", "--offline", "--quiet"], target)
    if checked.returncode:
        sys.stderr.write(checked.stderr)
        raise SystemExit(1)

    result = run(
        ["cargo", "run", "--locked", "--offline", "--quiet", "--bin", "_eval_probe"],
        target,
    )
    expected = [
        'Ok("v4-pro")',
        'Ok("api-2")',
        'Ok("release-2026")',
        'Err("invalid version")',
        'Err("invalid version")',
        'Err("invalid version")',
        'Err("invalid version")',
        'Err("invalid version")',
        'Err("invalid version")',
    ]
    if result.returncode or result.stdout.splitlines() != expected:
        raise SystemExit(1)
