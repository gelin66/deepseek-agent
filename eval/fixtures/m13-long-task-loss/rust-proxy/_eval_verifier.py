#!/usr/bin/python3
import os
import pathlib
import subprocess
import sys
import tempfile


root = pathlib.Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="codewhale-m13-rust-proxy-") as target:
    environment = dict(os.environ)
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_NET_OFFLINE"] = "true"
    environment["CARGO_TARGET_DIR"] = target
    result = subprocess.run(
        ["cargo", "test", "--offline", "--locked"],
        cwd=root,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
required = (
    "parses_headers_with_equals_and_last_value_wins",
    "rejects_invalid_endpoint_and_timeout",
    "rejects_unknown_and_duplicate_endpoint",
)
visible = (root / "tests/proxy_contract.rs").read_text(encoding="utf-8")
sys.exit(0 if result.returncode == 0 and all(name in visible for name in required) else 1)
