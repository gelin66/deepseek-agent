#!/usr/bin/python3
import os
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix="dse-m40-feature-target-") as target:
    env = dict(os.environ)
    env["CARGO_INCREMENTAL"] = "0"
    env["CARGO_NET_OFFLINE"] = "true"
    env["CARGO_TARGET_DIR"] = target
    default = subprocess.run(
        ["cargo", "test", "--workspace", "--locked", "--offline", "--quiet"],
        cwd=root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    minimal = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "feature-core",
            "--no-default-features",
            "--locked",
            "--offline",
            "--quiet",
            "--lib",
        ],
        cwd=root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    cli = subprocess.run(
        ["cargo", "run", "-p", "feature-cli", "--locked", "--offline", "--quiet", "--", "reef"],
        cwd=root,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
ok = (
    default.returncode == 0
    and minimal.returncode == 0
    and cli.returncode == 0
    and cli.stdout == b"json:reef\n"
)
raise SystemExit(0 if ok else 1)
