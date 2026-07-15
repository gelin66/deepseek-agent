#!/usr/bin/env python3
"""Measure one production Engine turn's full and model-visible tool catalogs.

The ignored Rust test runs the real Engine and production per-turn tool setup
with an injected offline model. Token counts are deterministic byte estimates,
not provider-reported usage.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys


MARKER = "TOOL_CATALOG_METRICS "


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    cmd = [
        "cargo",
        "test",
        "-p",
        "codewhale-tui",
        "--bin",
        "codewhale-tui",
        "--locked",
        "core::engine::tests::print_agent_tool_catalog_metrics",
        "--",
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]
    proc = subprocess.run(
        cmd,
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=False,
    )
    marker_payloads: list[str] = []

    def forward_without_marker(stream: str) -> None:
        for line in stream.splitlines(keepends=True):
            if MARKER in line:
                marker_payloads.append(line.split(MARKER, 1)[1].strip())
            else:
                sys.stderr.write(line)

    forward_without_marker(proc.stdout)
    forward_without_marker(proc.stderr)

    if proc.returncode != 0:
        sys.stderr.write(f"catalog measurement test failed with exit {proc.returncode}\n")
        return proc.returncode

    if len(marker_payloads) != 1:
        sys.stderr.write(
            f"expected exactly one TOOL_CATALOG_METRICS marker, got {len(marker_payloads)}\n"
        )
        return 1

    try:
        metrics = json.loads(marker_payloads[0])
    except json.JSONDecodeError as error:
        sys.stderr.write(f"invalid TOOL_CATALOG_METRICS JSON: {error}\n")
        return 1

    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_root,
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()
    dirty = bool(
        subprocess.run(
            ["git", "status", "--porcelain", "--untracked-files=all"],
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=True,
        ).stdout
    )
    metrics["revision"] = revision
    metrics["dirty"] = dirty
    print(json.dumps(metrics, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
