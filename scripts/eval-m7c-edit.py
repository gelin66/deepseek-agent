#!/usr/bin/env python3
"""Frozen M7-C offline edit baseline runner.

This harness never implements an editor or infers failures from prose. Rust
tests exercise production owners and assert canonical ToolOutcome/RunStore
facts; this file only freezes identity, invokes exact filters, and records
their process verdicts.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "eval/manifests/m7-c-edit-baseline-v1.json"
EXPECTED_SCHEMA = "codewhale.eval.m7-c-edit-baseline.v1"
EXPECTED_TASKS = [f"E{index:02d}" for index in range(1, 13)]
TARGET_DIR = "/private/tmp/codewhale-m7c-target"


class HarnessError(RuntimeError):
    pass


def canonical_json(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def load_manifest() -> tuple[dict, str]:
    raw = MANIFEST.read_bytes()
    value = json.loads(raw)
    if value.get("schema") != EXPECTED_SCHEMA:
        raise HarnessError("manifest_schema_mismatch")
    tasks = value.get("tasks")
    if not isinstance(tasks, list) or [task.get("id") for task in tasks] != EXPECTED_TASKS:
        raise HarnessError("manifest_tasks_must_be_exact_E01_through_E12")
    if len({task.get("mechanism_test") for task in tasks}) != 12:
        raise HarnessError("manifest_mechanism_tests_must_be_unique")
    execution = value.get("execution", {})
    if execution.get("maximum_reruns") != 0:
        raise HarnessError("maximum_reruns_must_be_zero")
    if execution.get("cargo_target_dir") != TARGET_DIR:
        raise HarnessError("cargo_target_dir_mismatch")
    admission = value.get("credential_admission", {})
    if admission.get("status") != "blocked_no_fim_surface_delta":
        raise HarnessError("credential_admission_must_remain_blocked_for_baseline")
    if admission.get("read_key_before_admission") is not False:
        raise HarnessError("baseline_must_not_read_key")
    contract = value.get("slice_contract", {})
    required_contract = {"problem", "acceptance", "owner", "replaces", "evidence", "cutover_deletion"}
    if not required_contract.issubset(contract):
        raise HarnessError("slice_contract_incomplete")
    return value, sha256_bytes(canonical_json(value))


def git_identity() -> dict:
    def git(*args: str) -> str:
        return subprocess.run(
            ["git", *args], cwd=ROOT, check=True, text=True, capture_output=True
        ).stdout.strip()

    return {
        "revision": git("rev-parse", "HEAD"),
        "tree": git("rev-parse", "HEAD^{tree}"),
        "dirty": bool(git("status", "--porcelain=v1", "--untracked-files=all")),
    }


def run_gate(package: str, test_filter: str) -> dict:
    env = os.environ.copy()
    env["CARGO_INCREMENTAL"] = "0"
    env["CARGO_TARGET_DIR"] = TARGET_DIR
    command = ["cargo", "test", "-p", package, "--locked", test_filter, "--", "--nocapture"]
    completed = subprocess.run(command, cwd=ROOT, env=env, text=True)
    return {"package": package, "filter": test_filter, "exit_code": completed.returncode}


def write_private_json(path: Path, value: dict) -> None:
    path = path.resolve()
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.exists() or path.is_symlink():
        raise HarnessError("output_must_not_exist")
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(fd, stat.S_IRUSR | stat.S_IWUSR)
        with os.fdopen(fd, "wb", closefd=True) as handle:
            handle.write(json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2).encode())
            handle.write(b"\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def self_test() -> int:
    manifest, digest = load_manifest()
    assert manifest["historical_directional_evidence"]["patch_calls"] == 158
    assert manifest["tasks"][10]["lane"] == "read_only_child"
    assert manifest["tasks"][11]["lane"] == "explicit_writer"
    print(json.dumps({"status": "pass", "manifest_sha256": digest}, sort_keys=True))
    return 0


def run(output: Path | None) -> int:
    manifest, digest = load_manifest()
    before = git_identity()
    gates = [
        run_gate("codewhale-tools", manifest["tests"]["tools_filter"]),
        run_gate("codewhale-app", manifest["tests"]["app_filter"]),
    ]
    after = git_identity()
    result = {
        "schema": "codewhale.eval.m7-c-edit-baseline-result.v1",
        "suite_id": manifest["suite_id"],
        "manifest_sha256": digest,
        "source_before": before,
        "source_after": after,
        "maximum_reruns": 0,
        "credential_read": False,
        "api_requests": 0,
        "gates": gates,
        "passed": before == after and not before["dirty"] and all(gate["exit_code"] == 0 for gate in gates),
    }
    if output is not None:
        write_private_json(output, result)
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0 if result["passed"] else 1


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("self-test")
    run_parser = subparsers.add_parser("run")
    run_parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        return self_test() if args.command == "self-test" else run(args.output)
    except (HarnessError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(f"m7c_harness_error:{error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
