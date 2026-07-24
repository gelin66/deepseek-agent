#!/usr/bin/python3
from pathlib import Path
import os
import subprocess
import sys
import tempfile


root = Path(__file__).parent
tests = (root / "tests/endpoint_contract.rs").read_text(encoding="utf-8")
required = {
    "accepts_http_and_https_with_port",
    "rejects_credentials_query_fragment_and_non_http",
}
if not all(name in tests for name in required):
    raise SystemExit(1)

with tempfile.TemporaryDirectory(prefix="codewhale-m12-rust-endpoint-") as target:
    environment = dict(os.environ)
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_TARGET_DIR"] = target
    result = subprocess.run(
        ["cargo", "test", "--locked", "--offline", "--quiet"],
        cwd=root,
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
