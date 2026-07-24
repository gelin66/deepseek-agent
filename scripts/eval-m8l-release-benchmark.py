#!/usr/bin/env python3
"""Run the M8-L release-benchmark successor without credentials or network.

The Harness owns no model loop, tool implementation, verifier, reducer or
accounting projection. It audits the immutable imported source, validates one
qualified historical real-DeepSeek result, and invokes exact current
production tests named by the frozen manifest.
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
MANIFEST = ROOT / "eval/manifests/m8-l-release-benchmark-successor-v1.json"
EXPECTED_SCHEMA = "codewhale.eval.m8-l-release-benchmark-successor.v1"
IMPORTED_REVISION = "352e86a611fdf3cd8bd27c36d24d482c06a71117"
TARGET_DIR = "/private/tmp/codewhale-m8l-target"


class HarnessError(RuntimeError):
    pass


def canonical_json(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=ROOT,
        check=True,
        text=True,
        capture_output=True,
    ).stdout.strip()


def git_show(revision: str, path: str) -> str:
    return git("show", f"{revision}:{path}")


def require(condition: bool, code: str) -> None:
    if not condition:
        raise HarnessError(code)


def load_manifest() -> tuple[dict, str]:
    raw = MANIFEST.read_bytes()
    manifest = json.loads(raw)
    require(manifest.get("schema") == EXPECTED_SCHEMA, "manifest_schema_mismatch")
    require(
        manifest.get("baseline", {}).get("revision")
        == "4fef6a34e999a8d592629308cb1b5fd7017bbf18",
        "baseline_revision_mismatch",
    )
    require(
        manifest.get("imported_audit", {}).get("revision") == IMPORTED_REVISION,
        "imported_revision_mismatch",
    )
    execution = manifest.get("execution", {})
    require(execution.get("maximum_reruns") == 0, "maximum_reruns_must_be_zero")
    require(
        execution.get("cargo_target_dir") == TARGET_DIR,
        "cargo_target_dir_mismatch",
    )
    require(execution.get("result_mode") == "0600", "result_mode_must_be_0600")
    for field in (
        "credential_read",
        "official_api_requests_allowed",
        "external_network_allowed",
    ):
        require(execution.get(field) is False, f"{field}_must_be_false")

    gates = manifest.get("current_release_benchmark", {}).get("gates")
    require(isinstance(gates, list) and len(gates) == 5, "five_gates_required")
    gate_ids = [gate.get("id") for gate in gates]
    require(len(gate_ids) == len(set(gate_ids)), "gate_ids_must_be_unique")
    cases = manifest.get("current_release_benchmark", {}).get("cases")
    require(isinstance(cases, list) and len(cases) == 5, "five_cases_required")
    require(
        {case.get("gate_id") for case in cases} == set(gate_ids),
        "case_gate_mapping_mismatch",
    )

    workflows = manifest.get("workflow_step_comparison", {}).get("workflows")
    require(
        isinstance(workflows, list)
        and [workflow.get("id") for workflow in workflows]
        == ["W01", "W02", "W03", "W04", "W05"],
        "workflow_ids_mismatch",
    )
    require(
        all(
            isinstance(workflow.get("imported_actions"), int)
            and isinstance(workflow.get("current_actions"), int)
            and workflow["current_actions"] <= workflow["imported_actions"]
            for workflow in workflows
        ),
        "workflow_step_regression",
    )
    require(
        manifest.get("successor_decision_contract", {}).get("V15")
        == "remains blocked",
        "V15_must_remain_blocked",
    )
    return manifest, sha256_bytes(canonical_json(manifest))


def audit_imported_source(manifest: dict) -> dict:
    audit = manifest["imported_audit"]
    require(git("rev-parse", f"{IMPORTED_REVISION}^{{tree}}") == audit["tree"], "imported_tree_mismatch")
    for name, blob in audit["source_blobs"].items():
        paths = {
            "tui_main": "crates/tui/src/main.rs",
            "turn_loop": "crates/tui/src/core/engine/turn_loop.rs",
            "events": "crates/tui/src/core/events.rs",
            "session": "crates/tui/src/core/session.rs",
            "cli": "crates/cli/src/lib.rs",
            "app_server": "crates/app-server/src/lib.rs",
        }
        require(
            git("rev-parse", f"{IMPORTED_REVISION}:{paths[name]}") == blob,
            f"imported_blob_mismatch:{name}",
        )

    main = git_show(IMPORTED_REVISION, "crates/tui/src/main.rs")
    turn_loop = git_show(
        IMPORTED_REVISION, "crates/tui/src/core/engine/turn_loop.rs"
    )
    events = git_show(IMPORTED_REVISION, "crates/tui/src/core/events.rs")
    session = git_show(IMPORTED_REVISION, "crates/tui/src/core/session.rs")
    cli = git_show(IMPORTED_REVISION, "crates/cli/src/lib.rs")
    app_server = git_show(IMPORTED_REVISION, "crates/app-server/src/lib.rs")

    retry_none_count = main.count("retry_count: None")
    stream_send_count = turn_loop.count(
        "client.create_message_stream(stream_request.clone())"
    )
    require("retry_count: Option<u32>" in main, "imported_retry_field_missing")
    require(retry_none_count >= 3, "imported_retry_is_not_always_unknown")
    require("api_request_count" not in main, "imported_request_count_unexpected")
    require(stream_send_count >= 2, "imported_retry_send_paths_missing")
    require("turn.add_usage(&usage)" in turn_loop, "imported_usage_commit_missing")
    require(
        "TurnComplete {" in events
        and "usage: Usage" in events
        and "api_request" not in events,
        "imported_turn_complete_shape_changed",
    )
    require(
        "pub struct SessionUsage" in session
        and "pub fn add(&mut self, usage: &Usage)" in session,
        "imported_session_usage_shape_changed",
    )
    require(
        "Commands::Sessions" in cli
        and "Commands::Exec" in cli
        and "Commands::Resume" in cli,
        "imported_common_workflow_surface_changed",
    )
    require(
        "TaskContract" not in app_server and "EvidenceReceipt" not in app_server,
        "imported_canonical_task_evidence_unexpected",
    )
    return {
        "tree": audit["tree"],
        "retry_none_count": retry_none_count,
        "stream_send_sites": stream_send_count,
        "exec_api_request_count_present": False,
        "turn_complete_physical_ledger_present": False,
        "canonical_task_evidence_present": False,
        "workflow_entries": ["login", "interactive", "exec", "sessions", "resume"],
        "paid_ab_admission": "inadmissible_incomplete_baseline_accounting",
    }


def audit_current_source() -> dict:
    exec_runtime = (ROOT / "crates/tui/src/exec_runtime.rs").read_text(
        encoding="utf-8"
    )
    protocol = (ROOT / "crates/protocol/src/agent_runtime.rs").read_text(
        encoding="utf-8"
    )
    cli = (ROOT / "crates/cli/src/lib.rs").read_text(encoding="utf-8")
    require(
        "api_request_count: Some(u32_saturating(accounting.total_started()))"
        in exec_runtime,
        "current_api_request_count_missing",
    )
    for field in (
        "usage_complete",
        "cost_complete",
        "api_request_root_started",
        "transport_retry_count",
    ):
        require(field in exec_runtime, f"current_terminal_field_missing:{field}")
    require(
        "pub struct ModelAccounting" in protocol
        and "pub root: ActorRequestAccounting" in protocol
        and "pub child: ActorRequestAccounting" in protocol
        and "pub transport_retries: u64" in protocol,
        "current_canonical_accounting_shape_changed",
    )
    require(
        "Runs(RunsArgs)" in cli
        and "Resume(TuiPassthroughArgs)" in cli
        and "Exec(TuiPassthroughArgs)" in cli,
        "current_common_workflow_surface_changed",
    )
    return {
        "exec_stream": 3,
        "api_request_count_present": True,
        "usage_complete_present": True,
        "cost_complete_present": True,
        "root_child_ledger_present": True,
        "transport_retry_count_present": True,
        "workflow_entries": ["login", "interactive", "exec", "runs", "resume"],
    }


def audit_real_coding_evidence(manifest: dict) -> dict:
    evidence = manifest["qualified_real_coding_evidence"]
    summary = ROOT / evidence["summary_path"]
    raw = ROOT / evidence["raw_path"]
    require(sha256_file(summary) == evidence["summary_sha256"], "m5_summary_sha_mismatch")
    require(raw.is_file() and not raw.is_symlink(), "m5_raw_missing")
    require(stat.S_IMODE(raw.stat().st_mode) == 0o600, "m5_raw_mode_mismatch")
    require(sha256_file(raw) == evidence["raw_sha256"], "m5_raw_sha_mismatch")
    result = json.loads(raw.read_bytes())
    require(
        result.get("schema") == "codewhale.eval.m5-completion-gate.v1",
        "m5_raw_schema_mismatch",
    )
    require(
        result.get("evaluation_id") == evidence["evaluation_id"],
        "m5_evaluation_id_mismatch",
    )
    require(len(result.get("records", [])) == 12, "m5_arm_count_mismatch")
    aggregate = result.get("aggregate", {})
    require(aggregate.get("product_metric_eligible") is True, "m5_not_metric_eligible")
    require(
        aggregate.get("exact_cell_repetitions") is True
        and aggregate.get("pair_first_model_request_equal") is True,
        "m5_identity_contract_invalid",
    )
    cells = {
        (cell["scenario"], cell["variant"]): cell
        for cell in aggregate.get("cells", [])
    }
    require(
        cells[("coding_fix", "baseline")]["verified_success"] == 3
        and cells[("coding_fix", "candidate")]["verified_success"] == 3
        and cells[("coding_fix", "candidate")]["false_success"] == 0,
        "m5_coding_result_mismatch",
    )
    require(
        cells[("forced_false_claim", "baseline")]["false_success"] == 3
        and cells[("forced_false_claim", "candidate")]["correct_rejection"] == 3
        and cells[("forced_false_claim", "candidate")]["false_success"] == 0,
        "m5_false_success_result_mismatch",
    )
    require(
        all(
            record.get("measurement_valid") is True
            and record.get("contract_valid") is True
            and record.get("completion_gate_valid") is True
            and record.get("accounting", {}).get("valid") is True
            for record in result["records"]
        ),
        "m5_arm_validity_mismatch",
    )
    return {
        "evaluation_id": result["evaluation_id"],
        "arms": 12,
        "product_metric_eligible": True,
        "coding_verified": {"baseline": 3, "candidate": 3},
        "candidate_false_success": 0,
        "candidate_correct_rejections": 3,
        "raw_sha256": evidence["raw_sha256"],
        "raw_mode": "0600",
    }


def git_identity() -> dict:
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
    command = ["cargo", "test", "-p", gate["package"], "--locked"]
    if gate.get("test_target"):
        command.extend(["--test", gate["test_target"]])
    command.extend([gate["filter"], "--", "--nocapture"])
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
        "test_target": gate.get("test_target"),
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
            handle.write(canonical_json(value))
            handle.write(b"\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        require(
            stat.S_IMODE(path.stat().st_mode) == 0o600,
            "output_mode_mismatch",
        )
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def workflow_result(manifest: dict) -> dict:
    workflows = manifest["workflow_step_comparison"]["workflows"]
    return {
        "unit": manifest["workflow_step_comparison"]["unit"],
        "cases": [
            {
                "id": workflow["id"],
                "imported_actions": workflow["imported_actions"],
                "current_actions": workflow["current_actions"],
                "non_increased": (
                    workflow["current_actions"] <= workflow["imported_actions"]
                ),
            }
            for workflow in workflows
        ],
        "imported_total": sum(item["imported_actions"] for item in workflows),
        "current_total": sum(item["current_actions"] for item in workflows),
        "all_non_increased": all(
            item["current_actions"] <= item["imported_actions"]
            for item in workflows
        ),
    }


def self_test() -> int:
    manifest, digest = load_manifest()
    imported = audit_imported_source(manifest)
    current = audit_current_source()
    real = audit_real_coding_evidence(manifest)
    workflows = workflow_result(manifest)
    with tempfile.TemporaryDirectory(prefix="codewhale-m8l-self-test-") as directory:
        output = Path(directory) / "result.json"
        value = {"schema": EXPECTED_SCHEMA, "manifest_sha256": digest}
        write_private_json(output, value)
        require(
            json.loads(output.read_text(encoding="utf-8")) == value,
            "private_output_round_trip_failed",
        )
        try:
            write_private_json(output, value)
        except HarnessError as error:
            require(str(error) == "output_must_not_exist", "wrong_overwrite_error")
        else:
            raise HarnessError("existing_output_did_not_fail")
    print(
        json.dumps(
            {
                "status": "pass",
                "manifest_sha256": digest,
                "imported_paid_ab": imported["paid_ab_admission"],
                "current_accounting": current["api_request_count_present"],
                "qualified_real_coding_arms": real["arms"],
                "workflow_cases": len(workflows["cases"]),
                "credential_read": False,
                "network_accessed": False,
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


def run(output: Path) -> int:
    manifest, digest = load_manifest()
    before = git_identity()
    require(not before["dirty"], "formal_run_requires_clean_tree")
    require(before["branch"] == manifest["baseline"]["branch"], "branch_mismatch")
    imported = audit_imported_source(manifest)
    current = audit_current_source()
    real = audit_real_coding_evidence(manifest)
    workflows = workflow_result(manifest)
    gates = [
        run_gate(gate)
        for gate in manifest["current_release_benchmark"]["gates"]
    ]
    after = git_identity()
    passed = (
        before == after
        and all(gate["exit_code"] == 0 for gate in gates)
        and workflows["all_non_increased"]
        and real["product_metric_eligible"]
        and imported["paid_ab_admission"]
        == "inadmissible_incomplete_baseline_accounting"
    )
    result = {
        "schema": "codewhale.eval.m8-l-release-benchmark-successor-result.v1",
        "suite_id": manifest["suite_id"],
        "manifest_sha256": digest,
        "harness_sha256": sha256_file(Path(__file__)),
        "source_before": before,
        "source_after": after,
        "maximum_reruns": 0,
        "imported_audit": imported,
        "current_audit": current,
        "qualified_real_coding_evidence": real,
        "workflow_step_comparison": workflows,
        "gates": gates,
        "credential_read": False,
        "official_api_requests": 0,
        "network_accessed": False,
        "imported_paid_ab_executed": False,
        "decision": (
            "keep_release_benchmark_successor_close_V13_V16"
            if passed
            else "offline_release_benchmark_failed"
        ),
        "projected_v1_matrix": "15 pass / 1 blocked" if passed else "unchanged",
        "release_status": "not_releasable",
        "passed": passed,
    }
    write_private_json(output, result)
    print(
        json.dumps(
            {
                "status": "passed" if passed else "failed",
                "suite_id": result["suite_id"],
                "failed_gates": [
                    gate["id"] for gate in gates if gate["exit_code"] != 0
                ],
                "workflow": (
                    f"{workflows['current_total']}/{workflows['imported_total']}"
                ),
                "credential_read": False,
                "official_api_requests": 0,
                "output": str(output.resolve()),
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
    run_parser.add_argument(
        "--output",
        type=Path,
        default=ROOT
        / "eval/results/m8-l-release-benchmark-4fef6a34-v1.json",
    )
    arguments = parser.parse_args()
    try:
        if arguments.command == "self-test":
            return self_test()
        return run(arguments.output)
    except (
        HarnessError,
        KeyError,
        OSError,
        subprocess.SubprocessError,
        json.JSONDecodeError,
    ) as error:
        print(f"m8l_harness_error:{error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
