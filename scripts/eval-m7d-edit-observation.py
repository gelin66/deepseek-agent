#!/usr/bin/env python3
"""Current-production M7-D edit failure observation baseline.

The runner reuses the frozen M7-A task executor and canonical Run API.  It
adds only a projection of RuntimeEvent v16 ToolOutcome.failure_code; it does
not implement, repair, retry, or otherwise emulate an editor.
"""

from __future__ import annotations

import argparse
import copy
from collections import Counter, defaultdict
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "eval/manifests/m7-d-edit-observation-v1.json"
TASK_MANIFEST_PATH = ROOT / "eval/manifests/m7-a2-agent-convergence-ab-v1.json"
EXECUTOR_PATH = ROOT / "scripts/eval-m7-agent-convergence.py"
EXPECTED_SCHEMA = "codewhale.eval.m7-d-edit-observation.v1"
RESULT_SCHEMA = "codewhale.eval.m7-d-edit-observation-result.v1"
OFFLINE_SCHEMA = "codewhale.eval.m7-d-edit-observation-offline.v1"
TASK_IDS = ("t1", "t2", "t3", "t4", "t5")
EDIT_TOOLS = {"apply_patch", "edit_file"}
TARGET_DIR = "/private/tmp/codewhale-m7d-target"
USAGE_FIELDS = (
    "input_tokens",
    "output_tokens",
    "cache_hit_tokens",
    "cache_miss_tokens",
    "cache_write_tokens",
    "reasoning_tokens",
    "reasoning_replay_tokens",
)


class ObservationError(RuntimeError):
    def __init__(self, code: str, details: dict[str, Any] | None = None) -> None:
        super().__init__(code)
        self.code = code
        self.details = details or {}


def require(condition: bool, code: str, details: dict[str, Any] | None = None) -> None:
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
    experiment = value.get("experiment", {})
    require(
        experiment.get("runs_per_task") == 3
        and experiment.get("formal_arms") == 15
        and experiment.get("maximum_reruns") == 0
        and tuple(value.get("task_source", {}).get("task_ids", [])) == TASK_IDS,
        "manifest_schedule_invalid",
    )
    schedule = experiment.get("round_order")
    require(
        isinstance(schedule, list)
        and len(schedule) == 3
        and all(sorted(round_tasks) == sorted(TASK_IDS) for round_tasks in schedule),
        "manifest_round_order_invalid",
    )
    resources = value.get("resources", {})
    require(
        resources.get("cargo_target_dir") == TARGET_DIR
        and resources.get("transport_max_retries_per_request") == 0,
        "manifest_resources_invalid",
    )
    buckets = value.get("failure_buckets", {})
    codes = [code for values in buckets.values() for code in values]
    require(len(codes) == len(set(codes)), "failure_bucket_overlap")
    return value


MANIFEST = load_manifest()


def load_executor() -> Any:
    spec = importlib.util.spec_from_file_location("codewhale_m7d_executor", EXECUTOR_PATH)
    require(spec is not None and spec.loader is not None, "executor_unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


EXECUTOR = load_executor()
ORIGINAL_EVENT_LEDGER_VALID = EXECUTOR.event_ledger_valid
ORIGINAL_VERIFICATION_SUMMARY = EXECUTOR.verification_summary
ORIGINAL_TOOL_SUMMARY = EXECUTOR.tool_summary


def v16_as_v14(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Project the unchanged v14 completion contract inside a v16 envelope."""
    return [{**stored, "schema_version": 14} for stored in events]


def event_ledger_valid_v16(events: list[dict[str, Any]], run: dict[str, Any]) -> bool:
    return ORIGINAL_EVENT_LEDGER_VALID(v16_as_v14(events), run)


def verification_summary_v16(
    task_id: str,
    events: list[dict[str, Any]],
    run: dict[str, Any],
    expected_workspace: str,
) -> dict[str, Any]:
    result = ORIGINAL_VERIFICATION_SUMMARY(
        task_id,
        v16_as_v14(events),
        run,
        expected_workspace,
    )
    result["runtime_event_schema"] = 16
    result["completion_contract_projection"] = "v14_fields_inside_v16_envelope"
    return result


def tool_summary_v16(events: list[dict[str, Any]]) -> dict[str, Any]:
    result = ORIGINAL_TOOL_SUMMARY(events)
    prepared: dict[str, dict[str, Any]] = {}
    model_request_sequences = [
        stored.get("sequence")
        for stored in events
        if EXECUTOR.event_kind(stored) == "model_request_prepared"
        and isinstance(stored.get("sequence"), int)
    ]
    for stored in events:
        if EXECUTOR.event_kind(stored) != "tool_prepared":
            continue
        event = stored.get("event", {})
        invocation = event.get("invocation", {})
        call_id = invocation.get("call_id")
        if isinstance(call_id, str):
            prepared[call_id] = {
                "sequence": stored.get("sequence"),
                "name": invocation.get("name"),
                "arguments_sha256": EXECUTOR.canonical_hash(invocation.get("arguments")),
            }
    attempts: list[dict[str, Any]] = []
    for stored in events:
        if EXECUTOR.event_kind(stored) != "tool_outcome_committed":
            continue
        event = stored.get("event", {})
        name = event.get("name")
        if name not in EDIT_TOOLS:
            continue
        outcome = event.get("outcome", {})
        call_id = event.get("call_id")
        source = prepared.get(call_id, {}) if isinstance(call_id, str) else {}
        revision = outcome.get("workspace_revision")
        success = bool(
            outcome.get("invocation") == "accepted"
            and outcome.get("transport") == "succeeded"
            and outcome.get("operation") == "succeeded"
            and outcome.get("failure_code") is None
        )
        attempts.append(
            {
                "prepared_sequence": source.get("sequence"),
                "outcome_sequence": stored.get("sequence"),
                "name": name,
                "arguments_sha256": source.get("arguments_sha256"),
                "success": success,
                "failure_code": outcome.get("failure_code"),
                "invocation": outcome.get("invocation"),
                "transport": outcome.get("transport"),
                "operation": outcome.get("operation"),
                "side_effect": outcome.get("side_effect"),
                "retry": outcome.get("retry"),
                "workspace_revision_sha256": (
                    digest_bytes(revision.encode("utf-8"))
                    if isinstance(revision, str)
                    else None
                ),
            }
        )
    for index, attempt in enumerate(attempts):
        if attempt["success"]:
            continue
        later_success = next(
            (candidate for candidate in attempts[index + 1 :] if candidate["success"]),
            None,
        )
        attempt["recovered"] = later_success is not None
        if later_success is not None:
            lower = attempt.get("outcome_sequence")
            upper = later_success.get("prepared_sequence")
            attempt["model_requests_to_recovery"] = sum(
                isinstance(lower, int)
                and isinstance(upper, int)
                and lower < sequence < upper
                for sequence in model_request_sequences
            )
    failures = [attempt for attempt in attempts if not attempt["success"]]
    result["edit_attempts"] = attempts
    result["edit_observation"] = {
        "attempts": len(attempts),
        "successes": sum(attempt["success"] for attempt in attempts),
        "first_attempt_success": attempts[0]["success"] if attempts else None,
        "first_success_ordinal": next(
            (index for index, attempt in enumerate(attempts, start=1) if attempt["success"]),
            None,
        ),
        "failure_codes": dict(
            sorted(Counter(attempt.get("failure_code") for attempt in failures).items())
        ),
        "recovered_failures": sum(attempt.get("recovered") is True for attempt in failures),
        "unrecovered_failures": sum(attempt.get("recovered") is False for attempt in failures),
        "model_requests_to_recovery": [
            attempt["model_requests_to_recovery"]
            for attempt in failures
            if isinstance(attempt.get("model_requests_to_recovery"), int)
        ],
    }
    return result


def configure_executor() -> None:
    task_manifest = load_json(TASK_MANIFEST_PATH, "task_manifest_unavailable")
    require(
        digest_file(TASK_MANIFEST_PATH) == MANIFEST["task_source"]["sha256"],
        "task_manifest_identity_mismatch",
    )
    configured = copy.deepcopy(task_manifest)
    source = MANIFEST["source_identity"]
    resources = copy.deepcopy(MANIFEST["resources"])
    resources["reasoning_effort"] = MANIFEST["experiment"]["reasoning_effort"]
    identity = {
        "revision": source["revision"],
        "source_tree": source["source_tree"],
        "version": source["binary_version"],
        "size_bytes": source["binary_size_bytes"],
        "sha256": source["binary_sha256"],
    }
    configured["binary_identities"] = {
        "baseline": copy.deepcopy(identity),
        "candidate": copy.deepcopy(identity),
    }
    configured["protocol_schemas"] = {
        "baseline": {"run_api": 10, "runtime_event": 16, "state": 21},
        "candidate": {"run_api": 10, "runtime_event": 16, "state": 21},
    }
    configured["experiment"]["model"] = MANIFEST["experiment"]["model"]
    configured["experiment"]["round_order"] = MANIFEST["experiment"]["round_order"]
    configured["resources"] = resources
    EXECUTOR.MANIFEST = configured
    EXECUTOR.RESOURCES = resources
    EXECUTOR.MODEL = MANIFEST["experiment"]["model"]
    EXECUTOR.RUN_API_SCHEMA = 10
    EXECUTOR.CANARY.RUN_API = 10
    EXECUTOR.CANARY.EVENT_API = 16
    EXECUTOR.event_ledger_valid = event_ledger_valid_v16
    EXECUTOR.verification_summary = verification_summary_v16
    EXECUTOR.tool_summary = tool_summary_v16


configure_executor()


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
    binary = Path(frozen["binary_path"])
    require(binary.is_file() and os.access(binary, os.X_OK), "binary_unavailable")
    version = subprocess.run(
        [str(binary), "--version"],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=30,
        check=False,
    )
    require(
        version.returncode == 0
        and version.stdout.strip() == frozen["binary_version"]
        and digest_file(binary) == frozen["binary_sha256"]
        and binary.stat().st_size == frozen["binary_size_bytes"]
        and git("rev-parse", f"{frozen['revision']}^{{tree}}") == frozen["source_tree"]
        and digest_file(ROOT / "Cargo.lock") == frozen["cargo_lock_sha256"]
        and digest_file(ROOT / "rust-toolchain.toml") == frozen["rust_toolchain_sha256"],
        "binary_identity_mismatch",
    )
    return {
        "revision": frozen["revision"],
        "source_tree": frozen["source_tree"],
        "binary_sha256": frozen["binary_sha256"],
        "binary_size_bytes": frozen["binary_size_bytes"],
        "binary_version": frozen["binary_version"],
    }


def repository_identity() -> dict[str, Any]:
    return {
        "revision": git("rev-parse", "HEAD"),
        "tree": git("rev-parse", "HEAD^{tree}"),
        "dirty": bool(git("status", "--porcelain=v1", "--untracked-files=all")),
    }


def output_path(value: str) -> Path:
    path = Path(value).expanduser().resolve()
    allowed = (ROOT / "eval/results").resolve()
    require(path.parent == allowed and path.name.endswith(".json"), "output_path_not_allowed")
    return path


def write_private(path: Path, value: dict[str, Any], *, replace: bool) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    require(not path.is_symlink(), "output_symlink_rejected")
    if not replace:
        require(not path.exists(), "output_already_exists")
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(descriptor, stat.S_IRUSR | stat.S_IWUSR)
        with os.fdopen(descriptor, "wb", closefd=True) as stream:
            stream.write(json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2).encode())
            stream.write(b"\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        require(stat.S_IMODE(path.stat().st_mode) == 0o600, "output_mode_invalid")
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def self_test() -> dict[str, Any]:
    source_identity()
    synthetic = [
        {
            "schema_version": 16,
            "sequence": 1,
            "event": {
                "kind": "tool_prepared",
                "invocation": {
                    "call_id": "call-1",
                    "name": "apply_patch",
                    "arguments": {"raw": "{}", "parsed": {}},
                },
            },
        },
        {
            "schema_version": 16,
            "sequence": 2,
            "event": {
                "kind": "tool_outcome_committed",
                "call_id": "call-1",
                "name": "apply_patch",
                "outcome": {
                    "invocation": "accepted",
                    "transport": "succeeded",
                    "operation": "failed",
                    "side_effect": "not_applied",
                    "retry": "after_correction",
                    "failure_code": "patch_parse",
                    "evidence": {"status": "none", "references": []},
                    "artifacts": [],
                    "workspace_revision": None,
                },
            },
        },
    ]
    observed = tool_summary_v16(synthetic)["edit_observation"]
    require(
        observed["failure_codes"] == {"patch_parse": 1}
        and observed["unrecovered_failures"] == 1,
        "failure_projection_self_test_failed",
    )
    return {
        "status": "pass",
        "manifest_sha256": digest_file(MANIFEST_PATH),
        "harness_sha256": digest_file(Path(__file__).resolve()),
        "task_manifest_sha256": digest_file(TASK_MANIFEST_PATH),
        "binary_sha256": MANIFEST["source_identity"]["binary_sha256"],
    }


def run_gate(command: list[str]) -> dict[str, Any]:
    environment = os.environ.copy()
    environment["CARGO_INCREMENTAL"] = "0"
    environment["CARGO_TARGET_DIR"] = TARGET_DIR
    started = time.monotonic()
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=3_600,
        check=False,
    )
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout_sha256": digest_bytes(completed.stdout),
        "stderr_sha256": digest_bytes(completed.stderr),
        "wall_time_ms": int((time.monotonic() - started) * 1_000),
    }


def offline(output: Path) -> dict[str, Any]:
    require(not repository_identity()["dirty"], "repository_must_be_clean")
    identity = repository_identity()
    gates = [
        run_gate(["cargo", "test", "-p", "codewhale-tools", "--locked", "m7c_"]),
        run_gate(["cargo", "test", "-p", "codewhale-app", "--locked", "m7c_"]),
        run_gate(["cargo", "test", "-p", "codewhale-state", "--locked", "tool_prepared_sigkill_executes_once_after_sqlite_reopen"]),
        run_gate(["cargo", "test", "-p", "codewhale-state", "--locked", "tool_side_effect_crash_is_not_executed_twice_after_reopen"]),
        run_gate(["cargo", "test", "-p", "codewhale-state", "--locked", "tool_outcome_committed_sigkill_never_reexecutes_after_sqlite_reopen"]),
        run_gate(["cargo", "test", "-p", "codewhale-app-server", "--locked", "sigkill_app_server_recovers_same_run_and_terminal_replays_without_key"]),
        run_gate(["./scripts/dev-deepseek-agent.sh", "focused"]),
        run_gate(["cargo", "fmt", "--all", "--", "--check"]),
    ]
    after = repository_identity()
    record = {
        "schema": OFFLINE_SCHEMA,
        "suite_id": MANIFEST["suite_id"],
        **self_test(),
        "repository_before": identity,
        "repository_after": after,
        "gates": gates,
        "passed": identity == after and not identity["dirty"] and all(gate["exit_code"] == 0 for gate in gates),
        "credential_read": False,
        "api_requests": 0,
    }
    write_private(output, record, replace=False)
    return record


def verify_offline(path: Path) -> dict[str, Any]:
    value = load_json(path, "offline_record_unavailable")
    current = repository_identity()
    require(
        value.get("schema") == OFFLINE_SCHEMA
        and value.get("suite_id") == MANIFEST["suite_id"]
        and value.get("passed") is True
        and value.get("manifest_sha256") == digest_file(MANIFEST_PATH)
        and value.get("harness_sha256") == digest_file(Path(__file__).resolve())
        and value.get("binary_sha256") == MANIFEST["source_identity"]["binary_sha256"]
        and value.get("repository_before") == value.get("repository_after") == current
        and not current["dirty"],
        "offline_record_identity_mismatch",
    )
    return value


def classify_arm(arm: dict[str, Any]) -> dict[str, Any]:
    attempts = list(arm.get("tool", {}).get("edit_attempts", []))
    for child in arm.get("child", {}).get("children", []):
        attempts.extend(child.get("tool", {}).get("edit_attempts", []))
    buckets: Counter[str] = Counter()
    code_to_bucket = {
        code: bucket
        for bucket, codes in MANIFEST["failure_buckets"].items()
        for code in codes
    }
    for attempt in attempts:
        if attempt.get("success"):
            continue
        buckets[code_to_bucket.get(attempt.get("failure_code"), "unclassified")] += 1
    failures = [attempt for attempt in attempts if not attempt.get("success")]
    return {
        "edit_attempts": len(attempts),
        "edit_successes": sum(attempt.get("success") is True for attempt in attempts),
        "first_edit_success": attempts[0].get("success") if attempts else None,
        "failure_codes": dict(
            sorted(Counter(attempt.get("failure_code") for attempt in failures).items())
        ),
        "failure_buckets": dict(sorted(buckets.items())),
        "recovered_failures": sum(attempt.get("recovered") is True for attempt in failures),
        "unrecovered_failures": sum(attempt.get("recovered") is False for attempt in failures),
        "model_requests_to_recovery": [
            attempt["model_requests_to_recovery"]
            for attempt in failures
            if isinstance(attempt.get("model_requests_to_recovery"), int)
        ],
    }


def arm_stop_reason(arm: dict[str, Any]) -> str | None:
    accounting = arm.get("accounting", {})
    if accounting.get("billing_unknown") is True or accounting.get("billing_unknown_attempts", 0):
        return "unknown_billing"
    if not accounting.get("valid") or not arm.get("measurement_valid"):
        return "incomplete_accounting_or_measurement"
    if accounting.get("cost_nanousd", 0) > MANIFEST["resources"]["max_known_cost_nanousd_per_arm"]:
        return "per_arm_cost_limit"
    if arm.get("false_success"):
        return "false_success"
    if not all(
        arm.get(field)
        for field in (
            "scope_valid",
            "path_authority_valid",
            "tool_authority_valid",
            "child_authority_valid",
        )
    ):
        return "authority_safety_failure"
    return None


def summarize(arms: list[dict[str, Any]], abort: dict[str, Any] | None) -> dict[str, Any]:
    task_cells: dict[str, dict[str, Any]] = {}
    bucket_totals: Counter[str] = Counter()
    bucket_tasks: defaultdict[str, set[str]] = defaultdict(set)
    failure_codes: Counter[str] = Counter()
    total_usage = {field: 0 for field in USAGE_FIELDS}
    for task_id in TASK_IDS:
        selected = [arm for arm in arms if arm["task_id"] == task_id]
        task_cells[task_id] = {
            "arms": len(selected),
            "verified_success": sum(arm["verified_success"] for arm in selected),
            "false_success": sum(arm["false_success"] for arm in selected),
            "first_edit_success": sum(arm["edit_observation"]["first_edit_success"] is True for arm in selected),
            "edit_attempts": sum(arm["edit_observation"]["edit_attempts"] for arm in selected),
        }
    for arm in arms:
        observation = arm["edit_observation"]
        for bucket, count in observation["failure_buckets"].items():
            bucket_totals[bucket] += count
            if count:
                bucket_tasks[bucket].add(arm["task_id"])
        failure_codes.update(observation["failure_codes"])
        for field in USAGE_FIELDS:
            total_usage[field] += arm["accounting"]["usage"][field]
    threshold = MANIFEST["decision_rules"]["minimum_typed_failures_for_mechanism"]
    task_threshold = MANIFEST["decision_rules"]["minimum_tasks_with_same_bucket"]
    patch_candidate = bucket_totals["patch_generation"] >= threshold and len(bucket_tasks["patch_generation"]) >= task_threshold
    recovery_candidate = (
        bucket_totals["edit_recovery"] >= threshold
        and len(bucket_tasks["edit_recovery"]) >= task_threshold
    ) or sum(arm["edit_observation"]["unrecovered_failures"] for arm in arms) >= threshold
    transaction_candidate = bucket_totals["transaction_ambiguity"] >= 1
    complete = abort is None and len(arms) == MANIFEST["experiment"]["formal_arms"]
    if not complete:
        decision = "hold_incomplete_observation"
    elif transaction_candidate:
        decision = "admit_transaction_investigation"
    elif patch_candidate:
        decision = "admit_patch_generation_treatment"
    elif recovery_candidate:
        decision = "admit_edit_recovery_treatment"
    else:
        decision = "no_edit_mechanism_admitted"
    return {
        "complete": complete,
        "product_metric_eligible": False,
        "decision": decision,
        "arms": len(arms),
        "verified_success": sum(arm["verified_success"] for arm in arms),
        "false_success": sum(arm["false_success"] for arm in arms),
        "first_edit_success": sum(arm["edit_observation"]["first_edit_success"] is True for arm in arms),
        "edit_attempts": sum(arm["edit_observation"]["edit_attempts"] for arm in arms),
        "edit_successes": sum(arm["edit_observation"]["edit_successes"] for arm in arms),
        "recovered_failures": sum(arm["edit_observation"]["recovered_failures"] for arm in arms),
        "unrecovered_failures": sum(arm["edit_observation"]["unrecovered_failures"] for arm in arms),
        "failure_codes": dict(sorted(failure_codes.items())),
        "failure_buckets": dict(sorted(bucket_totals.items())),
        "tasks_per_bucket": {bucket: sorted(tasks) for bucket, tasks in sorted(bucket_tasks.items())},
        "task_cells": task_cells,
        "requests": sum(arm["accounting"]["requests"]["physical_attempts"] for arm in arms),
        "usage": total_usage,
        "cost_nanousd": sum(arm["accounting"]["cost_nanousd"] for arm in arms),
        "cost_nanocny": sum(arm["accounting"]["cost_nanocny"] for arm in arms),
        "wall_time_ms": sum(arm["wall_time_ms"] for arm in arms),
        "normal_run_crash_frequency_claim_admissible": False,
    }


def schedule() -> list[dict[str, Any]]:
    return [
        {"task_id": task_id, "run_index": run_index, "arm_position": position}
        for run_index, tasks in enumerate(MANIFEST["experiment"]["round_order"], start=1)
        for position, task_id in enumerate(tasks, start=1)
    ]


def live(output: Path, offline_record: Path, acknowledge_cost: bool) -> dict[str, Any]:
    require(acknowledge_cost, "cost_acknowledgement_required")
    require(not output.exists(), "output_already_exists")
    verify_offline(offline_record)
    binary_identity = source_identity()
    key_path = Path(MANIFEST["credential_admission"]["key_path"])
    key = EXECUTOR.CANARY.read_key(key_path)
    record: dict[str, Any] = {
        "schema": RESULT_SCHEMA,
        "suite_id": MANIFEST["suite_id"],
        "status": "running",
        "manifest_sha256": digest_file(MANIFEST_PATH),
        "harness_sha256": digest_file(Path(__file__).resolve()),
        "task_manifest_sha256": digest_file(TASK_MANIFEST_PATH),
        "binary_identity": binary_identity,
        "evaluation_revision": repository_identity(),
        "maximum_reruns": 0,
        "credential_read": True,
        "api_surface": MANIFEST["experiment"]["api_surface"],
        "model": MANIFEST["experiment"]["model"],
        "schedule": schedule(),
        "active_arm": None,
        "arms": [],
        "abort": None,
        "aggregate": None,
    }
    write_private(output, record, replace=False)
    try:
        for scheduled in record["schedule"]:
            known_cost = sum(arm["accounting"]["cost_nanousd"] for arm in record["arms"])
            headroom = MANIFEST["resources"]["max_known_cost_nanousd_per_arm"]
            if known_cost + headroom > MANIFEST["resources"]["suite_known_cost_nanousd"]:
                record["abort"] = {"code": "suite_cost_limit", "before_arm": scheduled}
                break
            record["active_arm"] = {**scheduled, "status": "reserved"}
            write_private(output, record, replace=True)
            arm = EXECUTOR.execute_arm(
                scheduled["task_id"],
                "candidate",
                scheduled["run_index"],
                Path(MANIFEST["source_identity"]["binary_path"]),
                MANIFEST["source_identity"]["revision"],
                key,
            )
            arm["variant"] = "current"
            arm["arm_position"] = scheduled["arm_position"]
            arm["edit_observation"] = classify_arm(arm)
            record["arms"].append(arm)
            record["active_arm"] = None
            write_private(output, record, replace=True)
            if reason := arm_stop_reason(arm):
                record["abort"] = {
                    "code": reason,
                    "task_id": arm["task_id"],
                    "run_index": arm["run_index"],
                }
                break
    except (ObservationError, EXECUTOR.EvaluationError) as error:
        record["abort"] = {
            "code": getattr(error, "code", type(error).__name__),
            "details": getattr(error, "details", {}),
        }
    finally:
        key = ""
    record["active_arm"] = None
    record["status"] = "complete" if record["abort"] is None and len(record["arms"]) == 15 else "stopped"
    record["aggregate"] = summarize(record["arms"], record["abort"])
    write_private(output, record, replace=True)
    return record


def main() -> int:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("self-test")
    offline_parser = commands.add_parser("offline")
    offline_parser.add_argument("--output", required=True)
    live_parser = commands.add_parser("live")
    live_parser.add_argument("--output", required=True)
    live_parser.add_argument("--offline-record", required=True)
    live_parser.add_argument("--acknowledge-cost", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "self-test":
            result = self_test()
        elif args.command == "offline":
            result = offline(output_path(args.output))
        else:
            result = live(
                output_path(args.output),
                output_path(args.offline_record),
                args.acknowledge_cost,
            )
        print(json.dumps(result if args.command == "self-test" else {
            "schema": result["schema"],
            "status": result.get("status", "pass" if result.get("passed") else "failed"),
            "arms": len(result.get("arms", [])),
            "abort": result.get("abort"),
            "aggregate": result.get("aggregate"),
        }, ensure_ascii=False, sort_keys=True))
        if args.command == "offline":
            return 0 if result["passed"] else 1
        if args.command == "live":
            return 0 if result["status"] == "complete" else 1
        return 0
    except (ObservationError, EXECUTOR.EvaluationError, OSError, subprocess.SubprocessError) as error:
        print(
            json.dumps(
                {
                    "status": "error",
                    "code": getattr(error, "code", type(error).__name__),
                    "details": getattr(error, "details", {}),
                    "credential_read": False,
                    "api_requests": 0,
                },
                ensure_ascii=False,
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
