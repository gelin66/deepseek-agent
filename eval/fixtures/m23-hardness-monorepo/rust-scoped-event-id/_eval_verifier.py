#!/usr/bin/python3
from pathlib import Path
import os
import subprocess
import sys
import tempfile


root = Path(__file__).parent
tests = (root / "tests/event_id.rs").read_text(encoding="utf-8")
required = {
    "normalizes_dse_event_ids",
    "rejects_invalid_event_id_boundaries",
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


with tempfile.TemporaryDirectory(prefix="dse-m18-event-id-") as target:
    checked = run(["cargo", "test", "--locked", "--offline", "--quiet"], target)
    if checked.returncode:
        sys.stderr.write(checked.stderr)
        raise SystemExit(1)
    hidden = run(
        ["cargo", "run", "--locked", "--offline", "--quiet", "--bin", "_eval_probe"],
        target,
    )
    expected = [
        'Ok("dse.run.started")',
        'Ok("dse.tool.finished")',
        'Ok("dse.run.42")',
        'Err("invalid event id")',
        'Err("invalid event id")',
        'Err("invalid event id")',
        'Err("invalid event id")',
        'Err("invalid event id")',
        'Err("invalid event id")',
        'Err("invalid event id")',
        'Err("invalid event id")',
    ]
    if hidden.returncode or hidden.stdout.splitlines() != expected:
        raise SystemExit(1)
