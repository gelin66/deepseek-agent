#!/usr/bin/python3
import os
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix="dse-m39-rust-target-") as target:
    env = dict(os.environ)
    env["CARGO_INCREMENTAL"] = "0"
    env["CARGO_NET_OFFLINE"] = "true"
    env["CARGO_TARGET_DIR"] = target
    result = subprocess.run(
        ["cargo", "test", "--locked", "--offline", "--quiet"],
        cwd=root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
raise SystemExit(result.returncode)
