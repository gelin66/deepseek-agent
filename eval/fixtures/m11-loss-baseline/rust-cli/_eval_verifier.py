#!/usr/bin/python3
from pathlib import Path
import os
import subprocess
import sys
import tempfile


def run(
    arguments: list[str], target: str
) -> subprocess.CompletedProcess[str]:
    environment = dict(os.environ)
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_TARGET_DIR"] = target
    return subprocess.run(
        arguments,
        cwd=Path(__file__).parent,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=90,
        check=False,
    )


tests = Path("tests/cli_contract.rs").read_text(encoding="utf-8")
required = {
    "parses_bracketed_ipv6",
    "rejects_empty_host_and_zero_port",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

with tempfile.TemporaryDirectory(prefix="codewhale-m11-rust-cli-") as target:
    checked = run(
        ["cargo", "test", "--locked", "--offline", "--quiet"], target
    )
    if checked.returncode:
        sys.stderr.write(checked.stderr)
        raise SystemExit(1)

    for value, expected in (
        (" [2001:db8::1]:443 ", "2001:db8::1\t443\n"),
        ("localhost:65535", "localhost\t65535\n"),
    ):
        result = run(
            ["cargo", "run", "--locked", "--offline", "--quiet", "--", value],
            target,
        )
        if result.returncode or result.stdout != expected:
            raise SystemExit(1)

    for value in ("host:0", ":8080", "[::1]", "host:65536", "host:80:90"):
        result = run(
            ["cargo", "run", "--locked", "--offline", "--quiet", "--", value],
            target,
        )
        if result.returncode != 2:
            raise SystemExit(1)
