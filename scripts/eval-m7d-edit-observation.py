#!/usr/bin/env python3
"""RuntimeEvent v16-native M7-D edit observation readiness gate.

This evaluator only projects facts already owned by AgentRuntime and RunStore.
It has no model transport, credential, editor, retry, or write-execution path.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m7-d-edit-observation-v1.json"
TEST_PATH = ROOT / "scripts/test-eval-m7d-edit-observation.py"
EXPECTED_SCHEMA = "codewhale.eval.m7-d-edit-observation.v1"
OFFLINE_SCHEMA = "codewhale.eval.m7-d-edit-observation-offline.v3"
TARGET_DIR = "/private/tmp/codewhale-m7d-target"
EDIT_TOOLS = {"apply_patch", "edit_file"}
SUCCESS_AXES = ("accepted", "succeeded", "succeeded")
KNOWN_FAILURE_CODES = {
    "malformed_arguments",
    "schema_validation",
    "invocation_rejected",
    "unknown_tool",
    "missing_field",
    "invalid_field",
    "workspace_precondition",
    "stale_read",
    "ambiguous_edit",
    "patch_parse",
    "operation_failed",
    "transport_failed",
    "side_effect_ambiguous",
    "verifier_failed",
}


class ObservationError(RuntimeError):
    def __init__(self, code: str, details: dict[str, Any] | None = None) -> None:
        super().__init__(code)
        self.code = code
        self.details = details or {}


def require(
    condition: bool,
    code: str,
    details: dict[str, Any] | None = None,
) -> None:
    if not condition:
        raise ObservationError(code, details)


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        allow_nan=False,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def digest_bytes(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def digest_file(path: Path) -> str:
    return digest_bytes(path.read_bytes())


def load_json(path: Path, code: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ObservationError(code) from error
    require(isinstance(value, dict), code)
    return value


def load_manifest() -> dict[str, Any]:
    value = load_json(MANIFEST_PATH, "manifest_unavailable")
    require(value.get("schema") == EXPECTED_SCHEMA, "manifest_schema_mismatch")
    source = value.get("source_identity", {})
    require(
        source.get("runtime_event") == 16
        and source.get("run_api") == 10
        and source.get("state_schema") == 21
        and source.get("exec_stream") == 2,
        "manifest_protocol_identity_invalid",
    )
    observation = value.get("observation_contract", {})
    require(
        observation.get("lifecycle_identity") == "operation_id"
        and observation.get("product_metric_eligible") is False,
        "manifest_observation_contract_invalid",
    )
    admission = value.get("admission", {})
    require(
        admission
        == {
            "status": "inadmissible_no_treatment_delta",
            "reason": admission.get("reason"),
            "credential_read": False,
            "official_api_requests": 0,
            "release_binary_required": False,
        },
        "manifest_admission_invalid",
    )
    output = value.get("output", {})
    require(
        output.get("offline_result_name")
        == f"m7-d-edit-observation-offline-{source.get('production_revision', '')[:8]}-v2.json"
        and output.get("mode") == "0600"
        and output.get("maximum_reruns") == 0
        and output.get("replace") is False
        and output.get("failure_tail_bytes_per_stream") == 65_536,
        "manifest_output_contract_invalid",
    )
    resources = value.get("resources", {})
    require(
        resources.get("cargo_incremental") == "0"
        and resources.get("cargo_target_dir") == TARGET_DIR
        and resources.get("network") == "forbidden",
        "manifest_resources_invalid",
    )
    buckets = value.get("failure_buckets", {})
    codes = [code for values in buckets.values() for code in values]
    require(
        set(codes) == KNOWN_FAILURE_CODES and len(codes) == len(set(codes)),
        "failure_bucket_contract_invalid",
    )
    gates = value.get("offline_gates")
    require(
        isinstance(gates, list)
        and len(gates) == 14
        and len({gate.get("id") for gate in gates if isinstance(gate, dict)}) == 14
        and all(
            isinstance(gate, dict)
            and isinstance(gate.get("command"), list)
            and gate["command"]
            and all(isinstance(part, str) and part for part in gate["command"])
            for gate in gates
        ),
        "offline_gate_contract_invalid",
    )
    encoded = canonical_bytes(value)
    require(
        b"key.txt" not in encoded
        and b"credential_admission" not in encoded
        and b"--acknowledge-cost" not in encoded,
        "credential_surface_must_not_exist",
    )
    return value


MANIFEST = load_manifest()


def event_kind(stored: dict[str, Any]) -> str | None:
    event = stored.get("event")
    return event.get("kind") if isinstance(event, dict) else None


def nonempty(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def _validate_envelope(events: list[dict[str, Any]]) -> None:
    require(bool(events), "event_ledger_empty")
    require(
        all(isinstance(stored, dict) for stored in events),
        "stored_event_shape_invalid",
    )
    require(
        all(stored.get("schema_version") == 16 for stored in events),
        "runtime_event_schema_mismatch",
    )
    run_ids = {stored.get("run_id") for stored in events}
    require(
        len(run_ids) == 1 and nonempty(next(iter(run_ids))),
        "event_run_identity_mismatch",
    )
    sequences = [stored.get("sequence") for stored in events]
    require(
        all(isinstance(sequence, int) and not isinstance(sequence, bool) for sequence in sequences)
        and sequences == list(range(1, len(events) + 1)),
        "event_sequence_invalid",
    )
    event_ids = [stored.get("event_id") for stored in events]
    require(
        all(nonempty(event_id) for event_id in event_ids)
        and len(event_ids) == len(set(event_ids)),
        "event_id_invalid",
    )
    require(
        all(isinstance(stored.get("event"), dict) for stored in events),
        "runtime_event_shape_invalid",
    )


def _outcome_success(outcome: dict[str, Any]) -> bool:
    return (
        outcome.get("invocation"),
        outcome.get("transport"),
        outcome.get("operation"),
    ) == SUCCESS_AXES


def _validate_outcome(outcome: Any) -> bool:
    require(isinstance(outcome, dict), "tool_outcome_shape_invalid")
    success = _outcome_success(outcome)
    failure_code = outcome.get("failure_code")
    if success:
        require(failure_code is None, "successful_outcome_has_failure_code")
        require(
            outcome.get("retry") == "not_needed",
            "successful_outcome_retry_invalid",
        )
    else:
        require(
            failure_code in KNOWN_FAILURE_CODES,
            "unsuccessful_outcome_missing_failure_code",
        )
    require(
        outcome.get("invocation") in {"accepted", "rejected"}
        and outcome.get("transport")
        in {"not_started", "succeeded", "failed", "indeterminate"}
        and outcome.get("operation")
        in {"not_started", "succeeded", "failed", "cancelled", "indeterminate"}
        and outcome.get("side_effect")
        in {"not_applicable", "not_applied", "applied", "indeterminate"}
        and outcome.get("retry")
        in {"not_needed", "after_correction", "safe", "unsafe", "not_retryable"},
        "tool_outcome_axes_invalid",
    )
    return success


def _target_identity(name: str, invocation: dict[str, Any]) -> str | None:
    arguments = invocation.get("arguments")
    parsed = arguments.get("parsed") if isinstance(arguments, dict) else None
    if not isinstance(parsed, dict):
        return None
    paths: list[str] | None = None
    if name == "edit_file":
        path = parsed.get("path")
        if nonempty(path):
            paths = [path]
    elif name == "apply_patch":
        path = parsed.get("path")
        changes = parsed.get("changes")
        if nonempty(path):
            paths = [path]
        elif isinstance(changes, list) and changes:
            candidates = [
                change.get("path") if isinstance(change, dict) else None
                for change in changes
            ]
            if all(nonempty(candidate) for candidate in candidates):
                paths = sorted(set(candidates))
    return digest_bytes(canonical_bytes(paths)) if paths else None


def _workspace_revision(event: dict[str, Any]) -> dict[str, Any] | None:
    state = event.get("workspace_state")
    revision = state.get("revision") if isinstance(state, dict) else None
    if not isinstance(revision, dict):
        return None
    status = revision.get("status")
    if status == "known" and nonempty(revision.get("sha256")):
        return {"status": "known", "sha256": revision["sha256"]}
    if status == "unknown" and nonempty(revision.get("reason")):
        return {"status": "unknown", "reason": revision["reason"]}
    raise ObservationError("workspace_revision_shape_invalid")


def project_edit_observation(events: list[dict[str, Any]]) -> dict[str, Any]:
    """Project edit failures without emulating Runtime or either editor."""

    _validate_envelope(events)
    prepared: dict[str, tuple[int, dict[str, Any]]] = {}
    started: dict[str, int] = {}
    committed: dict[str, tuple[int, dict[str, Any]]] = {}
    model_request_sequences: list[int] = []

    for stored in events:
        sequence = stored["sequence"]
        event = stored["event"]
        kind = event_kind(stored)
        if kind == "model_request_prepared":
            model_request_sequences.append(sequence)
            continue
        if kind == "tool_prepared":
            operation_id = event.get("operation_id")
            require(nonempty(operation_id), "tool_operation_id_invalid")
            require(operation_id not in prepared, "duplicate_tool_prepared")
            invocation = event.get("invocation")
            require(
                isinstance(invocation, dict)
                and invocation.get("run_id") == stored.get("run_id")
                and nonempty(invocation.get("call_id"))
                and nonempty(invocation.get("name")),
                "tool_prepared_invocation_invalid",
            )
            prepared[operation_id] = (sequence, event)
        elif kind == "tool_execution_started":
            operation_id = event.get("operation_id")
            require(
                nonempty(operation_id) and operation_id in prepared,
                "tool_started_without_prepared",
            )
            require(operation_id not in started, "duplicate_tool_started")
            require(prepared[operation_id][0] < sequence, "tool_lifecycle_order_invalid")
            started[operation_id] = sequence
        elif kind == "tool_outcome_committed":
            operation_id = event.get("operation_id")
            require(
                nonempty(operation_id) and operation_id in prepared,
                "tool_outcome_without_prepared",
            )
            require(operation_id not in committed, "duplicate_tool_outcome")
            prepared_sequence, prepared_event = prepared[operation_id]
            invocation = prepared_event["invocation"]
            require(
                prepared_sequence < sequence
                and event.get("call_id") == invocation.get("call_id")
                and event.get("name") == invocation.get("name"),
                "tool_outcome_identity_mismatch",
            )
            success = _validate_outcome(event.get("outcome"))
            started_sequence = started.get(operation_id)
            if started_sequence is None:
                require(
                    not success
                    and event["outcome"].get("side_effect")
                    in {"not_applied", "not_applicable"},
                    "tool_outcome_missing_started",
                )
            else:
                require(
                    prepared_sequence < started_sequence < sequence,
                    "tool_lifecycle_order_invalid",
                )
                require(
                    event["outcome"].get("invocation") == "accepted"
                    and event["outcome"].get("operation") != "not_started",
                    "started_tool_outcome_invalid",
                )
            committed[operation_id] = (sequence, event)

    attempts: list[dict[str, Any]] = []
    for operation_id, (outcome_sequence, event) in sorted(
        committed.items(),
        key=lambda value: value[1][0],
    ):
        name = event["name"]
        if name not in EDIT_TOOLS:
            continue
        prepared_sequence, prepared_event = prepared[operation_id]
        invocation = prepared_event["invocation"]
        outcome = event["outcome"]
        success = _outcome_success(outcome)
        target_identity = _target_identity(name, invocation)
        attempt = {
            "operation_id": operation_id,
            "call_id": event["call_id"],
            "name": name,
            "prepared_sequence": prepared_sequence,
            "started_sequence": started.get(operation_id),
            "outcome_sequence": outcome_sequence,
            "arguments_sha256": digest_bytes(
                canonical_bytes(invocation.get("arguments"))
            ),
            "target_identity_sha256": target_identity,
            "success": success,
            "failure_code": outcome.get("failure_code"),
            "invocation": outcome.get("invocation"),
            "transport": outcome.get("transport"),
            "operation": outcome.get("operation"),
            "side_effect": outcome.get("side_effect"),
            "retry": outcome.get("retry"),
            "workspace_revision": _workspace_revision(event),
        }
        if not success:
            attempt["recovered"] = None if target_identity is None else False
            attempt["model_requests_to_recovery"] = None
        attempts.append(attempt)

    for index, attempt in enumerate(attempts):
        if attempt["success"] or attempt["target_identity_sha256"] is None:
            continue
        for candidate in attempts[index + 1 :]:
            if (
                candidate["success"]
                and candidate["name"] == attempt["name"]
                and candidate["target_identity_sha256"]
                == attempt["target_identity_sha256"]
            ):
                requests = [
                    sequence
                    for sequence in model_request_sequences
                    if attempt["outcome_sequence"]
                    < sequence
                    < candidate["prepared_sequence"]
                ]
                if requests:
                    attempt["recovered"] = True
                    attempt["model_requests_to_recovery"] = len(requests)
                    attempt["recovery_operation_id"] = candidate["operation_id"]
                    break

    incomplete: list[dict[str, Any]] = []
    for operation_id, started_sequence in sorted(
        started.items(), key=lambda value: value[1]
    ):
        if operation_id in committed:
            continue
        prepared_sequence, prepared_event = prepared[operation_id]
        invocation = prepared_event["invocation"]
        if (
            invocation.get("name") in EDIT_TOOLS
            and prepared_event.get("workspace_access") == "may_write"
        ):
            incomplete.append(
                {
                    "operation_id": operation_id,
                    "name": invocation["name"],
                    "prepared_sequence": prepared_sequence,
                    "started_sequence": started_sequence,
                    "target_identity_sha256": _target_identity(
                        invocation["name"], invocation
                    ),
                }
            )

    failures = [attempt for attempt in attempts if not attempt["success"]]
    code_to_bucket = {
        code: bucket
        for bucket, codes in MANIFEST["failure_buckets"].items()
        for code in codes
    }
    bucket_counts = Counter(
        code_to_bucket[attempt["failure_code"]] for attempt in failures
    )
    committed_ambiguities = sum(
        attempt["side_effect"] == "indeterminate" for attempt in failures
    )
    return {
        "edit_attempts": attempts,
        "attempts": len(attempts),
        "successes": sum(attempt["success"] for attempt in attempts),
        "first_attempt_success": attempts[0]["success"] if attempts else None,
        "failure_codes": dict(
            sorted(Counter(attempt["failure_code"] for attempt in failures).items())
        ),
        "failure_buckets": dict(sorted(bucket_counts.items())),
        "recovered_failures": sum(
            attempt.get("recovered") is True for attempt in failures
        ),
        "unrecovered_failures": sum(
            attempt.get("recovered") is False for attempt in failures
        ),
        "unscorable_recovery_failures": sum(
            attempt.get("recovered") is None for attempt in failures
        ),
        "model_requests_to_recovery": [
            attempt["model_requests_to_recovery"]
            for attempt in failures
            if isinstance(attempt.get("model_requests_to_recovery"), int)
        ],
        "incomplete_write_operations": len(incomplete),
        "incomplete_write_facts": incomplete,
        "indeterminate_committed_side_effects": committed_ambiguities,
        "transaction_ambiguities": committed_ambiguities + len(incomplete),
        "normal_run_crash_frequency_claim_admissible": False,
    }


def decide(records: list[dict[str, Any]]) -> dict[str, Any]:
    """Apply preregistered cross-task admission rules to projected records."""

    bucket_totals: Counter[str] = Counter()
    bucket_tasks: defaultdict[str, set[str]] = defaultdict(set)
    transaction_ambiguities = 0
    for record in records:
        task_id = record.get("task_id")
        observation = record.get("observation")
        require(
            nonempty(task_id) and isinstance(observation, dict),
            "observation_record_invalid",
        )
        for bucket, count in observation.get("failure_buckets", {}).items():
            require(
                bucket in MANIFEST["failure_buckets"]
                and isinstance(count, int)
                and count >= 0,
                "observation_bucket_invalid",
            )
            bucket_totals[bucket] += count
            if count:
                bucket_tasks[bucket].add(task_id)
        value = observation.get("transaction_ambiguities", 0)
        require(
            isinstance(value, int) and value >= 0,
            "transaction_observation_invalid",
        )
        transaction_ambiguities += value

    rules = MANIFEST["decision_rules"]
    failures_needed = rules["minimum_typed_failures_for_mechanism"]
    tasks_needed = rules["minimum_tasks_with_same_bucket"]
    patch = (
        bucket_totals["patch_generation"] >= failures_needed
        and len(bucket_tasks["patch_generation"]) >= tasks_needed
    )
    recovery = (
        bucket_totals["edit_recovery"] >= failures_needed
        and len(bucket_tasks["edit_recovery"]) >= tasks_needed
    )
    if transaction_ambiguities:
        decision = "admit_transaction_investigation"
    elif patch:
        decision = "admit_patch_generation_treatment"
    elif recovery:
        decision = "admit_edit_recovery_treatment"
    else:
        decision = "no_edit_mechanism_admitted"
    return {
        "decision": decision,
        "failure_buckets": dict(sorted(bucket_totals.items())),
        "tasks_per_bucket": {
            bucket: sorted(tasks) for bucket, tasks in sorted(bucket_tasks.items())
        },
        "transaction_ambiguities": transaction_ambiguities,
        "product_metric_eligible": False,
    }


def git(*arguments: str) -> str:
    result = subprocess.run(
        ["git", *arguments],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=30,
        check=False,
    )
    require(result.returncode == 0, "git_identity_unavailable")
    return result.stdout.strip()


def source_identity() -> dict[str, Any]:
    frozen = MANIFEST["source_identity"]
    revision = frozen["production_revision"]
    require(
        git("rev-parse", f"{revision}^{{tree}}") == frozen["production_tree"]
        and digest_file(ROOT / "Cargo.lock") == frozen["cargo_lock_sha256"]
        and digest_file(ROOT / "rust-toolchain.toml")
        == frozen["rust_toolchain_sha256"],
        "production_source_identity_mismatch",
    )
    return {
        "production_revision": revision,
        "production_tree": frozen["production_tree"],
        "run_api": frozen["run_api"],
        "runtime_event": frozen["runtime_event"],
        "state_schema": frozen["state_schema"],
        "exec_stream": frozen["exec_stream"],
    }


def repository_identity() -> dict[str, Any]:
    return {
        "revision": git("rev-parse", "HEAD"),
        "tree": git("rev-parse", "HEAD^{tree}"),
        "dirty": bool(git("status", "--porcelain=v1", "--untracked-files=all")),
    }


def output_path(value: str | Path, allowed_root: Path | None = None) -> Path:
    allowed = (
        allowed_root.resolve()
        if allowed_root is not None
        else (ROOT / MANIFEST["output"]["directory"]).resolve()
    )
    path = Path(value).expanduser().resolve()
    expected = allowed / MANIFEST["output"]["offline_result_name"]
    require(path == expected, "output_path_not_manifest_bound")
    require(not path.is_symlink(), "output_symlink_rejected")
    return path


def write_private_once(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    require(not path.exists() and not path.is_symlink(), "output_already_exists")
    descriptor: int | None = None
    try:
        descriptor = os.open(
            path,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL,
            stat.S_IRUSR | stat.S_IWUSR,
        )
        with os.fdopen(descriptor, "wb", closefd=True) as stream:
            descriptor = None
            stream.write(
                json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2).encode(
                    "utf-8"
                )
            )
            stream.write(b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        require(
            stat.S_IMODE(path.stat().st_mode) == 0o600,
            "output_mode_invalid",
        )
    except BaseException:
        if descriptor is not None:
            os.close(descriptor)
        try:
            path.unlink()
        except FileNotFoundError:
            pass
        raise


def _regression_result() -> dict[str, Any]:
    completed = subprocess.run(
        ["python3", str(TEST_PATH)],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=120,
        check=False,
    )
    require(
        completed.returncode == 0,
        "projector_regression_failed",
        {
            "stdout_sha256": digest_bytes(completed.stdout),
            "stderr_sha256": digest_bytes(completed.stderr),
        },
    )
    return {
        "tests": 15,
        "stdout_sha256": digest_bytes(completed.stdout),
        "stderr_sha256": digest_bytes(completed.stderr),
    }


def self_test(*, run_regression: bool = True) -> dict[str, Any]:
    identity = source_identity()
    regression = _regression_result() if run_regression else {"tests": 15}
    return {
        "status": "pass",
        "manifest_sha256": digest_file(MANIFEST_PATH),
        "harness_sha256": digest_file(Path(__file__).resolve()),
        "test_sha256": digest_file(TEST_PATH),
        "source_identity": identity,
        "regression": regression,
        "credential_read": False,
        "official_api_requests": 0,
    }


def run_gate(identifier: str, command: list[str]) -> dict[str, Any]:
    environment = os.environ.copy()
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_TARGET_DIR"] = TARGET_DIR
    for name in (
        "DEEPSEEK_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "CODEWHALE_DEEPSEEK_API_KEY",
    ):
        environment.pop(name, None)
    started_at = time.monotonic()
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=MANIFEST["resources"]["gate_timeout_seconds"],
        check=False,
    )
    record = {
        "id": identifier,
        "command": command,
        "exit_code": completed.returncode,
        "stdout_sha256": digest_bytes(completed.stdout),
        "stderr_sha256": digest_bytes(completed.stderr),
        "wall_time_ms": int((time.monotonic() - started_at) * 1_000),
    }
    if completed.returncode != 0:
        limit = MANIFEST["output"]["failure_tail_bytes_per_stream"]
        record["stdout_tail"] = completed.stdout[-limit:].decode(
            "utf-8", errors="replace"
        )
        record["stderr_tail"] = completed.stderr[-limit:].decode(
            "utf-8", errors="replace"
        )
    return record


def offline(output: Path) -> dict[str, Any]:
    before = repository_identity()
    require(not before["dirty"], "repository_must_be_clean")
    gates = [
        run_gate(gate["id"], gate["command"]) for gate in MANIFEST["offline_gates"]
    ]
    after = repository_identity()
    check = self_test(run_regression=False)
    record = {
        "schema": OFFLINE_SCHEMA,
        "suite_id": MANIFEST["suite_id"],
        "self_test": check,
        "repository_before": before,
        "repository_after": after,
        "gates": gates,
        "passed": before == after
        and not after["dirty"]
        and all(gate["exit_code"] == 0 for gate in gates),
        "admission": MANIFEST["admission"],
        "credential_read": False,
        "official_api_requests": 0,
    }
    write_private_once(output, record)
    return record


def result_status(result: dict[str, Any]) -> str:
    if isinstance(result.get("passed"), bool):
        return "pass" if result["passed"] else "failed"
    return result.get("status", "pass")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("self-test")
    offline_parser = commands.add_parser("offline")
    offline_parser.add_argument("--output", required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command == "self-test":
            result = self_test()
        else:
            result = offline(output_path(args.output))
        print(
            json.dumps(
                {
                    "schema": result.get("schema", EXPECTED_SCHEMA),
                    "status": result_status(result),
                    "suite_id": result.get("suite_id", MANIFEST["suite_id"]),
                    "credential_read": False,
                    "official_api_requests": 0,
                },
                ensure_ascii=False,
                sort_keys=True,
            )
        )
        return 0 if result.get("passed", True) else 1
    except (ObservationError, OSError, subprocess.SubprocessError) as error:
        print(
            json.dumps(
                {
                    "status": "error",
                    "code": getattr(error, "code", type(error).__name__),
                    "details": getattr(error, "details", {}),
                    "credential_read": False,
                    "official_api_requests": 0,
                },
                ensure_ascii=False,
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
