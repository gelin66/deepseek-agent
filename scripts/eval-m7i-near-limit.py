#!/usr/bin/env python3
"""Freeze the M7-I current-revision near-limit context/request baseline.

Rust tests own every semantic assertion. They execute the canonical Runtime,
ContextBroker, RunStore, DeepSeek planner, production loopback and crash
paths. This harness only validates the immutable manifest, invokes exact test
filters, and records process verdicts without reading a credential or
implementing a second estimator, reducer, planner, or terminal classifier.
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
import time


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "eval/manifests/m7-i-near-limit-context-v1.json"
EXPECTED_SCHEMA = "codewhale.eval.m7-i-near-limit-context.v1"
EXPECTED_MATRIX_IDS = [f"N{index:02d}" for index in range(1, 14)]
TARGET_DIR = "/private/tmp/codewhale-m7i-target"


class HarnessError(RuntimeError):
    pass


def canonical_json(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def load_manifest() -> tuple[dict, str]:
    raw = MANIFEST.read_bytes()
    manifest = json.loads(raw)
    if manifest.get("schema") != EXPECTED_SCHEMA:
        raise HarnessError("manifest_schema_mismatch")
    matrix = manifest.get("matrix")
    if not isinstance(matrix, list):
        raise HarnessError("matrix_missing")
    if [case.get("id") for case in matrix] != EXPECTED_MATRIX_IDS:
        raise HarnessError("matrix_ids_must_be_exact_N01_through_N13")
    if {case.get("lane") for case in matrix} != {
        "root",
        "read_only_child",
        "explicit_writer",
    }:
        raise HarnessError("matrix_must_cover_root_read_only_child_and_writer")
    execution = manifest.get("execution", {})
    if execution.get("maximum_reruns") != 0:
        raise HarnessError("maximum_reruns_must_be_zero")
    if execution.get("cargo_target_dir") != TARGET_DIR:
        raise HarnessError("cargo_target_dir_mismatch")
    if execution.get("result_mode") != "0600":
        raise HarnessError("result_mode_must_be_0600")
    admission = manifest.get("credential_admission", {})
    if admission.get("status") != "blocked_no_material_production_delta":
        raise HarnessError("credential_admission_must_be_blocked")
    if admission.get("read_key_before_admission") is not False:
        raise HarnessError("harness_must_not_read_key")
    if admission.get("official_api_requests_allowed") is not False:
        raise HarnessError("official_api_requests_must_be_disabled")
    gates = manifest.get("gates")
    if not isinstance(gates, list) or not gates:
        raise HarnessError("gates_missing")
    gate_ids = [gate.get("id") for gate in gates]
    if len(gate_ids) != len(set(gate_ids)):
        raise HarnessError("gate_ids_must_be_unique")
    for gate in gates:
        if not all(gate.get(field) for field in ("id", "package", "filter")):
            raise HarnessError("gate_contract_incomplete")
    referenced = {
        test
        for case in matrix
        for test in case.get("tests", [])
        if isinstance(test, str)
    }
    frozen_filters = {gate["filter"] for gate in gates}
    if not referenced.issubset(frozen_filters):
        raise HarnessError("matrix_test_missing_exact_gate")
    contract = manifest.get("slice_contract", {})
    required_contract = {
        "problem",
        "acceptance",
        "owner",
        "replaces",
        "evidence",
        "cutover_deletion",
    }
    if not required_contract.issubset(contract):
        raise HarnessError("slice_contract_incomplete")
    return manifest, sha256_bytes(canonical_json(manifest))


def git_identity() -> dict:
    def git(*args: str) -> str:
        return subprocess.run(
            ["git", *args],
            cwd=ROOT,
            check=True,
            text=True,
            capture_output=True,
        ).stdout.strip()

    return {
        "branch": git("branch", "--show-current"),
        "revision": git("rev-parse", "HEAD"),
        "tree": git("rev-parse", "HEAD^{tree}"),
        "dirty": bool(git("status", "--porcelain=v1", "--untracked-files=all")),
    }


def run_gate(gate: dict) -> dict:
    environment = os.environ.copy()
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_TARGET_DIR"] = TARGET_DIR
    environment["CARGO_NET_OFFLINE"] = "true"
    command = [
        "cargo",
        "test",
        "-p",
        gate["package"],
        "--locked",
        gate["filter"],
        "--",
        "--nocapture",
    ]
    started = time.monotonic()
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    result = {
        "id": gate["id"],
        "package": gate["package"],
        "filter": gate["filter"],
        "exit_code": completed.returncode,
        "wall_time_ms": round((time.monotonic() - started) * 1000),
    }
    if completed.returncode != 0:
        result["diagnostic_tail"] = completed.stdout[-65_536:]
    return result


def write_private_json(path: Path, value: dict) -> None:
    path = path.resolve()
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.exists() or path.is_symlink():
        raise HarnessError("output_must_not_exist")
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(descriptor, stat.S_IRUSR | stat.S_IWUSR)
        with os.fdopen(descriptor, "wb", closefd=True) as handle:
            handle.write(
                json.dumps(
                    value,
                    ensure_ascii=False,
                    sort_keys=True,
                    separators=(",", ":"),
                ).encode("utf-8")
            )
            handle.write(b"\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        mode = stat.S_IMODE(path.stat().st_mode)
        if mode != 0o600:
            raise HarnessError(f"output_mode_mismatch:{mode:04o}")
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def self_test() -> int:
    manifest, digest = load_manifest()
    assert manifest["production_revision"].startswith("6e9e9b7b")
    assert manifest["matrix"][4]["boundary"] == "mandatory_facts_over_limit"
    assert manifest["matrix"][10]["lane"] == "read_only_child"
    assert manifest["matrix"][11]["lane"] == "explicit_writer"
    with tempfile.TemporaryDirectory(prefix="codewhale-m7i-self-test-") as directory:
        output = Path(directory) / "result.json"
        value = {"schema": EXPECTED_SCHEMA, "manifest_sha256": digest}
        write_private_json(output, value)
        assert stat.S_IMODE(output.stat().st_mode) == 0o600
        assert json.loads(output.read_text(encoding="utf-8")) == value
        try:
            write_private_json(output, value)
        except HarnessError as error:
            assert str(error) == "output_must_not_exist"
        else:
            raise AssertionError("existing output must fail closed")
    print(
        json.dumps(
            {
                "status": "pass",
                "manifest_sha256": digest,
                "matrix_cases": len(manifest["matrix"]),
                "gates": len(manifest["gates"]),
                "credential_read": False,
                "network_accessed": False,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def run(output: Path | None) -> int:
    manifest, digest = load_manifest()
    before = git_identity()
    gates = [run_gate(gate) for gate in manifest["gates"]]
    after = git_identity()
    passed = (
        before == after
        and not before["dirty"]
        and before["branch"] == manifest["source_identity"]["branch"]
        and all(gate["exit_code"] == 0 for gate in gates)
    )
    result = {
        "schema": "codewhale.eval.m7-i-near-limit-context-result.v1",
        "suite_id": manifest["suite_id"],
        "manifest_sha256": digest,
        "production_revision": manifest["production_revision"],
        "production_tree": manifest["production_tree"],
        "source_before": before,
        "source_after": after,
        "maximum_reruns": 0,
        "matrix_cases": len(manifest["matrix"]),
        "gates": gates,
        "credential_read": False,
        "official_api_requests": 0,
        "network_accessed": False,
        "material_production_delta": False,
        "product_metric_eligible": False,
        "decision": (
            "close_request_token_optimization_and_admit_m8_cleanup"
            if passed
            else "offline_gate_failed"
        ),
        "passed": passed,
    }
    if output is not None:
        write_private_json(output, result)
    print(
        json.dumps(
            {
                "status": "passed" if passed else "failed",
                "suite_id": result["suite_id"],
                "matrix_cases": result["matrix_cases"],
                "gates": len(gates),
                "failed_gates": [
                    gate["id"] for gate in gates if gate["exit_code"] != 0
                ],
                "credential_read": False,
                "official_api_requests": 0,
                "output": str(output.resolve()) if output is not None else None,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0 if passed else 1


def main() -> int:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("self-test")
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    try:
        if arguments.command == "self-test":
            return self_test()
        return run(arguments.output)
    except (
        HarnessError,
        OSError,
        subprocess.SubprocessError,
        json.JSONDecodeError,
    ) as error:
        print(f"m7i_harness_error:{error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
